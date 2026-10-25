//! Wright's desktop app (egui/eframe, native; no web server, no browser).
//!
//! * Start a run: pick a domain, type a request, and the agent harness runs
//!   in-process (`live`), its events streaming into the window.
//! * Watch it: the 3D view of the assembly with click-to-select
//!   (`viewer3d`, `scene`), checks with margins and the headline numbers,
//!   the BOM, the agents' messages and the spend (`model`).
//! * Replay a recorded `events.jsonl` on a timeline with a scrubber
//!   (`replay`), stopping at a recorded markup for the person at the table.
//! * Draw on the view and send it to the agents (`markup`) as
//!   `Event::Feedback`, the sketch PNG saved under the run dir.
//! * The analysis agents' media (`media`): CFD/FEA/fab/CAD/USD/sim
//!   manifests, videos via ffmpeg-extracted frames, images, STEP/USD files.
//! * The build guide's pictures and checklists, natively.
//!
//! Why egui and not the Rerun viewer: see docs/ARCHITECTURE.md, "Desktop
//! app".

pub mod app;
pub mod live;
pub mod markup;
pub mod media;
pub mod model;
pub mod replay;
pub mod scene;
pub mod viewer3d;

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Open this recording on start.
    pub replay: Option<PathBuf>,
    /// Replay speed (1 = recorded pacing).
    pub speed: Option<f64>,
    /// Jump the replay to its end on open.
    pub seek_end: bool,
    /// Save a PNG of the window after this many seconds, then optionally
    /// quit (verification and docs screenshots).
    pub screenshot: Option<(PathBuf, f64)>,
    pub quit_after_screenshot: bool,
    /// Put a sample stroke and note on the view (verifies markup drawing
    /// without a mouse).
    pub markup_demo: bool,
    /// Start an offline run (scripted transport, $0) of this domain on open.
    pub offline_run: Option<String>,
    /// With `markup_demo`: press Send once, 3 s after start.
    pub markup_send: bool,
    /// Initial central tab: "3d" (default) or "renders"; right tab: log,
    /// bom, media, guide, files.
    pub right_tab: Option<String>,
    /// Post-design stages (CFD, FEA, fab, CAD, ...) a live run may call.
    /// `wright app` passes wright-cli's registry; the bare `wright-desktop`
    /// binary has none.
    pub stages: wright_core::Stages,
}

impl Options {
    pub fn from_args(args: &[String]) -> anyhow::Result<Options> {
        let mut o = Options::default();
        let mut it = args.iter();
        let mut shot_path = None;
        let mut after = 4.0;
        while let Some(a) = it.next() {
            let mut val = |name: &str| it.next().cloned().ok_or_else(|| anyhow::anyhow!("{name} needs a value"));
            match a.as_str() {
                "--replay" => o.replay = Some(PathBuf::from(val("--replay")?)),
                "--speed" => o.speed = Some(val("--speed")?.parse()?),
                "--seek-end" => o.seek_end = true,
                "--screenshot" => shot_path = Some(PathBuf::from(val("--screenshot")?)),
                "--after" => after = val("--after")?.parse()?,
                "--quit" => o.quit_after_screenshot = true,
                "--markup-demo" => o.markup_demo = true,
                "--markup-send" => o.markup_send = true,
                "--offline-run" => o.offline_run = Some(val("--offline-run")?),
                "--tab" => o.right_tab = Some(val("--tab")?),
                "-h" | "--help" => {
                    println!(
                        "wright-desktop [--replay EVENTS.jsonl] [--speed X] [--seek-end] [--tab log|bom|media|guide|files]\n\
                         \x20              [--screenshot OUT.png [--after SECS] [--quit]] [--markup-demo [--markup-send]]\n\
                         \x20              [--offline-run aircraft|rocket|balloon]"
                    );
                    std::process::exit(0);
                }
                other if other.ends_with(".jsonl") && o.replay.is_none() => o.replay = Some(PathBuf::from(other)),
                other => anyhow::bail!("unknown argument {other} (try --help)"),
            }
        }
        o.screenshot = shot_path.map(|p| (p, after));
        Ok(o)
    }
}

/// Open the window and run until it is closed.
pub fn launch(opts: Options) -> anyhow::Result<()> {
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Wright")
            .with_app_id("wright-desktop")
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Glow,
        multisampling: 4,
        depth_buffer: 24,
        ..Default::default()
    };
    eframe::run_native("Wright", native, Box::new(move |cc| Ok(Box::new(app::WrightApp::new(cc, opts)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}
