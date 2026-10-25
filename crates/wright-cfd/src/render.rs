//! Small raster plotting: filled scalar fields, line charts, labels, PNG.
//!
//! No solver output passes through a heavy plotting stack: a field is a set
//! of triangles with a value at each corner, shaded per pixel by barycentric
//! interpolation through a colormap (the same thing ParaView's surface
//! colouring does with "interpolate scalars before mapping"). Text is drawn
//! with `ab_glyph` from a system TrueType font (Arial on macOS, or
//! `WRIGHT_FONT`); with no font the picture is drawn without labels.
//!
//! Colormaps: Turbo (Google's polynomial approximation, Mikhailov 2019,
//! Apache-2.0, `turbo_colormap.glsl`) for magnitudes, and a blue-white-red
//! diverging map (Moreland 2009 "cool to warm" end points) for Cp.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::path::Path;
use std::sync::OnceLock;

pub type Rgb = [u8; 3];

pub const WHITE: Rgb = [255, 255, 255];
pub const BLACK: Rgb = [20, 20, 24];
pub const GREY: Rgb = [140, 140, 150];
pub const LIGHT: Rgb = [225, 228, 235];
pub const BG: Rgb = [250, 250, 252];
/// Series colours for charts (distinguishable, colour-blind safe: Okabe-Ito).
pub const SERIES: [Rgb; 6] = [[0, 114, 178], [213, 94, 0], [0, 158, 115], [204, 121, 167], [86, 180, 233], [230, 159, 0]];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmap {
    Turbo,
    CoolWarm,
}

fn clamp01(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}

impl Cmap {
    pub fn at(self, t: f64) -> Rgb {
        let t = clamp01(t);
        match self {
            Cmap::Turbo => {
                // Mikhailov's polynomial fit of Turbo (turbo_colormap.glsl)
                let r = 0.13572138 + t * (4.61539260 + t * (-42.66032258 + t * (132.13108234 + t * (-152.94239396 + t * 59.28637943))));
                let g = 0.09140261 + t * (2.19418839 + t * (4.84296658 + t * (-14.18503333 + t * (4.27729857 + t * 2.82956604))));
                let b = 0.10667330 + t * (12.64194608 + t * (-60.58204836 + t * (110.36276771 + t * (-89.90310912 + t * 27.34824973))));
                [(clamp01(r) * 255.0) as u8, (clamp01(g) * 255.0) as u8, (clamp01(b) * 255.0) as u8]
            }
            Cmap::CoolWarm => {
                // blue (59,76,192) -> white-ish (221,221,221) -> red (180,4,38)
                let (a, m, c) = ([59.0, 76.0, 192.0], [221.0, 221.0, 221.0], [180.0, 4.0, 38.0]);
                let (p, q, s) = if t < 0.5 { (a, m, t * 2.0) } else { (m, c, (t - 0.5) * 2.0) };
                [0, 1, 2].map(|i| (p[i] + (q[i] - p[i]) * s) as u8)
            }
        }
    }
}

fn font() -> Option<&'static FontVec> {
    static F: OnceLock<Option<FontVec>> = OnceLock::new();
    F.get_or_init(|| {
        let mut paths = vec![];
        if let Ok(p) = std::env::var("WRIGHT_FONT") {
            paths.push(p);
        }
        paths.push("/System/Library/Fonts/Supplemental/Arial.ttf".into());
        paths.push("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
        paths.iter().find_map(|p| std::fs::read(p).ok().and_then(|b| FontVec::try_from_vec(b).ok()))
    })
    .as_ref()
}

#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgb>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, bg: Rgb) -> Self {
        Canvas { w, h, px: vec![bg; w * h] }
    }
    #[inline]
    pub fn set(&mut self, x: i64, y: i64, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }
    fn blend(&mut self, x: i64, y: i64, c: Rgb, a: f64) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && a > 0.0 {
            let p = &mut self.px[y as usize * self.w + x as usize];
            let a = a.min(1.0);
            for i in 0..3 {
                p[i] = (p[i] as f64 * (1.0 - a) + c[i] as f64 * a).round() as u8;
            }
        }
    }
    pub fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: Rgb) {
        for y in y0.round() as i64..y1.round() as i64 {
            for x in x0.round() as i64..x1.round() as i64 {
                self.set(x, y, c);
            }
        }
    }
    /// Anti-aliased line of width `w` px (distance-to-segment coverage).
    pub fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, w: f64, c: Rgb) {
        let (minx, maxx) = (x0.min(x1) - w, x0.max(x1) + w);
        let (miny, maxy) = (y0.min(y1) - w, y0.max(y1) + w);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let l2 = (dx * dx + dy * dy).max(1e-12);
        for y in miny.floor() as i64..=maxy.ceil() as i64 {
            for x in minx.floor() as i64..=maxx.ceil() as i64 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = (((px - x0) * dx + (py - y0) * dy) / l2).clamp(0.0, 1.0);
                let d = ((px - x0 - t * dx).powi(2) + (py - y0 - t * dy).powi(2)).sqrt();
                let a = (w / 2.0 + 0.5 - d).clamp(0.0, 1.0);
                self.blend(x, y, c, a);
            }
        }
    }
    pub fn polyline(&mut self, pts: &[(f64, f64)], w: f64, c: Rgb) {
        for p in pts.windows(2) {
            self.line(p[0].0, p[0].1, p[1].0, p[1].1, w, c);
        }
    }
    /// Polyline dashed by arc length: `on` px drawn, `off` px skipped.
    pub fn dashed(&mut self, pts: &[(f64, f64)], w: f64, on: f64, off: f64, c: Rgb) {
        let mut phase = 0.0;
        for p in pts.windows(2) {
            let (a, b) = (p[0], p[1]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let mut s = 0.0;
            while s < len {
                let period = on + off;
                let in_on = phase % period < on;
                let step = if in_on { on - phase % period } else { period - phase % period }.min(len - s);
                if in_on {
                    let (t0, t1) = (s / len, (s + step) / len);
                    self.line(a.0 + t0 * (b.0 - a.0), a.1 + t0 * (b.1 - a.1), a.0 + t1 * (b.0 - a.0), a.1 + t1 * (b.1 - a.1), w, c);
                }
                s += step;
                phase += step;
            }
        }
    }
    /// Paint everything outside the box (x, y, w, h) with `c`.
    pub fn clip_outside(&mut self, x: f64, y: f64, w: f64, h: f64, c: Rgb) {
        let (wd, ht) = (self.w as f64, self.h as f64);
        self.rect(0.0, 0.0, wd, y, c);
        self.rect(0.0, y + h, wd, ht, c);
        self.rect(0.0, y, x, y + h, c);
        self.rect(x + w, y, wd, y + h, c);
    }
    pub fn disc(&mut self, x: f64, y: f64, r: f64, c: Rgb) {
        for yy in (y - r - 1.0).floor() as i64..=(y + r + 1.0).ceil() as i64 {
            for xx in (x - r - 1.0).floor() as i64..=(x + r + 1.0).ceil() as i64 {
                let d = ((xx as f64 + 0.5 - x).powi(2) + (yy as f64 + 0.5 - y).powi(2)).sqrt();
                self.blend(xx, yy, c, r + 0.5 - d);
            }
        }
    }
    /// Triangle with a scalar at each corner, mapped through `cmap` over
    /// [lo, hi]; `depth` per corner with a z-buffer when given.
    #[allow(clippy::too_many_arguments)]
    pub fn tri_scalar(&mut self, p: [(f64, f64); 3], s: [f64; 3], lo: f64, hi: f64, cmap: Cmap, zbuf: Option<(&mut Vec<f64>, [f64; 3])>, shade: f64) {
        let minx = p.iter().map(|q| q.0).fold(f64::INFINITY, f64::min).floor().max(0.0) as i64;
        let maxx = p.iter().map(|q| q.0).fold(f64::NEG_INFINITY, f64::max).ceil().min(self.w as f64 - 1.0) as i64;
        let miny = p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min).floor().max(0.0) as i64;
        let maxy = p.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max).ceil().min(self.h as f64 - 1.0) as i64;
        let den = (p[1].1 - p[2].1) * (p[0].0 - p[2].0) + (p[2].0 - p[1].0) * (p[0].1 - p[2].1);
        if den.abs() < 1e-12 {
            return;
        }
        let mut zb = zbuf;
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let a = ((p[1].1 - p[2].1) * (px - p[2].0) + (p[2].0 - p[1].0) * (py - p[2].1)) / den;
                let b = ((p[2].1 - p[0].1) * (px - p[2].0) + (p[0].0 - p[2].0) * (py - p[2].1)) / den;
                let c = 1.0 - a - b;
                if a < -1e-9 || b < -1e-9 || c < -1e-9 {
                    continue;
                }
                if let Some((buf, d)) = zb.as_mut() {
                    let z = a * d[0] + b * d[1] + c * d[2];
                    let i = y as usize * self.w + x as usize;
                    if z >= buf[i] {
                        continue;
                    }
                    buf[i] = z;
                }
                let v = a * s[0] + b * s[1] + c * s[2];
                let col = cmap.at((v - lo) / (hi - lo));
                let col = col.map(|k| (k as f64 * shade).clamp(0.0, 255.0) as u8);
                self.set(x, y, col);
            }
        }
    }
    pub fn text_width(&self, s: &str, size: f64) -> f64 {
        let Some(f) = font() else { return 0.0 };
        let sf = f.as_scaled(PxScale::from(size as f32));
        s.chars().map(|ch| sf.h_advance(f.glyph_id(ch)) as f64).sum()
    }
    /// Text with its baseline-left at (x, y); `anchor` 0 left, 0.5 centre, 1 right.
    pub fn text(&mut self, x: f64, y: f64, s: &str, size: f64, c: Rgb, anchor: f64) {
        let Some(f) = font() else { return };
        let sf = f.as_scaled(PxScale::from(size as f32));
        let mut pen = x - anchor * self.text_width(s, size);
        for ch in s.chars() {
            let id = f.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(size as f32), ab_glyph::point(pen as f32, y as f32));
            pen += sf.h_advance(id) as f64;
            if let Some(o) = f.outline_glyph(g) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    self.blend(bb.min.x as i64 + gx as i64, bb.min.y as i64 + gy as i64, c, cov as f64);
                });
            }
        }
    }
    /// Vertical text (rotated 90 deg counter-clockwise), centred on (x, y).
    pub fn text_vertical(&mut self, x: f64, y: f64, s: &str, size: f64, c: Rgb) {
        let w = self.text_width(s, size).ceil() as usize + 4;
        let h = (size * 1.4).ceil() as usize;
        let mut tmp = Canvas::new(w, h, [0, 0, 0]);
        tmp.text(2.0, size, s, size, [255, 255, 255], 0.0);
        for ty in 0..h {
            for tx in 0..w {
                let a = tmp.px[ty * w + tx][0] as f64 / 255.0;
                if a > 0.0 {
                    let ox = x - h as f64 / 2.0 + ty as f64;
                    let oy = y + w as f64 / 2.0 - tx as f64;
                    self.blend(ox as i64, oy as i64, c, a);
                }
            }
        }
    }
    /// Vertical colour bar at (x, y) of size (w, h) with ticks and a label.
    #[allow(clippy::too_many_arguments)]
    pub fn colorbar(&mut self, x: f64, y: f64, w: f64, h: f64, lo: f64, hi: f64, cmap: Cmap, label: &str) {
        for k in 0..h as i64 {
            let t = 1.0 - k as f64 / h;
            self.rect(x, y + k as f64, x + w, y + k as f64 + 1.0, cmap.at(t));
        }
        self.line(x, y, x + w, y, 1.0, GREY);
        self.line(x, y + h, x + w, y + h, 1.0, GREY);
        for t in nice_ticks(lo, hi, 6) {
            let yy = y + h * (1.0 - (t - lo) / (hi - lo));
            self.line(x + w, yy, x + w + 4.0, yy, 1.0, BLACK);
            self.text(x + w + 7.0, yy + 5.0, &fmt_tick(t), 14.0, BLACK, 0.0);
        }
        self.text(x + w / 2.0, y - 10.0, label, 15.0, BLACK, 0.5);
    }
    pub fn save_png(&self, path: &Path) -> anyhow::Result<()> {
        let mut raw = Vec::with_capacity(self.w * self.h * 3);
        for p in &self.px {
            raw.extend_from_slice(p);
        }
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&raw)?;
        Ok(())
    }
}

/// "Nice" tick values (1, 2, 5 x 10^k steps) covering [lo, hi].
pub fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return vec![];
    }
    let raw = (hi - lo) / n.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let mut t = (lo / step).ceil() * step;
    let mut v = vec![];
    while t <= hi + step * 1e-9 {
        v.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    v
}

pub fn fmt_tick(t: f64) -> String {
    let a = t.abs();
    if a == 0.0 {
        "0".into()
    } else if a >= 1000.0 || a < 0.001 {
        format!("{t:.1e}")
    } else if a >= 10.0 {
        format!("{t:.0}")
    } else if a >= 1.0 {
        let s = format!("{t:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        let s = format!("{t:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// One line on a chart: points, a name, and whether to draw markers only.
pub struct Series {
    pub name: String,
    pub pts: Vec<(f64, f64)>,
    pub color: Rgb,
    pub markers: bool,
    pub dashed: bool,
}

impl Series {
    pub fn line(name: impl Into<String>, pts: Vec<(f64, f64)>, color: Rgb) -> Self {
        Series { name: name.into(), pts, color, markers: false, dashed: false }
    }
    pub fn points(name: impl Into<String>, pts: Vec<(f64, f64)>, color: Rgb) -> Self {
        Series { name: name.into(), pts, color, markers: true, dashed: false }
    }
    pub fn dashed(mut self) -> Self {
        self.dashed = true;
        self
    }
}

/// A chart's plotting area inside a canvas, with its data ranges.
pub struct Axes {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
}

impl Axes {
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.x0 + (x - self.xr.0) / (self.xr.1 - self.xr.0) * (self.x1 - self.x0),
            self.y1 - (y - self.yr.0) / (self.yr.1 - self.yr.0) * (self.y1 - self.y0),
        )
    }
}

fn padded(v: impl Iterator<Item = f64>) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for x in v.filter(|x| x.is_finite()) {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if hi - lo < 1e-12 {
        return (lo - 0.5, hi + 0.5);
    }
    let p = (hi - lo) * 0.06;
    (lo - p, hi + p)
}

/// A line chart into the rectangle (x, y, w, h) of `cv`. Returns its axes.
#[allow(clippy::too_many_arguments)]
pub fn chart(cv: &mut Canvas, x: f64, y: f64, w: f64, h: f64, title: &str, xl: &str, yl: &str, series: &[Series], yr: Option<(f64, f64)>) -> Axes {
    let xr = padded(series.iter().flat_map(|s| s.pts.iter().map(|p| p.0)));
    let yr = yr.unwrap_or_else(|| padded(series.iter().flat_map(|s| s.pts.iter().map(|p| p.1))));
    let ax = Axes { x0: x + 70.0, y0: y + 40.0, x1: x + w - 20.0, y1: y + h - 55.0, xr, yr };
    cv.rect(ax.x0, ax.y0, ax.x1, ax.y1, WHITE);
    for t in nice_ticks(xr.0, xr.1, 6) {
        let (px, _) = ax.map(t, yr.0);
        cv.line(px, ax.y0, px, ax.y1, 1.0, LIGHT);
        cv.text(px, ax.y1 + 20.0, &fmt_tick(t), 14.0, BLACK, 0.5);
    }
    for t in nice_ticks(yr.0, yr.1, 5) {
        let (_, py) = ax.map(xr.0, t);
        cv.line(ax.x0, py, ax.x1, py, 1.0, LIGHT);
        cv.text(ax.x0 - 6.0, py + 5.0, &fmt_tick(t), 14.0, BLACK, 1.0);
    }
    cv.line(ax.x0, ax.y1, ax.x1, ax.y1, 1.2, GREY);
    cv.line(ax.x0, ax.y0, ax.x0, ax.y1, 1.2, GREY);
    cv.text(x + w / 2.0, y + 24.0, title, 17.0, BLACK, 0.5);
    cv.text((ax.x0 + ax.x1) / 2.0, y + h - 12.0, xl, 15.0, BLACK, 0.5);
    cv.text_vertical(x + 16.0, (ax.y0 + ax.y1) / 2.0, yl, 15.0, BLACK);
    for s in series {
        let pts: Vec<(f64, f64)> = s.pts.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).map(|p| ax.map(p.0, p.1)).collect();
        if s.markers {
            for p in &pts {
                cv.disc(p.0, p.1, 4.0, s.color);
            }
        } else if s.dashed {
            cv.dashed(&pts, 2.2, 9.0, 6.0, s.color);
        } else {
            cv.polyline(&pts, 2.4, s.color);
        }
    }
    // legend, top-left inside the axes
    let mut ly = ax.y0 + 18.0;
    for s in series {
        if s.markers {
            cv.disc(ax.x0 + 22.0, ly - 5.0, 4.0, s.color);
        } else {
            cv.line(ax.x0 + 12.0, ly - 5.0, ax.x0 + 32.0, ly - 5.0, 2.4, s.color);
        }
        cv.text(ax.x0 + 40.0, ly, &s.name, 14.0, BLACK, 0.0);
        ly += 19.0;
    }
    ax
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticks_are_nice() {
        let t = nice_ticks(0.0, 1.0, 5);
        assert_eq!(t.len(), 6);
        assert!((t[5] - 1.0).abs() < 1e-12 && t[0] == 0.0);
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
        assert_eq!(fmt_tick(0.25), "0.25");
    }
    #[test]
    fn colormaps_span() {
        let a = Cmap::Turbo.at(0.15);
        let b = Cmap::Turbo.at(1.0);
        assert!(a[2] > a[0] && b[0] > b[2]);
        assert_eq!(Cmap::CoolWarm.at(0.5), [221, 221, 221]);
    }
}
