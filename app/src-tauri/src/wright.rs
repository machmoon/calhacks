//! Drive the `wright` CLI from the app: start a design run, tail its
//! `events.jsonl`, send pause/approve/resume/stop through the run's control
//! mailbox, and list recordings to replay.
//!
//! The run is one child process, kept the way `engine.rs` keeps the engine
//! (which follows ollama/ollama `app/lifecycle/server.go`). The CLI is the
//! engine: this file never interprets events, the page folds them
//! (`src/pages/wright/reducer.ts`). The control files follow
//! `wright-agents/src/checkpoint.rs` `Mailbox`: a file named after the
//! command in `<run_dir>/control/`, its content the note; the harness
//! writes `control/status.json`. Solver paths come from `wright/scripts/env.sh`,
//! sourced by bash with every value passed as a positional parameter, never
//! interpolated into the script text.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct WrightProcess {
    child: Mutex<Option<(Child, String)>>,
    /// (run_dir, exit code) of the last child that ended on its own.
    last_exit: Mutex<Option<(String, i32)>>,
}

#[derive(Debug, Serialize)]
pub struct WrightStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub run_dir: Option<String>,
    pub log: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TailResult {
    /// New complete lines at or after `from_line`.
    pub lines: Vec<String>,
    /// Pass this as `from_line` next time.
    pub next: usize,
    /// `<run_dir>/control/status.json`, if the harness wrote one.
    pub status: Option<serde_json::Value>,
    /// True while the child we started for this run_dir is alive.
    pub running: bool,
    /// One plain sentence (cause and fix) when the child exited with an
    /// error; None otherwise.
    pub failure: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunListing {
    pub name: String,
    pub path: String,
    /// `demo` (a recording shipped in the repo) or `run`.
    pub kind: String,
    pub domain: String,
    pub request: String,
    pub events: usize,
    pub modified_ms: u64,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// The wright checkout. `WRIGHT_ROOT` overrides the default location.
pub(crate) fn wright_root() -> Result<PathBuf, String> {
    let root = std::env::var_os("WRIGHT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("Desktop/Coding/wright"));
    if root.join("scripts/env.sh").is_file() {
        Ok(root)
    } else {
        Err(format!("no wright checkout at {} (set WRIGHT_ROOT)", root.display()))
    }
}

fn wright_binary(root: &Path) -> Result<PathBuf, String> {
    let bin = root.join("target/release/wright");
    if bin.is_file() {
        Ok(bin)
    } else {
        Err(format!("{} is missing; build it in the wright repo first", bin.display()))
    }
}

/// A run directory must sit under the wright checkout or the user's home,
/// and may not contain `..`: the page names directories, not the app.
fn checked_dir(run_dir: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(run_dir);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("run_dir must be an absolute path without ..".into());
    }
    if !p.starts_with(home()) {
        return Err("run_dir must be under your home folder".into());
    }
    Ok(p)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Record how a child that ended on its own exited, then report.
fn status_of(state: &WrightProcess, slot: &mut Option<(Child, String)>, log: Option<String>) -> WrightStatus {
    let done = match slot.as_mut() {
        Some((child, dir)) => match child.try_wait() {
            Ok(Some(st)) => Some((dir.clone(), st.code().unwrap_or(-1))),
            _ => None,
        },
        None => None,
    };
    if let Some(d) = done {
        *slot = None;
        *state.last_exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(d);
    }
    WrightStatus {
        running: slot.is_some(),
        pid: slot.as_ref().map(|(c, _)| c.id()),
        run_dir: slot.as_ref().map(|(_, d)| d.clone()),
        log,
    }
}

#[tauri::command]
pub fn wright_show_overlay(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let window = app.get_webview_window("main").ok_or("Overlay window is unavailable")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wright_status(state: tauri::State<'_, WrightProcess>) -> WrightStatus {
    let mut slot = state.child.lock().unwrap_or_else(|e| e.into_inner());
    status_of(&state, &mut slot, None)
}

#[tauri::command]
pub async fn wright_start(
    state: tauri::State<'_, WrightProcess>,
    domain: String,
    request: String,
    run_dir: Option<String>,
    attachment_ids: Option<Vec<String>>,
) -> Result<WrightStatus, String> {
    if !["aircraft", "rocket", "balloon"].contains(&domain.as_str()) {
        return Err(format!("unknown domain {domain}"));
    }
    let attachment_ids = attachment_ids.unwrap_or_default();
    if attachment_ids.len() > 1 { return Err("Use one PDF per native CAD request".into()); }
    let attachments: Vec<PathBuf> = attachment_ids.iter().map(|id| crate::wright_pdf::prepared_path(id)).collect::<Result<_, _>>()?;
    if attachments.iter().any(|p| !p.is_file()) { return Err("PDF attachment is missing; attach it again".into()); }
    let request = if request.trim().is_empty() && !attachments.is_empty() { "Build the VTOL from the attached specification.".to_string() } else { request };
    if request.trim().is_empty() {
        return Err("the request is empty".into());
    }
    let mut slot = state.child.lock().unwrap_or_else(|e| e.into_inner());
    if status_of(&state, &mut slot, None).running {
        return Err("a Wright run is already in progress; stop it first".into());
    }
    let root = wright_root()?;
    let project = request.split_whitespace().map(|s| s.trim_matches(|c: char| c == '\'' || c == '"' || c == '.' || c == '`'))
        .find(|s| s.starts_with("cad/projects/") && s.ends_with("/BUILD_PROMPT.txt"))
        .or_else(|| if attachments.is_empty() { None } else { Some("cad/projects/vtol-demonstrator/BUILD_PROMPT.txt") });
    let bin = if project.is_some() { PathBuf::from("python3") } else {
        wright_binary(&root).map_err(|_| BIN_FIX.to_string())?
    };
    // The transport the harness will use must be ready before a run is paid for.
    let claude = claude_binary();
    if project.is_none() && (std::env::var("WRIGHT_TRANSPORT").unwrap_or_default().is_empty() || cli_transport()) {
        match &claude {
            None => return Err(explain("claude: command not found")),
            Some(c) if !claude_signed_in(c) => return Err(explain("not logged in")),
            _ => {}
        }
    }
    let dir = match run_dir.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => checked_dir(d)?,
        None => root.join("runs").join(format!("{}-{}", if project.is_some() { "cad-project" } else { &domain }, now_ms() / 1000)),
    };
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let log = dir.join("wright.log");
    let out = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|e| format!("could not open {}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new("bash");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Some(c) = &claude {
        // A GUI app's PATH has no ~/.local/bin: hand the harness the binary.
        if std::env::var_os("WRIGHT_CLAUDE_BIN").is_none() {
            cmd.env("WRIGHT_CLAUDE_BIN", c);
        }
    }
    *state.last_exit.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut job_args: Vec<String> = if let Some(brief) = project {
        vec![root.join("cad/project_runner/run.py").display().to_string(),
             "--brief".into(), brief.into(), "--request".into(), request.clone(),
             "--output-dir".into()]
    } else { vec!["run".into(), domain.clone(), request.clone(), "-o".into()] };
    // The output path follows --output-dir before any optional PDF argument.
    job_args.push(dir.display().to_string());
    for attachment in attachments { job_args.extend(["--pdf-manifest".into(), attachment.display().to_string()]); }
    let child = cmd
        .arg(root.join("scripts/run-with-storage.sh"))
        .arg(&root)
        .arg(&dir)
        .arg(&domain)
        .arg(&request)
        .arg(&bin)
        .args(&job_args)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("could not start wright: {e}"))?;
    *slot = Some((child, dir.display().to_string()));
    Ok(status_of(&state, &mut slot, Some(log.display().to_string())))
}

/// Kill the child. The page sends the `stop` control first so the harness
/// can checkpoint; this is the hard stop behind it.
#[tauri::command]
pub fn wright_stop(state: tauri::State<'_, WrightProcess>) -> WrightStatus {
    let mut slot = state.child.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((mut child, _)) = slot.take() {
        #[cfg(unix)]
        let _ = Command::new("/bin/kill").args(["-TERM", &format!("-{}", child.id())]).status();
        let _ = child.kill();
        let _ = child.wait();
    }
    status_of(&state, &mut slot, None)
}

pub fn stop_on_exit(state: &WrightProcess) {
    let mut slot = state.child.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((mut child, _)) = slot.take() {
        #[cfg(unix)]
        let _ = Command::new("/bin/kill").args(["-TERM", &format!("-{}", child.id())]).status();
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[tauri::command]
pub fn wright_tail(
    state: tauri::State<'_, WrightProcess>,
    run_dir: String,
    from_line: usize,
) -> Result<TailResult, String> {
    let dir = checked_dir(&run_dir)?;
    let text = fs::read_to_string(dir.join("events.jsonl")).unwrap_or_default();
    // Only complete lines: the harness may be mid-write on the last one.
    let complete = match text.rfind('\n') {
        Some(i) => &text[..i],
        None => "",
    };
    let all: Vec<&str> = complete.lines().collect();
    let lines: Vec<String> = all.iter().skip(from_line).map(|s| s.to_string()).collect();
    let status = fs::read_to_string(dir.join("control/status.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let mut slot = state.child.lock().unwrap_or_else(|e| e.into_inner());
    let running = status_of(&state, &mut slot, None).run_dir.as_deref() == Some(dir.to_str().unwrap_or(""));
    let failure = if running {
        None
    } else {
        let last = state.last_exit.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match last {
            Some((d, code)) if code != 0 && d == dir.display().to_string() => {
                let log = fs::read_to_string(dir.join("wright.log")).unwrap_or_default();
                let tail: String = log.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                Some(explain(&tail))
            }
            _ => None,
        }
    };
    Ok(TailResult { next: all.len().max(from_line), lines, status, running, failure })
}

#[tauri::command]
pub fn wright_control(run_dir: String, action: String, note: String) -> Result<(), String> {
    if !["pause", "resume", "approve", "stop"].contains(&action.as_str()) {
        return Err(format!("unknown control {action}"));
    }
    let dir = checked_dir(&run_dir)?.join("control");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // `<command>-<ms>`: the harness reads the word before the first `-`.
    fs::write(dir.join(format!("{action}-{}.txt", now_ms())), note).map_err(|e| e.to_string())
}

fn describe_run(path: &Path, kind: &str) -> Option<RunListing> {
    let events = path.join("events.jsonl");
    let meta = fs::metadata(&events).ok()?;
    let text = fs::read_to_string(&events).ok()?;
    let first: serde_json::Value = text.lines().next().and_then(|l| serde_json::from_str(l).ok()).unwrap_or_default();
    Some(RunListing {
        name: path.file_name()?.to_string_lossy().into_owned(),
        path: path.display().to_string(),
        kind: kind.into(),
        domain: first["domain"].as_str().unwrap_or("").into(),
        request: first["request"].as_str().unwrap_or("").into(),
        events: text.lines().count(),
        modified_ms: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    })
}

#[tauri::command]
pub fn wright_list_runs() -> Result<Vec<RunListing>, String> {
    let root = wright_root()?;
    let mut out = vec![];
    for (sub, kind) in [("runs", "run"), ("demo", "demo")] {
        let Ok(rd) = fs::read_dir(root.join(sub)) else { continue };
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.extend(describe_run(&e.path(), kind));
            }
        }
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(out)
}

/// A path an event recorded (relative to the recording's working directory,
/// to the run dir, or absolute), found on this machine. Same rule as
/// `wright-desktop` `model::locate`: as given, else under the run dir with
/// leading components dropped one at a time. Also tried under the wright root.
fn locate(dirs: &[PathBuf], recorded: &str) -> Option<PathBuf> {
    let p = Path::new(recorded);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let comps: Vec<_> = p.components().collect();
    for base in dirs {
        for k in 0..comps.len() {
            let c = base.join(comps[k..].iter().collect::<PathBuf>());
            if c.exists() {
                return Some(c);
            }
        }
    }
    None
}

/// Resolve a recorded path to an absolute one inside the checkout or the
/// run directory. Returns None when it cannot be found.
#[tauri::command]
pub fn wright_resolve(run_dir: String, recorded: String) -> Result<Option<String>, String> {
    let dir = checked_dir(&run_dir)?;
    let mut bases = vec![dir];
    if let Ok(root) = wright_root() {
        bases.push(root);
    }
    let Some(found) = locate(&bases, &recorded) else { return Ok(None) };
    let canon = found.canonicalize().map_err(|e| e.to_string())?;
    if !canon.starts_with(home()) || recorded.contains("..") {
        return Err("path is outside your home folder".into());
    }
    Ok(Some(canon.display().to_string()))
}

const MAX_READ: u64 = 128 * 1024 * 1024;

/// Bytes of a file the run wrote (a PNG, STL or GLB) for the page to draw.
/// Only files under the wright checkout or a run's own directory, and only
/// the formats the page shows.
#[tauri::command]
pub fn wright_read_file(path: String) -> Result<tauri::ipc::Response, String> {
    let p = PathBuf::from(&path).canonicalize().map_err(|e| format!("{path}: {e}"))?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["png", "jpg", "jpeg", "stl", "glb", "svg", "csv", "txt", "md", "json"].contains(&ext.as_str()) {
        return Err(format!("{ext} files are not shown here"));
    }
    let root = wright_root()?.canonicalize().map_err(|e| e.to_string())?;
    if !p.starts_with(&root) && !p.starts_with(home().join("Desktop/Coding")) {
        return Err("path is outside the wright checkout".into());
    }
    let mut f = fs::File::open(&p).map_err(|e| e.to_string())?;
    if f.metadata().map(|m| m.len()).unwrap_or(0) > MAX_READ {
        return Err("file is too large to show".into());
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(buf))
}

/// Open an artifact with the system's default app (the opener plugin).
#[tauri::command]
pub fn wright_open(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = PathBuf::from(&path).canonicalize().map_err(|e| format!("{path}: {e}"))?;
    if !p.starts_with(home()) {
        return Err("path is outside your home folder".into());
    }
    app.opener().open_path(p.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

// ---- environment status and plain-sentence errors ---------------------------

const BIN_FIX: &str = "The wright binary is missing: run `cargo build --release -p wright-cli` in the wright repo.";

#[derive(Debug, Serialize)]
pub struct EnvItem {
    pub name: String,
    pub ok: bool,
    /// Where it was found, or what was seen.
    pub detail: String,
    /// One sentence naming the fix; empty when ok.
    pub fix: String,
    /// Blocks Run when not ok.
    pub required: bool,
}

#[derive(Debug, Serialize)]
pub struct EnvStatus {
    pub items: Vec<EnvItem>,
    /// True when every required item is ok.
    pub ready: bool,
}

fn cli_transport() -> bool {
    matches!(std::env::var("WRIGHT_TRANSPORT").unwrap_or_default().as_str(), "" | "cli" | "claude-cli")
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The `claude` binary the harness will run: `WRIGHT_CLAUDE_BIN`, PATH, then
/// the places the installer puts it (a GUI app's PATH is minimal).
fn claude_binary() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("WRIGHT_CLAUDE_BIN") {
        let p = PathBuf::from(v);
        return if p.is_absolute() { p.is_file().then_some(p) } else { on_path(&p.to_string_lossy()) };
    }
    on_path("claude").or_else(|| {
        [home().join(".local/bin/claude"), "/opt/homebrew/bin/claude".into(), "/usr/local/bin/claude".into()]
            .into_iter()
            .find(|p| p.is_file())
    })
}

/// Run a command for at most `secs`; its stdout, or None on failure/timeout.
fn run_for(cmd: &mut Command, secs: u64) -> Option<String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed().as_secs() >= secs => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

/// `claude auth status` prints JSON with `loggedIn`.
fn claude_signed_in(bin: &Path) -> bool {
    run_for(Command::new(bin).args(["auth", "status"]), 8)
        .and_then(|o| serde_json::from_str::<serde_json::Value>(&o).ok())
        .and_then(|v| v["loggedIn"].as_bool())
        .unwrap_or(false)
}

/// One HEAD to api.anthropic.com, 3 s. Any HTTP status means reachable.
fn network_reachable() -> bool {
    run_for(
        Command::new("curl").args(["-sS", "-I", "-m", "3", "-o", "/dev/null", "-w", "%{http_code}", "https://api.anthropic.com"]),
        5,
    )
    .map(|c| c.trim() != "000" && !c.trim().is_empty())
    .unwrap_or(false)
}

fn solver_item(name: &str, found: Option<PathBuf>, hint: &str) -> EnvItem {
    EnvItem {
        name: name.into(),
        ok: found.is_some(),
        detail: found.map(|p| p.display().to_string()).unwrap_or_else(|| "not found, stage skipped".into()),
        fix: hint.into(),
        required: false,
    }
}

#[tauri::command]
pub async fn wright_env_status() -> Result<EnvStatus, String> {
    tauri::async_runtime::spawn_blocking(env_status).await.map_err(|e| e.to_string())
}

fn env_status() -> EnvStatus {
    let mut items = vec![];
    let root = wright_root();
    // 1. the binary
    let bin = root.as_ref().ok().and_then(|r| wright_binary(r).ok());
    items.push(EnvItem {
        name: "wright".into(),
        ok: bin.is_some(),
        detail: bin.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()),
        fix: if bin.is_some() { String::new() } else { BIN_FIX.into() },
        required: true,
    });
    // 2. the Claude transport (WRIGHT_TRANSPORT: cli by default, api needs a key)
    let transport = std::env::var("WRIGHT_TRANSPORT").unwrap_or_default();
    let (ok, detail, fix) = if cli_transport() {
        match claude_binary() {
            None => (false, "claude CLI not found".to_string(), explain("claude: command not found")),
            Some(c) if claude_signed_in(&c) => (true, format!("claude CLI signed in ({})", c.display()), String::new()),
            Some(c) => (false, format!("claude CLI at {} is not signed in", c.display()), explain("not logged in")),
        }
    } else if transport == "api" || transport == "anthropic" {
        let set = std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
        (set, if set { "ANTHROPIC_API_KEY is set".into() } else { "ANTHROPIC_API_KEY is not set".into() },
         if set { String::new() } else { explain("ANTHROPIC_API_KEY is not set") })
    } else {
        (true, format!("WRIGHT_TRANSPORT={transport}"), String::new())
    };
    items.push(EnvItem { name: "claude".into(), ok, detail, fix, required: true });
    // 3. network
    let net = network_reachable();
    items.push(EnvItem {
        name: "network".into(),
        ok: net,
        detail: if net { "api.anthropic.com answered".into() } else { "api.anthropic.com did not answer in 3 s".into() },
        fix: if net { String::new() } else { explain("could not resolve host") },
        required: true,
    });
    // optional solvers: missing ones are skipped stages, never a blocker
    let freecad = std::env::var_os("WRIGHT_FREECAD")
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| on_path("FreeCADCmd"))
        .or_else(|| on_path("freecadcmd"))
        .or_else(|| std::env::var_os("WRIGHT_FREECAD_APP").map(PathBuf::from)
            .map(|p| p.join("Contents/Resources/bin/python")).filter(|p| p.exists()))
        .or_else(|| Some(home().join("Applications/FreeCAD.app/Contents/Resources/bin/python")).filter(|p| p.exists()))
        .or_else(|| Some(PathBuf::from("/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd")).filter(|p| p.exists()));
    items.push(solver_item("FreeCAD", freecad, "Install FreeCAD to get CAD solids and STEP export."));
    let prusa = std::env::var_os("WRIGHT_PRUSASLICER")
        .map(PathBuf::from)
        .or_else(|| {
            [
                home().join("Applications/PrusaSlicer.app/Contents/MacOS/PrusaSlicer"),
                "/Applications/PrusaSlicer.app/Contents/MacOS/PrusaSlicer".into(),
                "/usr/bin/prusa-slicer".into(),
            ]
            .into_iter()
            .find(|p| p.exists())
        })
        .filter(|p| p.exists());
    items.push(solver_item("PrusaSlicer", prusa, "Install PrusaSlicer to get G-code and print times."));
    // The solver paths come from scripts/env.sh, the same file the run sources.
    let (py, su2) = match root {
        Ok(r) => {
            let out = run_for(
                Command::new("bash").arg("-c").arg(r#"source "$1" >/dev/null 2>&1; printf '%s\n%s' "$WRIGHT_PYTHON" "$WRIGHT_SU2""#).arg("env").arg(r.join("scripts/env.sh")),
                5,
            )
            .unwrap_or_default();
            let mut l = out.lines();
            let f = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).filter(|p| p.exists());
            (f(l.next()), f(l.next()))
        }
        Err(_) => (None, None),
    };
    items.push(solver_item("Python venv", py, "Create the solver venv (scripts/env.sh names where it is looked for)."));
    items.push(solver_item("SU2", su2, "Install SU2 to run CFD."));
    let ready = items.iter().filter(|i| i.required).all(|i| i.ok);
    EnvStatus { items, ready }
}

/// One plain sentence naming the cause and the fix, from a process's
/// stderr/log text (or one of our own causes). Unknown text falls back to
/// its last line, so nothing is hidden behind a generic message.
pub fn explain(text: &str) -> String {
    let t = text.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| t.contains(n));
    if has(&["anthropic_api_key is not set"]) {
        "ANTHROPIC_API_KEY is not set: export it, or unset WRIGHT_TRANSPORT to use the Claude CLI.".into()
    } else if has(&["not logged in", "please run /login", "invalid api key", "authentication_error", "oauth token has expired", "401"]) {
        "Claude CLI is not signed in: run `claude` once in a terminal and sign in.".into()
    } else if has(&["claude: command not found", "program not found", "no such file or directory (os error 2)"]) && has(&["claude"]) {
        "Claude CLI not found: install Claude Code, or set WRIGHT_CLAUDE_BIN to its path.".into()
    } else if has(&["could not resolve", "dns", "network is unreachable", "connection refused", "error sending request", "timed out", "offline"]) {
        "No network: Wright could not reach Claude. Check your connection and run again.".into()
    } else if has(&["budget"]) {
        "The run stopped at its spend budget: resume it with `wright run --resume <run folder>`.".into()
    } else {
        let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no output");
        let cut: String = last.chars().take(240).collect();
        format!("Wright stopped: {cut}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_the_common_failures_with_a_fix() {
        assert!(explain("Error: Not logged in · Please run /login").contains("run `claude` once"));
        assert!(explain("claude: command not found").contains("WRIGHT_CLAUDE_BIN"));
        assert!(explain("error sending request for url (https://api.anthropic.com)").starts_with("No network"));
        assert!(explain("ANTHROPIC_API_KEY is not set").contains("export it"));
        assert!(explain("budget exhausted at $5.00").contains("--resume"));
    }

    #[test]
    fn unknown_text_falls_back_to_its_last_line() {
        assert_eq!(explain("a\n\nsolver blew up\n"), "Wright stopped: solver blew up");
        assert_eq!(explain(""), "Wright stopped: no output");
    }

    #[test]
    fn control_names_are_a_closed_set() {
        assert!(wright_control("/nonexistent".into(), "format".into(), String::new()).is_err());
    }
}
