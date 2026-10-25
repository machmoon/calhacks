//! Offline benchmark of the agent team itself: scripted models (including
//! adversarial ones) against the real harness, judged by pass criteria.
//!
//! No network, no key, no spend: the models are scripts and the domain is a
//! toy (or any `DomainTools` the caller resolves). What is under test is the
//! harness -- does the team reach a passing design, keep the incumbent,
//! refuse a forbidden action, stop on budget, not read a 429 or an error
//! body as approval, and do it under a cost and turn cap.
//!
//! Shape, after inspect_ai (UKGovernmentBEIS/inspect_ai @main):
//!
//! * a [`Scenario`] is an inspect `Sample` (`src/inspect_ai/dataset/_dataset.py`:
//!   `id`, `input`, `target`, `metadata`) with the limits an inspect `Task`
//!   carries per sample (`src/inspect_ai/_eval/task/task.py`: `turn_limit`,
//!   `message_limit`, `cost_limit`) -- here [`Limits`] and the caps in
//!   [`Expect`];
//! * an [`Outcome`] is an inspect `Score` (`src/inspect_ai/scorer/_metric.py`:
//!   `value` CORRECT/INCORRECT, `explanation`, `metadata`), its explanation
//!   the list of criteria that failed;
//! * the pass criteria are SWE-bench's grading report (SWE-bench/SWE-bench
//!   `swebench/harness/grading.py`, `get_eval_report`): one `resolved` bit
//!   from named lists of things that must pass, with the per-item status kept
//!   beside it rather than folded into a score.
//!
//! Scenarios are data (`bench/scenarios/*.json`), one per file, so more can
//! be added without code; the scripted model is routed per role (orchestrator,
//! specialist, reviewer) by the tool set of the request, since the harness
//! never puts the agent's name in the request body.

use crate::client::{ApiError, BoxFut, Provider, Request, Transport};
use crate::harness::{self, RunConfig, StopReason};
use crate::tools::DomainTools;
use crate::{channel, Budget, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use wright_core::{Artifacts, Bom, CatalogOption, CheckId, Domain, Event, Finished, Report, Source};

// -------------------------------------------------------------- scenario

/// One benchmark case: the request, the scripted team, and what must hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    #[serde(default)]
    pub description: String,
    /// `toy` (built in, [`toy`]) or a domain the runner resolves.
    #[serde(default = "default_domain")]
    pub domain: String,
    pub request: String,
    /// The starting spec; `null` lets the orchestrator write it.
    #[serde(default)]
    pub spec: Value,
    #[serde(default)]
    pub limits: Limits,
    pub model: Script,
    #[serde(default)]
    pub expect: Expect,
}

fn default_domain() -> String {
    "toy".into()
}

/// Per-run limits (inspect `Task.turn_limit` / `cost_limit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub max_rounds: u32,
    pub orchestrator_turns: u32,
    pub specialist_turns: u32,
    pub reviewer_turns: u32,
    pub budget_usd: f64,
}

impl Default for Limits {
    fn default() -> Self {
        let d = RunConfig::new("", "", Value::Null);
        Limits { max_rounds: d.max_rounds, orchestrator_turns: d.orchestrator_turns, specialist_turns: d.specialist_turns, reviewer_turns: d.reviewer_turns, budget_usd: 1.25 }
    }
}

/// The scripted team, one step list per role. A role with no script answers
/// every call with a plain text turn (the model "has nothing to do").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Script {
    #[serde(default)]
    pub orchestrator: Vec<Step>,
    #[serde(default)]
    pub specialist: Vec<Step>,
    #[serde(default)]
    pub reviewer: Vec<Step>,
    /// What a role does once its steps are used up: `end_turn` (default, a
    /// text reply with no tool call) or `error` (the transport fails).
    #[serde(default)]
    pub on_exhausted: Exhausted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exhausted {
    #[default]
    EndTurn,
    Error,
}

/// One model reply. Exactly one of `tool`, `text`, `http`, `error_body`,
/// `max_tokens` is the point of the step; `repeat` plays it that many times.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Step {
    /// A tool call (`design`, `edit_design`, `attack`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default)]
    pub input: Value,
    /// Text said before the call, or the whole reply when there is no call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The transport answers with this HTTP status (429, 529, 500).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<u16>,
    /// A 200 whose body is an API error object, not a message.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error_body: bool,
    /// A reply cut off at `max_tokens` with no complete tool call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub max_tokens: bool,
    #[serde(default = "one")]
    pub repeat: u32,
    #[serde(default = "default_in")]
    pub input_tokens: u64,
    #[serde(default = "default_out")]
    pub output_tokens: u64,
}

fn one() -> u32 {
    1
}
fn default_in() -> u64 {
    2000
}
fn default_out() -> u64 {
    150
}

/// Pass criteria. Every field set must hold; unset fields are not judged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Expect {
    /// The run ended with a passing design (checks + reviewer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    /// `passed` | `round_limit` | `budget` | `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    /// Any of these stop reasons is acceptable (when one of several
    /// defensible harness behaviours ends the run).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_in: Vec<String>,
    /// Leaves the final design must have (numbers compared to 1e-9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<Value>,
    /// The final design is the best verified one (the incumbent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_incumbent: Option<bool>,
    /// No leaf of the starting spec changed: nobody rewrote the request.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stated_unchanged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    /// Model calls in total (every role).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
    /// Checks that must pass in the final report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks_pass: Vec<String>,
    /// Tool results that must never have happened (a specialist's
    /// `edit_spec` succeeding, a reviewer's attack on the request confirmed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbid: Vec<CallPattern>,
    /// Tool results that must have happened at least once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require: Vec<CallPattern>,
    /// Every scripted step was consumed before the run ended (`true`), or
    /// the script ran dry at least once (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_exhausted: Option<bool>,
}

/// An `Event::ToolResult` shape: `agent` matches exactly or by a trailing
/// `*` (`specialist:*`), `tool` exactly, `ok` when set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallPattern {
    pub agent: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

impl CallPattern {
    fn matches(&self, agent: &str, tool: &str, ok: bool) -> bool {
        let agent_ok = match self.agent.strip_suffix('*') {
            Some(prefix) => agent.starts_with(prefix),
            None => agent == self.agent,
        };
        agent_ok && tool == self.tool && self.ok.is_none_or(|o| o == ok)
    }
}

impl std::fmt::Display for CallPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.agent, self.tool)?;
        if let Some(ok) = self.ok {
            write!(f, " ok={ok}")?;
        }
        Ok(())
    }
}

/// Every `*.json` in `dir`, by file name.
pub fn load_dir(dir: &Path) -> anyhow::Result<Vec<Scenario>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("reading {}: {e}", p.display()))?;
            serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))
        })
        .collect()
}

// ------------------------------------------------------------- transport

/// Which role a request is for, from the tool it alone has.
fn role_of(body: &Value) -> &'static str {
    let has = |name: &str| body["tools"].as_array().is_some_and(|t| t.iter().any(|x| x["name"] == name));
    if has("verdict") {
        "reviewer"
    } else if has("report") {
        "specialist"
    } else {
        "orchestrator"
    }
}

struct Queues {
    orchestrator: VecDeque<Step>,
    specialist: VecDeque<Step>,
    reviewer: VecDeque<Step>,
    on_exhausted: Exhausted,
    /// Calls that found their role's script empty.
    exhausted: usize,
    next_id: usize,
}

fn expand(steps: &[Step]) -> VecDeque<Step> {
    steps.iter().flat_map(|s| std::iter::repeat_n(s.clone(), s.repeat.max(1) as usize)).collect()
}

/// The scripted team as a [`Transport`]: per-role queues, every request
/// counted, HTTP failures and unusable bodies played on cue.
pub struct ScriptTransport {
    q: Mutex<Queues>,
    pub calls: Mutex<usize>,
}

impl ScriptTransport {
    pub fn new(script: &Script) -> Self {
        ScriptTransport {
            q: Mutex::new(Queues {
                orchestrator: expand(&script.orchestrator),
                specialist: expand(&script.specialist),
                reviewer: expand(&script.reviewer),
                on_exhausted: script.on_exhausted,
                exhausted: 0,
                next_id: 0,
            }),
            calls: Mutex::new(0),
        }
    }

    pub fn exhausted(&self) -> usize {
        self.q.lock().unwrap().exhausted
    }

    fn answer(&self, body: &Value) -> Result<Value, ApiError> {
        let model = body["model"].as_str().unwrap_or("claude-fable-5-1").to_string();
        let mut q = self.q.lock().unwrap();
        let role = role_of(body);
        let queue = match role {
            "reviewer" => &mut q.reviewer,
            "specialist" => &mut q.specialist,
            _ => &mut q.orchestrator,
        };
        let Some(step) = queue.pop_front() else {
            q.exhausted += 1;
            return match q.on_exhausted {
                Exhausted::EndTurn => Ok(crate::fake::reply(&model, vec![crate::fake::text(&format!("({role} script exhausted)"))], 1000, 20)),
                Exhausted::Error => Err(ApiError::ScriptExhausted(q.exhausted)),
            };
        };
        q.next_id += 1;
        let id = format!("b{}", q.next_id);
        drop(q);
        if let Some(status) = step.http {
            return Err(ApiError::Http { status, body: format!("{{\"type\":\"error\",\"error\":{{\"type\":\"scripted\",\"message\":\"bench scripted HTTP {status}\"}}}}") });
        }
        if step.error_body {
            return Ok(json!({"type": "error", "error": {"type": "overloaded_error", "message": "bench scripted error body"}}));
        }
        let mut content = vec![];
        if let Some(t) = &step.text {
            content.push(crate::fake::text(t));
        }
        if step.max_tokens {
            if content.is_empty() {
                content.push(crate::fake::text("I will now"));
            }
            let mut r = crate::fake::reply(&model, content, step.input_tokens, step.output_tokens);
            r["stop_reason"] = json!("max_tokens");
            return Ok(r);
        }
        if let Some(tool) = &step.tool {
            content.push(crate::fake::tool_use(&id, tool, step.input.clone()));
        }
        if content.is_empty() {
            content.push(crate::fake::text("done"));
        }
        Ok(crate::fake::reply(&model, content, step.input_tokens, step.output_tokens))
    }
}

impl Transport for ScriptTransport {
    fn provider(&self) -> Provider {
        Provider::Fake
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            *self.calls.lock().unwrap() += 1;
            self.answer(&req.body)
        })
    }
}

// --------------------------------------------------------------- outcome

/// One scenario's result: an inspect `Score` with the run's numbers beside
/// it (SWE-bench's `tests_status`, kept rather than folded into `resolved`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    /// Every criterion held.
    pub passed: bool,
    /// The criteria that did not hold, each with the number seen.
    pub failures: Vec<String>,
    /// How the harness run itself ended.
    pub run_passed: bool,
    pub stop: String,
    pub rounds: u32,
    pub calls: usize,
    pub spent_usd: f64,
    pub objective: Option<f64>,
    pub script_exhausted: usize,
    /// Every tool result the run emitted, in order: what `forbid` and
    /// `require` were judged on.
    pub tool_results: Vec<ToolResultSeen>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultSeen {
    pub agent: String,
    pub tool: String,
    pub ok: bool,
}

fn stop_name(s: &StopReason) -> &'static str {
    match s {
        StopReason::Passed => "passed",
        StopReason::RoundLimit => "round_limit",
        StopReason::Budget(_) => "budget",
        StopReason::Error(_) => "error",
    }
}

/// `(path, value)` for every leaf of `v`.
fn leaves(v: &Value, at: String, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                leaves(x, if at.is_empty() { k.clone() } else { format!("{at}.{k}") }, out);
            }
        }
        other => out.push((at, other.clone())),
    }
}

fn leaf_eq(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

fn at<'a>(v: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(v, |cur, k| &cur[k])
}

/// Run one scenario against `domain` and judge it.
pub async fn run_scenario(sc: &Scenario, domain: Arc<dyn DomainTools>) -> Outcome {
    let transport = Arc::new(ScriptTransport::new(&sc.model));
    let (tx, mut rx) = channel();
    let run_id = format!("bench-{}", sc.id);
    let client = Client::new(transport.clone(), Budget::new(sc.limits.budget_usd, 1e9, None), tx, run_id.as_str());
    let mut cfg = RunConfig::new(&run_id, &sc.request, sc.spec.clone());
    cfg.max_rounds = sc.limits.max_rounds;
    cfg.orchestrator_turns = sc.limits.orchestrator_turns;
    cfg.specialist_turns = sc.limits.specialist_turns;
    cfg.reviewer_turns = sc.limits.reviewer_turns;
    let out = harness::run(client, domain, cfg).await;

    let mut results: Vec<ToolResultSeen> = vec![];
    while let Ok(e) = rx.try_recv() {
        if let Event::ToolResult { agent, tool, ok, .. } = e {
            results.push(ToolResultSeen { agent, tool, ok });
        }
    }
    let calls = *transport.calls.lock().unwrap();
    let exhausted = transport.exhausted();
    let e = &sc.expect;
    let mut failures = vec![];
    let mut want = |cond: bool, msg: String| {
        if !cond {
            failures.push(msg);
        }
    };
    if let Some(p) = e.passed {
        want(out.passed == p, format!("passed: expected {p}, got {} (stop {:?})", out.passed, out.stop));
    }
    if let Some(s) = &e.stop {
        want(stop_name(&out.stop) == s, format!("stop: expected {s}, got {:?}", out.stop));
    }
    if !e.stop_in.is_empty() {
        want(e.stop_in.iter().any(|s| s == stop_name(&out.stop)), format!("stop_in: expected one of {:?}, got {:?}", e.stop_in, out.stop));
    }
    if let Some(d) = &e.design {
        let mut ls = vec![];
        leaves(d, String::new(), &mut ls);
        let got = out.state.design.clone().unwrap_or(Value::Null);
        for (path, v) in ls {
            let g = at(&got, &path);
            want(leaf_eq(g, &v), format!("design.{path}: expected {v}, got {g}"));
        }
    }
    if let Some(k) = e.kept_incumbent {
        let kept = match (&out.state.best, &out.state.design) {
            (Some(b), Some(d)) => &b.design == d,
            _ => false,
        };
        want(kept == k, format!("kept_incumbent: expected {k}, got {kept}"));
    }
    if e.stated_unchanged {
        let mut ls = vec![];
        leaves(&sc.spec, String::new(), &mut ls);
        for (path, v) in ls {
            let g = at(&out.state.spec, &path);
            want(leaf_eq(g, &v), format!("stated_unchanged: spec.{path} was {v}, now {g}"));
        }
    }
    if let Some(c) = e.max_cost_usd {
        want(out.spent_usd <= c, format!("max_cost_usd: ${c:.4} allowed, spent ${:.4}", out.spent_usd));
    }
    if let Some(n) = e.max_calls {
        want(calls <= n, format!("max_calls: {n} allowed, made {calls}"));
    }
    if let Some(r) = e.max_rounds {
        want(out.rounds <= r, format!("max_rounds: {r} allowed, took {}", out.rounds));
    }
    for name in &e.checks_pass {
        let ok = out.state.report.as_ref().and_then(|r| r.get(name)).is_some_and(|o| o.passed());
        want(ok, format!("checks_pass: {name} is not passing in the final report"));
    }
    for p in &e.forbid {
        let hit = results.iter().find(|r| p.matches(&r.agent, &r.tool, r.ok));
        want(hit.is_none(), format!("forbid: {p} happened{}", hit.map(|r| format!(" ({} -> {} ok={})", r.agent, r.tool, r.ok)).unwrap_or_default()));
    }
    for p in &e.require {
        want(results.iter().any(|r| p.matches(&r.agent, &r.tool, r.ok)), format!("require: {p} never happened"));
    }
    if let Some(x) = e.script_exhausted {
        want((exhausted > 0) == x, format!("script_exhausted: expected {x}, the script ran dry {exhausted} time(s)"));
    }
    Outcome {
        id: sc.id.clone(),
        passed: failures.is_empty(),
        failures,
        run_passed: out.passed,
        stop: stop_name(&out.stop).into(),
        rounds: out.rounds,
        calls,
        spent_usd: out.spent_usd,
        objective: out.objective.map(|(v, _)| v),
        script_exhausted: exhausted,
        tool_results: results,
    }
}

/// Run every scenario; `resolve` maps a domain name to its tools (`toy` is
/// always known). A scenario naming an unknown domain fails with that said.
pub async fn run_all(scenarios: &[Scenario], resolve: &dyn Fn(&str) -> Option<Arc<dyn DomainTools>>) -> Vec<Outcome> {
    let mut out = vec![];
    for sc in scenarios {
        let domain = if sc.domain == "toy" { Some(toy()) } else { resolve(&sc.domain) };
        match domain {
            Some(d) => out.push(run_scenario(sc, d).await),
            None => out.push(Outcome {
                id: sc.id.clone(),
                passed: false,
                failures: vec![format!("no domain `{}` in this build", sc.domain)],
                run_passed: false,
                stop: "not_run".into(),
                rounds: 0,
                calls: 0,
                spent_usd: 0.0,
                objective: None,
                script_exhausted: 0,
                tool_results: vec![],
            }),
        }
    }
    out
}

/// A fixed-width table: one row per scenario, a totals line.
pub fn table(outcomes: &[Outcome]) -> String {
    let w = outcomes.iter().map(|o| o.id.len()).max().unwrap_or(8).max(8);
    let mut s = format!("{:<w$}  {:<6}  {:<11}  {:>6}  {:>5}  {:>8}  why\n", "scenario", "result", "stop", "rounds", "calls", "usd");
    s.push_str(&format!("{}\n", "-".repeat(w + 6 + 11 + 6 + 5 + 8 + 15)));
    for o in outcomes {
        let result = if o.passed { "pass" } else { "FAIL" };
        let why = if o.failures.is_empty() { String::new() } else { o.failures.join("; ") };
        s.push_str(&format!("{:<w$}  {:<6}  {:<11}  {:>6}  {:>5}  {:>8.4}  {}\n", o.id, result, o.stop, o.rounds, o.calls, o.spent_usd, why));
    }
    let passed = outcomes.iter().filter(|o| o.passed).count();
    let usd: f64 = outcomes.iter().map(|o| o.spent_usd).sum();
    s.push_str(&format!("{passed}/{} scenarios pass; ${usd:.4} notional\n", outcomes.len()));
    s
}

/// `bench/results.json`: the outcomes plus the headline counts.
pub fn results_json(outcomes: &[Outcome]) -> Value {
    json!({
        "passed": outcomes.iter().filter(|o| o.passed).count(),
        "total": outcomes.len(),
        "spent_usd": outcomes.iter().map(|o| o.spent_usd).sum::<f64>(),
        "scenarios": outcomes,
    })
}

// ------------------------------------------------------------------- toy

/// The bench's built-in domain: a span that must reach a target, a price
/// under a budget, and a part that must be a real catalog row. `design`
/// undersizes the span (0.9 x target) so the team has something to fix;
/// a specialist that invents a part id fails `part_known`.
pub struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToySpec {
    pub target_mm: f64,
    /// An outside condition the person did not state: a cold day's shrink.
    #[serde(default)]
    pub shrink_mm: f64,
    pub budget_usd: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToyDesign {
    pub span_mm: f64,
    pub price_usd: f64,
    #[serde(default = "default_part")]
    pub part_id: String,
}

fn default_part() -> String {
    "P-100".into()
}

const PARTS: &[(&str, &str, f64, bool)] = &[("P-100", "100 mm extrusion", 10.0, true), ("P-200", "200 mm extrusion", 30.0, true), ("P-300", "300 mm extrusion", 55.0, false)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToyCheck {
    Span,
    Budget,
    PartKnown,
}

impl CheckId for ToyCheck {
    const ALL: &'static [Self] = &[ToyCheck::Span, ToyCheck::Budget, ToyCheck::PartKnown];
    fn name(self) -> &'static str {
        match self {
            ToyCheck::Span => "span",
            ToyCheck::Budget => "budget",
            ToyCheck::PartKnown => "part_known",
        }
    }
    fn describe(self) -> &'static str {
        match self {
            ToyCheck::Span => "span reaches the target",
            ToyCheck::Budget => "price within budget",
            ToyCheck::PartKnown => "the part is a catalog row",
        }
    }
}

impl Domain for Toy {
    const NAME: &'static str = "toy";
    type Spec = ToySpec;
    type Design = ToyDesign;
    type Check = ToyCheck;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"target_mm": {"type": "number"}, "budget_usd": {"type": "number"}, "shrink_mm": {"type": "number", "minimum": -50, "maximum": 50}}})
    }
    fn design(s: &ToySpec) -> anyhow::Result<ToyDesign> {
        Ok(ToyDesign { span_mm: 0.9 * s.target_mm, price_usd: 10.0, part_id: "P-100".into() })
    }
    fn check(s: &ToySpec, d: &ToyDesign) -> Finished {
        let mut r = Report::<ToyCheck>::new();
        r.margin(ToyCheck::Span, d.span_mm - s.shrink_mm - s.target_mm, "mm", Source::Geometry, "span - shrink - target");
        r.margin(ToyCheck::Budget, s.budget_usd - d.price_usd, "USD", Source::Requirement, "budget - price");
        let known = PARTS.iter().any(|p| p.0 == d.part_id);
        r.margin(ToyCheck::PartKnown, if known { 1.0 } else { -1.0 }, "", Source::Rating, format!("part {} {}", d.part_id, if known { "is in the catalog" } else { "is not in the catalog" }));
        r.finish()
    }
    fn bom(_: &ToyDesign) -> Bom {
        Bom::default()
    }
    fn build(_: &ToyDesign, _: &Path) -> anyhow::Result<Artifacts> {
        Ok(Artifacts::default())
    }
    fn objective(d: &ToyDesign) -> (f64, &'static str) {
        (d.span_mm, "span mm")
    }
    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        vec![("part_id", "the extrusion the span is cut from")]
    }
    fn options(_: &ToyDesign, path: &str, query: Option<&str>) -> Vec<CatalogOption> {
        if path != "part_id" {
            return vec![];
        }
        let q = query.unwrap_or("").to_ascii_lowercase();
        PARTS
            .iter()
            .filter(|p| q.is_empty() || p.0.to_ascii_lowercase().contains(&q) || p.1.to_ascii_lowercase().contains(&q))
            .map(|p| CatalogOption { id: p.0.into(), label: p.1.into(), specs: Default::default(), price_usd: Some(p.2), in_stock: Some(p.3), url: String::new() })
            .collect()
    }
}

/// The built-in toy domain as tools.
pub fn toy() -> Arc<dyn DomainTools> {
    crate::tools::tools::<Toy>()
}
