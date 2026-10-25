//! The orchestrator's engineering plan (`wright_agents::plan`), offline:
//! a scripted transport and a toy domain, no network, no spend.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, PlanStatus, Report, Source};

struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    target_mm: f64,
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

const OPUS: &str = "claude-opus-5-5";
const FABLE: &str = "claude-fable-5-1";

fn last_user(req: &Value) -> Vec<Value> {
    let msgs = req["messages"].as_array().unwrap();
    msgs.last().unwrap()["content"].as_array().unwrap().clone()
}

fn plan_events(ev: &[Event]) -> Vec<Vec<(String, PlanStatus)>> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Plan { tasks, .. } => Some(tasks.iter().map(|t| (t.id.clone(), t.status)).collect()),
            _ => None,
        })
        .collect()
}

/// The lead plans, delegates by plan task, the harness settles each task
/// from the checks, and a check that fails again makes the task that
/// vouched for it and everything downstream stale.
#[tokio::test]
async fn the_plan_tracks_delegations_and_goes_stale_when_a_check_fails() {
    let plan = json!({
        "goal": "a 100 mm span under $50",
        "tasks": [
            {"id": "review", "title": "hostile review", "owner": "reviewer", "inputs": ["budget"], "checks": []},
            {"id": "budget", "title": "price the span", "owner": "generalist", "inputs": ["size"], "checks": ["budget"]},
            {"id": "size", "title": "size the span", "owner": "generalist", "inputs": ["req"], "checks": ["span"]},
            {"id": "req", "title": "write the spec", "owner": "orchestrator", "inputs": [], "checks": []}
        ]
    });
    let script = vec![
        reply(FABLE, vec![tool_use("p1", "plan_set", plan)], 100, 10),
        reply(FABLE, vec![tool_use("p2", "plan_update", json!({"id": "req", "status": "done", "note": "spec from the person"}))], 100, 10),
        reply(FABLE, vec![tool_use("o1", "design", json!({}))], 100, 10),
        // a delegation that names no plan task is refused
        reply(FABLE, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "fix span", "plan_task": ""}))], 100, 10),
        reply(FABLE, vec![tool_use("o3", "delegate", json!({"role": "generalist", "task": "span fails by -10 mm; fix it", "plan_task": "size"}))], 100, 10),
        reply(OPUS, vec![tool_use("s1", "edit_design", json!({"patch_json": "{\"span_mm\": 105}", "why": "reach target"}))], 100, 10),
        reply(OPUS, vec![tool_use("s2", "check", json!({}))], 100, 10),
        reply(OPUS, vec![tool_use("s3", "report", json!({"summary": "span 105 mm"}))], 100, 10),
        reply(FABLE, vec![tool_use("o4", "delegate", json!({"role": "generalist", "task": "confirm the price", "plan_task": "budget"}))], 100, 10),
        reply(OPUS, vec![tool_use("s4", "check", json!({}))], 100, 10),
        reply(OPUS, vec![tool_use("s5", "report", json!({"summary": "$10 of $50"}))], 100, 10),
        // the lead breaks the span again: size vouched for it
        reply(FABLE, vec![tool_use("o5", "edit_design", json!({"patch_json": "{\"span_mm\": 80}", "why": "try a shorter one"}))], 100, 10),
        reply(FABLE, vec![tool_use("o6", "check", json!({}))], 100, 10),
        reply(FABLE, vec![tool_use("o7", "submit", json!({"summary": "s"}))], 100, 10),
        reply(OPUS, vec![tool_use("r1", "verdict", json!({"summary": "span fails"}))], 100, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (tx, mut rx) = channel();
    let client = Client::new(transport.clone(), Budget::new(1.25, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "a span for a 100 mm gap under $50", json!({"target_mm": 100.0, "budget_usd": 50.0}));
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;

    assert_eq!(transport.calls(), 15, "{:?}", out.stop);
    let reqs = transport.requests.lock().unwrap();
    // the orchestrator has the plan tools and is told to plan
    let first = &reqs[0].body;
    let names: Vec<&str> = first["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"plan_set") && names.contains(&"plan_update"), "{names:?}");
    assert!(first["system"][0]["text"].as_str().unwrap().contains("plan_set"));
    // plan_set sorted the tasks and echoed them
    let r = &last_user(&reqs[1].body)[0];
    assert_eq!(r["is_error"], false, "{r}");
    let text = r["content"].as_str().unwrap();
    assert!(text.find("req:").unwrap() < text.find("size:").unwrap() && text.find("size:").unwrap() < text.find("budget:").unwrap(), "{text}");
    // the delegation without a plan task is refused and told what is ready
    let r = &last_user(&reqs[4].body)[0];
    assert_eq!(r["is_error"], true, "{r}");
    assert!(r["content"].as_str().unwrap().contains("plan_task") && r["content"].as_str().unwrap().contains("size"), "{r}");
    // the specialist is told its plan task
    assert!(reqs[5].body["messages"][0]["content"][0]["text"].as_str().unwrap().starts_with("Task from the lead: [plan task size: size the span]"));
    // the harness settled the task from the checks
    let r = &last_user(&reqs[8].body)[0];
    assert!(r["content"].as_str().unwrap().contains("plan task size is done"), "{r}");
    // the check after the break says what went stale
    let r = &last_user(&reqs[13].body)[0];
    assert_eq!(r["is_error"], true);
    let text = r["content"].as_str().unwrap();
    assert!(text.contains("PLAN:") && text.contains("now stale: size, budget"), "{text}");
    drop(reqs);

    let mut ev = vec![];
    while let Ok(e) = rx.try_recv() {
        ev.push(e);
    }
    let plans = plan_events(&ev);
    assert_eq!(plans[0].iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["req", "size", "budget", "review"]);
    assert!(plans.iter().any(|p| p.iter().any(|(id, s)| id == "size" && *s == PlanStatus::Running)));
    let last = plans.last().unwrap();
    assert_eq!(last.iter().find(|(id, _)| id == "req").unwrap().1, PlanStatus::Done);
    assert_eq!(last.iter().find(|(id, _)| id == "size").unwrap().1, PlanStatus::Stale);
    assert_eq!(last.iter().find(|(id, _)| id == "budget").unwrap().1, PlanStatus::Stale);
    assert_eq!(last.iter().find(|(id, _)| id == "review").unwrap().1, PlanStatus::Pending);
    assert!(ev.iter().any(|e| matches!(e, Event::AgentMessage { agent, text, .. } if agent == "harness" && text.contains("stale"))));
    // the plan is part of the saved state and the prompt
    assert_eq!(out.state.plan.summary(), "1/4 done, 2 stale");
    let s = out.state.for_prompt();
    assert!(s.contains("[stale] size: size the span (generalist) needs req checks span"), "{s}");
    // and it round-trips through the event log line
    let line = serde_json::to_string(&Event::Plan { run: "r".into(), goal: "g".into(), tasks: out.state.plan.tasks.clone() }).unwrap();
    assert!(line.starts_with(r#"{"type":"plan","run":"r","goal":"g","tasks":[{"id":"req""#), "{line}");
    let back: Event = serde_json::from_str(&line).unwrap();
    assert!(matches!(back, Event::Plan { tasks, .. } if tasks.len() == 4));
}

/// Without a plan the harness behaves as before: delegate needs no task,
/// and `plan_update` says to plan first.
#[tokio::test]
async fn without_a_plan_delegation_is_unchanged() {
    let script = vec![
        reply(FABLE, vec![tool_use("p1", "plan_update", json!({"id": "x", "status": "done", "note": ""}))], 100, 10),
        reply(FABLE, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(FABLE, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "fix span"}))], 100, 10),
        reply(OPUS, vec![tool_use("s1", "report", json!({"summary": "nothing"}))], 100, 10),
        reply(FABLE, vec![tool_use("o3", "submit", json!({"summary": "s"}))], 100, 10),
        reply(OPUS, vec![tool_use("r1", "verdict", json!({"summary": "span fails"}))], 100, 10),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (tx, mut rx) = channel();
    let client = Client::new(transport.clone(), Budget::new(1.25, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "x", json!({"target_mm": 100.0, "budget_usd": 50.0}));
    cfg.max_rounds = 1;
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_eq!(transport.calls(), 6, "{:?}", out.stop);
    let reqs = transport.requests.lock().unwrap();
    let r = &last_user(&reqs[1].body)[0];
    assert_eq!(r["is_error"], true);
    assert!(r["content"].as_str().unwrap().contains("plan_set first"));
    let r = &last_user(&reqs[4].body)[0];
    assert_eq!(r["is_error"], false, "{r}");
    let mut ev = vec![];
    while let Ok(e) = rx.try_recv() {
        ev.push(e);
    }
    assert!(plan_events(&ev).is_empty(), "no plan, no plan events");
    assert!(out.state.plan.is_empty());
}
