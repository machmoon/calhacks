//! Offline runs of the whole harness against a scripted transport and a toy
//! domain: no network, no key, no spend.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, text, tool_use, ScriptedTransport};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig, StopReason};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, Report, Source};

// A one-dimension "vehicle": a span that must reach a target length, and a
// price that must fit a budget. design() deliberately undersizes the span
// (0.9 x target) so the agents have something to fix.
struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    target_mm: f64,
    /// An outside condition the person did not state: thermal shrink of
    /// the span on a cold day. The reviewer's attacks vary this, never
    /// the person's target (`DesignState::stated` is locked).
    #[serde(default)]
    shrink_mm: f64,
    budget_usd: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Design {
    span_mm: f64,
    price_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Span,
    Budget,
}

impl CheckId for C {
    const ALL: &'static [Self] = &[C::Span, C::Budget];
    fn name(self) -> &'static str {
        match self {
            C::Span => "span",
            C::Budget => "budget",
        }
    }
    fn describe(self) -> &'static str {
        match self {
            C::Span => "span reaches the target",
            C::Budget => "price within budget",
        }
    }
}

impl Domain for Toy {
    const NAME: &'static str = "toy";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"target_mm": {"type": "number"}, "budget_usd": {"type": "number"}, "shrink_mm": {"type": "number"}}})
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        Ok(Design { span_mm: 0.9 * s.target_mm, price_usd: 10.0 })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Span, d.span_mm - s.shrink_mm - s.target_mm, "mm", Source::Geometry, "span - shrink - target");
        r.margin(C::Budget, s.budget_usd - d.price_usd, "USD", Source::Requirement, "budget - price");
        r.finish()
    }
    fn bom(_: &Design) -> Bom {
        Bom::default()
    }
    fn build(_: &Design, _: &Path) -> anyhow::Result<Artifacts> {
        Ok(Artifacts::default())
    }
    fn objective(d: &Design) -> (f64, &'static str) {
        (d.span_mm, "span mm")
    }
}

const SONNET: &str = "claude-sonnet-5-5";
const OPUS: &str = "claude-opus-5-5";

fn spec() -> Value {
    json!({"target_mm": 100.0, "budget_usd": 50.0})
}

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("wright-agents-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn setup(transport: Arc<ScriptedTransport>, budget: Budget) -> (Client, tokio::sync::broadcast::Receiver<Event>) {
    let (tx, rx) = channel();
    (Client::new(transport, budget, tx, "run-1"), rx)
}

fn events(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<Event> {
    let mut v = vec![];
    while let Ok(e) = rx.try_recv() {
        v.push(e);
    }
    v
}

fn last_user(req: &Value) -> Vec<Value> {
    let msgs = req["messages"].as_array().unwrap();
    msgs.last().unwrap()["content"].as_array().unwrap().clone()
}

#[tokio::test]
async fn full_loop_fixes_the_design_survives_an_attack_and_passes() {
    let regressions = tmp("regressions.jsonl");
    let script = vec![
        // Round 1.
        reply(SONNET, vec![text("Designing from the spec."), tool_use("o1", "design", json!({}))], 3000, 200),
        reply(SONNET, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "span fails by -10 mm; fix it"}))], 3500, 150),
        reply(OPUS, vec![tool_use("s1", "edit_design", json!({"patch_json": "{\"span_mm\": 105}", "why": "reach target"}))], 2000, 100),
        reply(OPUS, vec![tool_use("s2", "check", json!({}))], 2100, 50),
        reply(OPUS, vec![tool_use("s3", "report", json!({"summary": "span 90 -> 105 mm; span margin -10 -> +5 mm; all checks pass"}))], 2200, 80),
        reply(SONNET, vec![tool_use("o3", "submit", json!({"summary": "span 105 mm, all checks pass"}))], 4000, 100),
        // Reviewer: one attack the host refutes, one it confirms.
        reply(SONNET, vec![tool_use("r1", "attack", json!({"claim": "a cold day shrinks it 2 mm", "spec_patch_json": "{\"shrink_mm\": 2}", "expect_failing_check": "span"}))], 3000, 300),
        reply(SONNET, vec![tool_use("r2", "attack", json!({"claim": "a colder day shrinks it 8 mm", "spec_patch_json": "{\"shrink_mm\": 8}", "expect_failing_check": "span"}))], 3200, 300),
        reply(SONNET, vec![tool_use("r3", "verdict", json!({"summary": "span has no margin for a 108 mm gap"}))], 3300, 100),
        // Round 2.
        reply(SONNET, vec![tool_use("o4", "edit_design", json!({"patch_json": "{\"span_mm\": 110}", "why": "cover r1-1"}))], 5000, 100),
        reply(SONNET, vec![tool_use("o5", "check", json!({}))], 5100, 50),
        reply(SONNET, vec![tool_use("o6", "submit", json!({"summary": "span 110 mm"}))], 5200, 50),
        reply(SONNET, vec![tool_use("r4", "verdict", json!({"summary": "no blocking issues"}))], 3000, 100),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, mut rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "a span for a 100 mm gap under $50", spec());
    cfg.regressions_path = Some(regressions.clone());

    let out = harness::run(client, tools::<Toy>(), cfg).await;

    assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);
    assert!(out.passed);
    assert_eq!(out.rounds, 2);
    assert_eq!(transport.calls(), 13);
    assert_eq!(out.state.design.as_ref().unwrap()["span_mm"], 110.0);
    // The confirmed attack is a permanent regression case, on disk too.
    assert_eq!(out.state.regressions.len(), 1);
    assert_eq!(out.state.regressions[0].check, "span");
    assert!(out.state.report.as_ref().unwrap().get("span@run-1-r1-1").unwrap().passed());
    assert_eq!(wright_agents::state::load_regressions(&regressions, "toy").unwrap().len(), 1);

    let reqs = transport.requests.lock().unwrap();
    // Request shape: strict tools, auto tool choice, cached system, effort.
    let first = &reqs[0].body;
    // Normal mode uses Opus; demo-mode Fable is covered below.
    assert_eq!(first["model"], OPUS);
    assert_eq!(first["tool_choice"], json!({"type": "auto"}));
    assert_eq!(first["output_config"]["effort"], "medium");
    assert_eq!(first["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(first["system"][0]["cache_control"], json!({"type": "ephemeral"}));
    assert!(first["tools"].as_array().unwrap().iter().all(|t| t["strict"] == true));
    assert!(first.get("thinking").is_none() && first.get("temperature").is_none());
    // The default specialist is Opus at medium effort and starts a fresh conversation.
    assert_eq!(reqs[2].body["model"], OPUS);
    assert_eq!(reqs[2].body["output_config"]["effort"], "medium");
    assert_eq!(reqs[2].body["messages"].as_array().unwrap().len(), 1);
    // The reviewer sees the state and the report, not the transcript.
    assert_eq!(reqs[6].body["messages"].as_array().unwrap().len(), 1);
    assert!(reqs[6].body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("Last check report"));
    // A refuted attack is an error result with the numbers.
    let r = &last_user(&reqs[7].body)[0];
    assert_eq!(r["is_error"], true);
    assert!(r["content"].as_str().unwrap().contains("not confirmed"));
    // Round 2 resumes the orchestrator append-only: the owed submit result
    // leads the next user turn, and history only grew.
    let round2 = &reqs[9].body;
    let lu = last_user(round2);
    assert_eq!(lu[0]["type"], "tool_result");
    assert_eq!(lu[0]["tool_use_id"], "o3");
    let prev = reqs[5].body["messages"].as_array().unwrap();
    assert_eq!(&round2["messages"].as_array().unwrap()[..prev.len()], &prev[..]);
    drop(reqs);

    let ev = events(&mut rx);
    assert!(matches!(ev.first(), Some(Event::RunStarted { .. })));
    assert!(matches!(ev.last(), Some(Event::RunFinished { passed: true, .. })));
    assert!(ev.iter().any(|e| matches!(e, Event::Checks { failed: 1, .. })));
    assert!(ev.iter().any(|e| matches!(e, Event::ToolCall { agent, .. } if agent == "specialist:generalist")));
    let spends: Vec<f64> = ev.iter().filter_map(|e| if let Event::Spend { usd, .. } = e { Some(*usd) } else { None }).collect();
    assert_eq!(spends.len(), 13);
    assert!(spends.windows(2).all(|w| w[1] > w[0]));
    let _ = std::fs::remove_file(regressions);
}

#[tokio::test]
async fn budget_stop_refuses_the_call_that_could_cross_the_limit() {
    let ledger = tmp("ledger.json");
    let script = vec![
        reply(SONNET, vec![tool_use("o1", "design", json!({}))], 2000, 5000),
        reply(SONNET, vec![tool_use("o2", "submit", json!({"summary": "never sent"}))], 10, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    // First call: worst case ~$0.08 + prompt fits $0.12; it then costs
    // $0.054, and the next worst case does not fit what is left.
    let (client, mut rx) = setup(transport.clone(), Budget::new(0.12, 45.0, Some(ledger.clone())));
    // priced as the Sonnet orchestrator this arithmetic was written for
    let mut cfg = RunConfig::new("run-1", "x", spec());
    cfg.models.orchestrator = wright_agents::agent::ModelCfg::new(SONNET, "medium", 8000);
    let out = harness::run(client, tools::<Toy>(), cfg).await;

    assert!(matches!(out.stop, StopReason::Budget(ref m) if m.contains("run budget")), "{:?}", out.stop);
    assert!(!out.passed);
    assert_eq!(transport.calls(), 1, "the refused call was never sent");
    assert!((out.spent_usd - 0.054).abs() < 1e-9);
    // The global ledger kept the count on disk.
    let l = wright_agents::budget::Ledger::load(&ledger).unwrap();
    assert_eq!(l.calls, 1);
    assert!((l.spent_usd - 0.054).abs() < 1e-9);
    assert!(matches!(events(&mut rx).last(), Some(Event::RunFinished { passed: false, .. })));
    let _ = std::fs::remove_file(ledger);
}

fn bad_patch(id: &str) -> Value {
    reply(SONNET, vec![tool_use(id, "edit_spec", json!({"patch_json": "{target_mm: 1", "why": "x"}))], 100, 10)
}

#[tokio::test]
async fn malformed_input_is_requeried_three_times_then_recovers() {
    let script = vec![
        bad_patch("b1"),
        reply(SONNET, vec![tool_use("b2", "no_such_tool", json!({}))], 100, 10),
        bad_patch("b3"),
        // Fourth try is well-formed: the run carries on.
        reply(SONNET, vec![tool_use("ok", "edit_spec", json!({"patch_json": "{\"shrink_mm\": 1}", "why": "allow for a cold day"}))], 100, 10),
        reply(SONNET, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(SONNET, vec![tool_use("r1", "verdict", json!({"summary": "no design"}))], 100, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "x", spec());
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;

    assert_eq!(out.stop, StopReason::RoundLimit);
    assert_eq!(out.state.spec["shrink_mm"], 1.0);
    let reqs = transport.requests.lock().unwrap();
    let r = &last_user(&reqs[1].body)[0];
    assert_eq!(r["is_error"], true);
    assert!(r["content"].as_str().unwrap().contains("not valid JSON"));
    assert!(last_user(&reqs[2].body)[0]["content"].as_str().unwrap().contains("unknown tool"));
}

#[tokio::test]
async fn malformed_input_a_fourth_time_fails_the_run() {
    let script = vec![bad_patch("b1"), bad_patch("b2"), bad_patch("b3"), bad_patch("b4"), bad_patch("b5")];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let out = harness::run(client, tools::<Toy>(), RunConfig::new("run-1", "x", spec())).await;
    assert!(matches!(out.stop, StopReason::Error(ref m) if m.contains("malformed tool input 4 times")), "{:?}", out.stop);
    assert_eq!(transport.calls(), 4);
    assert!(!out.passed);
}

#[tokio::test]
async fn feedback_on_a_render_reaches_the_orchestrator_as_image_and_text() {
    let (tx, _keep) = channel();
    let person = tx.clone();
    let mut n = 0;
    // A person draws on the render while the orchestrator's first call is
    // in flight.
    let transport = Arc::new(ScriptedTransport::from_fn(move |_body| {
        n += 1;
        match n {
            1 => {
                let _ = person.send(Event::Feedback {
                    run: "run-1".into(),
                    view: "top".into(),
                    sketch_png: "data:image/png;base64,iVBORw0KGgo=".into(),
                    note: "make the span reach the red line".into(),
                });
                Some(reply(SONNET, vec![tool_use("o1", "design", json!({}))], 100, 10))
            }
            2 => Some(reply(SONNET, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10)),
            3 => Some(reply(SONNET, vec![tool_use("r1", "verdict", json!({"summary": "span fails"}))], 100, 10)),
            _ => None,
        }
    }));
    let client = Client::new(transport.clone(), Budget::new(1.25, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "x", spec());
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;

    let reqs = transport.requests.lock().unwrap();
    let lu = last_user(&reqs[1].body);
    assert_eq!(lu[0]["type"], "tool_result", "tool results come first");
    let img = lu.iter().find(|b| b["type"] == "image").expect("image block");
    assert_eq!(img["source"]["media_type"], "image/png");
    assert_eq!(img["source"]["data"], "iVBORw0KGgo=");
    assert!(lu.iter().any(|b| b["type"] == "text" && b["text"].as_str().unwrap().contains("red line")));
    // Exactly once: later requests carry it only as history.
    assert_eq!(reqs[1].body.to_string().matches("red line").count(), 1);
    assert_eq!(out.rounds, 1);
}

/// Live, on the signed-in `claude` CLI (a claude.ai subscription, not API
/// credits): `WRIGHT_LIVE_CLI=1 cargo test -p wright-agents --test harness live_cli -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn live_cli_toy_run() {
    if std::env::var("WRIGHT_LIVE_CLI").is_err() {
        return;
    }
    let (tx, mut rx) = channel();
    let client = Client::new(Arc::new(wright_agents::cli::CliTransport::from_env()), Budget::new(1.25, 45.0, None), tx, "live");
    let mut cfg = RunConfig::new("live", "a span for a 100 mm gap under $50", spec());
    cfg.max_rounds = 2;
    cfg.models.reviewer = cfg.models.orchestrator.clone();
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    for e in events(&mut rx) {
        match e {
            Event::ToolCall { agent, tool, input, .. } => println!("{agent} -> {tool} {input}"),
            Event::ToolResult { agent, tool, ok, summary, .. } => println!("{agent} <- {tool} ok={ok} {}", summary.lines().next().unwrap_or("")),
            Event::AgentMessage { agent, text, .. } => println!("{agent}: {}", text.chars().take(160).collect::<String>()),
            _ => {}
        }
    }
    println!("stop={:?} passed={} rounds={} notional=${:.4}", out.stop, out.passed, out.rounds, out.spent_usd);
    assert!(!matches!(out.stop, StopReason::Error(_)), "{:?}", out.stop);
}


/// Review finding 1 (2026-10-24): the orchestrator could lower the person's
/// own requirement with edit_spec and the run then passed against it.
#[tokio::test]
async fn the_orchestrator_cannot_lower_what_the_person_asked_for() {
    let script = vec![
        reply(SONNET, vec![tool_use("e1", "edit_spec", json!({"patch_json": "{\"target_mm\": 50}", "why": "easier"}))], 100, 10),
        reply(SONNET, vec![tool_use("s1", "submit", json!({"summary": "s"}))], 100, 10),
        reply(SONNET, vec![tool_use("v1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "x", spec());
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_eq!(out.state.spec["target_mm"], 100.0);
    assert!(!out.passed, "a 90 mm span must not pass a 100 mm request");
    let reqs = transport.requests.lock().unwrap();
    assert!(last_user(&reqs[1].body)[0]["content"].as_str().unwrap().contains("own requirement"));
}

/// Review finding 3: an error object from the API was read as the
/// reviewer's silent approval.
#[tokio::test]
async fn an_unusable_reviewer_reply_is_not_an_approval() {
    let err = json!({"type": "error", "error": {"type": "overloaded_error", "message": "x"}});
    let script = vec![
        reply(SONNET, vec![tool_use("e1", "edit_spec", json!({"patch_json": "{\"shrink_mm\": 0}", "why": "x"}))], 100, 10),
        reply(SONNET, vec![tool_use("s1", "submit", json!({"summary": "s"}))], 100, 10),
        err.clone(), err.clone(), err.clone(), err.clone(),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "x", spec());
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_ne!(out.stop, StopReason::Passed);
}

/// Live run 2026-10-24: with no spec file, the whole first spec was locked,
/// including a value the orchestrator itself called an assumption, and the
/// run stalled. Values it lists under `assumptions` stay open (and are shown
/// to the reviewer); since review 4 everything else in the first write is
/// locked by default.
#[tokio::test]
async fn values_the_orchestrator_assumed_stay_open_and_the_rest_lock() {
    let script = vec![
        reply(SONNET, vec![tool_use("e1", "edit_spec", json!({
            "patch_json": "{\"target_mm\": 100, \"budget_usd\": 50, \"shrink_mm\": 3}",
            "why": "from the request; shrink is my assumption",
            "assumptions": ["shrink_mm"]
        }))], 100, 10),
        // the assumption can be revised; the stated target cannot
        reply(SONNET, vec![tool_use("e2", "edit_spec", json!({"patch_json": "{\"shrink_mm\": 0}", "why": "revise my assumption", "assumptions": ["shrink_mm"]}))], 100, 10),
        reply(SONNET, vec![tool_use("e3", "edit_spec", json!({"patch_json": "{\"target_mm\": 10}", "why": "easier", "assumptions": []}))], 100, 10),
        reply(SONNET, vec![tool_use("s1", "submit", json!({"summary": "s"}))], 100, 10),
        reply(SONNET, vec![tool_use("v1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "a 100 mm span under $50", Value::Null);
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_eq!(out.state.stated, json!({"target_mm": 100, "budget_usd": 50}));
    assert_eq!(out.state.spec["shrink_mm"], 0.0);
    assert_eq!(out.state.spec["target_mm"], 100.0);
    assert!(!out.state.assumptions.is_empty());
}

#[tokio::test]
async fn transcripts_record_what_each_agent_was_sent() {
    let out = std::env::temp_dir().join(format!("wright-agents-{}-transcripts", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let script = vec![
        reply(SONNET, vec![text("Designing."), tool_use("o1", "design", json!({}))], 3000, 200),
        reply(SONNET, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "fix the span"}))], 3500, 150),
        reply(OPUS, vec![tool_use("s1", "report", json!({"summary": "nothing changed"}))], 2000, 100),
        reply(SONNET, vec![tool_use("o3", "submit", json!({"summary": "as designed"}))], 4000, 100),
        reply(SONNET, vec![tool_use("r1", "verdict", json!({"summary": "span fails"}))], 3000, 100),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (client, _rx) = setup(transport.clone(), Budget::new(1.25, 45.0, None));
    let mut cfg = RunConfig::new("run-1", "a span for a 100 mm gap under $50", spec());
    cfg.max_rounds = 1;
    cfg.out_dir = Some(out.clone());
    let summary = harness::run(client, tools::<Toy>(), cfg).await;
    assert!(!summary.passed);

    let read = |name: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(out.join("transcripts").join(name)).unwrap()).unwrap() };
    let orch = read("orchestrator.json");
    let reqs = transport.requests.lock().unwrap();
    // The orchestrator's record is exactly its last request's history plus
    // the final assistant turn (the submit call); tools and system as sent.
    let last = reqs.iter().rev().find(|r| r.body["system"][0]["text"] == orch["system"]).unwrap();
    let sent = last.body["messages"].as_array().unwrap();
    let kept = orch["messages"].as_array().unwrap();
    assert_eq!(&kept[..sent.len()], &sent[..]);
    assert_eq!(kept.len(), sent.len() + 1);
    assert_eq!(kept.last().unwrap()["content"][0]["name"], "submit");
    assert_eq!(orch["tools"], last.body["tools"]);
    assert_eq!(orch["agent"], "orchestrator");
    assert_eq!(orch["model"]["id"], OPUS);
    // The owed submit result waits in `pending`, not spliced into history.
    assert_eq!(orch["pending"][0]["tool_use_id"], "o3");
    drop(reqs);
    // Specialist and reviewer: fresh conversations, named per round.
    let sp = read("specialist-generalist-r1.json");
    assert_eq!(sp["model"]["id"], OPUS);
    assert_eq!(sp["messages"].as_array().unwrap().len(), 2);
    assert_eq!(sp["messages"][1]["content"][0]["name"], "report");
    let rv = read("reviewer-r1.json");
    assert_eq!(rv["agent"], "reviewer");
    assert_eq!(rv["messages"][1]["content"][0]["name"], "verdict");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn normal_and_demo_model_selection_remain_distinct() {
    let normal = harness::Models::standard(false);
    let demo = harness::Models::standard(true);
    assert_eq!(normal.orchestrator.id, OPUS);
    assert_eq!(demo.orchestrator.id, "claude-fable-5-1");
    assert_eq!(normal.specialist.id, OPUS);
    assert_eq!(demo.specialist.id, OPUS);
    assert_eq!(demo.reviewer.id, OPUS);
}
