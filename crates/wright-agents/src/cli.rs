//! The signed-in `claude` binary as a [`Transport`]: a run bills Pat's
//! claude.ai subscription instead of the weekend's $50 of API credits.
//!
//! Prior art: the Python baseline's `cli` backend (its
//! `engine/silkscreen/agents/claude_cli.py`), which drives the binary through the Claude Agent
//! SDK and whose flag mapping is the SDK's own
//! (`claude_agent_sdk/_internal/transport/subprocess_cli.py::
//! SubprocessCLITransport._build_command`, 0.2.171): `--system-prompt`
//! replaces Claude Code's prompt, `--tools ""` disables the built-in tools,
//! `--setting-sources ""` keeps the machine's CLAUDE.md and settings out,
//! `CLAUDE_CODE_MAX_OUTPUT_TOKENS` stands in for `max_tokens`, and
//! `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` are blanked in the child so
//! a stale key cannot outrank the login. Added here: `--strict-mcp-config`
//! (measured 2026-10-24: without it the user's claude.ai MCP connectors
//! load and the prompt grows from ~1.3K to ~49K tokens).
//!
//! What differs from the API, and why it is still the same contract:
//!
//! * The baseline sends one text turn with no tools. Wright's agents act only by
//!   tool calls, and `claude -p` does not take client tool definitions, so
//!   each request becomes **one stateless CLI turn**: the request's tools are
//!   listed in the system prompt as host actions, the transcript so far is
//!   rendered into one user message (images pass through as image blocks via
//!   `--input-format stream-json`), and `--json-schema` makes the answer
//!   `{text, tool_calls: [{name, input_json}]}`. That is mapped back to a
//!   Messages response with `tool_use` blocks, so the harness loop, the
//!   malformed-input requery and the checks run unchanged.
//! * No prompt caching control and no exact cost: `usage` is whatever the
//!   CLI reports, priced by the budget as if it were API traffic so the run
//!   limit still stops a runaway loop; the global ledger is not charged
//!   ([`Provider::bills_credits`]).
//! * `strict` tool schemas are not enforced by the CLI; the harness
//!   validates every input itself (it does for every transport).

use crate::client::{ApiError, BoxFut, Provider, Request, Transport};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const PREAMBLE: &str = "You are one turn of a tool-using agent that a host program runs. \
You have no tools of your own in this session. The host lists ACTIONS below; to take one, put it \
in tool_calls as {name, input_json} where input_json is a JSON object (as a string) that matches \
the action's input schema exactly. The host runs every call and shows you the results on the next \
turn. Use only the listed action names. Never try to call an action as a tool directly: it is not \
a tool in this session and the call fails with 'No such tool available'; the only way to act is to \
list it in tool_calls of your structured output (several at once is fine). Leave tool_calls empty \
only when you are finished, and put your words in text. Reply only through the structured output.\n\
Mechanically: the one tool you have is StructuredOutput. Every action goes INSIDE its tool_calls \
field, e.g. StructuredOutput({\"text\": \"looking up boom tubes\", \"tool_calls\": [{\"name\": \"catalog\", \
\"input_json\": \"{\\\"path\\\": \\\"structure.boom_tube\\\", \\\"query\\\": \\\"\\\"}\"}]}). Do not call catalog, check \
or any other action name as a tool of its own.\n\
A result marked ERROR in the transcript is the host's answer to that action (a rejected input, a \
path that does not exist, a failing check report), not a sign that the action is unavailable: \
read the message, fix the input, and list the action again in tool_calls.";

/// The action names a request offers (its `tools`), in order.
pub fn action_names(body: &Value) -> Vec<String> {
    body["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(str::to_string)).collect()
}

/// The structured-output schema for one request. `name` is an enum of
/// exactly this request's actions (this role's tools): a free string let
/// agents in a live run (demo/aircraft-markup, 10/24) believe an action
/// such as `catalog` "is not a tool in this session" and try other routes,
/// and nothing tied the answer to the role's tool list.
pub fn answer_schema(body: &Value) -> Value {
    let names = action_names(body);
    let name = if names.is_empty() { json!({"type": "string"}) } else { json!({"type": "string", "enum": names}) };
    json!({
        "type": "object",
        "properties": {
            "text": {"type": "string"},
            "tool_calls": {"type": "array", "items": {
                "type": "object",
                "properties": {"name": name, "input_json": {"type": "string"}},
                "required": ["name", "input_json"],
                "additionalProperties": false
            }}
        },
        "required": ["text", "tool_calls"],
        "additionalProperties": false
    })
}

pub struct CliTransport {
    pub binary: String,
    pub timeout: Duration,
    next_id: AtomicU64,
    /// One persistent CLI session per conversation (`--session-id` on the
    /// first call, `--resume` after), so only the new turn is sent and the
    /// transcript stays in the CLI's prompt cache. `WRIGHT_CLI_SESSIONS=0`
    /// goes back to one stateless call per turn.
    pub persist: bool,
    /// Prefix hash of (model, effort, system, schema, messages so far
    /// including our last reply) -> (session id, messages it covers).
    sessions: std::sync::Mutex<std::collections::HashMap<String, (String, usize)>>,
}

/// Where the CLI keeps the sessions it writes (`~/.claude/projects/<cwd>/`
/// is keyed by the working directory): a dedicated dir, so Wright's
/// sessions never mix with a person's own.
pub fn session_cwd() -> std::path::PathBuf {
    let d = std::env::temp_dir().join("wright-cli-sessions");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// A random RFC 4122 v4 UUID (the CLI's `--session-id` must be one).
fn new_uuid(n: u64) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = wright_core::cache::Hasher::new();
    h.update(&t.to_le_bytes()).update(&std::process::id().to_le_bytes()).update(&n.to_le_bytes());
    let x = h.hex().into_bytes();
    let mut c: Vec<char> = x.iter().map(|b| *b as char).collect();
    c[12] = '4';
    c[16] = ['8', '9', 'a', 'b'][(c[16].to_digit(16).unwrap_or(0) % 4) as usize];
    let s: String = c.into_iter().collect();
    format!("{}-{}-{}-{}-{}", &s[0..8], &s[8..12], &s[12..16], &s[16..20], &s[20..32])
}

/// The prefix hashes of a request: `out[k]` covers `messages[..k]` (k >= 1).
fn prefix_keys(body: &Value, messages: &[Value]) -> Vec<String> {
    let mut h = wright_core::cache::Hasher::new();
    h.json(&body["model"]).json(&body["output_config"]).json(&body["max_tokens"]).str(&system_prompt(body)).json(&answer_schema(body));
    let mut out = vec![String::new()];
    for m in messages {
        h.json(&m["role"]).json(&m["content"]);
        out.push(h.hex());
    }
    out
}

impl CliTransport {
    /// `WRIGHT_CLAUDE_BIN` (default `claude`), `WRIGHT_CLI_TIMEOUT_S`
    /// (default 300).
    pub fn from_env() -> Self {
        CliTransport {
            binary: std::env::var("WRIGHT_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            timeout: Duration::from_secs(
                std::env::var("WRIGHT_CLI_TIMEOUT_S").ok().and_then(|v| v.parse().ok()).unwrap_or(300),
            ),
            next_id: AtomicU64::new(1),
            persist: std::env::var("WRIGHT_CLI_SESSIONS").map(|v| v != "0").unwrap_or(true),
            sessions: Default::default(),
        }
    }

    /// The argv for one call in a persistent session: `--session-id` to
    /// start it, `--resume` to continue it (both persist; no
    /// `--no-session-persistence`).
    pub fn session_args(body: &Value, session: &str, resume: bool) -> Vec<String> {
        let mut a = Self::args(body);
        a.retain(|x| x != "--no-session-persistence");
        a.extend([if resume { "--resume" } else { "--session-id" }.to_string(), session.to_string()]);
        a
    }

    /// The session map, for the checkpoint (`crate::checkpoint`).
    pub fn export_sessions(&self) -> Value {
        let m = self.sessions.lock().unwrap();
        Value::Object(m.iter().map(|(k, (sid, n))| (k.clone(), json!([sid, n]))).collect())
    }

    pub fn import_sessions(&self, v: &Value) {
        let mut m = self.sessions.lock().unwrap();
        for (k, e) in v.as_object().into_iter().flatten() {
            if let (Some(sid), Some(n)) = (e[0].as_str(), e[1].as_u64()) {
                m.insert(k.clone(), (sid.to_string(), n as usize));
            }
        }
    }

    /// One CLI process: write `content` as the user line, read the result.
    async fn call(&self, args: Vec<String>, content: Vec<Value>, body: &Value) -> Result<(Value, String), ApiError> {
        let line = json!({"type": "user", "message": {"role": "user", "content": content}});
        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(args)
            .current_dir(if self.persist { session_cwd() } else { std::env::temp_dir() })
            .env("ANTHROPIC_API_KEY", "")
            .env("ANTHROPIC_AUTH_TOKEN", "")
            .env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", body["max_tokens"].as_u64().unwrap_or(16_000).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| ApiError::Cli(format!("spawn {}: {e}", self.binary)))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin.write_all(format!("{line}\n").as_bytes()).await.map_err(|e| ApiError::Cli(e.to_string()))?;
        drop(stdin);
        let out = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| ApiError::Cli(format!("timed out after {:?}", self.timeout)))?
            .map_err(|e| ApiError::Cli(e.to_string()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let result = stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["type"] == "result")
            .ok_or_else(|| {
                ApiError::Cli(format!(
                    "no result line (exit {:?}): {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).chars().take(400).collect::<String>()
                ))
            })?;
        if let Some(s) = result["api_error_status"].as_u64() {
            return Err(ApiError::Http { status: s as u16, body: result["result"].to_string() });
        }
        Ok((result, stdout))
    }

    /// The argv after the binary, for a request body.
    pub fn args(body: &Value) -> Vec<String> {
        let mut a: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(m) = body["model"].as_str() {
            a.extend(["--model".into(), m.into()]);
        }
        if let Some(e) = body["output_config"]["effort"].as_str() {
            a.extend(["--effort".into(), e.into()]);
        }
        a.extend(["--system-prompt".into(), system_prompt(body)]);
        a.extend(["--json-schema".into(), answer_schema(body).to_string()]);
        a
    }

    /// Map the CLI's final `result` line to a Messages API response.
    pub fn response(&self, model: &str, result: &Value) -> Result<Value, ApiError> {
        self.response_with(model, result, &[])
    }

    /// Like [`Self::response`], plus host actions the model tried to call
    /// as native tools inside the CLI turn (`native`, from
    /// [`native_action_calls`]). Those calls fail inside the CLI ("No such
    /// tool available") and the model then often gives up on the action;
    /// the host runs them instead, so the action is never lost.
    pub fn response_with(&self, model: &str, result: &Value, native: &[Value]) -> Result<Value, ApiError> {
        if result["is_error"].as_bool().unwrap_or(false) || result["subtype"] != "success" {
            return Err(ApiError::Cli(format!(
                "{} {}",
                result["subtype"].as_str().unwrap_or("error"),
                result["result"].as_str().unwrap_or("")
            )));
        }
        let out = &result["structured_output"];
        let mut content = vec![];
        if let Some(t) = out["text"].as_str().filter(|t| !t.trim().is_empty()) {
            content.push(json!({"type": "text", "text": t}));
        }
        let mut calls: Vec<Value> = out["tool_calls"].as_array().cloned().unwrap_or_default();
        // Native calls are a fallback for a turn whose answer names no
        // action at all (the model gave up after "No such tool"). When the
        // answer has actions, it is the model's final word: a native call
        // it then restated (or replaced) must not run again (review 5, F5:
        // the same attack ran twice because the two copies differed only
        // in JSON formatting). Duplicates are judged on the parsed input.
        if calls.is_empty() {
            let parsed = |c: &Value| serde_json::from_str::<Value>(c["input_json"].as_str().unwrap_or("")).ok();
            for n in native {
                if !calls.iter().any(|c| c["name"] == n["name"] && parsed(c) == parsed(n)) {
                    calls.push(n.clone());
                }
            }
        }
        for call in &calls {
            let raw = call["input_json"].as_str().unwrap_or("");
            // An unparseable input still becomes a tool_use, so the harness
            // answers it with an error and requeries -- same as the API path.
            let input = serde_json::from_str::<Value>(raw)
                .ok()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({"_unparsed_input": raw}));
            let id = format!("toolu_cli_{}", self.next_id.fetch_add(1, Ordering::Relaxed));
            content.push(json!({"type": "tool_use", "id": id, "name": call["name"], "input": input}));
        }
        let calls = content.iter().any(|b| b["type"] == "tool_use");
        Ok(json!({
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": content,
            "stop_reason": if calls { "tool_use" } else { "end_turn" },
            "usage": result["usage"].clone(),
        }))
    }
}

/// Host actions the model called as native tools in a CLI turn's
/// stream-json output, as `{name, input_json}` (the structured output's
/// shape). Only names the request offers count; `StructuredOutput` is the
/// CLI's own answer tool. A native call's input is either the action's
/// input object or `{"input_json": "..."}` copied from the answer schema.
pub fn native_action_calls(stdout: &str, body: &Value) -> Vec<Value> {
    let names = action_names(body);
    let mut out = vec![];
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["type"] != "assistant" {
            continue;
        }
        for b in v["message"]["content"].as_array().into_iter().flatten() {
            let Some(name) = b["name"].as_str() else { continue };
            if b["type"] != "tool_use" || !names.iter().any(|n| n == name) {
                continue;
            }
            let input = &b["input"];
            let input_json = match input.get("input_json").and_then(Value::as_str) {
                Some(s) if input.as_object().is_some_and(|m| m.len() == 1) => s.to_string(),
                _ => input.to_string(),
            };
            out.push(json!({"name": name, "input_json": input_json}));
        }
    }
    out
}

fn system_prompt(body: &Value) -> String {
    let system = match &body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n\n"),
        _ => String::new(),
    };
    let names = action_names(body);
    let mut s = format!(
        "{PREAMBLE}\n\n{system}\n\n# ACTIONS\nYour actions this turn (and only these): {}. Each is taken by listing it in tool_calls.\n",
        names.join(", ")
    );
    for t in body["tools"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "\n## {}\n{}\ninput schema: {}\n",
            t["name"].as_str().unwrap_or(""),
            t["description"].as_str().unwrap_or(""),
            t["input_schema"]
        ));
    }
    s
}

/// The transcript as one user message's content: text, with any image
/// blocks kept as image blocks in order.
pub fn transcript(body: &Value) -> Vec<Value> {
    let messages: Vec<Value> = body["messages"].as_array().cloned().unwrap_or_default();
    transcript_of(&messages, "Transcript so far (the last USER entry is the current turn):\n")
}

/// Only the turns after what a resumed session already holds.
pub fn transcript_delta(messages: &[Value]) -> Vec<Value> {
    transcript_of(messages, "The conversation continues (the host's answers to your last actions, then the current turn):\n")
}

fn transcript_of(messages: &[Value], header: &str) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let mut text = String::from(header);
    let flush = |text: &mut String, out: &mut Vec<Value>| {
        if !text.is_empty() {
            out.push(json!({"type": "text", "text": std::mem::take(text)}));
        }
    };
    for m in messages {
        let role = m["role"].as_str().unwrap_or("user").to_uppercase();
        text.push_str(&format!("\n=== {role} ===\n"));
        let blocks = match &m["content"] {
            Value::String(s) => vec![json!({"type": "text", "text": s})],
            Value::Array(a) => a.clone(),
            _ => vec![],
        };
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => {
                    text.push_str(b["text"].as_str().unwrap_or(""));
                    text.push('\n');
                }
                Some("tool_use") => {
                    text.push_str(&format!("[requested host action {} via tool_calls, id={}] input_json={}\n", b["name"].as_str().unwrap_or(""), b["id"].as_str().unwrap_or(""), b["input"]));
                }
                Some("tool_result") => {
                    let body = match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n"),
                        _ => String::new(),
                    };
                    let err = if b["is_error"].as_bool().unwrap_or(false) { " ERROR" } else { "" };
                    text.push_str(&format!("[result id={}{err}] {body}\n", b["tool_use_id"].as_str().unwrap_or("")));
                }
                Some("image") => {
                    text.push_str("[image attached below]\n");
                    flush(&mut text, &mut out);
                    out.push(b.clone());
                }
                _ => {} // thinking blocks: the CLI path produces none
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

impl Transport for CliTransport {
    fn provider(&self) -> Provider {
        Provider::ClaudeCli
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            let body = &req.body;
            let model = body["model"].as_str().unwrap_or("").to_string();
            if !self.persist {
                let (result, stdout) = self.call(Self::args(body), transcript(body), body).await?;
                return self.response_with(&model, &result, &native_action_calls(&stdout, body));
            }
            let messages: Vec<Value> = body["messages"].as_array().cloned().unwrap_or_default();
            let keys = prefix_keys(body, &messages);
            // the longest earlier prefix a session already holds
            let found = {
                let m = self.sessions.lock().unwrap();
                (1..messages.len()).rev().find_map(|k| m.get(&keys[k]).filter(|(_, n)| *n == k).map(|(sid, _)| (sid.clone(), k)))
            };
            let attempt = match &found {
                Some((sid, k)) => self.call(Self::session_args(body, sid, true), transcript_delta(&messages[*k..]), body).await.and_then(|(r, out)| {
                    // a resumed session that errors falls back to a fresh one
                    self.response_with(&model, &r, &native_action_calls(&out, body)).map(|resp| (resp, sid.clone()))
                }),
                None => Err(ApiError::Cli("no session".into())),
            };
            let (resp, sid) = match attempt {
                Ok(x) => x,
                Err(e) => {
                    if let Some((_, k)) = &found {
                        self.sessions.lock().unwrap().remove(&keys[*k]);
                        // a rate limit or an API error is the answer, not a broken session
                        if matches!(e, ApiError::Http { .. }) {
                            return Err(e);
                        }
                    }
                    let sid = new_uuid(self.next_id.fetch_add(1, Ordering::Relaxed));
                    let (r, out) = self.call(Self::session_args(body, &sid, false), transcript(body), body).await?;
                    (self.response_with(&model, &r, &native_action_calls(&out, body))?, sid)
                }
            };
            // the session now holds these messages plus this reply
            let mut held = messages.clone();
            held.push(json!({"role": "assistant", "content": resp["content"]}));
            let k2 = prefix_keys(body, &held);
            let mut m = self.sessions.lock().unwrap();
            if let Some((_, k)) = &found {
                m.remove(&keys[*k]);
            }
            m.insert(k2[held.len()].clone(), (sid, held.len()));
            Ok(resp)
        })
    }
    fn export_sessions(&self) -> Value {
        CliTransport::export_sessions(self)
    }
    fn import_sessions(&self, v: &Value) {
        CliTransport::import_sessions(self, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_disables_tools_settings_and_mcp() {
        let a = CliTransport::args(&json!({
            "model": "claude-haiku-5-5", "output_config": {"effort": "low"},
            "system": [{"type": "text", "text": "SYS"}],
            "tools": [{"name": "check", "description": "run checks", "input_schema": {"type": "object"}}]
        }));
        let i = a.iter().position(|s| s == "--tools").unwrap();
        assert_eq!(a[i + 1], "");
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        let sp = &a[a.iter().position(|s| s == "--system-prompt").unwrap() + 1];
        assert!(sp.contains("SYS") && sp.contains("## check"));
    }

    #[test]
    fn result_maps_to_tool_use_and_bad_input_survives() {
        let t = CliTransport::from_env();
        let r = t
            .response(
                "claude-haiku-5-5",
                &json!({"type": "result", "subtype": "success", "is_error": false,
                    "usage": {"input_tokens": 5, "output_tokens": 7},
                    "structured_output": {"text": "running", "tool_calls": [
                        {"name": "check", "input_json": "{}"},
                        {"name": "edit_spec", "input_json": "{not json"}]}}),
            )
            .unwrap();
        assert_eq!(r["stop_reason"], "tool_use");
        assert_eq!(r["content"][1]["name"], "check");
        assert_eq!(r["content"][2]["input"]["_unparsed_input"], "{not json");
    }

    /// Reproduced with the real CLI (10/24): with `--json-schema` the
    /// model's one tool is StructuredOutput, yet it called `catalog` as a
    /// native tool, got "No such tool available", and answered with no
    /// tool_calls. The host now runs such a call itself.
    #[test]
    fn a_native_call_to_a_host_action_is_run_not_lost() {
        let body = json!({"tools": [{"name": "catalog"}, {"name": "check"}]});
        let stdout = [
            json!({"type": "system", "tools": ["StructuredOutput"]}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t1", "name": "catalog",
                "input": {"input_json": "{\"path\": \"structure.boom_tube\", \"query\": \"\"}"}}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "is_error": true,
                "content": "<tool_use_error>Error: No such tool available: catalog</tool_use_error>"}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t2", "name": "check", "input": {}}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t3", "name": "Bash", "input": {"command": "ls"}}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t4", "name": "StructuredOutput",
                "input": {"text": "the catalog tool isn't available", "tool_calls": []}}]}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let native = native_action_calls(&stdout, &body);
        assert_eq!(native.len(), 2, "{native:?}");
        let t = CliTransport::from_env();
        let result = json!({"type": "result", "subtype": "success", "is_error": false, "usage": {},
            "structured_output": {"text": "the catalog tool isn't available", "tool_calls": [{"name": "check", "input_json": "{}"}]}});
        // the answer names an action: it is the final word, natives do not run
        let r = t.response_with("m", &result, &native).unwrap();
        let uses: Vec<&Value> = r["content"].as_array().unwrap().iter().filter(|b| b["type"] == "tool_use").collect();
        assert_eq!(uses.len(), 1, "{uses:?}");
        assert_eq!(uses[0]["name"], "check");
        // the answer gave up ("isn't available", no actions): natives run
        let gave_up = json!({"type": "result", "subtype": "success", "is_error": false, "usage": {},
            "structured_output": {"text": "the catalog tool isn't available", "tool_calls": []}});
        let r = t.response_with("m", &gave_up, &native).unwrap();
        let uses: Vec<&Value> = r["content"].as_array().unwrap().iter().filter(|b| b["type"] == "tool_use").collect();
        assert_eq!(uses.len(), 2, "{uses:?}");
        assert_eq!(uses[0]["name"], "catalog");
        assert_eq!(uses[0]["input"]["path"], "structure.boom_tube");
        assert_eq!(uses[1]["name"], "check");
        assert_eq!(r["stop_reason"], "tool_use");
    }

    /// Review 5, F5: one attack, called natively and restated
    /// pretty-printed in the answer, ran twice (raw-text dedupe); a native
    /// edit the answer replaced ran after the replacement.
    #[test]
    fn a_restated_or_replaced_native_call_does_not_run_again() {
        let body = json!({"tools": [{"name": "attack"}, {"name": "edit_design"}]});
        let attack_in = json!({"claim": "1000 m field", "spec_patch_json": "{\"altitude_m\":1000}", "expect_failing_check": "stall_speed"});
        let line = |name: &str, input: &Value| json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t", "name": name, "input": input}]}}).to_string();
        let t = CliTransport::from_env();
        let uses = |r: Value| r["content"].as_array().unwrap().iter().filter(|b| b["type"] == "tool_use").cloned().collect::<Vec<_>>();

        let native = native_action_calls(&line("attack", &attack_in), &body);
        let restated = json!({"type": "result", "subtype": "success", "is_error": false, "usage": {},
            "structured_output": {"text": "", "tool_calls": [{"name": "attack", "input_json": serde_json::to_string_pretty(&attack_in).unwrap()}]}});
        assert_eq!(uses(t.response_with("m", &restated, &native).unwrap()).len(), 1);

        let native = native_action_calls(&line("edit_design", &json!({"patch_json": "{\"span_mm\":200}", "why": "first idea"})), &body);
        let replaced = json!({"type": "result", "subtype": "success", "is_error": false, "usage": {},
            "structured_output": {"text": "", "tool_calls": [{"name": "edit_design", "input_json": "{\"patch_json\": \"{\\\"span_mm\\\":120}\", \"why\": \"better\"}"}]}});
        let u = uses(t.response_with("m", &replaced, &native).unwrap());
        assert_eq!(u.len(), 1);
        assert_eq!(u[0]["input"]["patch_json"], "{\"span_mm\":120}");

        // two native copies of one call, differently formatted: once
        let twice = [line("attack", &attack_in), line("attack", &json!({"input_json": serde_json::to_string_pretty(&attack_in).unwrap()}))].join("\n");
        let native = native_action_calls(&twice, &body);
        let empty = json!({"type": "result", "subtype": "success", "is_error": false, "usage": {}, "structured_output": {"text": "", "tool_calls": []}});
        assert_eq!(uses(t.response_with("m", &empty, &native).unwrap()).len(), 1);
    }

    #[test]
    fn transcript_keeps_images_in_order() {
        let c = transcript(&json!({"messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "look"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}},
                {"type": "text", "text": "fix it"}]}]}));
        assert_eq!(c.len(), 3);
        assert_eq!(c[1]["type"], "image");
    }

    /// Persistent sessions with a stand-in `claude` (a shell script that
    /// logs its argv and stdin and answers one result line): the first call
    /// starts a session with the whole transcript, the next resumes it with
    /// only the new turn, a failed resume falls back to a fresh session,
    /// and the map round-trips through export/import (the checkpoint).
    #[cfg(unix)]
    #[tokio::test]
    async fn sessions_start_resume_and_fall_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("wright-cli-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let bin = dir.join("claude");
        std::fs::write(&bin, format!(
            "#!/bin/sh\necho \"ARGS $*\" >> {log}\ncat >> {log}\necho >> {log}\ncase \"$*\" in *--resume\\ dead*) echo '{{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"no conversation\"}}'; exit 1;; esac\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}},\"structured_output\":{{\"text\":\"ok\",\"tool_calls\":[{{\"name\":\"check\",\"input_json\":\"{{}}\"}}]}}}}'\n",
            log = log.display()
        )).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t = CliTransport::from_env();
        t.binary = bin.to_string_lossy().into_owned();
        t.persist = true;
        let mut body = json!({"model": "m", "system": "SYS", "tools": [{"name": "check", "description": "", "input_schema": {}}],
            "messages": [{"role": "user", "content": [{"type": "text", "text": "FIRST TURN"}]}]});
        let r1 = t.send(&Request { body: body.clone(), betas: vec![] }).await.unwrap();
        let tid = r1["content"][1]["id"].as_str().unwrap().to_string();
        body["messages"].as_array_mut().unwrap().push(json!({"role": "assistant", "content": r1["content"]}));
        body["messages"].as_array_mut().unwrap().push(json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "content": "SECOND TURN RESULT", "is_error": false}]}));
        let r2 = t.send(&Request { body: body.clone(), betas: vec![] }).await.unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        let calls: Vec<&str> = text.split("ARGS ").skip(1).collect();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].contains("--session-id") && !calls[0].contains("--no-session-persistence") && calls[0].contains("FIRST TURN"));
        let sid = calls[0].split("--session-id ").nth(1).unwrap().split_whitespace().next().unwrap().to_string();
        assert!(calls[1].contains(&format!("--resume {sid}")), "{}", calls[1]);
        assert!(calls[1].contains("SECOND TURN RESULT") && !calls[1].contains("FIRST TURN"), "only the new turn: {}", calls[1]);

        // checkpoint round trip, then a resume that fails -> fresh session with the whole transcript
        let saved = t.export_sessions();
        let mut t2 = CliTransport::from_env();
        t2.binary = t.binary.clone();
        t2.persist = true;
        let dead = Value::Object(saved.as_object().unwrap().iter().map(|(k, e)| (k.clone(), json!(["dead", e[1]]))).collect());
        t2.import_sessions(&dead);
        body["messages"].as_array_mut().unwrap().push(json!({"role": "assistant", "content": r2["content"]}));
        body["messages"].as_array_mut().unwrap().push(json!({"role": "user", "content": [{"type": "text", "text": "THIRD"}]}));
        let r3 = t2.send(&Request { body: body.clone(), betas: vec![] }).await;
        assert!(r3.is_ok(), "{r3:?}");
        let text = std::fs::read_to_string(&log).unwrap();
        let last: Vec<&str> = text.split("ARGS ").skip(1).collect();
        assert!(last.iter().rev().take(2).any(|c| c.contains("--resume dead")), "tried the saved session");
        assert!(last.last().unwrap().contains("--session-id") && last.last().unwrap().contains("FIRST TURN"), "fell back to a fresh session");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
