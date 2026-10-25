//! Hand-off to the real tools, for the Wright page.
//!
//! `wright_kicad_svg` plots a board or schematic with KiCad's own
//! `kicad-cli` (`pcb export svg --mode-multi`, one SVG per layer on one
//! shared viewBox so the page can stack them and toggle layers, the way
//! KiCanvas's layers panel does in `src/kicanvas/elements/kc-board/
//! layers-panel.ts`; or `sch export svg`). Output is cached under the temp
//! dir by path and mtime, so switching tabs does not re-plot.
//!
//! `wright_open_in` opens a file in KiCad's PCB/Schematic Editor or in
//! FreeCAD with `open -a`, so the page shows the summary and the real tool
//! opens only when asked.

use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// A file under the home folder with one of `exts`, canonicalized.
fn checked_file(path: &str, exts: &[&str]) -> Result<PathBuf, String> {
    let p = PathBuf::from(path).canonicalize().map_err(|e| format!("{path}: {e}"))?;
    if !p.starts_with(home()) {
        return Err("path is outside your home folder".into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !exts.contains(&ext.as_str()) {
        return Err(format!(".{ext} files cannot be opened here"));
    }
    Ok(p)
}

fn kicad_cli() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("WRIGHT_KICAD_CLI") {
        let p = PathBuf::from(v);
        if p.is_file() {
            return Some(p);
        }
    }
    [
        PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"),
        PathBuf::from("/opt/homebrew/bin/kicad-cli"),
        PathBuf::from("/usr/local/bin/kicad-cli"),
        PathBuf::from("/usr/bin/kicad-cli"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// The layers the Board tab offers, back to front.
const PCB_LAYERS: &str = "B.Cu,F.Cu,F.Mask,F.SilkS,F.Fab,F.CrtYd,Edge.Cuts";

#[derive(Debug, Serialize)]
pub struct SvgLayer {
    /// KiCad layer name (`F.Cu`), or `Schematic`.
    pub layer: String,
    pub svg: String,
}

/// `B_Cu` from `board-B_Cu.svg` -> `B.Cu`; KiCad's file names swap the dot.
fn layer_from_file(stem: &str) -> String {
    let tail = stem.rsplit_once('-').map(|(_, t)| t).unwrap_or(stem);
    tail.replacen('_', ".", 1)
}

#[tauri::command]
pub async fn wright_kicad_svg(path: String, kind: String) -> Result<Vec<SvgLayer>, String> {
    tauri::async_runtime::spawn_blocking(move || kicad_svg(&path, &kind))
        .await
        .map_err(|e| e.to_string())?
}

fn kicad_svg(path: &str, kind: &str) -> Result<Vec<SvgLayer>, String> {
    let exts: &[&str] = if kind == "pcb" { &["kicad_pcb"] } else { &["kicad_sch"] };
    let file = checked_file(path, exts)?;
    let cli = kicad_cli().ok_or("kicad-cli was not found: install KiCad, or set WRIGHT_KICAD_CLI.")?;
    let mtime = fs::metadata(&file).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
    let mut h = DefaultHasher::new();
    (file.display().to_string(), mtime, kind).hash(&mut h);
    let out = std::env::temp_dir().join("wright-ui-svg").join(format!("{:016x}", h.finish()));
    let cached = |dir: &Path| fs::read_dir(dir).map(|rd| rd.flatten().any(|e| e.path().extension().is_some_and(|x| x == "svg"))).unwrap_or(false);
    if !cached(&out) {
        fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&cli);
        if kind == "pcb" {
            cmd.args(["pcb", "export", "svg", "--mode-multi", "--layers", PCB_LAYERS, "--page-size-mode", "2", "--exclude-drawing-sheet", "-o"]);
        } else {
            cmd.args(["sch", "export", "svg", "--exclude-drawing-sheet", "-o"]);
        }
        let status = cmd
            .arg(&out)
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("kicad-cli did not start: {e}"))?;
        if !status.success() || !cached(&out) {
            let _ = fs::remove_dir_all(&out);
            return Err(format!("kicad-cli could not plot {}: open it in KiCad to see why.", file.display()));
        }
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&out)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let layer = if kind == "pcb" { layer_from_file(stem) } else { "Schematic".to_string() };
            fs::read_to_string(&p).map(|svg| SvgLayer { layer, svg }).map_err(|e| e.to_string())
        })
        .collect()
}

/// The board in 3D as GLB bytes (`kicad-cli pcb export glb` with copper,
/// mask, silk and the library 3D models), for the page's own WebGL viewer.
/// STEP stays the CAD hand-off; GLB is only what the page draws.
#[tauri::command]
pub async fn wright_kicad_glb(path: String) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || kicad_glb(&path))
        .await
        .map_err(|e| e.to_string())?
        .map(tauri::ipc::Response::new)
}

fn kicad_glb(path: &str) -> Result<Vec<u8>, String> {
    let file = checked_file(path, &["kicad_pcb"])?;
    let cli = kicad_cli().ok_or("kicad-cli was not found: install KiCad, or set WRIGHT_KICAD_CLI.")?;
    let mtime = fs::metadata(&file).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
    let mut h = DefaultHasher::new();
    (file.display().to_string(), mtime, "glb").hash(&mut h);
    let dir = std::env::temp_dir().join("wright-ui-svg").join(format!("{:016x}", h.finish()));
    let glb = dir.join("board.glb");
    if !glb.is_file() {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ok = Command::new(&cli)
            .args(["pcb", "export", "glb", "--subst-models", "--include-tracks", "--include-pads", "--include-zones", "--include-silkscreen", "--include-soldermask", "-o"])
            .arg(&glb)
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("kicad-cli did not start: {e}"))?
            .success();
        if !ok || !glb.is_file() {
            return Err(format!("kicad-cli could not export {} to GLB: open it in KiCad to see why.", file.display()));
        }
    }
    fs::read(&glb).map_err(|e| e.to_string())
}

/// The app bundle that opens `file` for `app` (`kicad` or `freecad`).
fn app_for(app: &str, file: &Path) -> Result<PathBuf, String> {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let candidates: Vec<PathBuf> = match app {
        "kicad" => {
            let name = match ext.as_str() {
                "kicad_pcb" => "PCB Editor.app",
                "kicad_sch" => "Schematic Editor.app",
                _ => "KiCad.app",
            };
            vec![PathBuf::from("/Applications/KiCad").join(name), home().join("Applications/KiCad").join(name)]
        }
        "freecad" => {
            let mut v = vec![];
            if let Some(a) = std::env::var_os("WRIGHT_FREECAD_APP") {
                v.push(PathBuf::from(a));
            }
            v.push(home().join("Applications/FreeCAD.app"));
            v.push(PathBuf::from("/Applications/FreeCAD.app"));
            v
        }
        _ => return Err(format!("unknown app {app}")),
    };
    candidates.into_iter().find(|p| p.exists()).ok_or_else(|| match app {
        "kicad" => "KiCad is not installed: install it from kicad.org.".to_string(),
        _ => "FreeCAD is not installed: install it from freecad.org.".to_string(),
    })
}

#[tauri::command]
pub fn wright_open_in(app: String, path: String) -> Result<(), String> {
    let exts: &[&str] = match app.as_str() {
        "kicad" => &["kicad_pcb", "kicad_sch", "kicad_pro"],
        "freecad" => &["fcstd", "step", "stp", "stl", "fcmacro"],
        _ => return Err(format!("unknown app {app}")),
    };
    let file = checked_file(&path, exts)?;
    let bundle = app_for(&app, &file)?;
    Command::new("/usr/bin/open")
        .arg("-a")
        .arg(&bundle)
        .arg(&file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())
        .and_then(|s| if s.success() { Ok(()) } else { Err(format!("{} did not open {}", bundle.display(), file.display())) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_names_come_back_dotted() {
        assert_eq!(layer_from_file("board-F_Cu"), "F.Cu");
        assert_eq!(layer_from_file("my-board-Edge_Cuts"), "Edge.Cuts");
        assert_eq!(layer_from_file("board-F_Silkscreen"), "F.Silkscreen");
    }

    #[test]
    fn refuses_files_outside_home_or_of_the_wrong_kind() {
        assert!(checked_file("/etc/hosts", &["kicad_pcb"]).is_err());
        assert!(wright_open_in("paint".into(), "/x".into()).is_err());
    }
}
