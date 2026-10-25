//! The agent-team bench, offline: every shipped scenario in
//! `bench/scenarios/` must pass against the current harness, and the judge
//! must fail a scenario whose expectations are wrong.

use serde_json::json;
use std::path::PathBuf;
use wright_agents::bench::{self, CallPattern, Expect, Scenario, Script, Step};

fn scenarios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/scenarios")
}

fn no_other_domains(_: &str) -> Option<std::sync::Arc<dyn wright_agents::DomainTools>> {
    None
}

#[tokio::test]
async fn every_shipped_scenario_passes() {
    let scenarios = bench::load_dir(&scenarios_dir()).unwrap();
    assert!(scenarios.len() >= 11, "found {} scenarios", scenarios.len());
    let outcomes = bench::run_all(&scenarios, &no_other_domains).await;
    let table = bench::table(&outcomes);
    println!("{table}");
    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed).map(|o| format!("{}: {}", o.id, o.failures.join("; "))).collect();
    assert!(failed.is_empty(), "{failed:#?}");
    assert_eq!(table.lines().count(), outcomes.len() + 3, "one row per scenario, header, rule, totals");
    let json = bench::results_json(&outcomes);
    assert_eq!(json["passed"], json["total"]);
    assert_eq!(json["scenarios"].as_array().unwrap().len(), outcomes.len());
    // every scenario is offline and notional: the whole bench is a few dollars
    assert!(json["spent_usd"].as_f64().unwrap() < 5.0);
}

fn tool(name: &str, input: serde_json::Value) -> Step {
    Step { tool: Some(name.into()), input, repeat: 1, input_tokens: 1000, output_tokens: 100, ..Default::default() }
}

fn passing_script() -> Script {
    Script {
        orchestrator: vec![
            tool("design", json!({})),
            tool("edit_design", json!({"patch_json": "{\"span_mm\": 110}", "why": "reach"})),
            tool("check", json!({})),
            tool("submit", json!({"summary": "s"})),
        ],
        reviewer: vec![tool("verdict", json!({"summary": "ok"}))],
        ..Default::default()
    }
}

fn scenario(id: &str, model: Script, expect: Expect) -> Scenario {
    Scenario {
        id: id.into(),
        description: String::new(),
        domain: "toy".into(),
        request: "a span for a 100 mm gap".into(),
        spec: json!({"target_mm": 100.0, "budget_usd": 50.0}),
        limits: Default::default(),
        model,
        expect,
    }
}

#[tokio::test]
async fn wrong_expectations_are_named_not_swallowed() {
    let expect = Expect {
        passed: Some(false),
        stop: Some("round_limit".into()),
        design: Some(json!({"span_mm": 90})),
        max_calls: Some(2),
        max_cost_usd: Some(0.0),
        forbid: vec![CallPattern { agent: "orchestrator".into(), tool: "check".into(), ok: Some(true) }],
        require: vec![CallPattern { agent: "specialist:*".into(), tool: "report".into(), ok: None }],
        script_exhausted: Some(true),
        ..Default::default()
    };
    let out = bench::run_scenario(&scenario("wrong", passing_script(), expect), bench::toy()).await;
    assert!(!out.passed);
    assert!(out.run_passed, "the run itself passed; the expectations were wrong");
    let text = out.failures.join("\n");
    for key in ["passed:", "stop:", "design.span_mm", "max_calls", "max_cost_usd", "forbid:", "require:", "script_exhausted"] {
        assert!(text.contains(key), "missing {key} in:\n{text}");
    }
    assert_eq!(out.failures.len(), 8, "{text}");
}

#[tokio::test]
async fn unknown_domain_fails_the_scenario_without_running() {
    let mut sc = scenario("mars", passing_script(), Expect::default());
    sc.domain = "mars-lander".into();
    let out = bench::run_all(&[sc], &no_other_domains).await;
    assert!(!out[0].passed);
    assert_eq!(out[0].stop, "not_run");
    assert!(out[0].failures[0].contains("mars-lander"));
}

#[tokio::test]
async fn a_scripted_http_failure_ends_the_run_as_an_error() {
    let mut model = passing_script();
    model.orchestrator.insert(1, Step { http: Some(529), repeat: 1, ..Default::default() });
    let out = bench::run_scenario(&scenario("529", model, Expect::default()), bench::toy()).await;
    assert_eq!(out.stop, "error");
    assert_eq!(out.calls, 2);
    assert!(!out.run_passed);
}

#[tokio::test]
async fn an_exhausted_script_can_fail_the_transport() {
    let mut model = passing_script();
    model.reviewer.clear();
    model.on_exhausted = bench::Exhausted::Error;
    let out = bench::run_scenario(&scenario("dry", model, Expect::default()), bench::toy()).await;
    assert_eq!(out.stop, "error");
    assert_eq!(out.script_exhausted, 1);
}
