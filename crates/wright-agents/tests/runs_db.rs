//! The runs database end to end and offline: import of a recorded run
//! dir, ranking of prior efforts, and the harness's "Prior efforts" seed
//! (a no-op on an empty store; the finished run is recorded for the next).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::runs_db::{self, JsonlStore, Query, RecordKind, RunRecord, RunStore};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig, StopReason};
use wright_core::{Artifacts, Bom, CheckId, Domain, Finished, Report, Source};

struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    target_mm: f64,
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
        json!({"type": "object", "properties": {"target_mm": {"type": "number"}, "budget_usd": {"type": "number"}}})
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        Ok(Design { span_mm: s.target_mm + 5.0, price_usd: 10.0 })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Span, d.span_mm - s.target_mm, "mm", Source::Geometry, "span - target");
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

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("wright-runsdb-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    let _ = std::fs::remove_file(&p);
    p
}

fn system_text(req: &Value) -> String {
    req["system"].as_array().map(|b| b.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| req["system"].as_str().unwrap_or("").to_string())
}

fn rec(id: &str, domain: &str, request: &str, spec: Value, passed: bool) -> RunRecord {
    let mut r = runs_db::record_from_state(&json!({"domain": domain, "request": request, "spec": spec, "design": {"id": id}}), id, None).unwrap();
    r.passed = passed;
    r.failing = if passed { 0 } else { 2 };
    r
}

#[test]
fn import_reads_a_recorded_run_dir() {
    // a synthetic run dir in the harness's own output shape
    let dir = tmp("run-dir");
    std::fs::create_dir_all(dir.join("cad")).unwrap();
    let state = json!({
        "domain": "balloon", "request": "photograph the curvature of the Earth", "spec": {"target_altitude_m": 30000},
        "design": {"balloon": "totex-1200"}, "best": {"design": {"balloon": "totex-3000"}, "failing": 1, "objective": 38883.4, "round": 3},
        "report": {"outcomes": [
            {"name": "burst_altitude", "evidence": {"Margin": {"value": 7828.9, "unit": "m"}}, "detail": "burst at 37829 m", "source": "Physics"},
            {"name": "payload_fit", "evidence": {"Margin": {"value": -4.0, "unit": "mm"}}, "detail": "camera 4 mm too long", "source": "Geometry"}]},
        "feedback": [], "regressions": [{"id": "r1-1", "domain": "balloon", "claim": "cold morning", "spec_patch": {"ground_temperature_c": -5}, "check": "part_temperature", "margin_at_confirmation": -2.8}],
        "notes": [], "round": 3
    });
    std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
    std::fs::write(dir.join("events.jsonl"), "{\"type\":\"run_started\",\"run\":\"balloon-42\",\"domain\":\"balloon\",\"request\":\"x\"}\n{\"type\":\"run_finished\",\"run\":\"balloon-42\",\"passed\":false,\"objective\":38883.4,\"objective_label\":\"burst altitude m\"}\n").unwrap();
    std::fs::write(dir.join("cad/payload_box.step"), "ISO-10303-21;").unwrap();
    std::fs::write(dir.join("bom.csv"), "id,qty\n").unwrap();
    let r = runs_db::record_from_dir(&dir).expect("record");
    assert_eq!((r.id.as_str(), r.domain.as_str(), r.kind), ("balloon-42", "balloon", RecordKind::Run));
    assert_eq!(r.design, Some(json!({"balloon": "totex-3000"})), "the verified incumbent is the chosen design");
    assert_eq!((r.checks.len(), r.failing, r.passed), (2, 1, false));
    assert_eq!(r.objective_label, "burst altitude m");
    assert!(r.failures.iter().any(|f| f.starts_with("payload_fit failed: camera 4 mm too long")), "{:?}", r.failures);
    assert!(r.failures.iter().any(|f| f.contains("confirmed attack on part_temperature")), "{:?}", r.failures);
    assert!(r.artifacts.iter().any(|a| a.ends_with("cad/payload_box.step")) && r.artifacts.iter().any(|a| a.ends_with("bom.csv")), "{:?}", r.artifacts);
    assert_eq!(r.spec_key, runs_db::spec_key(&json!({"target_altitude_m": 30000})));
    assert_eq!(runs_db::import_tree(dir.parent().unwrap().join(dir.file_name().unwrap()).as_path()).len(), 1);
    let _ = std::fs::remove_dir_all(dir);

    // the shipped demo recording, when this checkout has it
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/balloon");
    if demo.join("state.json").is_file() {
        let r = runs_db::record_from_dir(&demo).expect("demo/balloon imports");
        assert_eq!(r.domain, "balloon");
        assert!(r.request.contains("weather balloon"), "{}", r.request);
        assert!(r.design.is_some() && !r.checks.is_empty());
        assert!(!r.artifacts.is_empty());
    }
}

#[test]
fn search_ranks_the_matching_prior_run_first_and_an_exact_spec_above_text() {
    let path = tmp("search.jsonl");
    let store = JsonlStore::new(&path);
    store.log_run(&rec("air-1", "aircraft", "a 3D-printed VTOL quadplane, 1.2 m wingspan, 130 miles range", json!({"range_km": 210}), true)).unwrap();
    store.log_run(&rec("rocket-1", "rocket", "student liquid bipropellant rocket for 10,000 ft", json!({"apogee_ft": 10000}), false)).unwrap();
    store.log_run(&rec("balloon-1", "balloon", "weather balloon photographing the curvature of the Earth", json!({"alt": 30000}), false)).unwrap();
    store.log_run(&rec("air-2", "aircraft", "fixed-wing glider for a long range survey", json!({"range_km": 80}), false)).unwrap();

    let hits = store.search_runs(&Query { text: "VTOL quadplane that flies 200 km".into(), ..Default::default() }, 5).unwrap();
    assert_eq!(hits[0].record.id, "air-1", "{:?}", hits.iter().map(|h| &h.record.id).collect::<Vec<_>>());
    assert!(hits.iter().all(|h| h.record.id != "rocket-1"), "a run sharing no term is not a match");

    // the same spec exactly outranks a better text match
    let hits = store.search_runs(&Query { domain: Some("aircraft".into()), text: "VTOL quadplane".into(), spec: Some(json!({"range_km": 80})) }, 5).unwrap();
    assert_eq!(hits[0].record.id, "air-2");
    assert!(hits[0].exact_spec && !hits[1].exact_spec);

    // a re-log replaces by id (last line wins), compaction keeps one line each
    store.log_run(&rec("air-1", "aircraft", "a 3D-printed VTOL quadplane, edited", json!({"range_km": 210}), true)).unwrap();
    assert_eq!(store.get_run("air-1").unwrap().unwrap().request, "a 3D-printed VTOL quadplane, edited");
    let hits = store.search_runs(&Query { text: "quadplane edited".into(), ..Default::default() }, 5).unwrap();
    assert_eq!(hits.iter().filter(|h| h.record.id == "air-1").count(), 1);
    assert!(hits[0].record.request.ends_with("edited"));
    assert_eq!(store.compact().unwrap(), 4);
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 4);
    let _ = std::fs::remove_file(path);
}

fn script() -> Vec<Value> {
    vec![
        reply(FABLE, vec![tool_use("o1", "design", json!({}))], 3000, 200),
        reply(FABLE, vec![tool_use("o2", "submit", json!({"summary": "span 105 mm, all checks pass"}))], 4000, 100),
        reply(OPUS, vec![tool_use("r1", "verdict", json!({"summary": "no blocking issues"}))], 3000, 100),
    ]
}

#[tokio::test]
async fn harness_seeding_is_a_noop_on_an_empty_store_and_the_next_run_is_seeded() {
    let path = tmp("harness.jsonl");
    let store: Arc<dyn RunStore> = Arc::new(JsonlStore::new(&path));
    let spec = json!({"target_mm": 100.0, "budget_usd": 50.0});

    // Run 1 with an empty store: the prompt is exactly the one with no store.
    let mut prompts = vec![];
    for db in [None, Some(store.clone())] {
        let transport = Arc::new(ScriptedTransport::new(script()));
        let (tx, _rx) = channel();
        let client = Client::new(transport.clone(), Budget::new(1.25, 45.0, None), tx, "run-1");
        let mut cfg = RunConfig::new("run-1", "a span for a 100 mm gap under $50", spec.clone());
        cfg.runs_db = db;
        let out = harness::run(client, tools::<Toy>(), cfg).await;
        assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);
        prompts.push(system_text(&transport.requests.lock().unwrap()[0].body));
    }
    assert_eq!(prompts[0], prompts[1], "an empty store changes nothing");
    assert!(!prompts[1].contains("Prior efforts"));

    // ... and the finished run was recorded
    let stored = store.list_runs(Some("toy")).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].passed && stored[0].id == "run-1" && stored[0].design.is_some());
    assert_eq!(stored[0].objective_label, "span mm");

    // Run 2, same spec: told the prior run, flagged as an exact match, with its design.
    let transport = Arc::new(ScriptedTransport::new(script()));
    let (tx, _rx) = channel();
    let client = Client::new(transport.clone(), Budget::new(1.25, 45.0, None), tx, "run-2");
    let mut cfg = RunConfig::new("run-2", "another span for the 100 mm gap", spec.clone());
    cfg.runs_db = Some(store.clone());
    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);
    let sys = system_text(&transport.requests.lock().unwrap()[0].body);
    let i = sys.find("Prior efforts").expect("seeded");
    let block = &sys[i..];
    assert!(block.contains("- run run-1 (") && block.contains("[exact spec match]: passed"), "{block}");
    assert!(block.contains("Best prior design (run run-1, passed every check") && block.contains("\"span_mm\":105.0"), "{block}");
    assert_eq!(store.list_runs(Some("toy")).unwrap().len(), 2);
    let _ = std::fs::remove_file(path);
}
