//! Regression tests for the harness findings of hostile review 5
//! (2026-10-24 night), from the reviewer's probes
//! (scratchpad/review5/src/bin/harness_probe.rs). Offline: fake transport.
//! F5 (CLI double execution) is a unit test in `src/cli.rs`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, Report, Source};

const S: &str = "claude-sonnet-5-5";

/// A shelf whose design records the target it was made for, with a
/// copy-consistency check on it (air's `spec_requirements`, rocket's
/// `apogee_accuracy`).
struct Shelf;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    target_mm: f64,
    #[serde(default)]
    shrink_mm: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Design {
    span_mm: f64,
    made_for_mm: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Span,
    Copy,
}
impl CheckId for C {
    const ALL: &'static [Self] = &[C::Span, C::Copy];
    fn name(self) -> &'static str {
        match self {
            C::Span => "span",
            C::Copy => "made_for_spec",
        }
    }
    fn describe(self) -> &'static str {
        "t"
    }
}
impl Domain for Shelf {
    const NAME: &'static str = "shelf5";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"target_mm": {"type": "number", "minimum": 10, "maximum": 1000}, "shrink_mm": {"type": "number", "minimum": 0, "maximum": 20}}})
    }
    fn consistency_checks() -> &'static [&'static str] {
        &["made_for_spec"]
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        Ok(Design { span_mm: s.target_mm + 5.0, made_for_mm: s.target_mm })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Span, d.span_mm - s.shrink_mm - s.target_mm, "mm", Source::Geometry, "");
        r.margin(C::Copy, 0.5 - (d.made_for_mm - s.target_mm).abs(), "mm", Source::Requirement, "");
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

fn tool_results(t: &ScriptedTransport) -> Vec<String> {
    t.requests
        .lock()
        .unwrap()
        .iter()
        .flat_map(|r| r.body["messages"].as_array().unwrap().last().unwrap()["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].as_str().unwrap_or("").to_string())
        .collect()
}

fn edit(id: &str, patch: &str, why: &str) -> Value {
    tool_use(id, "edit_design", json!({"patch_json": patch, "why": why}))
}

#[tokio::test]
async fn f1_an_attack_on_a_copy_consistency_check_is_refused_and_stored_ones_are_set_aside() {
    let dir = std::env::temp_dir().join(format!("wright-review5-f1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let reg = dir.join("regressions-shelf5.jsonl");
    // a poisoned case a run before the fix stored
    std::fs::write(&reg, json!({"id": "old-r1-1", "domain": "shelf5", "claim": "c", "spec_patch": {"shrink_mm": 2}, "check": "made_for_spec", "margin_at_confirmation": -1.5}).to_string() + "\n").unwrap();
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "attack", json!({"claim": "a 2 mm shrink", "spec_patch_json": "{\"shrink_mm\": 2}", "expect_failing_check": "made_for_spec"}))], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, _rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "x", json!({"target_mm": 100.0}));
    cfg.max_rounds = 1;
    cfg.regressions_path = Some(reg.clone());
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    let res = tool_results(&t);
    assert!(res.iter().any(|r| r.contains("rejected: `made_for_spec` checks that the design matches the spec")), "{res:?}");
    assert!(s.state.regressions.is_empty(), "{:?}", s.state.regressions);
    assert!(s.passed, "the poisoned stored case no longer fails every design: {:?}", s.state.report);
    assert!(std::fs::read_to_string(&reg).unwrap().trim().is_empty());
    let rejected = std::fs::read_to_string(dir.join("regressions-rejected.jsonl")).unwrap();
    assert!(rejected.contains("old-r1-1") && rejected.contains("copy-consistency"), "{rejected}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn f2_a_change_the_person_asked_for_is_not_reverted_to_a_higher_objective_design() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![edit("o2", "{\"span_mm\":130}", "longer")], 100, 10),
        reply(S, vec![tool_use("o3", "check", json!({}))], 100, 10),
        // (the person's sketch arrives here) the lead does what they asked
        reply(S, vec![edit("o4", "{\"span_mm\":104}", "the person drew: it has to fit their 105 mm shelf")], 100, 10),
        reply(S, vec![tool_use("o5", "check", json!({}))], 100, 10),
        reply(S, vec![tool_use("o6", "submit", json!({"summary": "shortened to 104 mm per the sketch"}))], 100, 10),
        reply(S, vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let (tx, mut rx) = channel();
    let tx2 = tx.clone();
    let q = Mutex::new(std::collections::VecDeque::from(script));
    let n = Mutex::new(0);
    let t = Arc::new(ScriptedTransport::from_fn(move |_| {
        let mut k = n.lock().unwrap();
        *k += 1;
        if *k == 4 {
            let _ = tx2.send(Event::Feedback { run: "run-1".into(), view: "iso".into(), sketch_png: String::new(), note: "this must fit my 105 mm shelf".into() });
        }
        q.lock().unwrap().pop_front()
    }));
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "a shelf bracket", json!({"target_mm": 100.0}));
    cfg.max_rounds = 1;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert!(t.requests.lock().unwrap().iter().any(|r| r.body.to_string().contains("105 mm shelf")), "the lead saw the feedback");
    assert_eq!(s.state.design.as_ref().unwrap()["span_mm"], 104.0);
    let reviewer = t.requests.lock().unwrap().last().unwrap().body["messages"][0]["content"][0]["text"].as_str().unwrap_or("").to_string();
    assert!(reviewer.contains("\"span_mm\": 104.0"), "the reviewer reviews what the person asked for");
    assert!(s.passed, "{:?}", s.stop);
    let mut said = vec![];
    while let Ok(e) = rx.try_recv() {
        if let Event::AgentMessage { agent, text, .. } = e {
            if agent == "harness" {
                said.push(text);
            }
        }
    }
    assert!(!said.iter().any(|m| m.contains("continuing from the best")), "{said:?}");
}

#[tokio::test]
async fn f2_a_swap_without_person_feedback_is_told_to_the_lead_in_feedback() {
    // the lead makes it worse on its own: the swap stands, and is in feedback
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "check", json!({}))], 100, 10),
        reply(S, vec![edit("o3", "{\"span_mm\":90}", "lighter")], 100, 10),
        reply(S, vec![tool_use("o4", "check", json!({}))], 100, 10),
        reply(S, vec![tool_use("o5", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, _rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "x", json!({"target_mm": 100.0}));
    cfg.max_rounds = 1;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert_eq!(s.state.design.as_ref().unwrap()["span_mm"], 105.0);
    assert!(s.state.feedback.iter().any(|f| f.starts_with("harness: Before review:")), "{:?}", s.state.feedback);
    let reviewer = t.requests.lock().unwrap().last().unwrap().body["messages"][0]["content"][0]["text"].as_str().unwrap_or("").to_string();
    assert!(reviewer.contains("harness: Before review"), "the reviewer's state shows the swap too");
}

/// Air's search refuses to return when no candidate passes every stored
/// case (review 5, F6: crates/wright-air/src/search.rs at review time).
struct Strict;
impl Domain for Strict {
    const NAME: &'static str = "shelf5";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        Shelf::spec_schema()
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        Shelf::design(s)
    }
    fn design_under(s: &Spec, conditions: &[wright_core::Condition<Spec>]) -> anyhow::Result<Design> {
        anyhow::ensure!(conditions.is_empty(), "no candidate passes every regression case");
        Shelf::design(s)
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        Shelf::check(s, d)
    }
    fn bom(d: &Design) -> Bom {
        Shelf::bom(d)
    }
    fn build(d: &Design, p: &Path) -> anyhow::Result<Artifacts> {
        Shelf::build(d, p)
    }
    fn objective(d: &Design) -> (f64, &'static str) {
        Shelf::objective(d)
    }
}

#[tokio::test]
async fn f6_a_search_that_passes_no_stored_case_still_returns_a_design() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "attack", json!({"claim": "10 mm shrink", "spec_patch_json": "{\"shrink_mm\": 10}", "expect_failing_check": "span"}))], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "blocked"}))], 100, 10),
        reply(S, vec![tool_use("o3", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o4", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r3", "verdict", json!({"summary": "x"}))], 100, 10),
    ];
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, _rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "x", json!({"target_mm": 100.0}));
    cfg.max_rounds = 2;
    let s = harness::run(c, tools::<Strict>(), cfg).await;
    let res = tool_results(&t);
    let r = res.iter().find(|r| r.contains("WITHOUT the regression cases")).unwrap_or_else(|| panic!("{res:?}"));
    assert!(r.contains("no candidate passes every regression case"), "{r}");
    assert!(s.state.design.is_some());
    assert!(!s.passed, "the case still fails, and the report says so");
}
