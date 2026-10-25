//! Frames to an H.264 MP4 with ffmpeg (libx264, yuv420p so every player
//! opens it; `-movflags +faststart` so a browser can start before the end).

use crate::render::Canvas;
use anyhow::{bail, Context};
use std::path::Path;
use std::process::Command;

/// Encode `frames` at `fps` into `out` (an .mp4). Each frame is held
/// `hold` times (slow sweeps without re-rendering).
pub fn encode(frames: &[Canvas], fps: u32, hold: usize, out: &Path) -> anyhow::Result<()> {
    if frames.is_empty() {
        bail!("no frames for {}", out.display());
    }
    let tmp = out.with_extension("frames");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let mut n = 0;
    for f in frames {
        for _ in 0..hold.max(1) {
            f.save_png(&tmp.join(format!("f{n:05}.png")))?;
            n += 1;
        }
    }
    let status = Command::new(crate::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-framerate"])
        .arg(fps.to_string())
        .arg("-i")
        .arg(tmp.join("f%05d.png"))
        .args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart"])
        .arg(out)
        .status()
        .context("running ffmpeg")?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !status.success() {
        bail!("ffmpeg failed ({status}) writing {}", out.display());
    }
    Ok(())
}
