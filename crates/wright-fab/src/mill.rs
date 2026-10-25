//! 2.5D milling operations and the material-removal simulation.
//!
//! Operations (what FreeCAD CAM calls Face, Pocket, Profile and Drilling;
//! src/Mod/CAM/Path/Op/{MillFace,Pocket,Profile,Drilling}.py):
//! * [`face`]: zig-zag passes over the stock top (MillFace "ZigZag");
//! * [`pocket`]: contour-parallel offsets from the boundary inward, one ring
//!   per step-over, each depth level in turn (Pocket "Offset" pattern);
//! * [`profile`]: the boundary offset outward by the tool radius, stepped
//!   down by the axial depth, with holding tabs on the last passes
//!   (Profile + the Tag dressup, Path/Dressup/Tags.py);
//! * [`drill`]: G83 peck cycles (Drilling op with peck "Q").
//!   Offsets are computed by `cavalier_contours` (`Polyline::parallel_offset`,
//!   the Rust port of jbuckmccready/CavalierContours), the same kind of
//!   polyline offsetter FreeCAD gets from libarea/Clipper.
//!
//! Material removal: a height field, as FreeCAD's original CAM simulator
//! (src/Mod/CAM/PathSimulator/App/VolSim.cpp, `cStock`): one height per
//! grid cell (`m_stock[x][y]`), lowered to the tool tip wherever the tool
//! passes (`cStock::ApplyLinearTool` walks the swept area in steps of
//! `SIM_WALK_RES` = 0.6 cell and keeps `min(height, tool z)`). Kept: the
//! height-field model and the min-update. Deviation: the tool is stamped as
//! a disc at each step along the move instead of walking the swept
//! rectangle plus end cup (same cells, simpler to get right for a flat end
//! mill, the only cutter shape here). Tessellation follows `TesselTop` /
//! `TesselSidesX/Y`: flat tops per run of equal cells, vertical walls where
//! neighbours differ, nothing where the stock is gone.

use crate::render::{rgb, Scene};
use crate::rules;
use crate::toolpath::{Mv, Op, Tool, ToolKind};
use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use wright_geom::{v3, V3};

/// A closed outline (x, y) mm, counter-clockwise.
pub type Outline = Vec<(f64, f64)>;

pub fn circle(cx: f64, cy: f64, r: f64, n: usize) -> Outline {
    (0..n).map(|i| {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        (cx + r * a.cos(), cy + r * a.sin())
    }).collect()
}

fn to_pline(o: &Outline) -> Polyline<f64> {
    let mut p = Polyline::new_closed();
    let ccw = signed_area(o) > 0.0;
    let pts: Vec<&(f64, f64)> = if ccw { o.iter().collect() } else { o.iter().rev().collect() };
    for &&(x, y) in &pts {
        p.add(x, y, 0.0);
    }
    p
}

fn signed_area(o: &Outline) -> f64 {
    let n = o.len();
    (0..n).map(|i| {
        let (a, b) = (o[i], o[(i + 1) % n]);
        a.0 * b.1 - b.0 * a.1
    }).sum::<f64>() / 2.0
}

/// A cavalier polyline (with arcs as bulges) to points, arcs as chords of
/// at most `chord` mm.
fn from_pline(p: &Polyline<f64>, chord: f64) -> Outline {
    let n = p.vertex_count();
    let mut out = Vec::new();
    for i in 0..n {
        let a: PlineVertex<f64> = p.at(i);
        let b: PlineVertex<f64> = p.at((i + 1) % n);
        out.push((a.x, a.y));
        if a.bulge.abs() > 1e-9 {
            // arc from a to b with bulge = tan(sweep / 4)
            let sweep = 4.0 * a.bulge.atan();
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let c = (dx * dx + dy * dy).sqrt();
            if c < 1e-12 {
                continue;
            }
            // centre on the chord's left normal at c/2 / tan(sweep/2)
            let d = c / 2.0 / (sweep / 2.0).tan();
            let (cx, cy) = ((a.x + b.x) / 2.0 - d * dy / c, (a.y + b.y) / 2.0 + d * dx / c);
            let r = ((a.x - cx).powi(2) + (a.y - cy).powi(2)).sqrt();
            let a0 = (a.y - cy).atan2(a.x - cx);
            let k = ((r * sweep.abs()) / chord).ceil().max(2.0) as usize;
            for j in 1..k {
                let t = a0 + sweep * j as f64 / k as f64;
                out.push((cx + r * t.cos(), cy + r * t.sin()));
            }
        }
    }
    out
}

/// Offset a closed outline: positive = inward, negative = outward.
/// Several loops can come back (a pocket that pinches in two).
pub fn offset(o: &Outline, d: f64) -> Vec<Outline> {
    let p = to_pline(o);
    p.parallel_offset(d).iter().filter(|q| q.vertex_count() >= 2).map(|q| from_pline(q, 0.4)).collect()
}

pub fn end_mill(number: u32, dia: f64, name: &str) -> Tool {
    Tool { number, name: name.into(), dia, kind: ToolKind::FlatEnd }
}
pub fn drill_tool(number: u32, dia: f64, name: &str) -> Tool {
    Tool { number, name: name.into(), dia, kind: ToolKind::Drill }
}

/// Spindle speed and feed for an end mill in 6061 (mm/min).
pub fn al_speeds(dia: f64) -> (f64, f64) {
    let n = rules::rpm(rules::AL_MILL_VC, dia, rules::MILL_MAX_RPM);
    let flutes = if dia < 4.0 { 2.0 } else { 3.0 };
    (n, n * flutes * rules::AL_CHIPLOAD_MM * (dia / 6.35).clamp(0.3, 1.5))
}

const CLEAR: f64 = 5.0;

fn levels(top: f64, bottom: f64, doc: f64) -> Vec<f64> {
    let n = ((top - bottom) / doc).ceil().max(1.0) as usize;
    (1..=n).map(|i| top - (top - bottom) * i as f64 / n as f64).collect()
}

/// Zig-zag facing over a box (x0..x1, y0..y1) at `z`.
#[allow(clippy::too_many_arguments)]
pub fn face(name: &str, tool: Tool, rpm: f64, feed: f64, x0: f64, x1: f64, y0: f64, y1: f64, z: f64) -> Op {
    let r = tool.dia / 2.0;
    let step = tool.dia * 0.7;
    let mut mv = vec![Mv::Rapid(v3(x0 - r - 2.0, y0, z + CLEAR)), Mv::Feed(v3(x0 - r - 2.0, y0, z), feed / 3.0)];
    let mut y = y0;
    let mut fwd = true;
    loop {
        let (xa, xb) = if fwd { (x0 - r - 2.0, x1 + r + 2.0) } else { (x1 + r + 2.0, x0 - r - 2.0) };
        mv.push(Mv::Feed(v3(xa, y, z), feed));
        mv.push(Mv::Feed(v3(xb, y, z), feed));
        if y >= y1 {
            break;
        }
        y = (y + step).min(y1);
        fwd = !fwd;
    }
    mv.push(Mv::Rapid(v3(match mv.last() { Some(Mv::Feed(p, _)) => p.x, _ => x0 }, y, z + CLEAR)));
    Op { name: name.into(), tool, rpm, css: None, moves: mv }
}

/// Contour-parallel pocket inside `boundary` (minus `islands`), from `top`
/// down to `bottom`. Rings are cut from the inside out at each level.
#[allow(clippy::too_many_arguments)]
pub fn pocket(name: &str, tool: Tool, rpm: f64, feed: f64, boundary: &Outline, islands: &[Outline], top: f64, bottom: f64, doc: f64) -> Op {
    let r = tool.dia / 2.0;
    let step = tool.dia * rules::AL_STEPOVER;
    let mut rings: Vec<Outline> = Vec::new();
    let mut d = r;
    loop {
        let rs = offset(boundary, d);
        if rs.is_empty() {
            break;
        }
        rings.extend(rs);
        d += step;
        if d > 1e4 {
            break;
        }
    }
    // islands (a boss left standing): drop ring points that would cut into
    // an island grown by the tool radius
    let grown: Vec<Outline> = islands.iter().flat_map(|i| offset(i, -r)).collect();
    let inside_island = |p: (f64, f64)| grown.iter().any(|g| point_in(g, p));
    let mut mv = Vec::new();
    for z in levels(top, bottom, doc) {
        for ring in rings.iter().rev() {
            let pts: Vec<(f64, f64)> = ring.iter().copied().filter(|&p| !inside_island(p)).collect();
            if pts.len() < 2 {
                continue;
            }
            let s = pts[0];
            mv.push(Mv::Rapid(v3(s.0, s.1, top + CLEAR)));
            mv.push(Mv::Feed(v3(s.0, s.1, z), feed / 4.0)); // plunge (a ramp on a real job)
            for &p in pts.iter().skip(1).chain(std::iter::once(&s)) {
                mv.push(Mv::Feed(v3(p.0, p.1, z), feed));
            }
            mv.push(Mv::Rapid(v3(s.0, s.1, top + CLEAR)));
        }
    }
    Op { name: name.into(), tool, rpm, css: None, moves: mv }
}

/// Outside profile of `outline` from `top` to `bottom`; below `tab_top`
/// the cutter lifts over `tabs` tabs of `tab_w` mm (0 tabs: cut free).
#[allow(clippy::too_many_arguments)]
pub fn profile(name: &str, tool: Tool, rpm: f64, feed: f64, outline: &Outline, top: f64, bottom: f64, doc: f64, tabs: usize, tab_w: f64, tab_top: f64) -> Op {
    let r = tool.dia / 2.0;
    let path = offset(outline, -r).into_iter().max_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs())).unwrap_or_default();
    let mut mv = Vec::new();
    if path.len() < 2 {
        return Op { name: name.into(), tool, rpm, css: None, moves: mv };
    }
    // arc length along the closed path
    let n = path.len();
    let mut s = vec![0.0];
    for i in 0..n {
        let (a, b) = (path[i], path[(i + 1) % n]);
        s.push(s[i] + ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt());
    }
    let total = s[n];
    let tab_at = |u: f64| tabs > 0 && (0..tabs).any(|k| {
        let c = total * (k as f64 + 0.5) / tabs as f64;
        (u - c).abs() < tab_w / 2.0 + r
    });
    let p0 = path[0];
    mv.push(Mv::Rapid(v3(p0.0, p0.1, top + CLEAR)));
    for z in levels(top, bottom, doc) {
        mv.push(Mv::Feed(v3(p0.0, p0.1, z.max(if tab_at(0.0) { tab_top } else { z })), feed / 4.0));
        // walk the loop in small pieces so tabs can lift exactly
        for i in 0..n {
            let (a, b) = (path[i], path[(i + 1) % n]);
            let len = s[i + 1] - s[i];
            let k = (len / 1.0).ceil().max(1.0) as usize;
            for j in 1..=k {
                let t = j as f64 / k as f64;
                let u = s[i] + t * len;
                let zz = if z < tab_top && tab_at(u) { tab_top } else { z };
                mv.push(Mv::Feed(v3(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1), zz), feed));
            }
        }
    }
    mv.push(Mv::Rapid(v3(p0.0, p0.1, top + CLEAR)));
    simplify(&mut mv);
    Op { name: name.into(), tool, rpm, css: None, moves: mv }
}

/// Merge collinear consecutive feeds (the 1 mm walk above).
fn simplify(mv: &mut Vec<Mv>) {
    let mut out: Vec<Mv> = Vec::with_capacity(mv.len());
    for m in mv.drain(..) {
        if let (Mv::Feed(p, f), Some(Mv::Feed(q, fq)), Some(prev)) = (&m, out.last(), out.len().checked_sub(2).and_then(|i| out.get(i))) {
            let pp = match prev {
                Mv::Feed(x, _) | Mv::Rapid(x) => Some(*x),
                _ => None,
            };
            if let Some(pp) = pp {
                let (d1, d2) = (*q - pp, *p - *q);
                if (f - fq).abs() < 1e-9 && d1.norm() > 0.0 && d2.norm() > 0.0 && d1.cross(d2).norm() < 1e-6 * d1.norm() * d2.norm() && d1.dot(d2) > 0.0 {
                    out.pop();
                }
            }
        }
        out.push(m);
    }
    *mv = out;
}

/// Peck-drill each point from `top` to `bottom`.
pub fn drill(name: &str, tool: Tool, points: &[(f64, f64)], top: f64, bottom: f64) -> Op {
    let rpm = rules::rpm(rules::DRILL_VC, tool.dia, rules::MILL_MAX_RPM);
    let feed = rpm * rules::DRILL_FPR * (tool.dia / 6.0).clamp(0.25, 1.5);
    let peck = (tool.dia * 1.5).max(0.8);
    let mv = points.iter().map(|&(x, y)| Mv::Drill { x, y, bottom, r_plane: top + 2.0, peck, feed }).collect::<Vec<_>>();
    let mut moves = vec![Mv::Rapid(v3(points.first().map_or(0.0, |p| p.0), points.first().map_or(0.0, |p| p.1), top + CLEAR))];
    moves.extend(mv);
    Op { name: name.into(), tool, rpm, css: None, moves }
}

pub fn point_in(poly: &Outline, p: (f64, f64)) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------- simulation

/// Height-field stock (FreeCAD VolSim `cStock`).
pub struct HeightField {
    pub x0: f64,
    pub y0: f64,
    pub res: f64,
    pub nx: usize,
    pub ny: usize,
    pub bottom: f64,
    pub top: f64,
    /// NaN where there never was material (round stock corners).
    pub h: Vec<f64>,
}

impl HeightField {
    pub fn block(x0: f64, y0: f64, lx: f64, ly: f64, bottom: f64, top: f64, res: f64) -> Self {
        let (nx, ny) = ((lx / res).ceil() as usize, (ly / res).ceil() as usize);
        HeightField { x0, y0, res, nx, ny, bottom, top, h: vec![top; nx * ny] }
    }
    pub fn disc(r: f64, bottom: f64, top: f64, res: f64) -> Self {
        let mut f = Self::block(-r, -r, 2.0 * r, 2.0 * r, bottom, top, res);
        for j in 0..f.ny {
            for i in 0..f.nx {
                let (x, y) = f.center(i, j);
                if x * x + y * y > r * r {
                    f.h[j * f.nx + i] = f64::NAN;
                }
            }
        }
        f
    }
    fn center(&self, i: usize, j: usize) -> (f64, f64) {
        (self.x0 + (i as f64 + 0.5) * self.res, self.y0 + (j as f64 + 0.5) * self.res)
    }
    /// Lower every cell under a flat tool of radius `r` with its tip at p.
    pub fn stamp(&mut self, p: V3, r: f64) -> f64 {
        let mut removed = 0.0;
        if p.z >= self.top {
            return 0.0;
        }
        let i0 = (((p.x - r - self.x0) / self.res).floor().max(0.0)) as usize;
        let i1 = (((p.x + r - self.x0) / self.res).ceil().max(0.0) as usize).min(self.nx);
        let j0 = (((p.y - r - self.y0) / self.res).floor().max(0.0)) as usize;
        let j1 = (((p.y + r - self.y0) / self.res).ceil().max(0.0) as usize).min(self.ny);
        let z = p.z.max(self.bottom - 0.01);
        for j in j0..j1 {
            for i in i0..i1 {
                let (x, y) = self.center(i, j);
                if (x - p.x).powi(2) + (y - p.y).powi(2) <= r * r {
                    let c = &mut self.h[j * self.nx + i];
                    if *c > z {
                        removed += (*c - z.max(self.bottom)).max(0.0);
                        *c = z;
                    }
                }
            }
        }
        removed * self.res * self.res
    }
    /// Sweep a straight move (cut steps only), walking at 0.6 cell like
    /// `SIM_WALK_RES`. Returns mm^3 removed.
    pub fn sweep(&mut self, a: V3, b: V3, r: f64) -> f64 {
        let len = (b - a).norm();
        let n = (len / (0.6 * self.res)).ceil().max(1.0) as usize;
        (0..=n).map(|k| self.stamp(a + (b - a) * (k as f64 / n as f64), r)).sum()
    }
    fn solid(&self, i: isize, j: isize) -> Option<f64> {
        if i < 0 || j < 0 || i >= self.nx as isize || j >= self.ny as isize {
            return None;
        }
        let h = self.h[j as usize * self.nx + i as usize];
        (h.is_finite() && h > self.bottom).then_some(h)
    }
    /// Tessellate (VolSim `Tessellate`: tops by runs, side walls).
    pub fn mesh(&self, scene: &mut Scene, color: [f32; 4], cut_color: [f32; 4]) {
        let r = self.res;
        for j in 0..self.ny as isize {
            let mut i = 0isize;
            while i < self.nx as isize {
                let Some(h) = self.solid(i, j) else {
                    i += 1;
                    continue;
                };
                let mut k = i + 1;
                while k < self.nx as isize && self.solid(k, j) == Some(h) {
                    k += 1;
                }
                let (xa, xb) = (self.x0 + i as f64 * r, self.x0 + k as f64 * r);
                let (ya, yb) = (self.y0 + j as f64 * r, self.y0 + (j + 1) as f64 * r);
                let c = if h < self.top - 1e-6 { cut_color } else { color };
                scene.quad(v3(xa, ya, h), v3(xb, ya, h), v3(xb, yb, h), v3(xa, yb, h), c);
                // bottom face
                scene.quad(v3(xa, ya, self.bottom), v3(xa, yb, self.bottom), v3(xb, yb, self.bottom), v3(xb, ya, self.bottom), color);
                i = k;
            }
        }
        // walls between neighbours in x and in y
        for j in 0..self.ny as isize {
            for i in 0..=self.nx as isize {
                let a = self.solid(i - 1, j).unwrap_or(self.bottom);
                let b = self.solid(i, j).unwrap_or(self.bottom);
                if (a - b).abs() > 1e-9 {
                    let x = self.x0 + i as f64 * r;
                    let (ya, yb) = (self.y0 + j as f64 * r, self.y0 + (j + 1) as f64 * r);
                    let (lo, hi) = (a.min(b), a.max(b));
                    let c = if hi < self.top - 1e-6 || lo > self.bottom { cut_color } else { color };
                    scene.quad(v3(x, ya, lo), v3(x, yb, lo), v3(x, yb, hi), v3(x, ya, hi), c);
                }
            }
        }
        for j in 0..=self.ny as isize {
            for i in 0..self.nx as isize {
                let a = self.solid(i, j - 1).unwrap_or(self.bottom);
                let b = self.solid(i, j).unwrap_or(self.bottom);
                if (a - b).abs() > 1e-9 {
                    let y = self.y0 + j as f64 * r;
                    let (xa, xb) = (self.x0 + i as f64 * r, self.x0 + (i + 1) as f64 * r);
                    let (lo, hi) = (a.min(b), a.max(b));
                    let c = if hi < self.top - 1e-6 || lo > self.bottom { cut_color } else { color };
                    scene.quad(v3(xa, y, lo), v3(xb, y, lo), v3(xb, y, hi), v3(xa, y, hi), c);
                }
            }
        }
    }
}

/// Colours of the simulation.
pub fn stock_colors(material: &str) -> ([f32; 4], [f32; 4]) {
    let m = material.to_ascii_lowercase();
    if m.contains("g10") || m.contains("fibreglass") || m.contains("fiberglass") {
        (rgb([150, 170, 120]), rgb([205, 215, 175]))
    } else if m.contains("phenolic") {
        (rgb([150, 100, 60]), rgb([196, 140, 92]))
    } else {
        (rgb([170, 176, 186]), rgb([214, 220, 228]))
    }
}
