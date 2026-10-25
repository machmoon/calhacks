//! `wright`: design, check and build a vehicle from a spec, without agents
//! (the agents drive the same three steps through `wright-agents`).
//!
//!     wright aircraft design spec.json -o out/     # resolve, check, build
//!     wright rocket check design.json --spec spec.json
//!     wright balloon schema                        # the spec's JSON Schema
//!     wright slice part.stl --printer bambu_a1 --material pla_aero --vase
//!     wright run aircraft "a VTOL that flies 200 km"
//!         # the agent team, every event recorded to events.jsonl for replay
//!     wright                # (or `wright app`) the desktop app: start a run,
//!                           # watch it, draw on it, replay a recording

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use wright_core::Domain;

mod bench;
mod electronics;
mod merge;
mod runs_cmd;
mod run_archive;
mod stages;
mod trace_cmd;

fn usage() -> ! {
    eprintln!(
        "usage:\n  wright <aircraft|rocket|balloon|electronics> design SPEC.json [-o DIR]\n  \
         wright <aircraft|rocket|balloon> check DESIGN.json --spec SPEC.json\n  \
         wright <aircraft|rocket|balloon> schema\n  \
         wright slice PART.stl [--printer ID] [--material ID] [--vase] [-o OUT.gcode]\n  \
         wright run <aircraft|rocket|balloon> \"REQUEST\" [--spec SPEC.json] [--demo] [--model ROLE=MODEL[:EFFORT]] [-o DIR]\n  \
         wright run electronics \"REQUEST\" [-o DIR] [--team] [--model ROLE=MODEL[:EFFORT]]   # PCB: spec + groups by Claude, place/judge/route, kicad-cli ERC/DRC/parity, STEP/SVG/GLB\n  \
         wright run --resume RUN_DIR   # continue a crashed, stopped or budget-stopped run from RUN_DIR/checkpoint.json\n  \
         wright [app] [--replay EVENTS.jsonl]   # the desktop app (also the `wright-desktop` binary)\n  \
         wright report RUN_DIR      # rebuild RUN_DIR/guide from a finished run's recorded build files\n  \
         wright stages RUN_DIR [--only NAME] [--force]   # re-run the post-design stages (CFD, FEA, fab, CAD, ...) on a finished run\n  \
         wright bench [SCENARIO_DIR] [-o RESULTS.json] [--shard i/n]   # offline agent-team benchmark (scripted models, no spend); --shard runs one slice\n  \
         wright merge SHARD.json... [-o OUT.json]   # join sharded bench / sweep results (docs/aci.md)\n  \
         wright train export RUN_DIR... [-o DIR] [--resolved-only]   # recorded runs as post-training trajectories (docs/post-training.md)\n  \
         wright trace RUN_DIR [--json] [--tool NAME]   # per-model and per-tool calls, p50/p95 latency, error rate, USD from RUN_DIR/trace.jsonl\n  \
         wright runs import [DIR...]   # index demo/, out/, runs/ + lessons/regressions into the runs database (WRIGHT_RUNS_URI, default data/runs-db.jsonl)\n  \
         wright runs search \"TEXT\" [--domain D] [--spec SPEC.json] [-n N] [--json]   # nearest prior efforts"
    );
    std::process::exit(2)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = |rest: &[String]| -> anyhow::Result<()> {
        let mut o = wright_desktop::Options::from_args(rest)?;
        o.stages = stages::registry();
        wright_desktop::launch(o)
    };
    let Some(first) = args.first() else { return app(&[]) };
    match first.as_str() {
        "app" => app(&args[1..]),
        #[cfg(feature = "air")]
        "aircraft" => run::<wright_air::Aircraft>(&args[1..]),
        #[cfg(not(feature = "air"))]
        "aircraft" => anyhow::bail!("this build has no aircraft domain (build with --features air)"),
        "rocket" => run::<wright_rocket::Rocket>(&args[1..]),
        "balloon" => run::<wright_balloon::Balloon>(&args[1..]),
        "electronics" => run::<wright_electronics::Electronics>(&args[1..]),
        "slice" => slice(&args[1..]),
        "run" => agents(&args[1..]),
        "report" => report_run(&args[1..]),
        "stages" => stages::stages_cmd(&args[1..]),
        "bench" => bench::bench_cmd(&args[1..]),
        "merge" => merge::merge_cmd(&args[1..]),
        "train" => train(&args[1..]),
        "trace" => trace_cmd::trace_cmd(&args[1..]),
        "runs" => runs_cmd::runs_cmd(&args[1..], &data_dir()),
        _ => usage(),
    }
}

/// `wright train export RUN_DIR... [-o DIR] [--resolved-only]`: every
/// recorded agent conversation of the runs as one JSON line, tagged by the
/// check report (`wright-train`, docs/post-training.md). Default output
/// `out/train/`.
fn train(args: &[String]) -> anyhow::Result<()> {
    if args.first().map(String::as_str) != Some("export") {
        usage()
    }
    let out = PathBuf::from(flag(args, "-o").unwrap_or("out/train"));
    let resolved_only = args.iter().any(|a| a == "--resolved-only");
    let mut dirs = vec![];
    let mut rest = args[1..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "-o" => {
                rest.next();
            }
            a if a.starts_with("--") => {}
            a => dirs.push(PathBuf::from(a)),
        }
    }
    if dirs.is_empty() {
        usage()
    }
    print!("{}", wright_train::export(&dirs, &out, resolved_only)?);
    Ok(())
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn read<T: serde::de::DeserializeOwned>(path: &str) -> anyhow::Result<T> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {path}"))
}

fn run<D: Domain>(args: &[String]) -> anyhow::Result<()> {
    let Some(cmd) = args.first() else { usage() };
    match cmd.as_str() {
        "schema" => {
            println!("{}", serde_json::to_string_pretty(&D::spec_schema())?);
            Ok(())
        }
        "design" => {
            let Some(spec_path) = args.get(1) else { usage() };
            let spec: D::Spec = read(spec_path)?;
            let out = PathBuf::from(flag(args, "-o").unwrap_or("out"));
            std::fs::create_dir_all(&out)?;
            let design = D::design(&spec)?;
            std::fs::write(out.join("design.json"), serde_json::to_string_pretty(&design)?)?;
            report::<D>(&spec, &design, &out)
        }
        "check" => {
            let (Some(design_path), Some(spec_path)) = (args.get(1), flag(args, "--spec")) else { usage() };
            let spec: D::Spec = read(spec_path)?;
            let design: D::Design = read(design_path)?;
            let finished = D::check(&spec, &design);
            println!("{finished}");
            if finished.passed() { Ok(()) } else { std::process::exit(1) }
        }
        _ => usage(),
    }
}

fn report<D: Domain>(spec: &D::Spec, design: &D::Design, out: &Path) -> anyhow::Result<()> {
    let finished = D::check(spec, design);
    let bom = D::bom(design);
    std::fs::write(out.join("checks.json"), serde_json::to_string_pretty(&finished)?)?;
    std::fs::write(out.join("bom.csv"), bom.csv())?;
    let artifacts = D::build(design, out)?;
    let (objective, label) = D::objective(design);
    // the builder's guide, from whatever the domain built
    let title = format!("{} build guide", D::NAME);
    let summary = format!(
        "{} of {} checks pass. {label}: {objective:.1}. Parts ${:.2}.",
        finished.outcomes.len() - finished.failures().count(),
        finished.outcomes.len(),
        bom.total().as_usd()
    );
    match wright_guide::from_artifacts(&title, &summary, &artifacts, &bom, &finished, None, &out.join("guide")) {
        Ok(p) => println!("  build guide: {}", p.display()),
        Err(e) => eprintln!("  build guide not written: {e:#}"),
    }
    println!("{finished}");
    println!("{label}: {objective:.2}");
    println!("BOM ${:.2} ({} unpriced lines)", bom.total().as_usd(), bom.unpriced().len());
    for (label, path) in &artifacts.files {
        println!("  {label}: {}", path.display());
    }
    if finished.passed() { Ok(()) } else { std::process::exit(1) }
}

/// Rebuild a finished run's build guide from what it recorded, with no
/// model calls: the design and check report from `state.json`, the build's
/// files from the `Artifact` events in `events.jsonl` (the same list
/// `Domain::build` returned), the BOM from the domain.
fn report_run(args: &[String]) -> anyhow::Result<()> {
    let Some(dir) = args.first().map(PathBuf::from) else { usage() };
    let state: serde_json::Value = read(&dir.join("state.json").to_string_lossy())?;
    let domain = state["domain"].as_str().context("state.json has no domain")?;
    let tools = match domain {
        #[cfg(feature = "air")]
        "aircraft" => wright_agents::tools::<wright_air::Aircraft>(),
        "rocket" => wright_agents::tools::<wright_rocket::Rocket>(),
        "balloon" => wright_agents::tools::<wright_balloon::Balloon>(),
        "electronics" => wright_agents::tools::<wright_electronics::Electronics>(),
        other => bail!("no domain {other} in this build"),
    };
    let report: wright_core::Finished = serde_json::from_value(state["report"].clone()).context("state.json report")?;
    let bom = tools.bom(&state["design"]).context("the recorded design no longer reads as a design of this domain")?;
    let arts = recorded_artifacts(&dir, domain)?;
    let request = state["request"].as_str().unwrap_or("");
    let line = format!("Designed by Wright's agents from: \"{request}\". {} of {} checks pass.", report.outcomes.len() - report.failures().count(), report.outcomes.len());
    let g = wright_guide::from_artifacts(&format!("{domain} build guide"), &line, &arts, &bom, &report, None, &dir.join("guide"))?;
    println!("build guide: {}", g.display());
    if !stages::registry().names(domain).is_empty() {
        stages::stages_cmd(&[dir.to_string_lossy().into_owned()])?;
    }
    Ok(())
}

/// The build's files as a run's `Artifact` events recorded them (the list
/// `Domain::build` returned), found again under `dir`; not the guide, the
/// state file, or a post-design stage's outputs.
fn recorded_artifacts(dir: &Path, domain: &str) -> anyhow::Result<wright_core::Artifacts> {
    let events = std::fs::read_to_string(dir.join("events.jsonl")).with_context(|| format!("reading {}/events.jsonl", dir.display()))?;
    let stage_names = stages::registry().names(domain);
    let mut arts = wright_core::Artifacts::default();
    for line in events.lines() {
        let Ok(wright_core::Event::Artifact { label, path, .. }) = serde_json::from_str(line) else { continue };
        let first = path.split('/').next().unwrap_or("");
        if label == "design state" || path.starts_with("guide/") || path.starts_with("wip/") || stage_names.contains(&first) {
            continue;
        }
        let p = locate(dir, &path).with_context(|| format!("recorded file {path} is not in {}", dir.display()))?;
        if !arts.files.iter().any(|(_, q)| q == &p) {
            arts.add(label, p);
        }
    }
    if arts.files.is_empty() {
        bail!("{}/events.jsonl records no build files (did the design pass?)", dir.display());
    }
    Ok(arts)
}

/// A path an event recorded (relative to the recording's working
/// directory, or absolute), found again under `dir`.
fn locate(dir: &Path, recorded: &str) -> Option<PathBuf> {
    let p = Path::new(recorded);
    if p.exists() {
        return Some(p.to_path_buf());
    }
    let comps: Vec<_> = p.components().collect();
    (0..comps.len()).map(|k| dir.join(comps[k..].iter().collect::<PathBuf>())).find(|c| c.exists())
}

/// Where Wright's shared data lives (`data/`: catalogs, regressions):
/// `WRIGHT_DATA_DIR`, else the repo's `data/` this binary was built from,
/// else `data/` beside the binary. Never the working directory: a run
/// started elsewhere silently used (or created) another regressions file
/// (review 4, finding 5).
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("WRIGHT_DATA_DIR") {
        return PathBuf::from(d);
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    if repo.is_dir() {
        return std::fs::canonicalize(&repo).unwrap_or(repo);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("data"))).unwrap_or_else(|| PathBuf::from("data"))
}

fn slice(args: &[String]) -> anyhow::Result<()> {
    use wright_geom::slice::{material, printer, slice, Recipe};
    let Some(stl) = args.first() else { usage() };
    let p = printer(flag(args, "--printer").unwrap_or("prusa_mk4"))?;
    let m = material(flag(args, "--material").unwrap_or("pla"))?;
    let recipe = if args.iter().any(|a| a == "--vase") {
        Recipe::Vase
    } else {
        Recipe::Structural { perimeters: 3, infill_pct: 15 }
    };
    let out = flag(args, "-o").map(PathBuf::from).unwrap_or_else(|| Path::new(stl).with_extension("gcode"));
    let s = slice(Path::new(stl), &out, p, m, recipe)?;
    if s.generic_gcode {
        eprintln!("note: {} wants a sliced .gcode.3mf from Bambu Studio/OrcaSlicer; this is generic G-code for time and grams", s.printer);
    }
    println!("{}: {:.1} g, {:.1} h -> {}", s.printer, s.grams, s.seconds / 3600.0, s.gcode.display());
    if s.grams <= 0.0 {
        bail!("the slicer reported no filament");
    }
    Ok(())
}

/// The agent team on one request, headless: every event appended to
/// `DIR/events.jsonl` so the desktop app (`wright-desktop --replay`) can
/// show the run again at the table without a network. The desktop app runs
/// the same harness in-process when a person wants to watch and draw.
fn agents(args: &[String]) -> anyhow::Result<()> {
    // `wright run --resume RUN_DIR`: domain, request and -o come from RUN_DIR/checkpoint.json (wright_agents::checkpoint)
    let (args, resume) = wright_agents::checkpoint::resume_args(args)?;
    let args = &args[..];
    // `wright run --domain electronics "REQ"` reads like `wright run electronics "REQ"`
    let args: Vec<String> = match args.first().map(String::as_str) {
        Some("--domain") => args[1..].to_vec(),
        _ => args.to_vec(),
    };
    let args = &args[..];
    let (Some(domain), Some(request)) = (args.first(), args.get(1)) else { usage() };
    // electronics: the fast agent path (spec + groups by Claude, deterministic
    // pipeline, kicad-cli); `--team` runs the full agent team instead
    if domain == "electronics" && resume.is_none() && !args.iter().any(|a| a == "--team") {
        return electronics::run(request, args);
    }
    let tools = match domain.as_str() {
        #[cfg(feature = "air")]
        "aircraft" => wright_agents::tools::<wright_air::Aircraft>(),
        #[cfg(not(feature = "air"))]
        "aircraft" => anyhow::bail!("this build has no aircraft domain (build with --features air)"),
        "rocket" => wright_agents::tools::<wright_rocket::Rocket>(),
        "balloon" => wright_agents::tools::<wright_balloon::Balloon>(),
        "electronics" => wright_agents::tools::<wright_electronics::Electronics>(),
        _ => usage(),
    };
    let spec: serde_json::Value = match flag(args, "--spec") {
        Some(p) => read(p)?,
        None => serde_json::Value::Null,
    };
    let run_id = match &resume { Some(ck) => ck.run_id.clone(), None => format!("{}-{}", domain, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()) };
    let out = PathBuf::from(flag(args, "-o").map(str::to_string).unwrap_or_else(|| format!("runs/{run_id}")));
    std::fs::create_dir_all(&out)?;
    // --demo / WRIGHT_DEMO=1: Fable 5.1 on the orchestrator (wright_agents::roles)
    let demo = args.iter().any(|a| a == "--demo") || wright_agents::roles::demo_from_env();
    // the role table, WRIGHT_MODEL_<ROLE> env, then `--model role=model[:effort[:max_tokens]]` flags
    let mut models = wright_agents::Models::from_env(demo).map_err(|e| anyhow::anyhow!(e))?;
    for (i, a) in args.iter().enumerate() {
        if a == "--model" {
            let v = args.get(i + 1).ok_or_else(|| anyhow::anyhow!("--model needs role=model[:effort]"))?;
            let (role, m) = v.split_once('=').ok_or_else(|| anyhow::anyhow!("--model {v}: expected role=model[:effort[:max_tokens]]"))?;
            models.set(role, m).map_err(|e| anyhow::anyhow!("--model {v}: {e}"))?;
        }
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let (tx, _rx) = wright_agents::channel();
        // record every event for replay
        let mut rec = tx.subscribe();
        let log = out.join("events.jsonl");
        // The recorder stops on `done` (after draining what is queued), not
        // on the channel closing (review 4, finding 17).
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();
        let recorder = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            use tokio::sync::broadcast::error::{RecvError, TryRecvError};
            // append: a resumed run continues its own events.jsonl
            let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open(&log).await else { return };
            let write = |ev: wright_core::Event| serde_json::to_string(&ev).map(|l| format!("{l}\n")).ok();
            loop {
                tokio::select! {
                    r = rec.recv() => match r {
                        Ok(ev) => if let Some(line) = write(ev) {
                            let _ = f.write_all(line.as_bytes()).await;
                            let _ = f.flush().await;
                        },
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    },
                    _ = &mut done_rx => {
                        loop {
                            match rec.try_recv() {
                                Ok(ev) => if let Some(line) = write(ev) { let _ = f.write_all(line.as_bytes()).await; },
                                Err(TryRecvError::Lagged(_)) => continue,
                                Err(_) => break,
                            }
                        }
                        let _ = f.flush().await;
                        break;
                    }
                }
            }
        });
        let transport = wright_agents::transport_from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
        let client = wright_agents::Client::new(transport, wright_agents::Budget::from_env(), tx.clone(), run_id.clone());
        let mut cfg = wright_agents::RunConfig::new(&run_id, request, spec);
        cfg.models = models;
        // WRIGHT_MAX_ROUNDS: a shorter run for A/B evals (scripts/ab-orchestrator.sh)
        if let Some(n) = std::env::var("WRIGHT_MAX_ROUNDS").ok().and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 0) {
            cfg.max_rounds = n;
        }
        // the design search memoized across rounds and reruns (~/.wright/cache/design-<domain>; WRIGHT_CACHE=0 turns it off)
        wright_agents::tools::enable_design_cache(true);
        cfg.out_dir = Some(out.clone());
        cfg.regressions_path = Some(data_dir().join(format!("regressions-{domain}.jsonl")));
        cfg.stages = stages::registry();
        // prior efforts in, this run's record out (wright_agents::runs_db; WRIGHT_RUNS_URI=off disables)
        cfg.runs_db = wright_agents::runs_db::from_env(&data_dir()).unwrap_or_else(|e| {
            eprintln!("runs database disabled: {e:#}");
            None
        });
        // proposer-vs-reviewer rebuttal before confirmed attacks are stored (WRIGHT_NO_REBUTTAL=1 turns it off)
        cfg.rebuttal = std::env::var_os("WRIGHT_NO_REBUTTAL").is_none();
        cfg.resume = resume;
        // spec elicitation + feasibility probe before the team starts, unattended (WRIGHT_NO_ELICIT=1 turns it off)
        if std::env::var_os("WRIGHT_NO_ELICIT").is_none() {
            cfg.elicit = Some(wright_agents::elicit::ElicitConfig::new(cfg.models.elicit.clone(), wright_agents::elicit::HumanInput::Never));
        }
        let summary = wright_agents::run(client, tools.clone(), cfg).await;
        println!(
            "{}: {:?} after {} rounds, ${:.2}{}",
            run_id,
            summary.stop,
            summary.rounds,
            summary.spent_usd,
            summary.objective.as_ref().map(|(v, l)| format!(", {l} {v:.1}")).unwrap_or_default()
        );
        println!("files and events.jsonl in {}", out.display());
        // the builder's guide from what the build wrote (a passing design
        // only), its renders sent as events and recorded for replay
        if let Some(g) = wright_desktop::live::finish_guide(&tx, tools.as_ref(), &summary, &run_id, request, domain, &out) {
            println!("build guide: {}", g.display());
        }
        // the Build Desk (Creao): only for a design that passed, and only
        // when an agent is configured (CREAO_APP_ID in .env)
        if summary.passed && args.iter().all(|a| a != "--no-creao") {
            match build_desk(&tx, tools.as_ref(), &summary, &run_id, request, &out).await {
                Ok(Some(line)) => println!("{line}"),
                Ok(None) => eprintln!("build desk: skipped (no CREAO_APP_ID in .env; see docs/creao.md)"),
                Err(e) => eprintln!("build desk failed: {e:#}"),
            }
        }
        let _ = done_tx.send(());
        let _ = recorder.await;
        drop(tx);
        anyhow::Ok(())
    })
}

/// Hand a passing design to the Creao Build Desk: it prices every BOM line
/// from live listings and writes the builder's page; Wright validates every
/// quote (`wright_creao::desk`). `Ok(None)` when no agent is configured.
async fn build_desk(
    tx: &tokio::sync::broadcast::Sender<wright_core::Event>,
    tools: &dyn wright_agents::DomainTools,
    summary: &wright_agents::RunSummary,
    run_id: &str,
    request: &str,
    out: &Path,
) -> anyhow::Result<Option<String>> {
    let dotenv = Path::new(".env");
    let env_text = std::fs::read_to_string(dotenv).unwrap_or_default();
    if wright_creao::config::dotenv_get(&env_text, "CREAO_APP_ID").is_none() && std::env::var("CREAO_APP_ID").is_err() {
        return Ok(None);
    }
    let cfg = wright_creao::Config::load(dotenv)?;
    let client = wright_creao::Client::new(cfg, wright_creao::transport::Reqwest::new()?);
    let state = &summary.state;
    let design = state.design.as_ref().ok_or_else(|| anyhow::anyhow!("no design"))?;
    let bom = tools.bom(design)?;
    let checks = state.report.as_ref().map(|r| r.outcomes.iter().map(Into::into).collect()).unwrap_or_default();
    let mut files = vec![];
    if let Ok(rd) = std::fs::read_dir(out) {
        for e in rd.flatten() {
            files.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    let objective = summary.objective.as_ref().map(|(v, l)| format!("{l} {v:.1}")).unwrap_or_default();
    let packet = wright_creao::DesignPacket {
        vehicle: request.to_string(),
        domain: state.domain.clone(),
        summary: format!("{request}. All checks pass. {objective}."),
        bom,
        checks,
        build_steps: vec![],
        files,
    };
    let desk_dir = out.join("build_desk");
    std::fs::create_dir_all(&desk_dir)?;
    let res = wright_creao::run_desk(&client, run_id, &packet, &desk_dir, Default::default()).await?;
    for ev in res.events {
        let _ = tx.send(ev);
    }
    std::fs::write(desk_dir.join("bom-sourced.csv"), res.sourcing.bom.csv())?;
    Ok(Some(format!(
        "build desk: {} lines priced from live listings, {} rejected, {} still unpriced; ${:.2} -> ${:.2}",
        res.sourcing.filled.len(),
        res.sourcing.rejected.len(),
        res.sourcing.still_unpriced.len(),
        res.sourcing.total_before_usd,
        res.sourcing.total_after_usd
    )))
}
