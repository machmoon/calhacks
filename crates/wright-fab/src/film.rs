//! Material-removal films: a program's steps replayed on simulated stock,
//! one frame per slice of machining time, encoded to MP4, with the last
//! frame kept as a PNG.

use crate::lathe::{self, LatheSim};
use crate::mill::{self, HeightField};
use crate::render::{rgb, Camera, Renderer, Scene};
use crate::toolpath::{Machine, Mv, Op, Program, Step, Stock, ToolKind};

fn indexed(o: &Op) -> bool {
    o.moves.iter().any(|m| matches!(m, Mv::Index(_)))
}
use crate::video::{fmt_minutes, Painter, Video};
use std::path::Path;
use wright_geom::{v3, V3};

pub const FILM_FRAMES: usize = 330;

pub struct FilmResult {
    pub minutes: f64,
    pub removed_g: f64,
    pub part_g: f64,
    pub frames: usize,
}

/// Frame times: equal slices of motion time, so long cuts take long.
fn frame_cuts(steps: &[Step], frames: usize) -> (Vec<f64>, f64) {
    let total: f64 = steps.iter().map(|s| s.minutes).sum();
    ((0..=frames).map(|k| total * k as f64 / frames as f64).collect(), total)
}

/// Where the tool is at time t (interpolated within its step).
fn tool_at(steps: &[Step], idx: usize, frac: f64) -> (V3, usize) {
    if steps.is_empty() {
        return (v3(0.0, 0.0, 50.0), 0);
    }
    let s = &steps[idx.min(steps.len() - 1)];
    (s.from + (s.to - s.from) * frac.clamp(0.0, 1.0), idx.min(steps.len() - 1))
}

pub fn film(r: &Renderer, painter: &Painter, p: &Program, mp4: &Path, png: &Path) -> anyhow::Result<FilmResult> {
    match p.machine {
        Machine::Lathe => film_lathe(r, painter, p, mp4, png),
        _ => film_mill(r, painter, p, mp4, png),
    }
}

fn op_caption(p: &Program, st: &Step) -> (String, String) {
    let s = &p.setups[st.setup];
    let o = &s.ops[st.op];
    let speed = match o.css {
        Some(vc) => format!("G96 {vc:.0} m/min"),
        None => format!("S{:.0}", o.rpm),
    };
    (format!("{}: {}", p.part, o.name), format!("T{} {}  {}  | setup {}/{}", o.tool.number, o.tool.name, speed, st.setup + 1, p.setups.len()))
}

fn film_mill(r: &Renderer, painter: &Painter, p: &Program, mp4: &Path, png: &Path) -> anyhow::Result<FilmResult> {
    let steps = p.steps();
    let (times, total) = frame_cuts(&steps, FILM_FRAMES);
    let (mut hf, center, size) = match p.stock {
        Stock::Block { x, y, z } => {
            let res = (x.max(y) / 320.0).max(0.25);
            (HeightField::block(0.0, 0.0, x, y, 0.0, z, res), v3(x / 2.0, y / 2.0, z / 2.0), x.max(y))
        }
        Stock::Round { d, len } | Stock::Tube { od: d, len, .. } => {
            let res = (d / 300.0).max(0.25);
            (HeightField::disc(d / 2.0, -len, 0.0, res), v3(0.0, 0.0, -len / 2.0), d)
        }
    };
    let (col, cut) = mill::stock_colors(&p.material);
    let mut video = Video::create(mp4)?;
    let mut removed = 0.0;
    let mut si = 0usize;
    let mut done_t = 0.0;
    let tools = p.tools();
    let mut last = Vec::new();
    // radial holes done so far: (angle deg, x along the disc axis, depth, radius)
    let mut radial: Vec<(f64, f64, f64, f64)> = Vec::new();
    let r_stock = p.stock_radius();
    for (k, &t) in times.iter().enumerate() {
        // advance whole steps up to t
        while si < steps.len() && done_t + steps[si].minutes <= t + 1e-12 {
            let s = steps[si];
            let o = &p.setups[s.setup].ops[s.op];
            if !s.rapid && !indexed(o) {
                removed += hf.sweep(s.from, s.to, o.tool.dia / 2.0);
            } else if !s.rapid && s.to.z < s.from.z {
                // a radial hole on the 4th axis: drawn on the rim
                radial.retain(|h: &(f64, f64, f64, f64)| (h.0 - s.a).abs() > 1e-6);
                radial.push((s.a, s.to.x, -s.to.z, o.tool.dia / 2.0));
            }
            done_t += s.minutes;
            si += 1;
        }
        let frac = if si < steps.len() && steps[si].minutes > 0.0 { (t - done_t) / steps[si].minutes } else { 1.0 };
        let (pos, cur) = tool_at(&steps, si, frac);
        if si < steps.len() && !steps[si].rapid && !indexed(&p.setups[steps[si].setup].ops[steps[si].op]) {
            let rad = p.setups[steps[si].setup].ops[steps[si].op].tool.dia / 2.0;
            removed += hf.sweep(steps[si].from, pos, rad);
        }
        let mut sc = Scene::default();
        hf.mesh(&mut sc, col, cut);
        for &(a, x, depth, rr) in &radial {
            let d = v3(a.to_radians().cos(), a.to_radians().sin(), 0.0);
            let z = x; // x along the disc axis = depth below the top face
            sc.cylinder(d * (r_stock - depth) + v3(0.0, 0.0, z), d * (r_stock + 0.15) + v3(0.0, 0.0, z), rr, 16, rgb([20, 22, 26]));
        }
        // fixture: a vise / spoilboard under the stock
        let (lo, hi) = sc.bounds().unwrap_or((v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0)));
        let pad = 0.08 * size;
        sc.cuboid(v3(lo.x - pad, lo.y - pad, hf.bottom - 0.25 * size.min(60.0)), v3(hi.x + pad, hi.y + pad, hf.bottom - 0.05), rgb([52, 56, 64]));
        let st = steps.get(cur).copied();
        let indexing = st.is_some_and(|s| indexed(&p.setups[s.setup].ops[s.op]));
        if let Some(s) = st {
            if indexing && k < times.len() - 1 {
                let tool = &p.setups[s.setup].ops[s.op].tool;
                let d = v3(s.a.to_radians().cos(), s.a.to_radians().sin(), 0.0);
                let tip = d * (r_stock - (-pos.z).max(-2.0)) + v3(0.0, 0.0, pos.x);
                sc.cylinder(tip, tip + d * 45.0, tool.dia / 2.0, 16, rgb([200, 160, 70]));
                sc.cylinder(tip + d * 45.0, tip + d * 120.0, 20.0, 24, rgb([66, 70, 80]));
            } else if k < times.len() - 1 {
                let tool = &p.setups[s.setup].ops[s.op].tool;
                let rad = tool.dia / 2.0;
                let flute = (tool.dia * 3.0).max(12.0);
                let fc = if tool.kind == ToolKind::Drill { rgb([200, 160, 70]) } else { rgb([215, 220, 228]) };
                sc.cylinder(pos, pos + v3(0.0, 0.0, flute), rad, 24, fc);
                sc.cylinder(pos + v3(0.0, 0.0, flute), pos + v3(0.0, 0.0, flute + 30.0), (rad * 1.6).max(6.0), 24, rgb([90, 96, 106]));
                sc.cylinder(pos + v3(0.0, 0.0, flute + 30.0), pos + v3(0.0, 0.0, flute + 120.0), (0.09 * size).clamp(8.0, 28.0), 32, rgb([66, 70, 80]));
            }
        }
        let prog = k as f64 / FILM_FRAMES as f64;
        let az = -60.0 + 50.0 * prog;
        let dist = Camera::fit_distance(0.62 * size, 30.0);
        let cam = Camera::orbit(center, dist, az, 42.0);
        let mut img = r.draw(&sc, &cam);
        let (title, status) = match st {
            Some(s) if k < times.len() - 1 => {
                let (a, b) = op_caption(p, &s);
                (a, if indexing { format!("{b}  | drawn, not swept") } else { b })
            }
            _ => (format!("{}: done", p.part), format!("{} ops, {} tools", p.setups.iter().map(|s| s.ops.len()).sum::<usize>(), tools.len())),
        };
        painter.caption(
            &mut img,
            &title,
            &status,
            &[
                format!("machining {} / {}", fmt_minutes(t), fmt_minutes(total)),
                format!("chips {:.0} g", removed * 1e-3 * p.density),
                p.material.to_string(),
            ],
            prog,
        );
        video.push(&img)?;
        last = img;
    }
    // hold the last frame a second
    for _ in 0..crate::video::FPS {
        video.push(&last)?;
    }
    let frames = video.frames;
    video.finish()?;
    std::fs::write(png, crate::render::png(&last))?;
    let stock_g = p.stock_grams();
    let removed_g = removed * 1e-3 * p.density;
    // routed sheet: what is left is the sheet with parts in it, not a part
    let part_g = if p.machine == Machine::Router { f64::NAN } else { (stock_g - removed_g).max(0.0) };
    Ok(FilmResult { minutes: p.minutes(), removed_g, part_g, frames })
}

fn film_lathe(r: &Renderer, painter: &Painter, p: &Program, mp4: &Path, png: &Path) -> anyhow::Result<FilmResult> {
    let steps = p.steps();
    let (times, total) = frame_cuts(&steps, FILM_FRAMES);
    let (ro, ri, blank) = match p.stock {
        Stock::Round { d, len } => (d / 2.0, 0.0, len),
        Stock::Tube { od, id, len } => (od / 2.0, id / 2.0, len),
        Stock::Block { .. } => anyhow::bail!("lathe stock must be round"),
    };
    // part length: the blank less 1.5 mm facing stock at each end
    let len = blank - 3.0;
    let mut sim = LatheSim::new(-1.5, len + 1.5, ro, ri, len);
    let v0 = sim.volume();
    let (col, cut) = mill::stock_colors(&p.material);
    let section = {
        let mut c = cut;
        for v in c.iter_mut().take(3) {
            *v *= 0.8;
        }
        c
    };
    let grip = (0.25 * len).clamp(8.0, 25.0);
    let mut video = Video::create(mp4)?;
    let mut si = 0usize;
    let mut done_t = 0.0;
    let mut last = Vec::new();
    let apply = |sim: &mut LatheSim, a: V3, b: V3, kind: ToolKind, dia: f64, flipped: bool| {
        let n = (((b - a).norm()) / 0.1).ceil().max(1.0) as usize;
        for k in 0..=n {
            let q = a + (b - a) * (k as f64 / n as f64);
            let rr = if kind == ToolKind::Drill { dia / 2.0 } else { q.x };
            sim.stamp(kind, rr, q.z, flipped);
        }
    };
    for (k, &t) in times.iter().enumerate() {
        while si < steps.len() && done_t + steps[si].minutes <= t + 1e-12 {
            let s = steps[si];
            if !s.rapid {
                let o = &p.setups[s.setup].ops[s.op];
                apply(&mut sim, s.from, s.to, o.tool.kind, o.tool.dia, p.setups[s.setup].flipped);
            }
            done_t += s.minutes;
            si += 1;
        }
        let frac = if si < steps.len() && steps[si].minutes > 0.0 { (t - done_t) / steps[si].minutes } else { 1.0 };
        let (pos, cur) = tool_at(&steps, si, frac);
        let st = steps.get(cur).copied();
        let flipped = st.map(|s| p.setups[s.setup].flipped).unwrap_or(false);
        if let Some(s) = st {
            if si < steps.len() && !s.rapid {
                let o = &p.setups[s.setup].ops[s.op];
                apply(&mut sim, s.from, pos, o.tool.kind, o.tool.dia, flipped);
            }
        }
        let mut sc = Scene::default();
        sim.mesh(&mut sc, flipped, col, cut, section);
        // the jaws hold the far end of the blank
        let z_back = sim.z_of(if flipped { -1.5 } else { len + 1.5 }, flipped);
        let spin = k as f64 * 23.0;
        lathe::chuck(&mut sc, z_back + grip, grip_radius(&sim, flipped, grip), grip, spin);
        if let Some(s) = st {
            if k < times.len() - 1 {
                let o = &p.setups[s.setup].ops[s.op];
                let rr = if o.tool.kind == ToolKind::Drill { 0.0 } else { pos.x };
                lathe::tool_mesh(&mut sc, o.tool.kind, rr, pos.z, o.tool.dia);
            }
        }
        let prog = k as f64 / FILM_FRAMES as f64;
        let rb = lathe::chuck_radius(ro);
        let z_far = -(len + 1.5) - 40.0;
        let center = v3(z_far / 2.0, 0.0, 0.0);
        let rad = (0.5 * (len + 45.0)).max(rb) * 0.9;
        // 3/4 view onto the section plane (y = 0) from the cut side
        let cam = Camera::orbit(center, Camera::fit_distance(rad, 30.0), 48.0 + 14.0 * prog, 24.0);
        let mut img = r.draw(&sc, &cam);
        let (title, status) = match st {
            Some(s) if k < times.len() - 1 => op_caption(p, &s),
            _ => (format!("{}: done", p.part), format!("{} setups", p.setups.len())),
        };
        let removed_g = (v0 - sim.volume()) * 1e-3 * p.density;
        painter.caption(
            &mut img,
            &title,
            &status,
            &[format!("turning {} / {}", fmt_minutes(t), fmt_minutes(total)), format!("chips {removed_g:.0} g"), p.material.clone()],
            prog,
        );
        video.push(&img)?;
        last = img;
    }
    for _ in 0..crate::video::FPS {
        video.push(&last)?;
    }
    let frames = video.frames;
    video.finish()?;
    std::fs::write(png, crate::render::png(&last))?;
    let removed_g = (v0 - sim.volume()) * 1e-3 * p.density;
    Ok(FilmResult { minutes: p.minutes(), removed_g, part_g: sim.volume() * 1e-3 * p.density, frames })
}

/// The stock radius under the jaws (they close on what is there).
fn grip_radius(sim: &LatheSim, flipped: bool, grip: f64) -> f64 {
    let n = sim.outer.len();
    let k = (grip / sim.res) as usize;
    let range: Vec<usize> = if flipped { (0..k.min(n)).collect() } else { (n.saturating_sub(k)..n).collect() };
    range.iter().map(|&i| sim.outer[i]).fold(0.0, f64::max)
}
