//! The window: top bar (start a run / open a recording), left (headlines +
//! checks), centre (3D view or renders, with markup), right (log, BOM,
//! media, guide, files), bottom (replay timeline).
//!
//! Layout follows the egui demo app's shape (`crates/egui_demo_app/src/
//! wrap_app.rs`: top bar, side panels, central panel, all `egui::Panel`s)
//! and the web studio's arrangement it replaces (checks left, the design in
//! the middle, the agents' log right).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Color32, RichText};
use wright_core::{Bound, Evidence};

use crate::live::{self, Live, Transport};
use crate::markup::{self, Markup};
use crate::media::{self, Found, Frames};
use crate::model::{event_label, locate, LogKind, RunState};
use crate::replay::{LiveFeedback, Mark, Replay};
use crate::scene::{scene_files, Scene};
use crate::viewer3d::Viewer;
use crate::Options;

const PASS: Color32 = Color32::from_rgb(0x3f, 0xc2, 0x7a);
const FAIL: Color32 = Color32::from_rgb(0xff, 0x5a, 0x4a);
const ACCENT: Color32 = Color32::from_rgb(0xff, 0x7a, 0x2e);
const INFO: Color32 = Color32::from_rgb(0x8a, 0xb4, 0xff);

// One Mode value lives in App; boxing Live would touch many match sites.
#[allow(clippy::large_enum_variant)]
enum Mode {
    Idle,
    Replay(Box<Replay>),
    Live { live: Live, state: RunState },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Center {
    View3d,
    Renders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Right {
    Checks,
    Log,
    Bom,
    Media,
    Guide,
    Files,
}

struct Shot {
    rect: egui::Rect,
    ppp: f32,
    view: String,
    note: String,
    /// Window screenshot for --screenshot rather than markup.
    whole_window: Option<PathBuf>,
}

struct Video {
    frames: Arc<Mutex<Frames>>,
    playing: bool,
    t: f64,
    shown: Option<usize>,
    tex: Option<egui::TextureHandle>,
}

pub struct WrightApp {
    mode: Mode,
    viewer: Viewer,
    /// The files the scene was loaded from, with their mtimes (a file
    /// rewritten in place reloads).
    scene_files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    center: Center,
    render_sel: usize,
    right: Right,
    markup: Markup,
    shot: Option<Shot>,
    shot_image: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    form_domain: usize,
    form_request: String,
    form_transport: Transport,
    replay_path: String,
    toast: Option<(String, Instant, bool)>,
    bom: Option<(PathBuf, Vec<Vec<String>>, Vec<String>)>,
    media: Vec<Found>,
    media_scanned: Option<Instant>,
    videos: HashMap<PathBuf, Video>,
    md: egui_commonmark::CommonMarkCache,
    md_files: HashMap<PathBuf, String>,
    only_failing: bool,
    last: Instant,
    started: Instant,
    opts: Options,
    auto_shot_done: bool,
}

fn transport_name() -> String {
    match std::env::var("WRIGHT_TRANSPORT").unwrap_or_default().as_str() {
        "api" | "anthropic" => "Anthropic API (paid)".into(),
        "" | "cli" | "claude-cli" => "claude CLI (subscription)".into(),
        "foundry" | "azure" => "Microsoft Foundry (paid)".into(),
        other => format!("WRIGHT_TRANSPORT={other}"),
    }
}

impl WrightApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.global_style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.visuals.panel_fill = Color32::from_rgb(0x16, 0x18, 0x1c);
        });
        let mut app = WrightApp {
            mode: Mode::Idle,
            viewer: Viewer::new(cc.gl.as_ref()),
            scene_files: vec![],
            center: Center::View3d,
            render_sel: 0,
            right: Right::Checks,
            markup: Markup::default(),
            shot: None,
            shot_image: Arc::new(Mutex::new(None)),
            form_domain: 0,
            form_request: "A 3D-printed VTOL quadplane that flies farther than 130 miles with a 15 % reserve: 1.2 m wingspan, prints on a Bambu A1, 6S, under $2000 including the battery.".into(),
            form_transport: Transport::Offline,
            replay_path: opts.replay.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "demo/aircraft/events.jsonl".into()),
            toast: None,
            bom: None,
            media: vec![],
            media_scanned: None,
            videos: HashMap::new(),
            md: Default::default(),
            md_files: HashMap::new(),
            only_failing: false,
            last: Instant::now(),
            started: Instant::now(),
            opts: opts.clone(),
            auto_shot_done: false,
        };
        app.right = match opts.right_tab.as_deref() {
            Some("bom") => Right::Bom,
            Some("media") => Right::Media,
            Some("guide") => Right::Guide,
            Some("files") => Right::Files,
            Some("log") => Right::Log,
            _ => Right::Checks,
        };
        if let Some(p) = &opts.replay {
            app.open_replay(&p.clone());
            if let Mode::Replay(r) = &mut app.mode {
                if let Some(s) = opts.speed {
                    r.speed = s;
                }
                if opts.seek_end {
                    r.seek(r.total());
                }
            }
        }
        if let Some(d) = &opts.offline_run {
            if let Some(i) = live::DOMAINS.iter().position(|x| x == d) {
                app.form_domain = i;
                app.form_transport = Transport::Offline;
                if d != "aircraft" {
                    app.form_request = format!("a {d} (offline demo run)");
                }
                let ctx = cc.egui_ctx.clone();
                app.start_live(&ctx);
            }
        }
        if opts.markup_demo {
            app.markup.on = true;
            app.markup.note = "Move the battery forward; this boom looks thin.".into();
            let circle: Vec<egui::Pos2> =
                (0..=48).map(|k| k as f32 / 48.0 * std::f32::consts::TAU).map(|a| egui::pos2(0.5 + 0.12 * a.cos(), 0.52 + 0.09 * a.sin())).collect();
            app.markup.lines.push(markup::Line { tool: markup::Tool::Pen, color: markup::COLORS[0].1, points: circle });
            app.markup.lines.push(markup::Line {
                tool: markup::Tool::Arrow,
                color: markup::COLORS[1].1,
                points: vec![egui::pos2(0.2, 0.2), egui::pos2(0.37, 0.43)],
            });
            app.markup.lines.push(markup::Line {
                tool: markup::Tool::Highlight,
                color: markup::COLORS[2].1,
                points: (0..20).map(|k| egui::pos2(0.62 + k as f32 * 0.01, 0.7 + (k as f32 * 0.5).sin() * 0.01)).collect(),
            });
        }
        app
    }

    fn say(&mut self, text: impl Into<String>, bad: bool) {
        self.toast = Some((text.into(), Instant::now(), bad));
    }

    fn open_replay(&mut self, p: &Path) {
        match Replay::open(p) {
            Ok(r) => {
                self.mode = Mode::Replay(Box::new(r));
                self.reset_view_state();
                self.say(format!("Replaying {}", p.display()), false);
            }
            Err(e) => self.say(format!("Could not open {}: {e:#}", p.display()), true),
        }
    }

    fn reset_view_state(&mut self) {
        self.scene_files.clear();
        self.viewer.set_scene(Scene::default());
        self.bom = None;
        self.media.clear();
        self.media_scanned = None;
        self.render_sel = 0;
        self.markup.clear();
    }

    fn start_live(&mut self, ctx: &egui::Context) {
        let domain = live::DOMAINS[self.form_domain];
        let c = ctx.clone();
        match live::start(domain, &self.form_request, self.form_transport, self.opts.stages.clone(), move || c.request_repaint()) {
            Ok(l) => {
                let msg = format!("Started {} in {}", l.run_id, l.dir.display());
                self.mode = Mode::Live { live: l, state: RunState::default() };
                self.reset_view_state();
                self.say(msg, false);
            }
            Err(e) => self.say(format!("Could not start: {e:#}"), true),
        }
    }

    fn state(&self) -> Option<&RunState> {
        match &self.mode {
            Mode::Idle => None,
            Mode::Replay(r) => Some(&r.state),
            Mode::Live { state, .. } => Some(state),
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        match &self.mode {
            Mode::Idle => None,
            Mode::Replay(r) => Some(r.dir.clone()),
            Mode::Live { live, .. } => Some(live.dir.clone()),
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        match &mut self.mode {
            Mode::Replay(r) => {
                r.tick(dt);
                if r.playing && !r.awaiting_feedback && r.cursor < r.total() {
                    ctx.request_repaint();
                }
                if r.awaiting_feedback && !self.markup.on {
                    self.markup.on = true;
                }
            }
            Mode::Live { live, state } => {
                while let Ok(ev) = live.rx.try_recv() {
                    state.apply(&ev);
                }
                if live.done.load(std::sync::atomic::Ordering::SeqCst) {
                    if let Some(s) = live.summary.lock().unwrap().take() {
                        state.notice(format!("Run ended: {s}. Events in {}/events.jsonl", live.dir.display()));
                    }
                }
            }
            Mode::Idle => {}
        }
        // the 3D scene follows the run's STL artifacts
        if let (Some(state), Some(dir)) = (self.state().cloned(), self.dir()) {
            let files = scene_files(&state.artifacts, &dir);
            let paths: Vec<(PathBuf, Option<std::time::SystemTime>)> =
                files.iter().map(|(_, p)| (p.clone(), std::fs::metadata(p).and_then(|m| m.modified()).ok())).collect();
            if paths != self.scene_files {
                self.scene_files = paths;
                self.viewer.set_scene(Scene::load(&files));
            }
            // interference: the CAD's own pairs, failing interference /
            // clearance checks (domain or stage) that name a part
            let flags = {
                let sc = self.viewer.scene.lock();
                let mut texts: Vec<String> = vec![];
                if let Some(c) = &state.checks {
                    for o in c.outcomes.iter().filter(|o| !o.passed()) {
                        let n = o.name.to_ascii_lowercase();
                        if n.contains("interfer") || n.contains("clearance") || n.contains("overlap") {
                            texts.push(o.detail.to_ascii_lowercase());
                        }
                    }
                }
                for f in &self.media {
                    if let Ok(m) = &f.manifest {
                        for c in m.checks.iter().filter(|c| !c.passed) {
                            let n = c.name.to_ascii_lowercase();
                            if n.contains("interfer") || n.contains("clearance") || n.contains("overlap") {
                                texts.push(c.detail.to_ascii_lowercase());
                            }
                        }
                    }
                }
                sc.parts
                    .iter()
                    .map(|p| {
                        let name = p.name.to_ascii_lowercase();
                        let alt = name.replace(' ', "_");
                        !p.interferes_with.is_empty() || texts.iter().any(|t| t.contains(&name) || t.contains(&alt))
                    })
                    .collect::<Vec<bool>>()
            };
            self.viewer.set_flags(flags);
            // BOM
            let bom_path = state.bom_path(&dir);
            if bom_path.as_ref() != self.bom.as_ref().map(|b| &b.0) {
                self.bom = bom_path.and_then(|p| read_csv(&p).ok().map(|(h, rows)| (p, rows, h)));
            }
            // media manifests, rescanned every 3 s (agents write them as they go)
            if self.media_scanned.is_none_or(|t| t.elapsed().as_secs_f32() > 3.0) {
                self.media = media::scan(&dir);
                self.media_scanned = Some(Instant::now());
            }
        }
        if let Some((_, t, _)) = &self.toast {
            if t.elapsed().as_secs_f32() > 6.0 {
                self.toast = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    // ------------------------------------------------------------ markup

    fn view_name(&self) -> String {
        match self.center {
            Center::View3d => {
                let c = &self.viewer.camera;
                format!("3d (azimuth {:.0} deg, elevation {:.0} deg)", c.yaw.to_degrees(), c.pitch.to_degrees())
            }
            Center::Renders => self.state().and_then(|s| s.renders.get(self.render_sel)).map(|(v, _)| v.clone()).unwrap_or_else(|| "render".into()),
        }
    }

    fn send_markup(&mut self, ctx: &egui::Context) {
        let Some(rect) = self.markup.rect else { return };
        if !self.markup.has_ink() && self.markup.note.trim().is_empty() {
            self.say("Draw something or write a note first.", true);
            return;
        }
        // In a replay, markup is taken only at the recorded markup step
        // (or after the end); refused before it is even saved, so an early
        // drawing never consumes the recorded pause (review 4, finding 7).
        if let Mode::Replay(r) = &self.mode {
            if !r.awaiting_feedback && r.cursor < r.total() {
                self.say("This is a recorded run: markup is replayed at the recorded step. The replay will stop there and hand you the pen.", true);
                return;
            }
        }
        let mut note = self.markup.note.trim().to_string();
        if self.center == Center::View3d {
            if let Some(i) = self.viewer.selected {
                if let Some(p) = self.viewer.scene.lock().parts.get(i) {
                    note = format!("{note} [selected part: {}]", p.name);
                }
            }
        }
        let slot = self.shot_image.clone();
        let c = ctx.clone();
        ctx.request_screenshot(move |img| {
            *slot.lock().unwrap() = Some(img);
            c.request_repaint();
        });
        self.shot = Some(Shot { rect, ppp: ctx.pixels_per_point(), view: self.view_name(), note, whole_window: None });
    }

    fn finish_shot(&mut self) {
        let Some(img) = self.shot_image.lock().unwrap().take() else { return };
        let Some(shot) = self.shot.take() else { return };
        if let Some(path) = shot.whole_window {
            let full = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(img.size[0] as f32, img.size[1] as f32) / shot.ppp);
            match markup::crop_png(&img, full, shot.ppp).and_then(|png| {
                if let Some(d) = path.parent() {
                    std::fs::create_dir_all(d)?;
                }
                Ok(std::fs::write(&path, png)?)
            }) {
                Ok(()) => eprintln!("wright-desktop: screenshot {}", path.display()),
                Err(e) => eprintln!("wright-desktop: screenshot failed: {e:#}"),
            }
            return;
        }
        let png = match markup::crop_png(&img, shot.rect, shot.ppp) {
            Ok(p) => p,
            Err(e) => return self.say(format!("Could not capture the view: {e:#}"), true),
        };
        let result: anyhow::Result<String> = match &mut self.mode {
            Mode::Idle => Err(anyhow::anyhow!("no run is active: start a run or open a recording")),
            Mode::Live { live, state } => (|| {
                let (rel, abs) = markup::save_sketch(&live.dir, &live.run_id, &shot.view, &png)?;
                live.send_feedback(&shot.view, &rel, &shot.note)?;
                let _ = state;
                Ok(format!("Sent to the agents; sketch saved to {}", abs.display()))
            })(),
            Mode::Replay(r) => (|| {
                let run = r.run_id().unwrap_or_default();
                // a recording's own files are never written to: replay
                // sketches go to a scratch dir (as the studio did)
                let root = std::env::temp_dir().join("wright-desktop").join("replay").join(markup::slug(&run));
                let (_, abs) = markup::save_sketch(&root, &run, &shot.view, &png)?;
                let ev = live::feedback_event(&run, &shot.view, &abs.to_string_lossy(), &shot.note)?;
                Ok(match r.live_feedback(ev) {
                    LiveFeedback::Substituted => format!("Your markup replaces the recorded one; the recorded response follows. Saved {}", abs.display()),
                    LiveFeedback::AfterEnd => {
                        r.state.notice("This is a recorded run and it has finished, so no agent will answer this markup. Press Restart to replay; the run stops at the markup step for yours.");
                        format!("Saved {} (the recording has ended)", abs.display())
                    }
                    LiveFeedback::NotAtMarkupStep => "markup is replayed at the recorded step".into(),
                })
            })(),
        };
        match result {
            Ok(m) => {
                self.markup.clear();
                self.markup.note.clear();
                self.markup.on = false;
                self.say(m, false);
            }
            Err(e) => self.say(format!("{e:#}"), true),
        }
    }

    // ------------------------------------------------------------ panels

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Wright").strong().size(18.0).color(ACCENT));
            ui.separator();
            egui::ComboBox::from_id_salt("domain").selected_text(live::DOMAINS[self.form_domain]).show_ui(ui, |ui| {
                for (i, d) in live::DOMAINS.iter().enumerate() {
                    ui.selectable_value(&mut self.form_domain, i, *d);
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.form_request).hint_text("Describe the vehicle").desired_width(380.0));
            let tname = transport_name();
            egui::ComboBox::from_id_salt("transport")
                .selected_text(match self.form_transport {
                    Transport::Offline => "Offline (no model, $0)".to_string(),
                    Transport::FromEnv => tname.clone(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.form_transport, Transport::Offline, "Offline (no model, $0)");
                    ui.selectable_value(&mut self.form_transport, Transport::FromEnv, tname.as_str());
                });
            let running = matches!(&self.mode, Mode::Live { live, .. } if !live.done.load(std::sync::atomic::Ordering::SeqCst));
            if ui.add_enabled(!running, egui::Button::new(RichText::new("Start run").strong())).clicked() {
                let ctx = ui.ctx().clone();
                self.start_live(&ctx);
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.replay_path).hint_text("events.jsonl").desired_width(230.0));
            if ui.button("Open replay").on_hover_text("or drop an events.jsonl on the window").clicked() {
                let p = PathBuf::from(self.replay_path.trim());
                self.open_replay(&p);
            }
        });
        ui.horizontal(|ui| {
            let Some(s) = self.state().cloned() else {
                ui.label(RichText::new("Start a run, or open a recording to replay it.").weak());
                return;
            };
            let mode = match &self.mode {
                Mode::Replay(_) => "REPLAY",
                Mode::Live { live, .. } if live.done.load(std::sync::atomic::Ordering::SeqCst) => "ENDED",
                Mode::Live { .. } => "LIVE",
                Mode::Idle => "",
            };
            ui.label(RichText::new(mode).strong().color(if mode == "LIVE" { FAIL } else { INFO }));
            ui.label(RichText::new(s.run.clone().unwrap_or_default()).monospace());
            if !s.request.is_empty() {
                ui.label(RichText::new(format!("\u{201c}{}\u{201d}", truncate(&s.request, 60))).italics()).on_hover_text(&s.request);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(f) = &s.finished {
                    let (t, c) = if f.passed { ("PASS", PASS) } else { ("NOT YET", FAIL) };
                    ui.label(RichText::new(format!("{t}  {}: {:.1}", f.label, f.objective)).strong().color(c));
                }
                if let Some(sp) = &s.spend {
                    ui.label(format!("${:.2} of ${:.2}", sp.usd, sp.budget_usd))
                        .on_hover_text(format!("{} input / {} output tokens", sp.input_tokens, sp.output_tokens));
                    ui.add(egui::ProgressBar::new((sp.usd / sp.budget_usd.max(1e-9)) as f32).desired_width(90.0));
                    ui.label("Spend");
                }
            });
        });
        if let Some(s) = self.state().cloned() {
            if !s.headlines.is_empty() {
                ui.horizontal_wrapped(|ui| self.headline_strip(ui, &s));
            }
        }
    }

    fn checks_panel(&mut self, ui: &mut egui::Ui, s: &RunState) {
        ui.heading("Headlines");
        if s.headlines.is_empty() {
            ui.label(RichText::new("none yet").weak());
        }
        let outcome = |name: &Option<String>| name.as_ref().and_then(|n| s.checks.as_ref()?.outcomes.iter().find(|o| &o.name == n));
        egui::Grid::new("headlines").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for h in &s.headlines {
                let c = match outcome(&h.check) {
                    Some(o) if o.passed() => PASS,
                    Some(_) => FAIL,
                    None => ui.visuals().text_color(),
                };
                ui.label(&h.label);
                let lim = match (h.limit, h.bound) {
                    (Some(l), Bound::AtLeast) => format!("  need >= {}", num(l)),
                    (Some(l), Bound::AtMost) => format!("  need <= {}", num(l)),
                    _ => String::new(),
                };
                ui.label(RichText::new(format!("{} {}", num(h.value), h.unit)).strong().color(c)).on_hover_text(lim.trim().to_string());
                ui.end_row();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Checks");
            if let Some(c) = &s.checks {
                let col = if c.failed == 0 { PASS } else { FAIL };
                ui.label(RichText::new(format!("{} / {}", c.passed, c.passed + c.failed)).strong().color(col));
                ui.label(RichText::new(format!("report {}", s.check_reports)).weak());
            }
        });
        ui.checkbox(&mut self.only_failing, "only failing");
        let Some(c) = &s.checks else {
            ui.label(RichText::new("no check report yet").weak());
            return;
        };
        let mut rows: Vec<&wright_core::Outcome> = c.outcomes.iter().filter(|o| !self.only_failing || !o.passed()).collect();
        rows.sort_by_key(|o| o.passed());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for o in rows {
                let ok = o.passed();
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, if ok { PASS } else { FAIL });
                    ui.label(RichText::new(&o.name).monospace());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &o.evidence {
                        Evidence::Margin { value, unit } => {
                            ui.label(RichText::new(format!("{} {unit}", num(*value))).color(if ok { PASS } else { FAIL }));
                        }
                        Evidence::Missing { .. } => {
                            ui.label(RichText::new("missing").color(FAIL));
                        }
                    });
                })
                .response
                .on_hover_text(format!("{}\nsource: {:?}", o.detail, o.source));
                ui.label(RichText::new(truncate(&o.detail, 90)).small().weak());
            }
        });
    }

    fn log_panel(&mut self, ui: &mut egui::Ui, s: &RunState) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for l in &s.log {
                let (who, col) = match &l.kind {
                    LogKind::Message => (l.agent.clone(), agent_color(&l.agent)),
                    LogKind::Call => (format!("{} ->", l.agent), ui.visuals().weak_text_color()),
                    LogKind::Result { ok: true } => ("  <- ok".into(), ui.visuals().weak_text_color()),
                    LogKind::Result { ok: false } => ("  <- error".into(), FAIL),
                    LogKind::Feedback => ("person".into(), ACCENT),
                    LogKind::Notice => ("app".into(), INFO),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(who).strong().color(col));
                    match l.kind {
                        LogKind::Call | LogKind::Result { .. } => ui.label(RichText::new(truncate(&l.text, 400)).monospace().small()),
                        _ => ui.label(&l.text),
                    };
                });
            }
        });
    }

    fn bom_panel(&mut self, ui: &mut egui::Ui) {
        let Some((path, rows, head)) = &self.bom else {
            ui.label(RichText::new("No BOM yet (bom.csv appears once the design builds).").weak());
            return;
        };
        let col = |n: &str| head.iter().position(|h| h == n);
        let (qty, name, unit, total, url, cat) = (col("qty"), col("name"), col("unit_price_usd"), col("total_usd"), col("url"), col("category"));
        let sum: f64 = rows.iter().filter_map(|r| total.and_then(|t| r.get(t)?.parse::<f64>().ok())).sum();
        let unpriced = rows.iter().filter(|r| total.and_then(|t| r.get(t)).is_none_or(|v| v.trim().is_empty())).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("${sum:.2}")).strong().size(16.0));
            ui.label(format!("{} lines", rows.len()));
            if unpriced > 0 {
                ui.label(RichText::new(format!("{unpriced} unpriced")).color(FAIL));
            }
            if ui.small_button("Reveal").clicked() {
                media::reveal(path);
            }
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("bom").striped(true).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for r in rows {
                    let get = |i: Option<usize>| i.and_then(|i| r.get(i)).cloned().unwrap_or_default();
                    ui.label(get(qty));
                    let n = truncate(&get(name), 46);
                    let link = get(url);
                    let resp = if link.starts_with("http") { ui.hyperlink_to(n, link) } else { ui.label(n) };
                    resp.on_hover_text(format!("{}\n{}", get(cat), get(name)));
                    let t = get(total);
                    ui.label(if t.is_empty() { RichText::new("unpriced").color(FAIL) } else { RichText::new(format!("${t}")) })
                        .on_hover_text(format!("unit ${}", get(unit)));
                    ui.end_row();
                }
            });
        });
    }

    fn media_panel(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self.dir() else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("<run>/<kind>/manifest.json in {}", dir.display())).small().weak());
            if ui.small_button("Rescan").clicked() {
                self.media_scanned = None;
            }
        });
        if self.media.is_empty() {
            ui.label(RichText::new("No analysis media yet. CFD, FEA, fab timelapses, CAD/USD and flight-sim agents write cfd/, fea/, fab/, cad/, usd/, sim/ manifests here.").weak());
            return;
        }
        let found = self.media.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for f in &found {
                let m = match &f.manifest {
                    Ok(m) => m,
                    Err(e) => {
                        ui.colored_label(FAIL, format!("{}: {e}", f.dir.display()));
                        continue;
                    }
                };
                egui::CollapsingHeader::new(RichText::new(format!("{}  {}", m.kind.to_uppercase(), m.title)).strong())
                    .id_salt(&f.dir)
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(e) = &m.error {
                            ui.colored_label(FAIL, format!("stage failed: {e}"));
                        }
                        for c in &m.checks {
                            ui.horizontal_wrapped(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.5, if c.passed { PASS } else { FAIL });
                                ui.label(RichText::new(&c.name).monospace());
                                ui.label(RichText::new(&c.detail).small().weak());
                            });
                        }
                        for item in &m.items {
                            self.media_item(ui, f, item);
                            ui.separator();
                        }
                    });
            }
        });
    }

    fn media_item(&mut self, ui: &mut egui::Ui, f: &Found, item: &media::Item) {
        ui.label(RichText::new(&item.label).strong());
        if !item.summary.is_empty() {
            ui.label(&item.summary);
        }
        if !item.metrics.is_empty() {
            egui::Grid::new(("metrics", &f.dir, &item.path)).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                for (k, v) in &item.metrics {
                    ui.label(RichText::new(k).monospace().small());
                    ui.label(RichText::new(num(*v)).small().strong());
                    ui.end_row();
                }
            });
        }
        let Some(path) = f.item_path(item) else {
            ui.colored_label(FAIL, format!("{}: missing, or outside {}", item.path, f.dir.display()));
            return;
        };
        let w = ui.available_width();
        match item.media.as_str() {
            "png" | "gif" => {
                ui.add(egui::Image::new(format!("file://{}", path.display())).max_width(w).corner_radius(4));
            }
            "mp4" => self.video(ui, &path, w),
            "step" | "stp" | "usda" | "usd" | "usdc" | "usdz" | "glb" => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).monospace());
                    if ui.button("Open in Finder").clicked() {
                        media::reveal(&path);
                    }
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).monospace());
                    if ui.small_button("Open").clicked() {
                        media::open(&path);
                    }
                    if ui.small_button("Reveal").clicked() {
                        media::reveal(&path);
                    }
                });
            }
        }
    }

    fn video(&mut self, ui: &mut egui::Ui, path: &Path, width: f32) {
        let ctx = ui.ctx().clone();
        let v = self.videos.entry(path.to_path_buf()).or_insert_with(|| {
            let frames = Arc::new(Mutex::new(Frames::NotStarted));
            let c = ctx.clone();
            media::extract(path.to_path_buf(), frames.clone(), move || c.request_repaint());
            Video { frames, playing: true, t: 0.0, shown: None, tex: None }
        });
        let state = v.frames.lock().unwrap().clone();
        match state {
            Frames::NotStarted | Frames::Extracting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("extracting frames with ffmpeg...");
                });
            }
            Frames::Failed(e) => {
                ui.colored_label(FAIL, format!("video: {e}"));
            }
            Frames::Ready { fps, files } => {
                let n = files.len();
                if v.playing {
                    v.t += ui.input(|i| i.stable_dt as f64);
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / fps));
                }
                let idx = ((v.t * fps) as usize) % n;
                if v.shown != Some(idx) {
                    if let Ok(img) = image::open(&files[idx]) {
                        let rgba = img.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                        match &mut v.tex {
                            Some(t) => t.set(ci, Default::default()),
                            None => v.tex = Some(ctx.load_texture(path.to_string_lossy(), ci, Default::default())),
                        }
                        v.shown = Some(idx);
                    }
                }
                if let Some(t) = &v.tex {
                    let size = t.size_vec2();
                    let scale = (width / size.x).min(1.0);
                    ui.image((t.id(), size * scale));
                }
                ui.horizontal(|ui| {
                    if ui.button(if v.playing { "Pause" } else { "Play" }).clicked() {
                        v.playing = !v.playing;
                    }
                    let mut k = idx;
                    if ui.add(egui::Slider::new(&mut k, 0..=n - 1).show_value(false)).changed() {
                        v.t = k as f64 / fps;
                        v.playing = false;
                    }
                    ui.label(format!("{:.1} / {:.1} s", idx as f64 / fps, n as f64 / fps));
                    if ui.small_button("Reveal").clicked() {
                        media::reveal(path);
                    }
                });
            }
        }
    }

    fn guide_panel(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self.dir() else { return };
        let gdir = dir.join("guide");
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if gdir.join("guide.html").is_file() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Build guide").strong());
                    if ui.button("Open the printable guide").on_hover_text("guide/guide.html: a static report file").clicked() {
                        media::open(&gdir.join("guide.html"));
                    }
                });
                let img = gdir.join("img");
                let mut pics: Vec<PathBuf> = std::fs::read_dir(&img)
                    .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect())
                    .unwrap_or_default();
                pics.sort_by_key(|p| {
                    let n = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    (if n.starts_with("cover") { 0 } else if n.starts_with("view") { 1 } else if n.starts_with("step") { 2 } else { 3 }, n)
                });
                let w = ui.available_width();
                let (big, small): (Vec<_>, Vec<_>) = pics.iter().partition(|p| !p.file_name().unwrap_or_default().to_string_lossy().starts_with("print-"));
                for p in big {
                    ui.label(RichText::new(p.file_stem().unwrap_or_default().to_string_lossy()).small().weak());
                    ui.add(egui::Image::new(format!("file://{}", p.display())).max_width(w));
                }
                if !small.is_empty() {
                    ui.label(RichText::new(format!("What you'll print ({} pieces)", small.len())).strong());
                    ui.horizontal_wrapped(|ui| {
                        for p in small {
                            ui.add(egui::Image::new(format!("file://{}", p.display())).max_width(92.0))
                                .on_hover_text(p.file_stem().unwrap_or_default().to_string_lossy());
                        }
                    });
                }
            } else {
                ui.label(RichText::new("No build guide yet (written after a passing design).").weak());
            }
            // checklists the build wrote (first_flight_checklist.md, checklist.md)
            let mut mds: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect())
                .unwrap_or_default();
            mds.sort();
            for p in mds {
                let text = self.md_files.entry(p.clone()).or_insert_with(|| std::fs::read_to_string(&p).unwrap_or_default()).clone();
                ui.separator();
                egui::CollapsingHeader::new(RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).strong())
                    .default_open(p.to_string_lossy().contains("checklist"))
                    .show(ui, |ui| {
                        egui_commonmark::CommonMarkViewer::new().show(ui, &mut self.md, &text);
                    });
            }
        });
    }

    fn files_panel(&mut self, ui: &mut egui::Ui, s: &RunState) {
        let Some(dir) = self.dir() else { return };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (label, recorded) in &s.artifacts {
                ui.horizontal(|ui| {
                    let found = locate(&dir, recorded);
                    ui.label(RichText::new(label).strong());
                    match found {
                        Some(p) => {
                            if ui.small_button("Reveal").clicked() {
                                media::reveal(&p);
                            }
                            if p.is_file() && ui.small_button("Open").clicked() {
                                media::open(&p);
                            }
                        }
                        None => {
                            ui.colored_label(FAIL, "not found");
                        }
                    }
                });
                ui.label(RichText::new(recorded).monospace().small().weak());
            }
        });
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let Mode::Replay(r) = &mut self.mode else { return };
        if r.awaiting_feedback {
            egui::Frame::new().fill(Color32::from_rgb(0x4a, 0x2a, 0x10)).inner_margin(8.0).corner_radius(4).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("The person in this recording drew on the design here.").strong().color(ACCENT));
                    ui.label("Draw yours on the view and press Send, or");
                    if ui.button("Use the recorded markup").clicked() {
                        r.use_recorded();
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Restart").clicked() {
                r.restart();
            }
            if ui.button(if r.playing { "Pause" } else { "Play" }).clicked() {
                r.playing = !r.playing;
            }
            egui::ComboBox::from_id_salt("speed").width(64.0).selected_text(format!("{}x", r.speed)).show_ui(ui, |ui| {
                for s in [0.5, 1.0, 2.0, 4.0, 8.0, 20.0] {
                    ui.selectable_value(&mut r.speed, s, format!("{s}x"));
                }
            });
            let total = r.total();
            let mut c = r.cursor;
            let w = (ui.available_width() - 260.0).max(120.0);
            ui.spacing_mut().slider_width = w;
            let resp = ui.add(egui::Slider::new(&mut c, 0..=total).show_value(false));
            // ticks: check reports (green/red), markup (orange), finish
            let rect = resp.rect;
            let x = |i: usize| rect.left() + 8.0 + (rect.width() - 16.0) * (i as f32 + 1.0) / total.max(1) as f32;
            for (i, m) in r.marks() {
                let col = match m {
                    Mark::Feedback => ACCENT,
                    Mark::ChecksPass => PASS,
                    Mark::ChecksFail => FAIL,
                    Mark::Finished => INFO,
                };
                ui.painter().line_segment([egui::pos2(x(i), rect.top() - 1.0), egui::pos2(x(i), rect.top() + 5.0)], egui::Stroke::new(2.0, col));
            }
            if resp.changed() {
                r.seek(c);
                r.playing = false;
            }
            let label = if r.cursor == 0 { "start".to_string() } else { event_label(&r.events[r.cursor - 1].event) };
            ui.label(RichText::new(format!("{}/{}  {}", r.cursor, total, truncate(&label, 34))).monospace().small());
        });
    }

    fn center(&mut self, ui: &mut egui::Ui, s: Option<&RunState>) {
        use crate::viewer3d::{Section, Tool};
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.center, Center::View3d, RichText::new("CAD").strong());
            let n_r = s.map_or(0, |s| s.renders.len() + s.feedback.len());
            ui.selectable_value(&mut self.center, Center::Renders, RichText::new(format!("Renders ({n_r})")).strong());
            ui.separator();
            if self.center == Center::View3d {
                ui.selectable_value(&mut self.viewer.tool, Tool::Select, "Select").on_hover_text("click a part (S)");
                if ui.selectable_value(&mut self.viewer.tool, Tool::Measure, "Measure").on_hover_text("click two points on the model (D)").clicked() {
                    self.viewer.measure.clear();
                }
                ui.separator();
                for v in ["iso", "top", "front", "side"] {
                    if ui.small_button(v).clicked() {
                        self.viewer.camera.preset(v);
                    }
                }
                if ui.small_button("fit all").clicked() {
                    let sc = self.viewer.scene.lock().clone();
                    let (yaw, pitch) = (self.viewer.camera.yaw, self.viewer.camera.pitch);
                    self.viewer.camera = crate::viewer3d::Camera::fit(&sc);
                    self.viewer.camera.yaw = yaw;
                    self.viewer.camera.pitch = pitch;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.markup.on { "Done drawing" } else { "Mark up (M)" };
                if ui.add(egui::Button::new(RichText::new(label).strong().color(ACCENT)).selected(self.markup.on)).clicked() {
                    self.markup.on = !self.markup.on;
                }
            });
        });
        if self.center == Center::View3d {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.viewer.show_edges, "edges");
                ui.separator();
                let mut sec = self.viewer.section;
                let axis_names = ["X", "Y", "Z"];
                egui::ComboBox::from_id_salt("section")
                    .width(70.0)
                    .selected_text(match sec {
                        None => "section off".to_string(),
                        Some(x) => format!("section {}", axis_names[x.axis]),
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(sec.is_none(), "off").clicked() {
                            sec = None;
                        }
                        for (a, n) in axis_names.iter().enumerate() {
                            if ui.selectable_label(sec.is_some_and(|x| x.axis == a), *n).clicked() {
                                sec = Some(Section { axis: a, at: sec.map_or(0.5, |x| x.at), flip: sec.is_some_and(|x| x.flip) });
                            }
                        }
                    });
                if let Some(x) = &mut sec {
                    ui.add(egui::Slider::new(&mut x.at, 0.0..=1.0).show_value(false));
                    ui.checkbox(&mut x.flip, "flip");
                }
                self.viewer.section = sec;
                ui.separator();
                ui.label("explode");
                ui.add(egui::Slider::new(&mut self.viewer.explode, 0.0..=1.5).show_value(false));
            });
        }
        if self.markup.on || self.markup.has_ink() {
            ui.horizontal(|ui| self.markup.toolbar(ui));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.markup.note).hint_text("What should change? (sent with the drawing)").desired_width(ui.available_width() - 150.0));
                let sending = self.shot.is_some();
                if ui.add_enabled(!sending, egui::Button::new(RichText::new("Send to agents").strong())).clicked() {
                    self.send_markup(&ctx);
                }
            });
        }
        let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        match self.center {
            Center::View3d => {
                let resp = self.viewer.show(ui, rect, !self.markup.on);
                self.cad_overlay(ui, rect, &resp, s);
            }
            Center::Renders => self.renders(ui, rect, s),
        }
        self.markup.canvas(ui, rect);
    }

    /// Text and markers drawn over the CAD view: hints, selection,
    /// measurement, CG, interference.
    fn cad_overlay(&mut self, ui: &mut egui::Ui, rect: egui::Rect, resp: &egui::Response, s: Option<&RunState>) {
        use crate::viewer3d::{Tool, CLASH, SELECT};
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(13.0);
        let scene = self.viewer.scene.lock().clone();
        if scene.parts.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No CAD yet: the view fills when the build writes the assembly (GLB or STL)", egui::FontId::proportional(15.0), Color32::GRAY);
            return;
        }
        let shadow = |p: egui::Pos2, a: egui::Align2, t: &str, c: Color32| {
            painter.text(p + egui::vec2(1.0, 1.0), a, t, font.clone(), Color32::from_black_alpha(200));
            painter.text(p, a, t, font.clone(), c);
        };
        shadow(
            rect.left_top() + egui::vec2(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            &format!(
                "{}  {} parts  {} triangles  {} edges   drag orbit  shift/right-drag pan  scroll zoom  click {}",
                scene.source,
                scene.parts.len(),
                scene.triangles(),
                scene.edge_positions.len() / 2,
                if self.viewer.tool == Tool::Measure { "measure" } else { "select" }
            ),
            Color32::from_gray(225),
        );
        let mut y = 28.0;
        if let Some(check) = &scene.geometry_check {
            let checked = check["status"] == "checked";
            let passed = checked && check["passed"] == true;
            let text = if checked {
                format!("Selected geometry checks: {} | {} overlaps, {} unmet contacts | connected: {} | not flight approval",
                    if passed { "PASS" } else { "FAIL" }, check["overlap_count"], check["unmet_contact_count"], check["connected"])
            } else {
                "Selected geometry checks: not current; run check again".to_string()
            };
            shadow(rect.left_top() + egui::vec2(10.0, y), egui::Align2::LEFT_TOP, &text,
                if checked && !passed { CLASH } else { Color32::from_gray(225) });
            y += 18.0;
        }
        if let Some(p) = self.viewer.selected.and_then(|i| scene.parts.get(i)) {
            let d = p.max - p.min;
            shadow(rect.left_top() + egui::vec2(10.0, y), egui::Align2::LEFT_TOP, &format!("selected: {}  {:.0} x {:.0} x {:.0} mm", p.name, d.x, d.y, d.z), SELECT);
            y += 18.0;
        }
        let clashes: Vec<&str> = scene.parts.iter().zip(&self.viewer.flags).filter(|(_, f)| **f).map(|(p, _)| p.name.as_str()).collect();
        if !clashes.is_empty() {
            shadow(rect.left_top() + egui::vec2(10.0, y), egui::Align2::LEFT_TOP, &format!("interference: {}", clashes.join(", ")), CLASH);
        }
        // hover name
        if let (Some(h), Some(pos)) = (self.viewer.hover, resp.hover_pos()) {
            if let Some(p) = scene.parts.get(h) {
                shadow(pos + egui::vec2(14.0, 10.0), egui::Align2::LEFT_TOP, &p.name, Color32::WHITE);
            }
        }
        // measurement
        let pts: Vec<egui::Pos2> = self.viewer.measure.iter().filter_map(|p| self.viewer.project(*p, rect)).collect();
        for p in &pts {
            painter.circle_filled(*p, 4.0, ACCENT);
        }
        if let ([a, b], [pa, pb]) = (self.viewer.measure.as_slice(), pts.as_slice()) {
            painter.line_segment([*pa, *pb], egui::Stroke::new(2.0, ACCENT));
            let d = *b - *a;
            let label = format!("{:.1} mm  (dx {:.1}, dy {:.1}, dz {:.1})", d.length(), d.x, d.y, d.z);
            shadow(pa.lerp(*pb, 0.5) + egui::vec2(8.0, -18.0), egui::Align2::LEFT_TOP, &label, ACCENT);
        }
        // mass / CG: from the CAD's part masses when it has them, else the
        // design's own numbers from the checks
        let mut bottom = vec![];
        if let Some((m, cg, n)) = scene.mass_cg() {
            if let Some(p) = self.viewer.project(cg, rect) {
                painter.circle_stroke(p, 9.0, egui::Stroke::new(2.0, Color32::WHITE));
                painter.line_segment([p - egui::vec2(12.0, 0.0), p + egui::vec2(12.0, 0.0)], egui::Stroke::new(2.0, Color32::WHITE));
                painter.line_segment([p - egui::vec2(0.0, 12.0), p + egui::vec2(0.0, 12.0)], egui::Stroke::new(2.0, Color32::WHITE));
                shadow(p + egui::vec2(12.0, 6.0), egui::Align2::LEFT_TOP, "CG", Color32::WHITE);
            }
            bottom.push(format!("mass {:.0} g from {n} of {} parts; CG ({:.0}, {:.0}, {:.0}) mm", m, scene.parts.len(), cg.x, cg.y, cg.z));
        } else if let Some(s) = s {
            if let Some(h) = s.headlines.iter().find(|h| h.label.to_ascii_lowercase().contains("mass")) {
                bottom.push(format!("{} {} {} (design)", h.label, num(h.value), h.unit));
            }
            if let Some(o) = s.checks.as_ref().and_then(|c| c.outcomes.iter().find(|o| o.name.contains("static_margin") || o.name.contains("cg"))) {
                bottom.push(format!("{}: {}", o.name, truncate(&o.detail, 110)));
            }
        }
        for (k, l) in bottom.iter().rev().enumerate() {
            shadow(rect.left_bottom() + egui::vec2(10.0, -10.0 - 18.0 * k as f32), egui::Align2::LEFT_BOTTOM, l, Color32::from_gray(230));
        }
        for (path, why) in &scene.errors {
            shadow(rect.right_bottom() + egui::vec2(-10.0, -10.0), egui::Align2::RIGHT_BOTTOM, &format!("{}: {why}", path.display()), FAIL);
        }
    }

    /// The part tree and the selected part's properties.
    fn parts_panel(&mut self, ui: &mut egui::Ui) {
        let scene = self.viewer.scene.lock().clone();
        ui.horizontal(|ui| {
            ui.heading("Parts");
            ui.label(RichText::new(format!("{}", scene.parts.len())).weak());
        });
        if scene.parts.is_empty() {
            ui.label(RichText::new("No CAD yet.").weak());
            return;
        }
        // group by source file (an assembly's pieces under its file)
        let mut files: Vec<PathBuf> = vec![];
        for p in &scene.parts {
            if !files.contains(&p.file) {
                files.push(p.file.clone());
            }
        }
        let avail = ui.available_height();
        egui::ScrollArea::vertical().id_salt("tree").max_height(avail * 0.5).auto_shrink([false, true]).show(ui, |ui| {
            for f in &files {
                let fname = f.file_name().unwrap_or_default().to_string_lossy().to_string();
                let idx: Vec<usize> = (0..scene.parts.len()).filter(|&i| &scene.parts[i].file == f).collect();
                let draw = |ui: &mut egui::Ui, this: &mut Self| {
                    for &i in &idx {
                        let p = &scene.parts[i];
                        let clash = this.viewer.flags.get(i).copied().unwrap_or(false);
                        let mut text = RichText::new(&p.name);
                        if clash {
                            text = text.color(crate::viewer3d::CLASH);
                        }
                        let r = ui.selectable_label(this.viewer.selected == Some(i), text);
                        if r.hovered() {
                            this.viewer.hover = Some(i);
                        }
                        if r.clicked() {
                            this.viewer.selected = if this.viewer.selected == Some(i) { None } else { Some(i) };
                        }
                    }
                };
                if files.len() == 1 && idx.len() == 1 {
                    draw(ui, self);
                } else {
                    egui::CollapsingHeader::new(RichText::new(fname).strong()).id_salt(f).default_open(true).show(ui, |ui| draw(ui, self));
                }
            }
        });
        ui.separator();
        let Some(i) = self.viewer.selected else {
            ui.label(RichText::new("Select a part in the tree or the view.").weak());
            return;
        };
        let Some(p) = scene.parts.get(i) else { return };
        ui.label(RichText::new(&p.name).strong().color(crate::viewer3d::SELECT));
        let d = p.max - p.min;
        egui::Grid::new("props").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            ui.label("size");
            ui.label(format!("{:.1} x {:.1} x {:.1} mm", d.x, d.y, d.z));
            ui.end_row();
            ui.label("material");
            ui.label(p.material.clone().unwrap_or_else(|| "not given by the CAD".into()));
            ui.end_row();
            ui.label("mass");
            ui.label(p.mass_g.map(|m| format!("{m:.1} g")).unwrap_or_else(|| "not given by the CAD".into()));
            ui.end_row();
            ui.label("faces / edges");
            ui.label(format!("{} triangles, {} edge segments", p.tris, p.edges));
            ui.end_row();
            ui.label("file");
            ui.label(RichText::new(p.file.file_name().unwrap_or_default().to_string_lossy()).monospace().small());
            ui.end_row();
            if !p.interferes_with.is_empty() {
                ui.label(RichText::new("overlaps").color(crate::viewer3d::CLASH));
                ui.label(p.interferes_with.join(", "));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Parameters").strong());
        if p.params.is_empty() {
            ui.label(RichText::new("No editable parameter metadata was exported for this part.").weak().small());
            return;
        }
        // read-only: editing and regeneration belong to the CAD editor
        // (docs/cad-handoff.md, "Desktop viewer")
        egui::Grid::new("params").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            for (k, v) in &p.params {
                ui.label(RichText::new(k).monospace());
                ui.label(v.as_f64().map(num).unwrap_or_else(|| v.to_string()));
                ui.end_row();
            }
        });
    }

    /// The headline numbers as chips (top bar).
    fn headline_strip(&self, ui: &mut egui::Ui, s: &RunState) {
        let outcome = |name: &Option<String>| name.as_ref().and_then(|n| s.checks.as_ref()?.outcomes.iter().find(|o| &o.name == n));
        for h in &s.headlines {
            let c = match outcome(&h.check) {
                Some(o) if o.passed() => PASS,
                Some(_) => FAIL,
                None => ui.visuals().text_color(),
            };
            let lim = match (h.limit, h.bound) {
                (Some(l), Bound::AtLeast) => format!(" (need {})", num(l)),
                (Some(l), Bound::AtMost) => format!(" (cap {})", num(l)),
                _ => String::new(),
            };
            ui.label(RichText::new(format!("{} ", h.label)).weak());
            ui.label(RichText::new(format!("{} {}", num(h.value), h.unit)).strong().color(c)).on_hover_text(lim.trim().to_string());
            ui.add_space(8.0);
        }
    }

    fn renders(&mut self, ui: &mut egui::Ui, rect: egui::Rect, s: Option<&RunState>) {
        let dir = self.dir().unwrap_or_default();
        let mut items: Vec<(String, PathBuf)> = vec![];
        if let Some(s) = s {
            for (v, p) in &s.renders {
                if let Some(f) = locate(&dir, p) {
                    items.push((format!("render: {v}"), f));
                }
            }
            for (v, p, note) in &s.feedback {
                if let Some(f) = locate(&dir, p) {
                    items.push((format!("person on {v}: {}", truncate(note, 40)), f));
                }
            }
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        if items.is_empty() {
            child.centered_and_justified(|ui| ui.label(RichText::new("No renders in this run yet. The 3D view is live; draw on it.").weak()));
            return;
        }
        self.render_sel = self.render_sel.min(items.len() - 1);
        child.horizontal_wrapped(|ui| {
            for (i, (l, _)) in items.iter().enumerate() {
                ui.selectable_value(&mut self.render_sel, i, l);
            }
        });
        let (_, p) = &items[self.render_sel];
        let avail = child.available_size();
        child.centered_and_justified(|ui| {
            ui.add(egui::Image::new(format!("file://{}", p.display())).max_size(avail).maintain_aspect_ratio(true));
        });
    }
}

impl eframe::App for WrightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        self.finish_shot();
        self.markup.keys(&ctx);
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::M) && !i.modifiers.any()) && !self.markup.on {
            self.markup.on = true;
        }
        if !self.markup.on && !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                if !i.modifiers.any() {
                    if i.key_pressed(egui::Key::S) {
                        self.viewer.tool = crate::viewer3d::Tool::Select;
                    }
                    if i.key_pressed(egui::Key::D) {
                        self.viewer.tool = crate::viewer3d::Tool::Measure;
                        self.viewer.measure.clear();
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        self.viewer.measure.clear();
                        self.viewer.selected = None;
                    }
                }
            });
        }
        // a dropped recording opens
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.into_iter().find(|p| p.extension().is_some_and(|e| e == "jsonl")) {
            self.replay_path = p.display().to_string();
            self.open_replay(&p);
        }
        // --markup-send: press Send once, 3 s in (verification)
        if self.opts.markup_send && self.started.elapsed().as_secs_f32() > 3.0 {
            self.opts.markup_send = false;
            self.send_markup(&ctx);
        } else if self.opts.markup_send {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        // --screenshot: the whole window, after the given delay
        if let Some((path, after)) = self.opts.screenshot.clone() {
            if !self.auto_shot_done && self.started.elapsed().as_secs_f64() >= after && self.shot.is_none() {
                self.auto_shot_done = true;
                let slot = self.shot_image.clone();
                let c = ctx.clone();
                ctx.request_screenshot(move |img| {
                    *slot.lock().unwrap() = Some(img);
                    c.request_repaint();
                });
                self.shot = Some(Shot { rect: egui::Rect::NOTHING, ppp: ctx.pixels_per_point(), view: String::new(), note: String::new(), whole_window: Some(path) });
            } else if self.auto_shot_done && self.shot.is_none() && self.opts.quit_after_screenshot {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if !self.auto_shot_done {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        let state = self.state().cloned();
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            if let Some((t, _, bad)) = &self.toast {
                ui.label(RichText::new(t).color(if *bad { FAIL } else { INFO }));
            }
            ui.add_space(2.0);
        });
        if matches!(self.mode, Mode::Replay(_)) {
            egui::Panel::bottom("timeline").show(ui, |ui| {
                ui.add_space(4.0);
                self.timeline(ui);
                ui.add_space(4.0);
            });
        }
        egui::Panel::left("parts").resizable(true).default_size(290.0).min_size(220.0).show(ui, |ui| {
            self.parts_panel(ui);
        });
        egui::Panel::right("side").resizable(true).default_size(420.0).min_size(280.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let checks_label = match state.as_ref().and_then(|s| s.checks.as_ref()) {
                    Some(c) => RichText::new(format!("Checks {}/{}", c.passed, c.passed + c.failed)).color(if c.failed == 0 { PASS } else { FAIL }),
                    None => RichText::new("Checks"),
                };
                ui.selectable_value(&mut self.right, Right::Checks, checks_label);
                ui.selectable_value(&mut self.right, Right::Log, "Agents");
                ui.selectable_value(&mut self.right, Right::Bom, "BOM");
                let nm = self.media.iter().filter_map(|f| f.manifest.as_ref().ok()).map(|m| m.items.len()).sum::<usize>();
                ui.selectable_value(&mut self.right, Right::Media, format!("Media ({nm})"));
                ui.selectable_value(&mut self.right, Right::Guide, "Guide");
                ui.selectable_value(&mut self.right, Right::Files, "Files");
            });
            ui.separator();
            match (&state, self.right) {
                (None, _) => {
                    ui.label(RichText::new("Nothing to show yet.").weak());
                }
                (Some(s), Right::Checks) => self.checks_panel(ui, s),
                (Some(s), Right::Log) => self.log_panel(ui, s),
                (Some(_), Right::Bom) => self.bom_panel(ui),
                (Some(_), Right::Media) => self.media_panel(ui),
                (Some(_), Right::Guide) => self.guide_panel(ui),
                (Some(s), Right::Files) => self.files_panel(ui, s),
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.center(ui, state.as_ref());
        });
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(gl) = gl {
            self.viewer.destroy(gl);
        }
    }
}

fn read_csv(p: &Path) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut r = csv::Reader::from_path(p)?;
    let head: Vec<String> = r.headers()?.iter().map(str::to_string).collect();
    let rows = r.records().filter_map(Result::ok).map(|rec| rec.iter().map(str::to_string).collect()).collect();
    Ok((head, rows))
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() > n {
        format!("{}...", s.chars().take(n).collect::<String>())
    } else {
        s
    }
}

fn num(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

fn agent_color(agent: &str) -> Color32 {
    if agent.starts_with("reviewer") {
        FAIL
    } else if agent.starts_with("specialist") {
        Color32::from_rgb(0xc0, 0x9a, 0xff)
    } else if agent == "orchestrator" {
        INFO
    } else {
        Color32::from_gray(200)
    }
}
