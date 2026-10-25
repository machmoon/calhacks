//! The post-design stages this build runs (CFD, FEA, fab, CAD, USD, sims),
//! and `wright stages RUN_DIR`, which re-runs them on a finished run.
//!
//! A stage implements `wright_core::Stage` in its own crate; register it
//! here with one line. The harness runs them after a passing design is
//! built (`wright_agents::harness::run_stages`), and the orchestrator can
//! run one mid-design with its `run_stage` tool.

use anyhow::{bail, Context};
use std::path::PathBuf;
use wright_core::Stages;

/// Every registered stage. Add `s.register(wright_x::XStage);` here.
pub fn registry() -> Stages {
    #[allow(unused_mut)]
    let mut s = Stages::new();
    s.register(wright_balloon::sim::SimStage);
    s.register(wright_rocket::sim::SimStage);
    s.register(wright_fea::cases::FeaStage);
    s.register(wright_fab::FabStage);
    #[cfg(feature = "air")]
    s.register(wright_air::sim::SimStage);
    #[cfg(feature = "air")]
    s.register(wright_cfd::CfdStage);
    s
}

/// `wright stages RUN_DIR [--only NAME] [--force]`: re-run the stages on a
/// recorded run (its `state.json` design and spec, and the build files its
/// events recorded), skipping those already up to date unless `--force`.
/// The new `Artifact` events are appended to the run's `events.jsonl`, so a
/// replay shows them.
pub fn stages_cmd(args: &[String]) -> anyhow::Result<()> {
    let Some(dir) = args.first().map(PathBuf::from) else { bail!("usage: wright stages RUN_DIR [--only NAME] [--force]") };
    let only = super::flag(args, "--only");
    let force = args.iter().any(|a| a == "--force");
    let state: wright_agents::DesignState = super::read(&dir.join("state.json").to_string_lossy())?;
    let design = state.design.clone().context("state.json has no design")?;
    let artifacts = super::recorded_artifacts(&dir, &state.domain)?;
    let reg = registry();
    if reg.names(&state.domain).is_empty() {
        bail!("no post-design stages are registered for {} in this build (crates/wright-cli/src/stages.rs)", state.domain);
    }
    let base = wright_core::StageCtx {
        domain: state.domain.clone(),
        out_dir: dir.clone(),
        dir: dir.clone(),
        design,
        spec: state.effective_spec(),
        artifacts,
        upstream: Default::default(),
    };
    let run_id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (tx, mut rx) = wright_agents::channel();
    let runs = wright_agents::harness::run_stages(&tx, &run_id, &reg, &base, only, force);
    let mut lines = String::new();
    while let Ok(ev) = rx.try_recv() {
        if let wright_core::Event::AgentMessage { text, .. } = &ev {
            println!("{text}");
        }
        if let Ok(l) = serde_json::to_string(&ev) {
            lines.push_str(&l);
            lines.push('\n');
        }
    }
    use std::io::Write as _;
    std::fs::OpenOptions::new().append(true).create(true).open(dir.join("events.jsonl"))?.write_all(lines.as_bytes())?;
    if runs.iter().any(|r| !r.manifest.ok()) {
        std::process::exit(1)
    }
    Ok(())
}
