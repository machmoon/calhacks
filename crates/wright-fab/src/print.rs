//! Print timelapse from the sliced G-code the build already wrote.
//!
//! Reading the G-code follows PrusaSlicer's own viewer:
//! * layers: PrusaSlicer's `;LAYER_CHANGE` / `;Z:` / `;HEIGHT:` tags, one
//!   layer per contiguous run of moves (libvgcode `Layers::update`,
//!   src/libvgcode/src/Slic3r/App/libvgcode/Layers.cpp: a new item when
//!   `vertex.layer_id == m_items.size()`, the range extended and the move
//!   times summed otherwise; the layer's z from its extrusion moves);
//! * roles from `;TYPE:` and widths from `;WIDTH:`, coloured with
//!   libvgcode's `DEFAULT_EXTRUSION_ROLES_COLORS` (version_2.8.0,
//!   src/libvgcode/src/ViewerImpl.cpp);
//! * each extrusion drawn as a prism with a diamond cross-section,
//!   `width` wide and `height` tall, the shape of libvgcode's
//!   `SegmentTemplate` (src/libvgcode/src/SegmentTemplate.cpp,
//!   version_2.8.0: body vertices top/right/bottom/left at both ends);
//!   the end spikes are left out (they only round the joints).
//!   Times: each move's length / F, scaled so the total matches the
//!   slicer's "estimated printing time (normal mode)" (PrusaSlicer's
//!   estimate includes acceleration; ours would be short without it).

use crate::render::{rgb, Camera, Renderer, Scene};
use crate::video::{fmt_minutes, Painter, Video, FPS};
use std::path::Path;
use wright_geom::{v3, V3};

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub a: V3,
    pub b: V3,
    pub w: f64,
    pub h: f64,
    pub role: u8,
    pub layer: u32,
    pub minutes: f64,
}

pub struct Gcode {
    pub segs: Vec<Seg>,
    pub layers: u32,
    pub est_minutes: Option<f64>,
    pub grams: Option<f64>,
    pub bed: (f64, f64),
    pub printer: String,
    pub material: String,
}

const ROLES: [(&str, [u8; 3]); 15] = [
    ("None", [230, 179, 179]),
    ("Perimeter", [255, 230, 77]),
    ("External perimeter", [255, 125, 56]),
    ("Overhang perimeter", [31, 31, 255]),
    ("Internal infill", [176, 48, 41]),
    ("Solid infill", [150, 84, 204]),
    ("Top solid infill", [240, 64, 64]),
    ("Ironing", [255, 140, 105]),
    ("Bridge infill", [77, 128, 186]),
    ("Gap fill", [255, 255, 255]),
    ("Skirt/Brim", [0, 135, 110]),
    ("Support material", [0, 255, 0]),
    ("Support material interface", [0, 128, 0]),
    ("Wipe tower", [179, 227, 171]),
    ("Custom", [94, 209, 148]),
];

fn role_of(t: &str) -> u8 {
    let t = t.trim();
    ROLES.iter().position(|r| r.0.eq_ignore_ascii_case(t)).unwrap_or(if t.starts_with("Skirt") { 10 } else { 0 }) as u8
}

/// "1h 2m 3s" / "27m 54s" / "1d 2h 3m 4s" -> minutes.
fn parse_duration(s: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut any = false;
    for tok in s.split_whitespace() {
        let (num, unit) = tok.split_at(tok.find(|c: char| !c.is_ascii_digit() && c != '.')?);
        let v: f64 = num.parse().ok()?;
        total += v * match unit {
            "d" => 1440.0,
            "h" => 60.0,
            "m" => 1.0,
            "s" => 1.0 / 60.0,
            _ => return None,
        };
        any = true;
    }
    any.then_some(total)
}

pub fn parse(text: &str) -> Gcode {
    let mut segs = Vec::new();
    let (mut x, mut y, mut z, mut e, mut f) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 1500.0f64);
    let mut rel_e = false;
    let (mut w, mut h) = (0.45f64, 0.2f64);
    let mut role = 0u8;
    let mut layer: i64 = -1;
    let mut est = None;
    let mut grams = None;
    let mut bed = (250.0, 210.0);
    let mut printer = String::new();
    let mut material = String::new();
    let mut travel_min = 0.0;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(c) = line.strip_prefix(';') {
            if c == "LAYER_CHANGE" {
                layer += 1;
            } else if let Some(v) = c.strip_prefix("HEIGHT:") {
                h = v.trim().parse().unwrap_or(h);
            } else if let Some(v) = c.strip_prefix("WIDTH:") {
                w = v.trim().parse().unwrap_or(w);
            } else if let Some(v) = c.strip_prefix("TYPE:") {
                role = role_of(v);
            } else if let Some(v) = c.strip_prefix(" estimated printing time (normal mode) = ") {
                est = parse_duration(v);
            } else if let Some(v) = c.strip_prefix(" filament used [g] = ") {
                grams = v.trim().split(',').map(|g| g.trim().parse::<f64>().unwrap_or(0.0)).sum::<f64>().into();
            } else if let Some(v) = c.strip_prefix(" bed_shape = ") {
                let pts: Vec<(f64, f64)> = v.split(',').filter_map(|p| {
                    let (a, b) = p.trim().split_once('x')?;
                    Some((a.parse().ok()?, b.parse().ok()?))
                }).collect();
                if let (Some(mx), Some(my)) = (pts.iter().map(|p| p.0).reduce(f64::max), pts.iter().map(|p| p.1).reduce(f64::max)) {
                    bed = (mx, my);
                }
            } else if let Some(v) = c.strip_prefix(" printer_settings_id = ") {
                printer = v.trim().to_string();
            } else if let Some(v) = c.strip_prefix(" filament_settings_id = ") {
                material = v.trim().trim_matches('"').to_string();
            }
            continue;
        }
        let code = line.split(';').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        let mut words = code.split_whitespace();
        let cmd = words.next().unwrap_or("");
        match cmd {
            "M82" => rel_e = false,
            "M83" => rel_e = true,
            "G92" => {
                for wd in words {
                    if let Some(v) = wd.strip_prefix('E') {
                        e = v.parse().unwrap_or(0.0);
                    }
                }
            }
            "G0" | "G1" => {
                let (mut nx, mut ny, mut nz, mut de) = (x, y, z, 0.0);
                for wd in words {
                    let (k, v) = wd.split_at(1);
                    let v: f64 = match v.parse() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    match k {
                        "X" => nx = v,
                        "Y" => ny = v,
                        "Z" => nz = v,
                        "F" => f = v,
                        "E" => {
                            if rel_e {
                                de = v;
                            } else {
                                de = v - e;
                                e = v;
                            }
                        }
                        _ => {}
                    }
                }
                let len = ((nx - x).powi(2) + (ny - y).powi(2) + (nz - z).powi(2)).sqrt();
                let minutes = if f > 0.0 { len / f } else { 0.0 };
                if de > 1e-6 && ((nx - x).abs() + (ny - y).abs()) > 1e-4 && layer >= 0 {
                    segs.push(Seg { a: v3(x, y, nz), b: v3(nx, ny, nz), w, h, role, layer: layer as u32, minutes: minutes + travel_min });
                    travel_min = 0.0;
                } else {
                    travel_min += minutes;
                }
                x = nx;
                y = ny;
                z = nz;
            }
            _ => {}
        }
    }
    // scale move times to the slicer's estimate
    let sum: f64 = segs.iter().map(|s| s.minutes).sum();
    if let (Some(t), true) = (est, sum > 0.0) {
        let k = t / sum;
        for s in &mut segs {
            s.minutes *= k;
        }
    }
    let layers = segs.last().map_or(0, |s| s.layer + 1);
    Gcode { segs, layers, est_minutes: est, grams, bed, printer, material }
}

const VERTS_PER_SEG: u32 = 24;

/// The diamond prism of one extrusion (8 triangles).
fn seg_mesh(sc: &mut Scene, s: &Seg) {
    let d = s.b - s.a;
    let l = (d.x * d.x + d.y * d.y).sqrt();
    if l < 1e-9 {
        return;
    }
    let side = v3(-d.y / l, d.x / l, 0.0) * (s.w / 2.0);
    let up = v3(0.0, 0.0, s.h / 2.0);
    let c = rgb(ROLES[s.role as usize % ROLES.len()].1);
    let mid = |p: V3| p - up;
    let ring = |p: V3| [mid(p) + up, mid(p) + side, mid(p) - up, mid(p) - side];
    let (ra, rb) = (ring(s.a), ring(s.b));
    for k in 0..4 {
        let k1 = (k + 1) % 4;
        sc.quad(ra[k], ra[k1], rb[k1], rb[k], c);
    }
}

pub struct PrintFilm {
    pub minutes: f64,
    pub grams: f64,
    pub layers: u32,
}

pub fn film(r: &Renderer, painter: &Painter, gcode_path: &Path, title: &str, mp4: &Path, png: &Path) -> anyhow::Result<PrintFilm> {
    let text = std::fs::read_to_string(gcode_path)?;
    let g = parse(&text);
    anyhow::ensure!(!g.segs.is_empty(), "{}: no extrusions", gcode_path.display());
    let mut all = Scene::default();
    // every segment as exactly VERTS_PER_SEG vertices (zero-length ones
    // padded) so a prefix of segments is a prefix of vertices
    for s in &g.segs {
        let before = all.verts.len();
        seg_mesh(&mut all, s);
        while all.verts.len() < before + VERTS_PER_SEG as usize {
            let v = all.verts.last().copied().unwrap_or(crate::render::Vertex { pos: [0.0; 3], normal: [0.0, 0.0, 1.0], color: [0.0; 4] });
            all.verts.push(v);
        }
    }
    let gpu = r.upload(&all).ok_or_else(|| anyhow::anyhow!("empty print"))?;
    let (lo, hi) = all.bounds().unwrap();
    let center = (lo + hi) * 0.5;
    let radius = ((hi - lo).norm() * 0.5).max(15.0);
    // the bed, under the part
    let mut bed = Scene::default();
    let pad = radius * 1.6;
    bed.cuboid(v3((center.x - pad).max(0.0), (center.y - pad).max(0.0), -1.5), v3((center.x + pad).min(g.bed.0), (center.y + pad).min(g.bed.1), 0.0), rgb([40, 42, 48]));
    let total: f64 = g.segs.iter().map(|s| s.minutes).sum();
    let mut cum = Vec::with_capacity(g.segs.len());
    let mut acc = 0.0;
    for s in &g.segs {
        acc += s.minutes;
        cum.push(acc);
    }
    let frames = 300usize;
    let mut video = Video::create(mp4)?;
    let mut last = Vec::new();
    let mut idx = 0usize;
    for k in 0..=frames {
        let t = total * k as f64 / frames as f64;
        while idx < cum.len() && cum[idx] <= t {
            idx += 1;
        }
        let shown = idx.min(g.segs.len());
        let cur = g.segs[shown.min(g.segs.len() - 1)];
        let mut sc = bed.clone();
        if k < frames {
            // the hot end over the current move
            let p = cur.b;
            sc.cylinder(p + v3(0.0, 0.0, 0.2), p + v3(0.0, 0.0, 3.0), 0.6, 12, rgb([200, 160, 70]));
            sc.cylinder(p + v3(0.0, 0.0, 3.0), p + v3(0.0, 0.0, 14.0), 4.0, 20, rgb([200, 160, 70]));
            sc.cuboid(p + v3(-11.0, -11.0, 14.0), p + v3(11.0, 11.0, 34.0), rgb([60, 64, 72]));
        }
        let prog = k as f64 / frames as f64;
        let cam = Camera::orbit(center, Camera::fit_distance(radius * 1.15, 30.0), -50.0 + 60.0 * prog, 30.0);
        let mut img = r.draw_with(&[(&gpu, shown as u32 * VERTS_PER_SEG)], &sc, &cam);
        let layer = if k < frames { cur.layer + 1 } else { g.layers };
        painter.caption(
            &mut img,
            &format!("{title}: printing"),
            &format!("layer {layer}/{}  z = {:.2} mm  | {}", g.layers, cur.a.z, if g.printer.is_empty() { "PrusaSlicer G-code".to_string() } else { g.printer.clone() }),
            &[format!("print {} / {}", fmt_minutes(t), fmt_minutes(total)), format!("{:.1} g {}", g.grams.unwrap_or(0.0), g.material)],
            prog,
        );
        video.push(&img)?;
        last = img;
    }
    for _ in 0..FPS {
        video.push(&last)?;
    }
    video.finish()?;
    std::fs::write(png, crate::render::png(&last))?;
    Ok(PrintFilm { minutes: g.est_minutes.unwrap_or(total), grams: g.grams.unwrap_or(0.0), layers: g.layers })
}
