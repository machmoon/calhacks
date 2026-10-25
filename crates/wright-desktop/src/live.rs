//! A live run, in-process: the same harness call `wright run` makes
//! (crates/wright-cli/src/main.rs, `agents`), on a tokio runtime on its own
//! thread. Events reach the UI through a std channel (and a repaint); the
//! person's markup goes back on the harness's broadcast channel as
//! `Event::Feedback`, where the harness's `Inbox` reads it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::broadcast;
use wright_agents::DomainTools;
use wright_core::Event;

pub const DOMAINS: [&str; 3] = ["aircraft", "rocket", "balloon"];

pub fn tools(domain: &str) -> anyhow::Result<Arc<dyn DomainTools>> {
    Ok(match domain {
        "aircraft" => wright_agents::tools::<wright_air::Aircraft>(),
        "rocket" => wright_agents::tools::<wright_rocket::Rocket>(),
        "balloon" => wright_agents::tools::<wright_balloon::Balloon>(),
        other => anyhow::bail!("no domain {other}"),
    })
}

/// Who answers the agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// `WRIGHT_TRANSPORT` (api / cli / foundry), budgeted as the CLI is.
    FromEnv,
    /// No model at all: a scripted orchestrator runs `design`, submits, and
    /// the reviewer approves. Real domain code, real checks and build, $0.
    Offline,
}

/// Where Wright's shared data lives (the CLI's `data_dir`, same rule).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("WRIGHT_DATA_DIR") {
        return PathBuf::from(d);
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    if repo.is_dir() {
        return std::fs::canonicalize(&repo).unwrap_or(repo);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("data"))).unwrap_or_else(|| PathBuf::from("data"))
}

/// The scripted transport for offline runs (`wright_agents::fake`).
pub fn offline_transport() -> Arc<dyn wright_agents::Transport> {
    use wright_agents::fake::{reply, text, tool_use, ScriptedTransport};
    let mut n = 0;
    Arc::new(ScriptedTransport::from_fn(move |_body: &Value| {
        n += 1;
        let m = "offline";
        match n {
            1 => Some(reply(m, vec![text("Offline run (no model): designing from the demo recording's spec."), tool_use("o1", "design", json!({}))], 0, 0)),
            2 => Some(reply(m, vec![tool_use("o2", "submit", json!({"summary": "offline design submitted for review"}))], 0, 0)),
            3 => Some(reply(m, vec![tool_use("r1", "verdict", json!({"summary": "offline run: no review performed"}))], 0, 0)),
            _ => None,
        }
    }))
}

/// The person's markup as the harness expects it: for this run (the
/// harness's inbox ignores any other run id, and an empty one is refused
/// here before it is sent; review 4, finding 14), with the sketch path
/// relative to the run's out dir.
pub fn feedback_event(run: &str, view: &str, sketch_rel: &str, note: &str) -> anyhow::Result<Event> {
    if run.trim().is_empty() {
        anyhow::bail!("markup needs the current run id; no run is active");
    }
    Ok(Event::Feedback { run: run.into(), view: view.into(), sketch_png: sketch_rel.into(), note: note.into() })
}

pub struct Live {
    pub run_id: String,
    pub dir: PathBuf,
    pub tx: broadcast::Sender<Event>,
    pub rx: std::sync::mpsc::Receiver<Event>,
    pub done: Arc<std::sync::atomic::AtomicBool>,
    /// The harness's last word ("Passed after 1 round, $0.16").
    pub summary: Arc<std::sync::Mutex<Option<String>>>,
}

/// The spec a recorded demo run settled on (`demo/<domain>/state.json`),
/// for offline runs: the scripted orchestrator writes no spec, and every
/// domain needs one to design.
pub fn demo_spec(domain: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo").join(domain).join("state.json");
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| v["spec"].clone())
        .unwrap_or(Value::Null)
}

pub fn start(domain: &str, request: &str, transport: Transport, stages: wright_core::Stages, repaint: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Live> {
    let tools = tools(domain)?;
    let spec = if transport == Transport::Offline { demo_spec(domain) } else { Value::Null };
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let run_id = format!("{domain}-{secs}");
    let dir = PathBuf::from(format!("runs/{run_id}"));
    std::fs::create_dir_all(&dir)?;
    let transport: Arc<dyn wright_agents::Transport> = match transport {
        Transport::FromEnv => wright_agents::transport_from_env().map_err(|e| anyhow::anyhow!("{e}"))?,
        Transport::Offline => offline_transport(),
    };
    let (tx, _keep) = wright_agents::channel();
    let mut sub = tx.subscribe();
    let log_rx = tx.subscribe();
    let (ui_tx, ui_rx) = std::sync::mpsc::channel();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let summary = Arc::new(std::sync::Mutex::new(None));
    let rt = tokio::runtime::Runtime::new()?;
    let (tx2, dir2, run2, req2, dom2, done2, sum2) =
        (tx.clone(), dir.clone(), run_id.clone(), request.to_string(), domain.to_string(), done.clone(), summary.clone());
    let repaint = Arc::new(repaint);
    std::thread::Builder::new().name("wright-harness".into()).spawn(move || {
        rt.block_on(async move {
            let _log = wright_agents::harness::spawn_event_log(log_rx, dir2.join("events.jsonl"));
            let r2 = repaint.clone();
            tokio::spawn(async move {
                use broadcast::error::RecvError;
                loop {
                    match sub.recv().await {
                        Ok(ev) => {
                            if ui_tx.send(ev).is_err() {
                                break;
                            }
                            r2();
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            let client = wright_agents::Client::new(transport, wright_agents::Budget::from_env(), tx2.clone(), run2.clone());
            let mut cfg = wright_agents::RunConfig::new(&run2, &req2, spec);
            cfg.out_dir = Some(dir2.clone());
            cfg.stages = stages;
            cfg.regressions_path = Some(data_dir().join(format!("regressions-{dom2}.jsonl")));
            let s = wright_agents::run(client, tools.clone(), cfg).await;
            finish_guide(&tx2, tools.as_ref(), &s, &run2, &req2, &dom2, &dir2);
            *sum2.lock().unwrap() = Some(format!(
                "{:?} after {} round(s), ${:.2}{}",
                s.stop,
                s.rounds,
                s.spent_usd,
                s.objective.as_ref().map(|(v, l)| format!(", {l} {v:.1}")).unwrap_or_default()
            ));
            // let the log and forwarder drain
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            done2.store(true, std::sync::atomic::Ordering::SeqCst);
            repaint();
        });
    })?;
    Ok(Live { run_id, dir, tx, rx: ui_rx, done, summary })
}

/// After a passing run: the builder's guide from the build's own artifacts,
/// and its four renders + the guide sent as events (recorded for replay).
/// Shared with `wright run` (wright-cli), which used to inline this.
pub fn finish_guide(
    tx: &broadcast::Sender<Event>,
    tools: &dyn DomainTools,
    summary: &wright_agents::RunSummary,
    run_id: &str,
    request: &str,
    domain: &str,
    out: &Path,
) -> Option<PathBuf> {
    let (true, Some(design), Some(report), Some(arts)) = (summary.passed, &summary.state.design, &summary.state.report, &summary.artifacts) else {
        return None;
    };
    let bom = tools.bom(design).ok()?;
    let line = format!("Designed by Wright's agents from: \"{request}\". All {} checks pass.", report.outcomes.len());
    match wright_guide::from_artifacts(&format!("{domain} build guide"), &line, arts, &bom, report, None, &out.join("guide")) {
        Ok(g) => {
            for view in ["iso", "top", "front", "side"] {
                let rel = if view == "iso" { "guide/img/cover-iso.png".to_string() } else { format!("guide/img/view-{view}.png") };
                if out.join(&rel).exists() {
                    let _ = tx.send(Event::Render { run: run_id.into(), view: view.into(), path: rel });
                }
            }
            let _ = tx.send(Event::Artifact { run: run_id.into(), label: "Build guide (HTML)".into(), path: "guide/guide.html".into() });
            Some(g)
        }
        Err(e) => {
            eprintln!("build guide not written: {e:#}");
            None
        }
    }
}

impl Live {
    /// Send the person's markup to the running harness.
    pub fn send_feedback(&self, view: &str, sketch_rel: &str, note: &str) -> anyhow::Result<()> {
        let ev = feedback_event(&self.run_id, view, sketch_rel, note)?;
        self.tx.send(ev).map_err(|_| anyhow::anyhow!("the run has ended; no agent is listening"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_carries_the_run_id_and_an_empty_one_is_refused() {
        assert!(feedback_event("", "3d", "feedback/x.png", "n").is_err());
        assert!(feedback_event("  ", "3d", "feedback/x.png", "n").is_err());
        match feedback_event("aircraft-1", "3d", "feedback/x.png", "move the battery").unwrap() {
            Event::Feedback { run, sketch_png, .. } => {
                assert_eq!(run, "aircraft-1");
                assert_eq!(sketch_png, "feedback/x.png");
            }
            e => panic!("{e:?}"),
        }
    }

    /// The markup reaches the harness's inbox: an offline run (scripted
    /// transport, no model, $0) with a sketch sent on the run's channel
    /// while the orchestrator's first call is in flight -- the same path
    /// `Live::send_feedback` uses.
    #[test]
    fn markup_reaches_a_running_harness() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            use wright_agents::fake::{reply, tool_use, ScriptedTransport};
            let (tx, _keep) = wright_agents::channel();
            let person = tx.clone();
            let mut n = 0;
            let t = Arc::new(ScriptedTransport::from_fn(move |_b: &Value| {
                n += 1;
                match n {
                    1 => {
                        let _ = person.send(feedback_event("run-d", "3d", "data:image/png;base64,iVBORw0KGgo=", "thicker spar here").unwrap());
                        Some(reply("m", vec![tool_use("o1", "design", json!({}))], 0, 0))
                    }
                    2 => Some(reply("m", vec![tool_use("o2", "submit", json!({"summary": "s"}))], 0, 0)),
                    3 => Some(reply("m", vec![tool_use("r1", "verdict", json!({"summary": "ok"}))], 0, 0)),
                    _ => None,
                }
            }));
            let client = wright_agents::Client::new(t.clone(), wright_agents::Budget::new(1.0, 1.0, None), tx, "run-d");
            let mut cfg = wright_agents::RunConfig::new("run-d", "a balloon", Value::Null);
            cfg.max_rounds = 1;
            let _ = wright_agents::run(client, tools("balloon").unwrap(), cfg).await;
            let reqs = t.requests.lock().unwrap();
            assert!(reqs.len() >= 2);
            assert!(reqs[1].body.to_string().contains("thicker spar here"), "the orchestrator's next turn carries the note");
        });
    }
}
