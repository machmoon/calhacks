//! The post-design pipeline through the harness, offline: a stub FEA
//! stage the orchestrator runs mid-design with `run_stage` (its failing
//! check comes back as evidence), and every stage after a passing build,
//! where a crashing stage never fails the run.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, Manifest, Report, Source, Stage, StageCtx, Stages};

const S: &str = "claude-sonnet-5-5";

struct Beam;
#[derive(Clone, Serialize, Deserialize)]
struct Spec {
    load_n: f64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Design {
    depth_mm: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Strength,
}
impl CheckId for C {
    const ALL: &'static [Self] = &[C::Strength];
    fn name(self) -> &'static str {
        "strength"
    }
    fn describe(self) -> &'static str {
        "hand calc"
    }
}
impl Domain for Beam {
    const NAME: &'static str = "beam";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object"})
    }
    fn design(_: &Spec) -> anyhow::Result<Design> {
        Ok(Design { depth_mm: 10.0 })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Strength, d.depth_mm * 10.0 - s.load_n, "N", Source::Requirement, "");
        r.finish()
    }
    fn bom(_: &Design) -> Bom {
        Bom::default()
    }
    fn build(d: &Design, out: &Path) -> anyhow::Result<Artifacts> {
        std::fs::create_dir_all(out)?;
        let p = out.join("beam.stl");
        std::fs::write(&p, format!("solid beam depth {}", d.depth_mm))?;
        let mut a = Artifacts::default();
        a.add("beam", p);
        Ok(a)
    }
    fn objective(d: &Design) -> (f64, &'static str) {
        (-d.depth_mm, "lightness")
    }
}

/// The FEA stub: a hot spot at the root unless the beam is at least 14 mm deep.
struct Fea;
impl Stage for Fea {
    fn name(&self) -> &'static str {
        "fea"
    }
    fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
        assert!(!ctx.artifacts.files.is_empty(), "the stage gets the build");
        let depth = ctx.design["depth_mm"].as_f64().unwrap();
        let stress = 2000.0 / depth;
        let f = ctx.dir.join("stress.png");
        std::fs::write(&f, b"png")?;
        let mut m = Manifest::new("fea", "FEA (stub)");
        m.add(&ctx.dir, &f, "von Mises", "root stress", &[("max_mpa", stress)]);
        m.check("root_stress", stress <= 150.0, &format!("{stress:.0} MPa vs 150 MPa allowable at the root"));
        Ok(m)
    }
}
struct Crash;
impl Stage for Crash {
    fn name(&self) -> &'static str {
        "sim"
    }
    fn run(&self, _: &StageCtx) -> anyhow::Result<Manifest> {
        anyhow::bail!("solver not installed")
    }
}

fn stages() -> Stages {
    let mut s = Stages::new();
    s.register(Fea).register(Crash);
    s
}

#[tokio::test]
async fn fea_evidence_drives_an_edit_and_the_pipeline_runs_after_the_build() {
    let out = std::env::temp_dir().join(format!("wright-stages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        reply(S, vec![tool_use("o2", "run_stage", json!({"stage": "fea"}))], 100, 10),
        // the FEA hot spot is evidence: deepen the beam and look again
        reply(S, vec![tool_use("o3", "edit_design", json!({"patch_json": "{\"depth_mm\": 15}", "why": "FEA root_stress 200 MPa"}))], 100, 10),
        reply(S, vec![tool_use("o4", "run_stage", json!({"stage": "fea"}))], 100, 10),
        reply(S, vec![tool_use("o5", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, mut rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "a beam", json!({"load_n": 50.0}));
    cfg.max_rounds = 1;
    cfg.out_dir = Some(out.clone());
    cfg.stages = stages();
    let s = harness::run(c, tools::<Beam>(), cfg).await;

    // the orchestrator was offered run_stage with the registered names
    let req0 = t.requests.lock().unwrap()[0].body.clone();
    let rs = req0["tools"].as_array().unwrap().iter().find(|x| x["name"] == "run_stage").expect("run_stage offered");
    assert_eq!(rs["input_schema"]["properties"]["stage"]["enum"], json!(["fea", "sim"]));
    assert!(req0["system"].to_string().contains("run_stage"));

    let results: Vec<String> = t
        .requests
        .lock()
        .unwrap()
        .iter()
        .flat_map(|r| r.body["messages"].as_array().unwrap().last().unwrap()["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(results.iter().any(|r| r.contains("check root_stress: FAIL 200 MPa")), "{results:?}");
    assert!(results.iter().any(|r| r.contains("check root_stress: PASS 133 MPa")), "{results:?}");
    assert!(s.state.notes.iter().any(|n| n.starts_with("stage fea (round 1)") && n.contains("FAIL")), "evidence is shared: {:?}", s.state.notes);
    assert!(out.join("wip/fea/manifest.json").exists(), "mid-design stages write under wip/");

    // after the passing build: every stage, the crash contained
    assert!(s.passed, "{:?}", s.stop);
    let by: Vec<(&str, bool)> = s.stages.iter().map(|r| (r.name.as_str(), r.manifest.ok())).collect();
    assert_eq!(by, [("fea", true), ("sim", false)]);
    let sim = Manifest::read(&out.join("sim")).unwrap();
    assert!(sim.error.unwrap().contains("solver not installed"));
    let fea = Manifest::read(&out.join("fea")).unwrap();
    assert!(fea.checks[0].passed && fea.input_hash.is_some());

    let mut artifacts = vec![];
    let mut said = vec![];
    while let Ok(e) = rx.try_recv() {
        match e {
            Event::Artifact { label, path, .. } => artifacts.push((label, path)),
            Event::AgentMessage { agent, text, .. } if agent == "harness" => said.push(text),
            _ => {}
        }
    }
    assert!(artifacts.iter().any(|(l, p)| l == "FEA (stub): von Mises" && p == "fea/stress.png"), "{artifacts:?}");
    assert!(artifacts.iter().any(|(_, p)| p == "wip/fea/stress.png"), "mid-design items are shown live too: {artifacts:?}");
    assert!(said.iter().any(|m| m == "stage sim: failed: solver not installed"), "{said:?}");

    // re-running on the same build: fea is up to date, sim runs again
    let base = StageCtx {
        domain: "beam".into(),
        out_dir: out.clone(),
        dir: out.clone(),
        design: s.state.design.clone().unwrap(),
        spec: s.state.effective_spec(),
        artifacts: s.artifacts.clone().unwrap(),
        upstream: Default::default(),
    };
    let (tx, _rx) = channel();
    let again = harness::run_stages(&tx, "run-1", &stages(), &base, None, false);
    assert!(again[0].fresh && !again[1].fresh);
    let _ = std::fs::remove_dir_all(&out);
}

#[tokio::test]
async fn without_stages_there_is_no_run_stage_tool() {
    let t = Arc::new(ScriptedTransport::new(vec![reply(S, vec![tool_use("o1", "submit", json!({"summary": "s"}))], 100, 10), reply(S, vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 100, 10)]));
    let (tx, _rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "a beam", json!({"load_n": 50.0}));
    cfg.max_rounds = 1;
    let s = harness::run(c, tools::<Beam>(), cfg).await;
    assert!(s.stages.is_empty());
    let req0 = t.requests.lock().unwrap()[0].body.clone();
    assert!(req0["tools"].as_array().unwrap().iter().all(|x| x["name"] != "run_stage"));
    assert!(!req0["system"].to_string().contains("run_stage"));
}

/// Cost + latency (10/24 night): run_stage calls made in one turn run
/// together (`Stages::run_many`), each answered in request order, with
/// the artifact events in request order whatever finished first.
#[tokio::test]
async fn run_stage_calls_in_one_turn_run_together_in_a_fixed_order() {
    struct Slow(&'static str, u64);
    impl Stage for Slow {
        fn name(&self) -> &'static str {
            self.0
        }
        fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
            std::thread::sleep(std::time::Duration::from_millis(self.1));
            let f = ctx.dir.join("out.txt");
            std::fs::write(&f, self.0)?;
            let mut m = Manifest::new(self.0, self.0);
            m.add(&ctx.dir, &f, self.0, "", &[]);
            Ok(m)
        }
    }
    let out = std::env::temp_dir().join(format!("wright-stages-par-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let script = vec![
        reply(S, vec![tool_use("o1", "design", json!({}))], 100, 10),
        // sim is slowest and asked first: its result still comes first
        reply(S, vec![tool_use("o2", "run_stage", json!({"stage": "sim"})), tool_use("p", "plan_update", json!({"id": "x", "status": "done", "note": ""})), tool_use("o3", "run_stage", json!({"stage": "fab"})), tool_use("o4", "run_stage", json!({"stage": "fea"}))], 100, 10),
        reply(S, vec![tool_use("o5", "submit", json!({"summary": "s"}))], 100, 10),
        reply(S, vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 100, 10),
    ];
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, mut rx) = channel();
    let c = Client::new(t.clone(), Budget::new(10.0, 45.0, None), tx, "run-1");
    let mut cfg = RunConfig::new("run-1", "a beam", json!({"load_n": 50.0}));
    cfg.out_dir = Some(out.clone());
    cfg.max_rounds = 1;
    let mut st = Stages::new();
    st.register(Slow("sim", 600)).register(Slow("fab", 600)).register(Slow("fea", 50));
    cfg.stages = st;
    let t0 = std::time::Instant::now();
    let _ = harness::run(c, tools::<Beam>(), cfg).await;
    let took = t0.elapsed();
    // the turn's batch (0.6 s together, 1.25 s serial) plus the pipeline after the passing build (same again)
    assert!(took < std::time::Duration::from_millis(1800), "stages ran one at a time: {took:?}");
    let reqs = t.requests.lock().unwrap();
    let results: Vec<Value> = reqs[2].body["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
    let ids: Vec<&str> = results.iter().filter_map(|b| b["tool_use_id"].as_str()).collect();
    assert_eq!(ids, vec!["o2", "p", "o3", "o4"]);
    for (i, name) in [(0, "sim"), (2, "fab"), (3, "fea")] {
        assert!(results[i]["content"].as_str().unwrap().contains(name), "{:?}", results[i]);
    }
    drop(reqs);
    let mut arts = vec![];
    while let Ok(e) = rx.try_recv() {
        if let Event::Artifact { path, .. } = e {
            arts.push(path);
        }
    }
    let order: Vec<&str> = arts.iter().filter_map(|p| ["sim", "fab", "fea"].into_iter().find(|n| p.contains(&format!("wip/{n}/")))).collect();
    assert_eq!(&order[..3], &["sim", "fab", "fea"], "{arts:?}");
    let _ = std::fs::remove_dir_all(&out);
}
