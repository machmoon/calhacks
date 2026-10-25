//! The media panel: every `<out_dir>/<kind>/manifest.json` the analysis
//! agents write (CFD, FEA, fab timelapses, CAD/USD, flight sim). The
//! contract is in docs/ARCHITECTURE.md, "Artifacts manifest".
//!
//! Video: frames are extracted once with ffmpeg (`-vf fps=N,scale=W:-2`
//! into numbered PNGs, the documented recipe for image sequences, ffmpeg
//! docs "image2 muxer") into a cache dir, then played as a texture. That is
//! a frame decoder with no new Rust dependency; a pure-Rust mp4 decoder
//! would still need an H.264 decoder (openh264, C) to be useful.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};


/// The manifest types are wright-core's (`crates/wright-core/src/stage.rs`),
/// shared with the stages that write them.
pub use wright_core::{Item, Manifest, StageCheck};

/// One manifest found on disk (or the reason it could not be read).
#[derive(Debug, Clone)]
pub struct Found {
    pub dir: PathBuf,
    pub manifest: Result<Manifest, String>,
}

impl Found {
    /// The item's file, only if it is inside the manifest's directory (see
    /// [`inside`]). `None`: missing, or refused.
    pub fn item_path(&self, item: &Item) -> Option<PathBuf> {
        inside(&self.dir, &item.path)
    }
}

/// Follow a manifest path without leaving `dir`: refused if absolute, if
/// any component is `..`, `.` or a prefix, or if the resolved file (after
/// symlinks) lands outside the canonical `dir`. The web studio had the same
/// rule for its file server (review 4, finding 8); a manifest is written by
/// another agent, so it is input, not trusted.
pub fn inside(dir: &Path, rel: &str) -> Option<PathBuf> {
    use std::path::Component;
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let root = std::fs::canonicalize(dir).ok()?;
    let real = std::fs::canonicalize(root.join(p)).ok()?;
    (real.starts_with(&root) && real.is_file()).then_some(real)
}

/// Every `<out_dir>/*/manifest.json`, sorted by directory name.
pub fn scan(out_dir: &Path) -> Vec<Found> {
    let Ok(rd) = std::fs::read_dir(out_dir) else { return vec![] };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("manifest.json").is_file()).collect();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| {
            let p = dir.join("manifest.json");
            let manifest = std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Manifest>(&t).map_err(|e| format!("{}: {e}", p.display())));
            Found { dir, manifest }
        })
        .collect()
}

pub const FFMPEG_CANDIDATES: [&str; 3] = ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "ffmpeg"];

pub fn ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WRIGHT_FFMPEG") {
        return Some(PathBuf::from(p));
    }
    FFMPEG_CANDIDATES.iter().map(PathBuf::from).find(|p| p.is_file() || p.as_os_str() == "ffmpeg")
}

/// Extraction state of one video.
#[derive(Debug, Clone, Default)]
pub enum Frames {
    #[default]
    NotStarted,
    Extracting,
    Ready { fps: f64, files: Vec<PathBuf> },
    Failed(String),
}

pub const VIDEO_FPS: f64 = 12.0;
const VIDEO_WIDTH: u32 = 720;

/// Where a video's frames are cached: keyed by path, size and mtime, so a
/// re-rendered video is extracted again.
pub fn cache_dir(video: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    video.hash(&mut h);
    if let Ok(m) = std::fs::metadata(video) {
        m.len().hash(&mut h);
        if let Ok(t) = m.modified() {
            t.hash(&mut h);
        }
    }
    std::env::temp_dir().join("wright-desktop").join("frames").join(format!("{:016x}", h.finish()))
}

fn list_frames(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Extract frames on a background thread; `state` is updated when done.
pub fn extract(video: PathBuf, state: Arc<Mutex<Frames>>, repaint: impl Fn() + Send + 'static) {
    *state.lock().unwrap() = Frames::Extracting;
    std::thread::spawn(move || {
        // at most two ffmpeg processes at once: a media tab with a dozen
        // films must not start a dozen decoders
        use std::sync::atomic::{AtomicUsize, Ordering};
        static RUNNING: AtomicUsize = AtomicUsize::new(0);
        while RUNNING.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < 2).then_some(n + 1)).is_err() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        struct Slot;
        impl Drop for Slot {
            fn drop(&mut self) {
                RUNNING.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _slot = Slot;
        let dir = cache_dir(&video);
        let done = dir.join(".done");
        let result = if done.is_file() {
            Ok(())
        } else {
            (|| -> Result<(), String> {
                let ff = ffmpeg().ok_or("ffmpeg not found (set WRIGHT_FFMPEG)")?;
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let out = std::process::Command::new(&ff)
                    .args(["-hide_banner", "-loglevel", "error", "-i"])
                    .arg(&video)
                    .args(["-vf", &format!("fps={VIDEO_FPS},scale='min({VIDEO_WIDTH},iw)':-2")])
                    .arg(dir.join("%05d.png"))
                    .output()
                    .map_err(|e| format!("running {}: {e}", ff.display()))?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
                }
                std::fs::write(&done, b"").map_err(|e| e.to_string())
            })()
        };
        *state.lock().unwrap() = match result {
            Ok(()) => {
                let files = list_frames(&dir);
                if files.is_empty() { Frames::Failed("ffmpeg wrote no frames".into()) } else { Frames::Ready { fps: VIDEO_FPS, files } }
            }
            Err(e) => Frames::Failed(e),
        };
        repaint();
    });
}

/// Reveal a file in Finder (macOS `open -R`); elsewhere open its folder.
pub fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}

/// Open a file with its default app.
pub fn open(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_are_found_per_kind_dir() {
        let dir = std::env::temp_dir().join(format!("wright-media-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cfd")).unwrap();
        std::fs::create_dir_all(dir.join("fea")).unwrap();
        std::fs::write(
            dir.join("cfd/manifest.json"),
            r#"{"kind":"cfd","title":"Wing CFD","items":[{"label":"Cp","path":"cp.png","media":"png","summary":"s","metrics":{"CL":0.61}}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("fea/manifest.json"), "{not json").unwrap();
        let found = scan(&dir);
        assert_eq!(found.len(), 2);
        let m = found[0].manifest.as_ref().unwrap();
        assert_eq!(m.kind, "cfd");
        assert_eq!(m.items[0].metrics["CL"], 0.61);
        assert_eq!(found[0].item_path(&m.items[0]), None, "cp.png does not exist yet");
        std::fs::write(dir.join("cfd/cp.png"), b"x").unwrap();
        assert_eq!(found[0].item_path(&m.items[0]), Some(std::fs::canonicalize(dir.join("cfd/cp.png")).unwrap()));
        assert!(found[1].manifest.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manifest_paths_cannot_leave_their_dir() {
        let dir = std::env::temp_dir().join(format!("wright-media-escape-{}", std::process::id()));
        let kind = dir.join("cfd");
        std::fs::create_dir_all(&kind).unwrap();
        std::fs::write(dir.join("secret.txt"), b"s").unwrap();
        std::fs::write(kind.join("ok.png"), b"x").unwrap();
        assert!(inside(&kind, "ok.png").is_some());
        assert!(inside(&kind, "../secret.txt").is_none());
        assert!(inside(&kind, "./ok.png").is_none());
        assert!(inside(&kind, dir.join("secret.txt").to_str().unwrap()).is_none());
        assert!(inside(&kind, "").is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("secret.txt"), kind.join("link.png")).unwrap();
            assert!(inside(&kind, "link.png").is_none(), "a symlink out of the dir is refused");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
