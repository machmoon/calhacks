//! Tool-call reliability and tracing.
//!
//! Two things, both wrappers so that `agent.rs`, `client.rs` and the
//! harness handlers stay as they are:
//!
//! 1. **Structured tool errors with a repair budget.** Every failed tool
//!    result that reaches the model is one of five kinds -- `schema`,
//!    `not_found`, `domain`, `timeout`, `transport` -- rendered as a small
//!    JSON object with a one-line repair hint, and a tool that keeps failing
//!    the same way is cut off after [`MAX_TOOL_REPAIRS`] repairs. The shape
//!    is pydantic-ai's `ToolManager` (pydantic/pydantic-ai @main,
//!    `pydantic_ai_slim/pydantic_ai/tool_manager.py`): a per-tool-name
//!    `retries` map carried across steps, `_check_max_retries` raising
//!    "Tool {name!r} exceeded max retries count of {max_retries}", and a
//!    `RetryPromptPart` whose `cause` is `unknown_tool` for a name that
//!    does not exist. The error kinds themselves follow the JSON-RPC codes
//!    MCP uses for protocol errors (`-32601` method not found, `-32602`
//!    invalid params, `-32603` internal, `-32001` request timeout in the
//!    TypeScript SDK's `ErrorCode`), with `transport` added for the
//!    `claude` CLI path. Added here, not in pydantic-ai: an identical call
//!    (same tool, same input) that fails identically counts as a repair
//!    attempt even when the error is a domain error, which is the
//!    "duplicate action calls" failure seen under the CLI transport.
//!
//! 2. **Spans for every model call and tool call**, written to
//!    `<run>/trace.jsonl`. Attribute names are the OpenTelemetry GenAI
//!    semantic conventions (open-telemetry/semantic-conventions-genai @main,
//!    `model/gen-ai/registry.yaml` and `model/gen-ai/spans.yaml`):
//!    `gen_ai.operation.name` (`chat` / `execute_tool`), `gen_ai.provider.name`
//!    (`anthropic`), `gen_ai.request.model`, `gen_ai.request.max_tokens`,
//!    `gen_ai.response.model`, `gen_ai.response.id`,
//!    `gen_ai.response.finish_reasons`, `gen_ai.usage.input_tokens`,
//!    `gen_ai.usage.output_tokens`, `gen_ai.usage.cache_read.input_tokens`,
//!    `gen_ai.usage.cache_write.input_tokens`, `gen_ai.tool.name`,
//!    `gen_ai.tool.call.id`, `gen_ai.tool.type`, `gen_ai.agent.name`,
//!    `gen_ai.conversation.id`, `error.type`. Span names follow
//!    `spans.yaml`: `chat {gen_ai.request.model}` (kind CLIENT) and
//!    `execute_tool {gen_ai.tool.name}` (kind INTERNAL). The registry has no
//!    cost attribute, so the price is `wright.usage.cost_usd` (a vendor
//!    prefix, as the spec asks for custom attributes). Each line is one
//!    OTLP/JSON `ExportTraceServiceRequest` (`resourceSpans` -> `scopeSpans`
//!    -> `spans`, hex `traceId`/`spanId`, nanosecond strings), which is what
//!    the OpenTelemetry Collector's `fileexporter` writes with its default
//!    `format: json` ("newline-delimited OTLP JSON, one line per batch",
//!    opentelemetry-collector-contrib `exporter/fileexporter/README.md`) and
//!    what its `otlpjsonfilereceiver` reads back, so the file can be fed to
//!    any OTLP backend.
//!
//! Messages are not recorded here: the transcripts under
//! `<run>/transcripts/` (`agent::save_transcript`) hold them, and a span's
//! `gen_ai.conversation.id` is that transcript's name.

use crate::agent::{Handler, ToolOutput};
use crate::client::{ApiError, BoxFut, Client, Provider, Request, Transport};
use crate::price::Usage;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Repairs a tool call gets before the harness stops offering it another
/// try (pydantic-ai: `default_max_retries` is 1; three here because the
/// agents' inputs are whole JSON patches and one hint rarely fixes them).
pub const MAX_TOOL_REPAIRS: u32 = 3;

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolErrorKind {
    /// The input does not match the tool's schema or is not JSON.
    Schema,
    /// No such tool, path, check, role or stage.
    NotFound,
    /// The tool ran and the domain refused or the check failed.
    Domain,
    /// A sub-agent, stage or external process ran out of time.
    Timeout,
    /// The model transport failed (HTTP, network, the `claude` CLI).
    Transport,
}

impl ToolErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolErrorKind::Schema => "schema",
            ToolErrorKind::NotFound => "not_found",
            ToolErrorKind::Domain => "domain",
            ToolErrorKind::Timeout => "timeout",
            ToolErrorKind::Transport => "transport",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "schema" => ToolErrorKind::Schema,
            "not_found" => ToolErrorKind::NotFound,
            "domain" => ToolErrorKind::Domain,
            "timeout" => ToolErrorKind::Timeout,
            "transport" => ToolErrorKind::Transport,
            _ => return None,
        })
    }
    /// The one-line repair hint the model sees.
    pub fn hint(self) -> &'static str {
        match self {
            ToolErrorKind::Schema => "fix the input to match the tool's input_schema exactly (field names, types, JSON-as-string where asked) and call it again",
            ToolErrorKind::NotFound => "use only names listed in your tools and in the catalog/check/role listings; do not guess ids",
            ToolErrorKind::Domain => "read the numbers in the message, change the design or spec accordingly, then re-run design and check",
            ToolErrorKind::Timeout => "the work took too long; try a smaller task or move on and say so in your report",
            ToolErrorKind::Transport => "the host's model connection failed, not your call; retry once, then report the failure",
        }
    }
    /// Whether the call was the model's to fix: these count toward the
    /// per-tool repair budget on every occurrence. A domain error counts
    /// only when the same call is repeated and fails the same way.
    pub fn repairable(self) -> bool {
        matches!(self, ToolErrorKind::Schema | ToolErrorKind::NotFound)
    }
}

/// A failed tool call, as the model sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
    pub hint: String,
    /// `attempt/max` once the repair budget has been consulted.
    pub attempt: Option<(u32, u32)>,
}

impl ToolError {
    pub fn new(kind: ToolErrorKind, message: impl Into<String>) -> Self {
        ToolError { kind, message: message.into(), hint: kind.hint().into(), attempt: None }
    }
    pub fn schema(m: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Schema, m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NotFound, m)
    }
    pub fn domain(m: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Domain, m)
    }
    pub fn timeout(m: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Timeout, m)
    }
    pub fn transport(m: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Transport, m)
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// The `tool_result` text: the message as the tool wrote it (check
    /// tables stay readable), then one JSON line with the kind, the attempt
    /// and the hint -- pydantic-ai's `RetryPromptPart` shape (the errors,
    /// then "Fix the errors and try again." as a trailer).
    pub fn render(&self) -> String {
        let mut e = json!({"type": self.kind.as_str(), "hint": self.hint});
        if let Some((n, max)) = self.attempt {
            e["attempt"] = json!(format!("{n}/{max}"));
        }
        format!("{}\n\n{}", self.message.trim_end(), json!({"error": e}))
    }

    /// A `ToolOutput` carrying this error. Schema and not-found errors are
    /// `malformed` so `agent::run_agent`'s requery limit still applies.
    pub fn into_output(self) -> ToolOutput {
        let malformed = self.kind.repairable();
        let mut out = ToolOutput::err(self.render());
        out.malformed = malformed;
        out
    }

    /// Read a rendered error back.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim_end();
        let (message, last) = text.rsplit_once('\n').map_or(("", text), |(m, l)| (m, l));
        let v: Value = serde_json::from_str(last.trim()).ok()?;
        let e = v.get("error")?;
        let kind = ToolErrorKind::parse(e["type"].as_str()?)?;
        let attempt = e["attempt"].as_str().and_then(|s| {
            let (a, b) = s.split_once('/')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        });
        Some(ToolError {
            kind,
            message: message.trim_end().into(),
            hint: e["hint"].as_str().unwrap_or(kind.hint()).into(),
            attempt,
        })
    }

    /// The kind of a failed output: its own if it was rendered by
    /// [`ToolError::render`], else guessed from the free text the harness
    /// handlers write today (`"input does not match the tool schema"`,
    /// `"unknown tool"`, `"no check named"`, ...). An `ok` output is `None`.
    pub fn classify(out: &ToolOutput) -> Option<Self> {
        if !out.is_error {
            return None;
        }
        if let Some(e) = Self::parse(&out.content) {
            return Some(e);
        }
        let m = out.content.to_ascii_lowercase();
        let has = |ps: &[&str]| ps.iter().any(|p| m.contains(p));
        let kind = if out.malformed
            || has(&["does not match the tool schema", "not valid json", "must be a json object", "cut off at max_tokens", "args_json"])
        {
            if has(&["unknown tool", "no specialist", "no such"]) {
                ToolErrorKind::NotFound
            } else {
                ToolErrorKind::Schema
            }
        } else if has(&["timed out", "timeout", "deadline"]) {
            ToolErrorKind::Timeout
        } else if has(&["claude cli:", "http 4", "http 5", "network:", " stopped: ", "scripted transport ran out", "over budget", "budget exceeded"]) {
            ToolErrorKind::Transport
        } else if has(&["unknown", "no such", "not a catalog-backed path", "no check named", "no declared check", "no stage", "no options found", "does not exist"]) {
            ToolErrorKind::NotFound
        } else {
            ToolErrorKind::Domain
        };
        Some(ToolError::new(kind, out.content.clone()))
    }
}

/// What the repair budget decided for one failed call.
#[derive(Debug, Clone, PartialEq)]
pub enum Repair {
    /// Not counted (an ok result, or a fresh domain error).
    Free,
    /// Counted: attempt `n` of `max`; the model may try again.
    Attempt(u32, u32),
    /// Over budget: the result is marked malformed so the agent loop's
    /// requery limit ends a stuck agent.
    Exhausted(u32),
}

/// Per-tool repair counts for one agent (pydantic-ai `RunContext.retries`:
/// reset on a success of that tool, carried across turns otherwise).
#[derive(Debug, Default)]
pub struct RepairBudget {
    retries: HashMap<String, u32>,
    last_failure: HashMap<String, (u64, String)>,
    pub max: u32,
}

impl RepairBudget {
    pub fn new(max: u32) -> Self {
        RepairBudget { max, ..Default::default() }
    }

    /// Account for `out` and rewrite it: a failed result becomes a
    /// structured [`ToolError`] (with the attempt count) and an exhausted
    /// one is also `malformed`.
    pub fn apply(&mut self, name: &str, input: &Value, out: ToolOutput) -> (ToolOutput, Repair) {
        let Some(mut err) = ToolError::classify(&out) else {
            self.retries.remove(name);
            self.last_failure.remove(name);
            return (out, Repair::Free);
        };
        let fp = fingerprint(input);
        let repeated = self.last_failure.get(name).is_some_and(|(f, msg)| *f == fp && *msg == err.message);
        self.last_failure.insert(name.into(), (fp, err.message.clone()));
        if !(err.kind.repairable() || repeated) {
            let mut o = out;
            o.content = err.render();
            return (o, Repair::Free);
        }
        let n = self.retries.entry(name.to_string()).or_insert(0);
        *n += 1;
        let n = *n;
        if repeated && err.kind == ToolErrorKind::Domain {
            err.hint = format!("you already called {name} with this exact input and got this exact error; change the input or the approach");
        }
        if n > self.max {
            err.attempt = Some((n, self.max));
            err.hint = format!("tool '{name}' exceeded its {} repairs; stop retrying it this way and change course", self.max);
            let mut o = err.into_output();
            o.malformed = true;
            o.blocks = out.blocks;
            o.stop = out.stop;
            (o, Repair::Exhausted(n))
        } else {
            err.attempt = Some((n, self.max));
            let mut o = out;
            o.content = err.render();
            (o, Repair::Attempt(n, self.max))
        }
    }
}

fn fingerprint(v: &Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    h.finish()
}

// ----------------------------------------------------------------- spans

/// OTLP `SpanKind` numbers.
pub const KIND_INTERNAL: u8 = 1;
pub const KIND_CLIENT: u8 = 3;
/// OTLP `Status.code` numbers.
pub const STATUS_OK: u8 = 1;
pub const STATUS_ERROR: u8 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: u8,
    pub start_unix_nanos: u128,
    pub end_unix_nanos: u128,
    pub attributes: BTreeMap<String, Value>,
    pub status: u8,
    pub status_message: String,
}

impl Span {
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.end_unix_nanos.saturating_sub(self.start_unix_nanos) as u64)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).and_then(Value::as_str)
    }
    pub fn num(&self, key: &str) -> f64 {
        self.attributes.get(key).and_then(Value::as_f64).unwrap_or(0.0)
    }
    pub fn operation(&self) -> &str {
        self.str("gen_ai.operation.name").unwrap_or("")
    }

    /// One OTLP/JSON `ExportTraceServiceRequest` holding this span.
    pub fn to_otlp(&self, service: &str) -> Value {
        let attr = |k: &str, v: &Value| {
            let value = match v {
                Value::Bool(b) => json!({"boolValue": b}),
                Value::Number(n) if n.is_i64() || n.is_u64() => json!({"intValue": n.to_string()}),
                Value::Number(n) => json!({"doubleValue": n}),
                Value::String(s) => json!({"stringValue": s}),
                Value::Array(a) => json!({"arrayValue": {"values": a.iter().map(|x| json!({"stringValue": x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())})).collect::<Vec<_>>()}}),
                other => json!({"stringValue": other.to_string()}),
            };
            json!({"key": k, "value": value})
        };
        let attributes: Vec<Value> = self.attributes.iter().map(|(k, v)| attr(k, v)).collect();
        let mut span = json!({
            "traceId": self.trace_id,
            "spanId": self.span_id,
            "name": self.name,
            "kind": self.kind,
            "startTimeUnixNano": self.start_unix_nanos.to_string(),
            "endTimeUnixNano": self.end_unix_nanos.to_string(),
            "attributes": attributes,
            "status": {"code": self.status, "message": self.status_message},
        });
        if let Some(p) = &self.parent_span_id {
            span["parentSpanId"] = json!(p);
        }
        json!({"resourceSpans": [{
            "resource": {"attributes": [attr("service.name", &json!(service))]},
            "scopeSpans": [{"scope": {"name": "wright-agents", "version": env!("CARGO_PKG_VERSION")}, "spans": [span]}]
        }]})
    }

    /// Every span in one OTLP/JSON line (any number of resource/scope groups).
    pub fn from_otlp(line: &Value) -> Vec<Span> {
        let mut out = vec![];
        for rs in line["resourceSpans"].as_array().into_iter().flatten() {
            for ss in rs["scopeSpans"].as_array().into_iter().flatten() {
                for s in ss["spans"].as_array().into_iter().flatten() {
                    let mut attributes = BTreeMap::new();
                    for a in s["attributes"].as_array().into_iter().flatten() {
                        let Some(k) = a["key"].as_str() else { continue };
                        let v = &a["value"];
                        let val = if let Some(s) = v["stringValue"].as_str() {
                            json!(s)
                        } else if let Some(s) = v["intValue"].as_str() {
                            s.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(s))
                        } else if let Some(n) = v["intValue"].as_i64() {
                            json!(n)
                        } else if let Some(d) = v["doubleValue"].as_f64() {
                            json!(d)
                        } else if let Some(b) = v["boolValue"].as_bool() {
                            json!(b)
                        } else if let Some(vals) = v["arrayValue"]["values"].as_array() {
                            json!(vals.iter().map(|x| x["stringValue"].clone()).collect::<Vec<_>>())
                        } else {
                            continue;
                        };
                        attributes.insert(k.to_string(), val);
                    }
                    let nanos = |v: &Value| v.as_str().and_then(|s| s.parse::<u128>().ok()).or_else(|| v.as_u64().map(u128::from)).unwrap_or(0);
                    out.push(Span {
                        trace_id: s["traceId"].as_str().unwrap_or("").into(),
                        span_id: s["spanId"].as_str().unwrap_or("").into(),
                        parent_span_id: s["parentSpanId"].as_str().map(str::to_string),
                        name: s["name"].as_str().unwrap_or("").into(),
                        kind: s["kind"].as_u64().unwrap_or(0) as u8,
                        start_unix_nanos: nanos(&s["startTimeUnixNano"]),
                        end_unix_nanos: nanos(&s["endTimeUnixNano"]),
                        attributes,
                        status: s["status"]["code"].as_u64().unwrap_or(0) as u8,
                        status_message: s["status"]["message"].as_str().unwrap_or("").into(),
                    });
                }
            }
        }
        out
    }
}

/// Load every span of a `trace.jsonl`; a bad line is skipped.
pub fn read_spans(path: &Path) -> std::io::Result<Vec<Span>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).flat_map(|v| Span::from_otlp(&v)).collect())
}

fn now_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// Writes spans for one run. Cheap to share; the file is appended under a
/// mutex so parallel agents interleave whole lines.
pub struct Tracer {
    pub trace_id: String,
    pub run_id: String,
    pub path: Option<PathBuf>,
    file: Mutex<Option<std::fs::File>>,
    /// Which agent's conversation is about to call the model
    /// ([`Traced::observe_user_blocks`] runs right before `client.create`).
    current: Mutex<Option<(String, String)>>,
    /// The last chat span per agent: the parent of the tool calls it made.
    last_chat: Mutex<HashMap<String, String>>,
    seq: AtomicU64,
    rng: std::collections::hash_map::RandomState,
    /// Everything recorded, for tests and the end-of-run summary.
    pub spans: Mutex<Vec<Span>>,
}

impl Tracer {
    /// Record to `<out_dir>/trace.jsonl`; with no out dir, spans are kept in
    /// memory only.
    pub fn for_run(out_dir: Option<&Path>, run_id: &str) -> Arc<Tracer> {
        let path = out_dir.map(|d| d.join("trace.jsonl"));
        let file = path.as_ref().and_then(|p| {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
        });
        let rng = std::collections::hash_map::RandomState::new();
        let t = Tracer {
            trace_id: String::new(),
            run_id: run_id.into(),
            path,
            file: Mutex::new(file),
            current: Mutex::new(None),
            last_chat: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(1),
            rng,
            spans: Mutex::new(vec![]),
        };
        let trace_id = format!("{:016x}{:016x}", t.random(), t.random());
        Arc::new(Tracer { trace_id, ..t })
    }

    /// The tracer behind `client`'s transport, if it is traced.
    pub fn of(client: &Client) -> Option<Arc<Tracer>> {
        client.transport.tracer()
    }

    fn random(&self) -> u64 {
        self.rng.hash_one((self.seq.fetch_add(1, Ordering::Relaxed), now_nanos()))
    }
    pub fn new_span_id(&self) -> String {
        format!("{:016x}", self.random())
    }

    pub fn set_current(&self, agent: &str, conversation: &str) {
        *self.current.lock().unwrap() = Some((agent.into(), conversation.into()));
    }

    pub fn record(&self, span: Span) {
        let line = span.to_otlp(&self.run_id).to_string();
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = writeln!(f, "{line}");
            }
        }
        self.spans.lock().unwrap().push(span);
    }

    /// Record one model call (span name `chat {model}`, kind CLIENT).
    pub fn record_chat(&self, req: &Request, resp: &Result<Value, ApiError>, provider: Provider, start: u128, end: u128) -> String {
        let body = &req.body;
        let model = body["model"].as_str().unwrap_or("").to_string();
        let (agent, conversation) = self.current.lock().unwrap().clone().unwrap_or_default();
        let mut attributes = BTreeMap::new();
        let mut put = |k: &str, v: Value| {
            attributes.insert(k.to_string(), v);
        };
        put("gen_ai.operation.name", json!("chat"));
        put("gen_ai.provider.name", json!(provider_name(provider)));
        put("gen_ai.request.model", json!(model));
        if let Some(m) = body["max_tokens"].as_u64() {
            put("gen_ai.request.max_tokens", json!(m));
        }
        if let Some(e) = body["output_config"]["effort"].as_str() {
            put("gen_ai.request.reasoning.level", json!(e));
        }
        if !agent.is_empty() {
            put("gen_ai.agent.name", json!(agent));
        }
        if !conversation.is_empty() {
            put("gen_ai.conversation.id", json!(conversation));
        }
        put("wright.transport", json!(provider.name()));
        let (status, message) = match resp {
            Ok(r) => {
                if let Some(id) = r["id"].as_str() {
                    put("gen_ai.response.id", json!(id));
                }
                let priced_as = r["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(&model).to_string();
                put("gen_ai.response.model", json!(priced_as));
                if let Some(s) = r["stop_reason"].as_str() {
                    put("gen_ai.response.finish_reasons", json!([s]));
                }
                let usage = Usage::from_json(&r["usage"]);
                put("gen_ai.usage.input_tokens", json!(usage.prompt()));
                put("gen_ai.usage.output_tokens", json!(usage.output));
                put("gen_ai.usage.cache_read.input_tokens", json!(usage.cache_read));
                put("gen_ai.usage.cache_write.input_tokens", json!(usage.cache_write_5m + usage.cache_write_1h));
                put("wright.usage.cost_usd", json!(usage.usd(&priced_as)));
                let tools = r["content"].as_array().map(|c| c.iter().filter(|b| b["type"] == "tool_use").count()).unwrap_or(0);
                put("wright.response.tool_calls", json!(tools));
                (STATUS_OK, String::new())
            }
            Err(e) => {
                put("error.type", json!(api_error_type(e)));
                (STATUS_ERROR, e.to_string())
            }
        };
        let span_id = self.new_span_id();
        if !agent.is_empty() {
            self.last_chat.lock().unwrap().insert(agent, span_id.clone());
        }
        self.record(Span {
            trace_id: self.trace_id.clone(),
            span_id: span_id.clone(),
            parent_span_id: None,
            name: format!("chat {model}"),
            kind: KIND_CLIENT,
            start_unix_nanos: start,
            end_unix_nanos: end,
            attributes,
            status,
            status_message: message,
        });
        span_id
    }

    /// Record one tool call (span name `execute_tool {name}`, kind
    /// INTERNAL), parented to the agent's last model call.
    pub fn record_tool(&self, agent: &str, conversation: &str, name: &str, out: &ToolOutput, repair: &Repair, (start, end): (u128, u128)) -> String {
        let mut attributes = BTreeMap::new();
        attributes.insert("gen_ai.operation.name".into(), json!("execute_tool"));
        attributes.insert("gen_ai.tool.name".into(), json!(name));
        attributes.insert("gen_ai.tool.type".into(), json!("function"));
        attributes.insert("gen_ai.agent.name".into(), json!(agent));
        if !conversation.is_empty() {
            attributes.insert("gen_ai.conversation.id".into(), json!(conversation));
        }
        attributes.insert("wright.tool.result_chars".into(), json!(out.content.chars().count()));
        if out.stop {
            attributes.insert("wright.tool.stop".into(), json!(true));
        }
        let (status, message) = match ToolError::classify(out) {
            Some(e) => {
                attributes.insert("error.type".into(), json!(e.kind.as_str()));
                if let Some((n, max)) = e.attempt {
                    attributes.insert("wright.tool.repair_attempt".into(), json!(n));
                    attributes.insert("wright.tool.repair_max".into(), json!(max));
                }
                if matches!(repair, Repair::Exhausted(_)) {
                    attributes.insert("wright.tool.repairs_exhausted".into(), json!(true));
                }
                (STATUS_ERROR, e.message.chars().take(200).collect())
            }
            None => (STATUS_OK, String::new()),
        };
        let parent = self.last_chat.lock().unwrap().get(agent).cloned();
        let span_id = self.new_span_id();
        self.record(Span {
            trace_id: self.trace_id.clone(),
            span_id: span_id.clone(),
            parent_span_id: parent,
            name: format!("execute_tool {name}"),
            kind: KIND_INTERNAL,
            start_unix_nanos: start,
            end_unix_nanos: end,
            attributes,
            status,
            status_message: message,
        });
        span_id
    }
}

fn provider_name(p: Provider) -> &'static str {
    // `gen_ai.provider.name` enum: Foundry is Claude served by Azure, the
    // CLI and the fake are still Anthropic's models.
    match p {
        Provider::Foundry => "azure.ai.inference",
        Provider::CodexCli => "openai",
        _ => "anthropic",
    }
}

fn api_error_type(e: &ApiError) -> String {
    match e {
        ApiError::Http { status, .. } => status.to_string(),
        ApiError::Network(_) => "network".into(),
        ApiError::Cli(_) => "cli".into(),
        ApiError::Config(_) => "config".into(),
        ApiError::ScriptExhausted(_) => "script_exhausted".into(),
    }
}

/// Any [`Transport`], with a `chat` span per call.
pub struct TracedTransport {
    inner: Arc<dyn Transport>,
    tracer: Arc<Tracer>,
}

impl TracedTransport {
    pub fn wrap(inner: Arc<dyn Transport>, tracer: Arc<Tracer>) -> Arc<dyn Transport> {
        if inner.tracer().is_some() {
            return inner;
        }
        Arc::new(TracedTransport { inner, tracer })
    }
}

impl Transport for TracedTransport {
    fn provider(&self) -> Provider {
        self.inner.provider()
    }
    fn tracer(&self) -> Option<Arc<Tracer>> {
        Some(self.tracer.clone())
    }
    fn export_sessions(&self) -> Value {
        self.inner.export_sessions()
    }
    fn import_sessions(&self, sessions: &Value) {
        self.inner.import_sessions(sessions)
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            let start = now_nanos();
            let r = self.inner.send(req).await;
            self.tracer.record_chat(req, &r, self.inner.provider(), start, now_nanos());
            r
        })
    }
}

/// `client` with its transport traced; a no-op if it already is.
pub fn trace_client(client: &Client, tracer: Arc<Tracer>) -> Client {
    let mut c = client.clone();
    c.transport = TracedTransport::wrap(c.transport.clone(), tracer);
    c
}

/// Any [`Handler`], with an `execute_tool` span per call, structured
/// errors and the repair budget. `conversation` is the transcript name the
/// harness will save (`orchestrator`, `reviewer-r2`, `specialist-aero-r1`).
pub struct Traced<'h, H: Handler> {
    inner: &'h mut H,
    tracer: Option<Arc<Tracer>>,
    agent: String,
    conversation: String,
    pub repairs: RepairBudget,
}

impl<'h, H: Handler> Traced<'h, H> {
    pub fn new(tracer: Option<Arc<Tracer>>, agent: &str, conversation: &str, inner: &'h mut H) -> Self {
        Traced { inner, tracer, agent: agent.into(), conversation: conversation.into(), repairs: RepairBudget::new(MAX_TOOL_REPAIRS) }
    }
}

impl<H: Handler> Handler for Traced<'_, H> {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            let start = now_nanos();
            let t0 = Instant::now();
            let raw = self.inner.call(name, input).await;
            let (out, repair) = self.repairs.apply(name, input, raw);
            if let Some(t) = &self.tracer {
                t.record_tool(&self.agent, &self.conversation, name, &out, &repair, (start, start + t0.elapsed().as_nanos()));
            }
            out
        })
    }
    fn observe_user_blocks(&mut self, blocks: &[Value]) {
        if let Some(t) = &self.tracer {
            t.set_current(&self.agent, &self.conversation);
        }
        self.inner.observe_user_blocks(blocks);
    }
    fn observe_tool_result(&mut self, name: &str, input: &Value, output: &ToolOutput) {
        self.inner.observe_tool_result(name, input, output);
    }
    fn extra_blocks(&mut self) -> Vec<Value> {
        self.inner.extra_blocks()
    }
    // forwarded so the orchestrator's checkpoint hook (lane 10) still fires
    fn after_turn(&mut self, conv: &crate::agent::Conversation, owed: &[Value], turns: u32) {
        self.inner.after_turn(conv, owed, turns);
    }
    /// Parallel delegates (lane 9, `crate::merge::fan_out`) run as one
    /// batch; each output then goes through the repair budget and gets its
    /// own span, as in [`Handler::call`]. The spans share the batch's start
    /// time because the calls ran concurrently.
    fn call_many<'a>(&'a mut self, calls: &'a [(String, Value)]) -> BoxFut<'a, Option<Vec<ToolOutput>>> {
        Box::pin(async move {
            let start = now_nanos();
            let outs = self.inner.call_many(calls).await?;
            let end = now_nanos();
            let mut traced = Vec::with_capacity(outs.len());
            for ((name, input), raw) in calls.iter().zip(outs) {
                let (out, repair) = self.repairs.apply(name, input, raw);
                if let Some(t) = &self.tracer {
                    t.record_tool(&self.agent, &self.conversation, name, &out, &repair, (start, end));
                }
                traced.push(out);
            }
            Some(traced)
        })
    }
}

// --------------------------------------------------------------- summary

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolStats {
    pub calls: u64,
    pub errors: u64,
    pub by_error: BTreeMap<String, u64>,
    pub latencies_ms: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStats {
    pub calls: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub latencies_ms: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub tools: BTreeMap<String, ToolStats>,
    pub models: BTreeMap<String, ModelStats>,
    pub spans: usize,
}

/// Nearest-rank percentile of `xs` (`p` in 0..=100); 0 for no samples.
pub fn percentile(xs: &[f64], p: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((p / 100.0) * s.len() as f64).ceil().max(1.0) as usize;
    s[rank.min(s.len()) - 1]
}

pub fn summarize(spans: &[Span]) -> Summary {
    let mut sum = Summary { spans: spans.len(), ..Default::default() };
    for s in spans {
        let ms = s.duration().as_secs_f64() * 1e3;
        match s.operation() {
            "execute_tool" => {
                let name = s.str("gen_ai.tool.name").unwrap_or("?").to_string();
                let t = sum.tools.entry(name).or_default();
                t.calls += 1;
                t.latencies_ms.push(ms);
                if s.status == STATUS_ERROR {
                    t.errors += 1;
                    *t.by_error.entry(s.str("error.type").unwrap_or("error").to_string()).or_default() += 1;
                }
            }
            "chat" => {
                let model = s.str("gen_ai.response.model").or_else(|| s.str("gen_ai.request.model")).unwrap_or("?").to_string();
                let m = sum.models.entry(model).or_default();
                m.calls += 1;
                m.latencies_ms.push(ms);
                if s.status == STATUS_ERROR {
                    m.errors += 1;
                }
                m.input_tokens += s.num("gen_ai.usage.input_tokens") as u64;
                m.output_tokens += s.num("gen_ai.usage.output_tokens") as u64;
                m.cache_read_tokens += s.num("gen_ai.usage.cache_read.input_tokens") as u64;
                m.cost_usd += s.num("wright.usage.cost_usd");
            }
            _ => {}
        }
    }
    sum
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} spans", self.spans)?;
        writeln!(f)?;
        writeln!(f, "{:<28} {:>6} {:>9} {:>9} {:>7}  errors", "model", "calls", "p50 ms", "p95 ms", "USD")?;
        let (mut calls, mut cost, mut inp, mut outp) = (0, 0.0, 0, 0);
        for (name, m) in &self.models {
            calls += m.calls;
            cost += m.cost_usd;
            inp += m.input_tokens;
            outp += m.output_tokens;
            writeln!(
                f,
                "{:<28} {:>6} {:>9.0} {:>9.0} {:>7.3}  {}",
                name,
                m.calls,
                percentile(&m.latencies_ms, 50.0),
                percentile(&m.latencies_ms, 95.0),
                m.cost_usd,
                m.errors
            )?;
        }
        writeln!(f, "{:<28} {:>6} {:>9} {:>9} {:>7.3}  {} in / {} out tokens", "total", calls, "", "", cost, inp, outp)?;
        writeln!(f)?;
        writeln!(f, "{:<28} {:>6} {:>9} {:>9} {:>7}  by kind", "tool", "calls", "p50 ms", "p95 ms", "err %")?;
        for (name, t) in &self.tools {
            let kinds = t.by_error.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ");
            writeln!(
                f,
                "{:<28} {:>6} {:>9.1} {:>9.1} {:>6.0}%  {}",
                name,
                t.calls,
                percentile(&t.latencies_ms, 50.0),
                percentile(&t.latencies_ms, 95.0),
                if t.calls == 0 { 0.0 } else { 100.0 * t.errors as f64 / t.calls as f64 },
                kinds
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{run_agent, Conversation, ModelCfg};
    use crate::fake::{reply, text, tool_use, ScriptedTransport};

    struct Echo;
    impl Handler for Echo {
        fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
            Box::pin(async move {
                match name {
                    "check" => ToolOutput::err("check failed: range 150 km vs 180 km required"),
                    "catalog" => ToolOutput::err(format!("`{}` is not a catalog-backed path. Catalog-backed paths:\n...", input["path"].as_str().unwrap_or(""))),
                    "design" => ToolOutput::ok("designed"),
                    other => ToolOutput::malformed(format!("unknown tool '{other}'")),
                }
            })
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-trace-{name}-{}-{}", std::process::id(), now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn errors_render_and_parse() {
        let e = ToolError::schema("input does not match the tool schema: missing field `path`");
        let out = e.clone().into_output();
        assert!(out.is_error && out.malformed);
        let back = ToolError::parse(&out.content).unwrap();
        assert_eq!(back.kind, ToolErrorKind::Schema);
        assert_eq!(back.hint, ToolErrorKind::Schema.hint());
        assert!(out.content.starts_with("input does not match the tool schema: missing field `path`\n\n{\"error\":{"), "{}", out.content);
        assert_eq!(back.message, "input does not match the tool schema: missing field `path`");
        // a multi-line check report keeps its table; the trailer is the last line
        let rep = ToolError::classify(&ToolOutput::err("designed.\nFAIL budget   -30.000 USD\n2 checks, 1 failing")).unwrap();
        assert_eq!(rep.kind, ToolErrorKind::Domain);
        assert_eq!(ToolError::parse(&rep.render()).unwrap().message, "designed.\nFAIL budget   -30.000 USD\n2 checks, 1 failing");
        // free text from today's handlers is classified
        let k = |s: &str| ToolError::classify(&ToolOutput::err(s)).unwrap().kind;
        assert_eq!(k("no check named 'range' in the report"), ToolErrorKind::NotFound);
        assert_eq!(k("check failed: stall 14.3 vs 14"), ToolErrorKind::Domain);
        assert_eq!(k("aero stopped: claude cli: exit 1"), ToolErrorKind::Transport);
        assert_eq!(k("stage cfd timed out after 600 s"), ToolErrorKind::Timeout);
        assert_eq!(ToolError::classify(&ToolOutput::malformed("patch_json is not valid JSON")).unwrap().kind, ToolErrorKind::Schema);
        assert_eq!(ToolError::classify(&ToolOutput::malformed("unknown tool 'foo'")).unwrap().kind, ToolErrorKind::NotFound);
        assert!(ToolError::classify(&ToolOutput::ok("fine")).is_none());
        assert!(!e.into_output().stop);
    }

    #[test]
    fn repair_budget_counts_schema_errors_and_identical_repeats() {
        let mut b = RepairBudget::new(2);
        let bad = || ToolOutput::malformed("input does not match the tool schema");
        let (o, r) = b.apply("edit_design", &json!({"a": 1}), bad());
        assert_eq!(r, Repair::Attempt(1, 2));
        assert_eq!(ToolError::parse(&o.content).unwrap().attempt, Some((1, 2)));
        let (_, r) = b.apply("edit_design", &json!({"a": 2}), bad());
        assert_eq!(r, Repair::Attempt(2, 2));
        let (o, r) = b.apply("edit_design", &json!({"a": 3}), bad());
        assert_eq!(r, Repair::Exhausted(3));
        assert!(o.malformed && o.content.contains("exceeded its 2 repairs"));
        // success resets
        let (_, r) = b.apply("edit_design", &json!({}), ToolOutput::ok("ok"));
        assert_eq!(r, Repair::Free);
        let (_, r) = b.apply("edit_design", &json!({"a": 1}), bad());
        assert_eq!(r, Repair::Attempt(1, 2));
        // a fresh domain error is free; the same call failing the same way is not
        let dom = || ToolOutput::err("check failed: range 150 vs 180");
        let (o, r) = b.apply("check", &json!({}), dom());
        assert_eq!(r, Repair::Free);
        assert_eq!(ToolError::parse(&o.content).unwrap().kind, ToolErrorKind::Domain);
        assert!(!o.malformed);
        let (o, r) = b.apply("check", &json!({}), dom());
        assert_eq!(r, Repair::Attempt(1, 2));
        assert!(ToolError::parse(&o.content).unwrap().hint.contains("exact input"));
        // a different input with the same message is a new attempt at the problem: free
        let (_, r) = b.apply("check", &json!({"x": 1}), dom());
        assert_eq!(r, Repair::Free);
    }

    #[tokio::test]
    async fn spans_are_written_as_otlp_json_lines() {
        let dir = tmp("spans");
        let transport = Arc::new(ScriptedTransport::new(vec![
            reply("claude-opus-5-5", vec![tool_use("t1", "design", json!({})), tool_use("t2", "catalog", json!({"path": "nope"}))], 1000, 50),
            reply("claude-opus-5-5", vec![text("done")], 1200, 10),
        ]));
        let (events, _) = crate::channel();
        let client = Client::new(transport.clone(), crate::Budget::new(5., 45., None), events, "run-1");
        let tracer = Tracer::for_run(Some(&dir), "run-1");
        let client = trace_client(&client, tracer.clone());
        assert!(Tracer::of(&client).is_some());
        let client2 = trace_client(&client, tracer.clone()); // idempotent
        let mut conv = Conversation::new("specialist:aero", ModelCfg::new("claude-opus-5-5", "low", 100), String::new(), vec![]);
        let mut echo = Echo;
        let mut h = Traced::new(Tracer::of(&client2), "specialist:aero", "specialist-aero-r1", &mut echo);
        run_agent(&client2, &mut conv, vec![text("go")], &mut h, 3).await.unwrap();

        let spans = read_spans(&dir.join("trace.jsonl")).unwrap();
        assert_eq!(spans.len(), 4, "{spans:#?}");
        let chats: Vec<&Span> = spans.iter().filter(|s| s.operation() == "chat").collect();
        let tools: Vec<&Span> = spans.iter().filter(|s| s.operation() == "execute_tool").collect();
        assert_eq!((chats.len(), tools.len()), (2, 2));
        assert_eq!(chats[0].name, "chat claude-opus-5-5");
        assert_eq!(chats[0].kind, KIND_CLIENT);
        assert_eq!(chats[0].str("gen_ai.provider.name"), Some("anthropic"));
        assert_eq!(chats[0].str("gen_ai.agent.name"), Some("specialist:aero"));
        assert_eq!(chats[0].str("gen_ai.conversation.id"), Some("specialist-aero-r1"));
        assert_eq!(chats[0].num("gen_ai.usage.input_tokens"), 1000.0);
        assert_eq!(chats[0].num("gen_ai.usage.output_tokens"), 50.0);
        assert!(chats[0].num("wright.usage.cost_usd") > 0.0);
        assert_eq!(chats[0].attributes["gen_ai.response.finish_reasons"], json!(["tool_use"]));
        // tools hang off the chat that asked for them
        assert_eq!(tools[0].parent_span_id.as_deref(), Some(chats[0].span_id.as_str()));
        assert_eq!(tools[0].name, "execute_tool design");
        assert_eq!(tools[0].status, STATUS_OK);
        assert_eq!(tools[1].name, "execute_tool catalog");
        assert_eq!(tools[1].status, STATUS_ERROR);
        assert_eq!(tools[1].str("error.type"), Some("not_found"));
        assert!(spans.iter().all(|s| s.trace_id == tracer.trace_id && s.span_id.len() == 16 && s.trace_id.len() == 32));
        // the model saw a structured error with a hint
        let reqs = transport.requests.lock().unwrap();
        let result = &reqs[1].body["messages"][2]["content"][1];
        assert_eq!(result["is_error"], true);
        let e = ToolError::parse(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(e.kind, ToolErrorKind::NotFound);
        assert_eq!(e.attempt, Some((1, MAX_TOOL_REPAIRS)));
        // the line shape is OTLP/JSON
        let first: Value = serde_json::from_str(std::fs::read_to_string(dir.join("trace.jsonl")).unwrap().lines().next().unwrap()).unwrap();
        assert!(first["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"].is_string());
        assert_eq!(first["resourceSpans"][0]["resource"]["attributes"][0]["value"]["stringValue"], "run-1");

        let s = summarize(&spans);
        assert_eq!(s.tools["catalog"].errors, 1);
        assert_eq!(s.tools["catalog"].by_error["not_found"], 1);
        assert_eq!(s.models["claude-opus-5-5"].calls, 2);
        assert_eq!(s.models["claude-opus-5-5"].input_tokens, 2200);
        let text = s.to_string();
        assert!(text.contains("execute_tool") || text.contains("catalog"));
        assert!(text.contains("claude-opus-5-5"));
        if std::env::var_os("WRIGHT_KEEP_TRACE").is_some() { eprintln!("trace kept at {}", dir.display()) } else { let _ = std::fs::remove_dir_all(&dir); }
    }

    #[tokio::test]
    async fn exhausted_repairs_end_a_stuck_agent() {
        let dir = tmp("stuck");
        // the model keeps calling an unknown tool
        let transport = Arc::new(ScriptedTransport::from_fn(|_| Some(reply("m", vec![tool_use("t", "nope", json!({}))], 1, 1))));
        let (events, _) = crate::channel();
        let client = Client::new(transport.clone(), crate::Budget::new(5., 45., None), events, "run-2");
        let tracer = Tracer::for_run(Some(&dir), "run-2");
        let client = trace_client(&client, tracer.clone());
        let mut conv = Conversation::new("orchestrator", ModelCfg::new("m", "low", 100), String::new(), vec![]);
        let mut echo = Echo;
        let mut h = Traced::new(Some(tracer.clone()), "orchestrator", "orchestrator", &mut echo);
        let r = run_agent(&client, &mut conv, vec![text("go")], &mut h, 20).await;
        assert!(matches!(r, Err(crate::agent::AgentError::Malformed(..))), "{r:?}");
        // the agent loop's own requery limit ends it (MAX_REQUERIES + 1 calls)
        assert_eq!(transport.calls() as u32, crate::agent::MAX_REQUERIES + 1);
        let spans = tracer.spans.lock().unwrap();
        let last = spans.iter().rev().find(|s| s.operation() == "execute_tool").unwrap();
        assert_eq!(last.str("error.type"), Some("not_found"));
        assert_eq!(last.attributes.get("wright.tool.repairs_exhausted"), Some(&json!(true)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_structured_error_keeps_is_error_and_batches_are_accounted() {
        // every path of the repair budget leaves `is_error` set
        let mut b = RepairBudget::new(1);
        let schema = || ToolOutput::malformed("input does not match the tool schema");
        let (o, r) = b.apply("edit_design", &json!({}), schema());
        assert!(o.is_error && o.malformed && r == Repair::Attempt(1, 1));
        let (o, r) = b.apply("edit_design", &json!({}), schema());
        assert!(o.is_error && o.malformed && r == Repair::Exhausted(2));
        let (o, r) = b.apply("check", &json!({}), ToolOutput::err("check failed: range 1 vs 2"));
        assert!(o.is_error && !o.malformed && r == Repair::Free);
        let (o, _) = b.apply("check", &json!({}), ToolOutput::err("check failed: range 1 vs 2"));
        assert!(o.is_error);
        let (o, _) = b.apply("submit", &json!({}), ToolOutput::err("rejected").stop());
        assert!(o.is_error && o.stop, "stop survives the rewrite");
        for k in [ToolErrorKind::Schema, ToolErrorKind::NotFound, ToolErrorKind::Domain, ToolErrorKind::Timeout, ToolErrorKind::Transport] {
            assert!(ToolError::new(k, "m").into_output().is_error, "{k:?}");
        }

        // a batched turn (call_many) is accounted and traced per call
        struct Batch;
        impl Handler for Batch {
            fn call<'a>(&'a mut self, _: &'a str, _: &'a Value) -> BoxFut<'a, ToolOutput> {
                Box::pin(async { unreachable!("batched") })
            }
            fn call_many<'a>(&'a mut self, calls: &'a [(String, Value)]) -> BoxFut<'a, Option<Vec<ToolOutput>>> {
                Box::pin(async move {
                    Some(calls.iter().map(|(n, _)| if n == "delegate" { ToolOutput::ok("aero: done") } else { ToolOutput::malformed(format!("unknown tool '{n}'")) }).collect())
                })
            }
        }
        let tracer = Tracer::for_run(None, "run-3");
        let mut inner = Batch;
        let mut h = Traced::new(Some(tracer.clone()), "orchestrator", "orchestrator", &mut inner);
        let calls = vec![("delegate".to_string(), json!({"role": "aero"})), ("bogus".to_string(), json!({}))];
        let outs = futures_block(h.call_many(&calls)).unwrap();
        assert_eq!(outs.len(), 2);
        assert!(!outs[0].is_error);
        assert!(outs[1].is_error && outs[1].malformed);
        assert_eq!(ToolError::parse(&outs[1].content).unwrap().attempt, Some((1, MAX_TOOL_REPAIRS)));
        let spans = tracer.spans.lock().unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].str("error.type"), Some("not_found"));
        assert_eq!(spans[0].name, "execute_tool delegate");
    }

    fn futures_block<T>(f: BoxFut<'_, T>) -> T {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let xs = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile(&xs, 50.0), 30.0);
        assert_eq!(percentile(&xs, 95.0), 50.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
    }
}
