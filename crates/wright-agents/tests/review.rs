//! The systematic hostile reviewer (`src/review.rs`): taxonomy in the
//! prompt and tool schemas, evidence vetting, the rebuttal round, and the
//! per-run metrics. Offline: scripted transport, toy domain.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::review::Category;
use wright_agents::{channel, harness, prompts, tools, Budget, Client, RunConfig, StopReason};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, Report, Source};

const S: &str = "claude-sonnet-5-5";

/// A shelf whose span passes by 5 mm; a cold-day shrink is the outside
/// condition attacks may vary.
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
    bracket: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Span,
}
impl CheckId for C {
    const ALL: &'static [Self] = &[C::Span];
    fn name(self) -> &'static str {
        "span"
    }
    fn describe(self) -> &'static str {
        "span reaches the target"
    }
}
impl Domain for Shelf {
    const NAME: &'static str = "shelf-review";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"target_mm": {"type": "number", "minimum": 10, "maximum": 1000}, "shrink_mm": {"type": "number", "minimum": 0, "maximum": 20}}})
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        Ok(Design { span_mm: s.target_mm + 5.0, bracket: "BRK-200".into() })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Span, d.span_mm - s.shrink_mm - s.target_mm, "mm", Source::Geometry, "span - shrink - target");
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

fn client(script: Vec<Value>) -> (Client, Arc<ScriptedTransport>, tokio::sync::broadcast::Receiver<Event>) {
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, rx) = channel();
    (Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1"), t, rx)
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

fn harness_says(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<String> {
    let mut v = vec![];
    while let Ok(e) = rx.try_recv() {
        if let Event::AgentMessage { agent, text, .. } = e {
            if agent == "harness" {
                v.push(text);
            }
        }
    }
    v
}

fn base_cfg() -> RunConfig {
    let mut c = RunConfig::new("run-1", "a 100 mm shelf", json!({"target_mm": 100.0}));
    c.max_rounds = 1;
    c
}

fn attack(id: &str, patch: &str, category: &str, evidence: &str) -> Value {
    tool_use(id, "attack", json!({"claim": "a cold day", "category": category, "evidence": evidence, "spec_patch_json": patch, "expect_failing_check": "span"}))
}

#[test]
fn reviewer_prompt_and_tools_carry_the_taxonomy() {
    let d = tools::<Shelf>();
    let system = prompts::reviewer_system(d.as_ref());
    assert!(system.contains("Attack taxonomy"), "{system}");
    for c in Category::ALL {
        assert!(system.contains(&format!("- {}: ", c.name())), "missing {c}");
    }
    let t = prompts::reviewer_tools();
    let attack = t.iter().find(|x| x["name"] == "attack").unwrap();
    let props = &attack["input_schema"]["properties"];
    assert_eq!(props["category"]["enum"].as_array().unwrap().len(), 8);
    assert!(props["evidence"].is_object());
    let req = attack["input_schema"]["required"].as_array().unwrap();
    assert!(req.iter().any(|r| r == "evidence") && req.iter().any(|r| r == "category"));
    let verdict = t.iter().find(|x| x["name"] == "verdict").unwrap();
    assert_eq!(verdict["input_schema"]["properties"]["walked_clear"]["items"]["enum"].as_array().unwrap().len(), 8);
    // the reviewer may read the catalog (stock/sourcing evidence) but has no edit tool
    let names: Vec<&str> = t.iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"catalog"), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("edit_")), "{names:?}");
}

#[tokio::test]
async fn the_reviewer_can_read_the_catalog_but_not_edit() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "catalog", json!({"path": "", "query": ""}))], 100, 10),
        reply(S, vec![tool_use("r2", "edit_design", json!({"patch_json": "{}", "why": "x"}))], 100, 10),
        reply(S, vec![tool_use("r3", "verdict", json!({"summary": "x", "walked_clear": []}))], 100, 10),
    ];
    let (c, t, _rx) = client(script);
    let s = harness::run(c, tools::<Shelf>(), base_cfg()).await;
    let res = tool_results(&t);
    assert!(res.iter().any(|r| r.contains("exposes no catalog lookups yet")), "{res:?}");
    assert!(res.iter().any(|r| r.contains("unknown tool 'edit_design'")), "{res:?}");
    assert_eq!(s.stop, StopReason::Passed);
}

#[tokio::test]
async fn attacks_must_cite_evidence_and_metrics_count_them_by_category() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        // evidence that names nothing in the report, design or spec
        reply(S, vec![attack("r1", "{\"shrink_mm\": 8}", "environment", "the moon phase")], 100, 10),
        // a category outside the taxonomy
        reply(S, vec![attack("r2", "{\"shrink_mm\": 8}", "vibes", "span")], 100, 10),
        // a part id in the design is evidence; the host confirms it
        reply(S, vec![attack("r3", "{\"shrink_mm\": 8}", "environment", "BRK-200 bracket, span margin 5 mm")], 100, 10),
        // the same condition, refuted (2 mm shrink still passes)
        reply(S, vec![attack("r4", "{\"shrink_mm\": 2}", "environment", "span")], 100, 10),
        reply(S, vec![tool_use("r5", "verdict", json!({"summary": "ok", "walked_clear": ["interference", "stock_sourcing", "not_a_category"]}))], 100, 10),
    ];
    let (c, t, mut rx) = client(script);
    let s = harness::run(c, tools::<Shelf>(), base_cfg()).await;
    let res = tool_results(&t);
    assert!(res.iter().any(|r| r.contains("rejected: evidence `the moon phase` names no check")), "{res:?}");
    assert!(res.iter().any(|r| r.contains("rejected: category `vibes` is not in the taxonomy")), "{res:?}");
    assert!(res.iter().any(|r| r.starts_with("confirmed (run-1-r1-1)")), "{res:?}");
    assert!(res.iter().any(|r| r.contains("not confirmed")), "{res:?}");
    assert_eq!(s.state.regressions.len(), 1);

    let m = &s.review;
    assert_eq!((m.raised, m.confirmed, m.refuted, m.rejected, m.withdrawn), (4, 1, 1, 2, 0));
    assert_eq!(m.rejected_by_kind["bad_evidence"], 1);
    assert_eq!(m.rejected_by_kind["bad_category"], 1);
    assert_eq!(m.by_category["environment"].confirmed, 1);
    assert_eq!(m.by_category["environment"].refuted, 1);
    // walked: environment (attacked), interference + stock_sourcing (cleared); unknown names ignored
    assert_eq!(m.unwalked.len(), 5, "{:?}", m.unwalked);
    assert!(m.unwalked.contains(&"omission".to_string()));
    assert!(!m.unwalked.contains(&"interference".to_string()));
    assert_eq!(m.rebuttal_rounds, 0);

    let said = harness_says(&mut rx);
    let line = said.iter().find(|t| t.starts_with("review metrics round 1: ")).expect("metrics event");
    let v: Value = serde_json::from_str(line.trim_start_matches("review metrics round 1: ")).unwrap();
    assert_eq!(v["confirmed"], 1);
    // the off-taxonomy `vibes` attack is logged under its inferred category (a shrink patch: environment)
    assert_eq!(v["by_category"]["environment"]["raised"], 4);
}

#[tokio::test]
async fn a_withdrawn_attack_after_rebuttal_is_not_stored_and_the_round_is_clean() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![attack("r1", "{\"shrink_mm\": 8}", "stock_sourcing", "span")], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "cold day fails", "walked_clear": []}))], 100, 10),
        // proposer contests
        reply(S, vec![tool_use("p1", "rebut", json!({"attack_id": "run-1-r1-1", "stance": "contest", "argument": "the request says the shelf lives indoors; an 8 mm shrink is a freezer"}))], 100, 10),
        reply(S, vec![tool_use("p2", "done", json!({}))], 100, 10),
        // reviewer withdraws
        reply(S, vec![tool_use("a1", "rule", json!({"attack_id": "run-1-r1-1", "ruling": "withdraw", "reason": "indoor use is in the request"}))], 100, 10),
        reply(S, vec![tool_use("a2", "verdict", json!({"summary": "withdrawn"}))], 100, 10),
    ];
    let (c, t, mut rx) = client(script);
    let mut cfg = base_cfg();
    cfg.rebuttal = true;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert_eq!(s.stop, StopReason::Passed, "{:?} {:?}", s.stop, s.state.feedback);
    assert!(s.state.regressions.is_empty());
    assert_eq!(t.calls(), 8);
    assert_eq!((s.review.confirmed, s.review.withdrawn, s.review.rebuttal_rounds), (0, 1, 1));
    // the proposer saw the attack with its category and evidence
    let reqs = t.requests.lock().unwrap();
    let proposer = reqs[4].body["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert!(proposer.contains("\"category\": \"stock_sourcing\"") && proposer.contains("run-1-r1-1"), "{proposer}");
    assert_eq!(reqs[4].body["tools"][0]["name"], "rebut");
    assert_eq!(reqs[6].body["tools"][0]["name"], "rule");
    assert!(reqs[6].body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("freezer"));
    drop(reqs);
    assert!(harness_says(&mut rx).iter().any(|t| t.starts_with("attack run-1-r1-1 withdrawn after rebuttal")));
}

#[tokio::test]
async fn an_upheld_or_accepted_attack_stands() {
    // contested, upheld
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![attack("r1", "{\"shrink_mm\": 8}", "stock_sourcing", "span")], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "x", "walked_clear": []}))], 100, 10),
        reply(S, vec![tool_use("p1", "rebut", json!({"attack_id": "run-1-r1-1", "stance": "contest", "argument": "too cold"}))], 100, 10),
        reply(S, vec![tool_use("p2", "done", json!({}))], 100, 10),
        reply(S, vec![tool_use("a1", "rule", json!({"attack_id": "run-1-r1-1", "ruling": "uphold", "reason": "garages freeze"}))], 100, 10),
        reply(S, vec![tool_use("a2", "verdict", json!({"summary": "stands"}))], 100, 10),
    ];
    let (c, t, _rx) = client(script);
    let mut cfg = base_cfg();
    cfg.rebuttal = true;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert_eq!(s.stop, StopReason::RoundLimit);
    assert_eq!(s.state.regressions.len(), 1);
    assert_eq!(t.calls(), 8);
    assert_eq!((s.review.confirmed, s.review.withdrawn), (1, 0));

    // accepted: no adjudication call at all
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![attack("r1", "{\"shrink_mm\": 8}", "stock_sourcing", "span")], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "x", "walked_clear": []}))], 100, 10),
        reply(S, vec![tool_use("p1", "rebut", json!({"attack_id": "run-1-r1-1", "stance": "accept", "argument": ""}))], 100, 10),
        reply(S, vec![tool_use("p2", "done", json!({}))], 100, 10),
    ];
    let (c, t, _rx) = client(script);
    let mut cfg = base_cfg();
    cfg.rebuttal = true;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert_eq!(s.state.regressions.len(), 1);
    assert_eq!(t.calls(), 6);
    assert_eq!(s.review.confirmed, 1);
}

#[tokio::test]
async fn legacy_attacks_without_category_or_evidence_still_run() {
    // older scripts (tests/harness.rs, review4.rs) name neither; the host
    // infers the category and derives the evidence from the report
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "attack", json!({"claim": "a price rise", "spec_patch_json": "{\"shrink_mm\": 8}", "expect_failing_check": "span"}))], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "x"}))], 100, 10),
    ];
    let (c, _t, _rx) = client(script);
    let s = harness::run(c, tools::<Shelf>(), base_cfg()).await;
    assert_eq!(s.review.confirmed, 1);
    assert_eq!(s.review.by_category["stock_sourcing"].confirmed, 1);
}

/// The rebuttal gate (cost + latency, 10/24 night): a confirmed
/// environment attack on a value no agent chose goes straight to the
/// regression store, with no proposer call, and is counted as skipped.
#[tokio::test]
async fn an_environment_attack_on_a_value_nobody_chose_skips_the_rebuttal() {
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![attack("r1", "{\"shrink_mm\": 8}", "environment", "span")], 100, 10),
        reply(S, vec![tool_use("r2", "verdict", json!({"summary": "x", "walked_clear": []}))], 100, 10),
    ];
    let (c, t, mut rx) = client(script);
    let mut cfg = base_cfg();
    cfg.rebuttal = true;
    let s = harness::run(c, tools::<Shelf>(), cfg).await;
    assert_eq!(t.calls(), 4, "no proposer call");
    assert_eq!(s.state.regressions.len(), 1);
    assert_eq!((s.review.rebuttal_rounds, s.review.rebuttals_skipped), (0, 1));
    assert_eq!(s.review.rebuttals_skipped_by_reason["not_an_agent_choice"], 1);
    assert!(harness_says(&mut rx).iter().any(|t| t == "rebuttal skipped for run-1-r1-1: not_an_agent_choice"));
}
