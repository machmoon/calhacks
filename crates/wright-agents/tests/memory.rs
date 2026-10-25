//! Cross-run lessons memory, end to end and offline: a first run leaves a
//! lesson (the reviewer's confirmed attack and the edit that fixed it) in
//! `lessons-<domain>.jsonl` beside the regressions file; a second run of
//! the domain reads it back into the orchestrator's and every
//! specialist's system prompt with its provenance.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, text, tool_use, ScriptedTransport};
use wright_agents::memory::{self, Kind};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig, StopReason};
use wright_core::{Artifacts, Bom, CheckId, Domain, Finished, Report, Source};

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

fn spec() -> Value {
    json!({"target_mm": 100.0, "budget_usd": 50.0})
}

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("wright-memory-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn client(transport: Arc<ScriptedTransport>, run: &str) -> Client {
    let (tx, _rx) = channel();
    Client::new(transport, Budget::new(1.25, 45.0, None), tx, run)
}

fn system_text(req: &Value) -> String {
    req["system"].as_array().map(|b| b.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| req["system"].as_str().unwrap_or("").to_string())
}

#[tokio::test]
async fn a_confirmed_attack_and_its_fix_become_a_lesson_the_next_run_is_told() {
    let regressions = tmp("regressions-toy.jsonl");
    let lessons = memory::lessons_path(&regressions);
    let _ = std::fs::remove_file(&lessons);
    assert_eq!(lessons.file_name().unwrap().to_str().unwrap(), format!("wright-memory-it-{}-lessons-toy.jsonl", std::process::id()));

    // Run 1: the reviewer confirms a cold-day attack; round 2 fixes it.
    let script = vec![
        reply(FABLE, vec![text("Designing from the spec."), tool_use("o1", "design", json!({}))], 3000, 200),
        reply(FABLE, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "span fails by -10 mm; fix it"}))], 3500, 150),
        reply(OPUS, vec![tool_use("s1", "edit_design", json!({"patch_json": "{\"span_mm\": 105}", "why": "reach target"}))], 2000, 100),
        reply(OPUS, vec![tool_use("s2", "check", json!({}))], 2100, 50),
        reply(OPUS, vec![tool_use("s3", "report", json!({"summary": "span 90 -> 105 mm; all checks pass"}))], 2200, 80),
        reply(FABLE, vec![tool_use("o3", "submit", json!({"summary": "span 105 mm, all checks pass"}))], 4000, 100),
        reply(OPUS, vec![tool_use("r1", "attack", json!({"claim": "a colder day shrinks the span 8 mm", "spec_patch_json": "{\"shrink_mm\": 8}", "expect_failing_check": "span"}))], 3200, 300),
        reply(OPUS, vec![tool_use("r2", "verdict", json!({"summary": "span has no cold-day margin"}))], 3300, 100),
        reply(FABLE, vec![tool_use("o4", "edit_design", json!({"patch_json": "{\"span_mm\": 110}", "why": "cover the cold-day case"}))], 5000, 100),
        reply(FABLE, vec![tool_use("o5", "check", json!({}))], 5100, 50),
        reply(FABLE, vec![tool_use("o6", "submit", json!({"summary": "span 110 mm"}))], 5200, 50),
        reply(OPUS, vec![tool_use("r3", "verdict", json!({"summary": "no blocking issues"}))], 3000, 100),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let mut cfg = RunConfig::new("run-1", "a span for a 100 mm gap under $50", spec());
    cfg.regressions_path = Some(regressions.clone());
    let out = harness::run(client(transport.clone(), "run-1"), tools::<Toy>(), cfg).await;
    assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);

    // No lessons existed, so run 1's prompts carry no section.
    {
        let reqs = transport.requests.lock().unwrap();
        assert!(!system_text(&reqs[0].body).contains("Lessons from earlier runs"));
    }
    let stored = memory::load(&lessons, "toy").unwrap();
    assert_eq!(stored.len(), 1, "{stored:?}");
    let l = &stored[0];
    assert_eq!(l.kind, Kind::Attack);
    assert_eq!(l.run, "run-1");
    assert_eq!(l.date.len(), 10);
    assert!(l.text.contains("a colder day shrinks the span 8 mm") && l.text.contains("span failed under spec patch {\"shrink_mm\":8}"), "{}", l.text);
    assert!(l.fix.as_deref().unwrap().contains("{\"span_mm\": 110}"), "{:?}", l.fix);
    assert_eq!(l.key, "attack:span:shrink_mm");
    assert_eq!(l.request, "a span for a 100 mm gap under $50");

    // Run 2 of the same domain, a different request: the lesson reaches
    // the orchestrator and the specialist, with provenance.
    let script = vec![
        reply(FABLE, vec![tool_use("o1", "design", json!({}))], 3000, 200),
        reply(FABLE, vec![tool_use("o2", "delegate", json!({"role": "generalist", "task": "span fails; fix it with cold-day margin"}))], 3500, 150),
        reply(OPUS, vec![tool_use("s1", "edit_design", json!({"patch_json": "{\"span_mm\": 130}", "why": "cold-day margin"}))], 2000, 100),
        reply(OPUS, vec![tool_use("s2", "report", json!({"summary": "span 130 mm"}))], 2200, 80),
        reply(FABLE, vec![tool_use("o3", "submit", json!({"summary": "span 130 mm"}))], 4000, 100),
        reply(OPUS, vec![tool_use("r1", "verdict", json!({"summary": "fine"}))], 3000, 100),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let mut cfg = RunConfig::new("run-2", "a longer span for a 120 mm gap, cold garage", json!({"target_mm": 120.0, "budget_usd": 50.0}));
    cfg.regressions_path = Some(regressions.clone());
    let out = harness::run(client(transport.clone(), "run-2"), tools::<Toy>(), cfg).await;
    assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);
    let reqs = transport.requests.lock().unwrap();
    let orch = system_text(&reqs[0].body);
    let i = orch.find("Lessons from earlier runs").expect("orchestrator told the lessons");
    let section = &orch[i..];
    assert!(section.contains("- [reviewer attack] a colder day shrinks the span 8 mm (span failed under spec patch {\"shrink_mm\":8}, margin -3.000 when confirmed) -- fix: "), "{section}");
    assert!(section.contains("{\"span_mm\": 110}") && section.contains("(run run-1, "), "{section}");
    assert!(section.contains(lessons.to_str().unwrap()), "names its source file: {section}");
    let spec_sys = system_text(&reqs[2].body);
    assert_eq!(reqs[2].body["model"], OPUS);
    assert!(spec_sys.contains("Lessons from earlier runs") && spec_sys.contains("(run run-1, "), "specialist told too");
    drop(reqs);
    // Run 2 confirmed nothing new: the file is unchanged (dedupe, append-only).
    assert_eq!(memory::load(&lessons, "toy").unwrap().len(), 1);
    assert_eq!(wright_agents::state::load_regressions(&regressions, "toy").unwrap().len(), 1, "the regressions file is untouched");

    let _ = std::fs::remove_file(regressions);
    let _ = std::fs::remove_file(lessons);
}

#[tokio::test]
async fn a_check_that_never_passes_and_an_unfixed_attack_are_remembered_as_such() {
    let regressions = tmp("regressions-toy-never.jsonl");
    let lessons = memory::lessons_path(&regressions);
    let _ = std::fs::remove_file(&lessons);
    // One round: design (span fails), submit without fixing; the reviewer
    // cites the failing check and confirms an attack; the run ends on the
    // round limit with span still failing.
    let script = vec![
        reply(FABLE, vec![tool_use("o1", "design", json!({}))], 3000, 200),
        reply(FABLE, vec![tool_use("o2", "submit", json!({"summary": "giving up"}))], 4000, 100),
        reply(OPUS, vec![tool_use("r1", "cite_failing_check", json!({"check": "span", "argument": "it is short"}))], 3000, 100),
        reply(OPUS, vec![tool_use("r2", "attack", json!({"claim": "a cold day shrinks it 2 mm", "spec_patch_json": "{\"shrink_mm\": 2}", "expect_failing_check": "span"}))], 3200, 300),
        reply(OPUS, vec![tool_use("r3", "verdict", json!({"summary": "short"}))], 3300, 100),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let mut cfg = RunConfig::new("run-9", "a 100 mm span", spec());
    cfg.regressions_path = Some(regressions.clone());
    cfg.max_rounds = 1;
    let out = harness::run(client(transport, "run-9"), tools::<Toy>(), cfg).await;
    assert_eq!(out.stop, StopReason::RoundLimit, "{:?}", out.stop);

    let stored = memory::load(&lessons, "toy").unwrap();
    let kinds: Vec<Kind> = stored.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, vec![Kind::Attack, Kind::NeverPassed], "{stored:?}");
    assert_eq!(stored[0].fix.as_deref(), Some("NOT fixed by the end of the run"));
    assert!(stored[1].text.starts_with("span failed in every one of 1 round(s); last: "), "{}", stored[1].text);
    assert_eq!(stored[1].key, "never:span");
    let _ = std::fs::remove_file(regressions);
    let _ = std::fs::remove_file(lessons);
}
