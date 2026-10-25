//! Frames to H.264 MP4 through the ffmpeg CLI, and captions drawn on frames.
//!
//! Encoding: raw frames piped to `ffmpeg -f rawvideo -pix_fmt rgba -s WxH
//! -r FPS -i -`, the way matplotlib's `FFMpegWriter` does it
//! (lib/matplotlib/animation.py, `FFMpegWriter._args`: `-f rawvideo -vcodec
//! rawvideo -s WxH -pix_fmt rgba -framerate FPS -i -`, then `-vcodec h264
//! -pix_fmt yuv420p`). Kept: rawvideo on stdin and yuv420p (players refuse
//! yuv444 H.264). Added: `-crf`/`-preset` to keep a clip at a few MB and
//! `+faststart` so the desktop app can start playing before the end.
//!
//! Captions: ab_glyph rasterises the Ubuntu Light / Hack fonts egui already
//! ships (`epaint_default_fonts`); the homebrew ffmpeg has no `drawtext`
//! (built without libfreetype), so text is drawn on the RGBA frame here.

use crate::render::{H, W};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub const FPS: u32 = 30;

pub fn ffmpeg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WRIGHT_FFMPEG") {
        return Some(PathBuf::from(p));
    }
    for p in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"] {
        if Path::new(p).exists() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

pub struct Video {
    child: Child,
    pub path: PathBuf,
    pub frames: usize,
}

impl Video {
    pub fn create(path: &Path) -> anyhow::Result<Self> {
        let ff = ffmpeg().ok_or_else(|| anyhow::anyhow!("ffmpeg not found (set WRIGHT_FFMPEG)"))?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let child = Command::new(ff)
            .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
            .arg(format!("{W}x{H}"))
            .args(["-r", &FPS.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "26", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;
        Ok(Video { child, path: path.to_path_buf(), frames: 0 })
    }
    pub fn push(&mut self, rgba: &[u8]) -> anyhow::Result<()> {
        self.child.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("ffmpeg stdin closed"))?.write_all(rgba)?;
        self.frames += 1;
        Ok(())
    }
    pub fn finish(mut self) -> anyhow::Result<PathBuf> {
        drop(self.child.stdin.take());
        let st = self.child.wait()?;
        anyhow::ensure!(st.success(), "ffmpeg failed writing {}", self.path.display());
        Ok(self.path)
    }
}

/// Text drawn onto an RGBA frame.
pub struct Painter {
    sans: FontRef<'static>,
    mono: FontRef<'static>,
}

impl Default for Painter {
    fn default() -> Self {
        Painter {
            sans: FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font"),
            mono: FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR).expect("bundled font"),
        }
    }
}

impl Painter {
    /// Draw `text` with its top-left at (x, y), `px` tall; returns the width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&self, img: &mut [u8], x: f32, y: f32, px: f32, text: &str, color: [u8; 3], mono: bool) -> f32 {
        let font = if mono { &self.mono } else { &self.sans };
        let sf = font.as_scaled(PxScale::from(px));
        let mut cx = x;
        let base = y + sf.ascent();
        let mut prev = None;
        for ch in text.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                cx += sf.kern(p, id);
            }
            let g = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(cx, base));
            if let Some(o) = font.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px_, py_) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px_ < 0 || py_ < 0 || px_ >= W as i32 || py_ >= H as i32 {
                        return;
                    }
                    let i = 4 * (py_ as usize * W as usize + px_ as usize);
                    for k in 0..3 {
                        img[i + k] = (img[i + k] as f32 * (1.0 - cov) + color[k] as f32 * cov) as u8;
                    }
                });
            }
            cx += sf.h_advance(id);
            prev = Some(id);
        }
        cx - x
    }
    /// A translucent dark band (behind captions).
    pub fn band(&self, img: &mut [u8], x0: u32, y0: u32, x1: u32, y1: u32, alpha: f32) {
        for y in y0.min(H)..y1.min(H) {
            for x in x0.min(W)..x1.min(W) {
                let i = 4 * (y * W + x) as usize;
                for k in 0..3 {
                    img[i + k] = (img[i + k] as f32 * (1.0 - alpha) + 8.0 * alpha) as u8;
                }
            }
        }
    }
    /// A filled bar (progress).
    pub fn bar(&self, img: &mut [u8], x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 3]) {
        for y in y0.min(H)..y1.min(H) {
            for x in x0.min(W)..x1.min(W) {
                let i = 4 * (y * W + x) as usize;
                img[i..i + 3].copy_from_slice(&color);
            }
        }
    }
    /// The standard caption: title top-left, a status line under it, a
    /// progress bar along the bottom, and right-aligned metrics.
    pub fn caption(&self, img: &mut [u8], title: &str, status: &str, right: &[String], progress: f64) {
        self.band(img, 0, 0, W, 78, 0.55);
        self.text(img, 24.0, 12.0, 30.0, title, [240, 240, 240], false);
        self.text(img, 24.0, 48.0, 19.0, status, [255, 180, 90], true);
        let mut y = 12.0;
        for r in right {
            let w = self.measure(r, 19.0, true);
            self.text(img, W as f32 - 24.0 - w, y, 19.0, r, [210, 220, 230], true);
            y += 24.0;
        }
        self.bar(img, 0, H - 6, W, H, [40, 44, 52]);
        self.bar(img, 0, H - 6, (W as f64 * progress.clamp(0.0, 1.0)) as u32, H, [240, 128, 32]);
    }
    pub fn measure(&self, text: &str, px: f32, mono: bool) -> f32 {
        let font = if mono { &self.mono } else { &self.sans };
        let sf = font.as_scaled(PxScale::from(px));
        text.chars().map(|c| sf.h_advance(sf.glyph_id(c))).sum()
    }
}

/// "1 h 23 min" / "12.5 min" / "40 s".
pub fn fmt_minutes(min: f64) -> String {
    if min >= 60.0 {
        format!("{} h {:02} min", (min / 60.0).floor() as u32, (min % 60.0).round() as u32)
    } else if min >= 1.0 {
        format!("{min:.1} min")
    } else {
        format!("{:.0} s", min * 60.0)
    }
}
