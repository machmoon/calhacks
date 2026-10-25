//! Turning: facing, G71-style roughing (outside and bore), G70-style
//! finishing, grooving, centre drilling, and the stock simulation.
//!
//! Roughing follows LinuxCNC's G71 (src/emc/rs274ngc/interp_g7x.cc,
//! `g7x::do_g71` and `g7x::pocket`): the finished profile is offset by the
//! finishing allowance (`add_distance(d)`), passes are made at X levels
//! counted off the start, `x = top - (++steps) * delta` with `delta` the
//! depth of cut `I`; each pass feeds along -Z from the start until it meets
//! the offset profile, then escapes at 45 deg by `R`
//! (`escape = r * (1 + 1i)`: `dest += escape`) and rapids back to the start
//! Z for the next level. G70 then follows the profile once (`do_g70`).
//! Kept: those semantics (type I, profile monotonic per setup) and names
//! (`delta`, `escape`). The passes are written out as plain G0/G1 rather
//! than as a `G71 Q... ` canned call, so any Fanuc-subset control runs the
//! file and the simulation reads the same moves. Bores are G71 mirrored
//! (levels grow from the drilled hole outward), as LinuxCNC does for an
//! internal profile by `swap()`ing the profile's X direction.
//! LibLathe (dubstar-04/LibLathe `liblathe/op/rough.py`, the FreeCAD lathe
//! workbench's engine) builds the same horizontal passes by intersecting
//! each pass line with the offset boundary; it agrees with G71 on a
//! monotonic profile.
//!
//! Coordinates: machine Z along the spindle with the part's free face at
//! Z = 0 and the chuck at -Z; X is RADIUS here (written as diameter, G7).
//! The simulation keeps the stock in part coordinates `u` (distance from
//! the part's first end) so a part turned end for end keeps its cuts.

use crate::render::{rgb, Scene};
use crate::rules;
use crate::toolpath::{Mv, Op, Tool, ToolKind};
use wright_geom::{v3, V3};

/// A turned profile: (u from the face, radius), u increasing into the part.
pub type Profile = Vec<(f64, f64)>;

pub fn radius_at(p: &Profile, u: f64) -> f64 {
    if u <= p[0].0 {
        return p[0].1;
    }
    for w in p.windows(2) {
        if u <= w[1].0 {
            let t = if w[1].0 > w[0].0 { (u - w[0].0) / (w[1].0 - w[0].0) } else { 1.0 };
            return w[0].1 + t * (w[1].1 - w[0].1);
        }
    }
    p.last().unwrap().1
}

pub fn turning_tool(n: u32) -> Tool {
    Tool { number: n, name: "CNMG 432 turning insert, 0.8 mm nose".into(), dia: 1.6, kind: ToolKind::Turning(0.8) }
}
pub fn boring_bar(n: u32, min_bore: f64) -> Tool {
    let d = (min_bore * 0.7).clamp(6.0, 25.0);
    Tool { number: n, name: format!("boring bar {d:.0} mm, 0.4 mm nose"), dia: d, kind: ToolKind::Boring(0.4) }
}
pub fn groove_tool(n: u32, w: f64) -> Tool {
    Tool { number: n, name: format!("grooving insert {w:.2} mm"), dia: w, kind: ToolKind::Groove(w) }
}
pub fn lathe_drill(n: u32, d: f64) -> Tool {
    Tool { number: n, name: format!("{d:.1} mm drill (tailstock / turret)"), dia: d, kind: ToolKind::Drill }
}

/// Facing passes from `z_from` down to `z_to` (z_from > z_to), from the
/// stock radius in to `r_in` (0 for solid).
pub fn face(tool: Tool, vc: f64, r_stock: f64, r_in: f64, z_from: f64, z_to: f64) -> Op {
    let n = ((z_from - z_to) / 1.0).ceil().max(1.0) as usize;
    let mut mv = Vec::new();
    for k in 1..=n {
        let z = z_from - (z_from - z_to) * k as f64 / n as f64;
        mv.push(Mv::Rapid(v3(r_stock + 2.0, 0.0, z + 1.0)));
        mv.push(Mv::Rapid(v3(r_stock + 2.0, 0.0, z)));
        mv.push(Mv::Feed(v3((r_in - 0.5).max(-0.4), 0.0, z), rules::TURN_ROUGH_FPR));
        mv.push(Mv::Rapid(v3((r_in - 0.5).max(-0.4), 0.0, z + 1.0)));
    }
    mv.push(Mv::Rapid(v3(r_stock + 2.0, 0.0, z_to + 2.0)));
    Op { name: "face".into(), tool, rpm: 0.0, css: Some(vc), moves: mv }
}

/// G71 outside roughing then G70 finishing of `prof` (part coordinates,
/// mapped to machine Z by `z = z0 - u`), from radius `r_stock`, over
/// `u_end` of the part (the rest is in the jaws).
pub fn turn_od(tool: Tool, vc: f64, doc: f64, r_stock: f64, prof: &Profile, z0: f64, u_end: f64) -> Vec<Op> {
    let allow = rules::FINISH_ALLOWANCE;
    let escape = 0.5;
    let target = |u: f64| radius_at(prof, u) + allow;
    let rmin = prof.iter().filter(|p| p.0 <= u_end).map(|p| p.1).fold(f64::MAX, f64::min) + allow;
    let mut mv = Vec::new();
    let top = r_stock;
    let mut steps = 0;
    loop {
        steps += 1;
        let x = top - steps as f64 * doc;
        let x = x.max(rmin);
        // feed from u = -1 (off the face) until the offset profile rises to x
        let mut u_stop = u_end;
        let mut u = 0.0;
        while u <= u_end {
            if target(u) > x + 1e-9 {
                u_stop = u;
                break;
            }
            u += 0.05;
        }
        if u_stop > 0.05 {
            mv.push(Mv::Rapid(v3(x + escape, 0.0, z0 + 1.0)));
            mv.push(Mv::Rapid(v3(x, 0.0, z0 + 1.0)));
            mv.push(Mv::Feed(v3(x, 0.0, z0 - u_stop), rules::TURN_ROUGH_FPR));
            mv.push(Mv::Rapid(v3(x + escape, 0.0, z0 - u_stop + escape)));
            mv.push(Mv::Rapid(v3(x + escape, 0.0, z0 + 1.0)));
        }
        if x <= rmin + 1e-9 {
            break;
        }
    }
    let rough = Op { name: "rough turn OD (G71-style passes)".into(), tool: tool.clone(), rpm: 0.0, css: Some(vc), moves: mv };
    // finishing along the profile
    let mut fm = vec![Mv::Rapid(v3(prof[0].1 + 2.0, 0.0, z0 + 1.0)), Mv::Rapid(v3(prof[0].1, 0.0, z0 + 1.0))];
    for &(u, r) in prof.iter().filter(|p| p.0 <= u_end) {
        fm.push(Mv::Feed(v3(r, 0.0, z0 - u), rules::TURN_FINISH_FPR));
    }
    fm.push(Mv::Feed(v3(radius_at(prof, u_end), 0.0, z0 - u_end), rules::TURN_FINISH_FPR));
    fm.push(Mv::Rapid(v3(r_stock + 2.0, 0.0, z0 - u_end + 0.5)));
    fm.push(Mv::Rapid(v3(r_stock + 2.0, 0.0, z0 + 2.0)));
    let finish = Op { name: "finish turn OD (G70-style profile pass)".into(), tool, rpm: 0.0, css: Some(vc * 1.2), moves: fm };
    vec![rough, finish]
}

/// Centre drill on the axis to `depth` (u), peck.
pub fn drill(tool: Tool, z0: f64, depth: f64) -> Op {
    let rpm = rules::rpm(rules::DRILL_VC, tool.dia, rules::LATHE_MAX_RPM);
    let peck = (tool.dia * 1.5).max(2.0);
    let mv = vec![
        Mv::Rapid(v3(0.0, 0.0, z0 + 5.0)),
        Mv::Drill { x: 0.0, y: 0.0, bottom: z0 - depth, r_plane: z0 + 2.0, peck, feed: rules::DRILL_FPR },
    ];
    Op { name: format!("drill {:.1} mm on centre, {:.0} mm deep", tool.dia, depth), tool, rpm, css: None, moves: mv }
}

/// G71 roughing of a bore (levels grow outward from `r_hole`), then the
/// finishing pass. `prof` is the finished bore radius by u, cut to `u_end`.
pub fn bore(tool: Tool, vc: f64, doc: f64, r_hole: f64, prof: &Profile, z0: f64, u_end: f64) -> Vec<Op> {
    let allow = rules::FINISH_ALLOWANCE;
    let escape = 0.4;
    let target = |u: f64| radius_at(prof, u) - allow;
    let rmax = prof.iter().filter(|p| p.0 <= u_end).map(|p| p.1).fold(f64::MIN, f64::max) - allow;
    let mut mv = Vec::new();
    let mut steps = 0;
    if rmax > r_hole {
        loop {
            steps += 1;
            let x = (r_hole + steps as f64 * doc).min(rmax);
            let mut u_stop = u_end;
            let mut u = 0.0;
            while u <= u_end {
                if target(u) < x - 1e-9 {
                    u_stop = u;
                    break;
                }
                u += 0.05;
            }
            if u_stop > 0.05 {
                mv.push(Mv::Rapid(v3(x - escape, 0.0, z0 + 1.0)));
                mv.push(Mv::Rapid(v3(x, 0.0, z0 + 1.0)));
                mv.push(Mv::Feed(v3(x, 0.0, z0 - u_stop), rules::TURN_ROUGH_FPR * 0.75));
                mv.push(Mv::Rapid(v3(x - escape, 0.0, z0 - u_stop + escape)));
                mv.push(Mv::Rapid(v3(x - escape, 0.0, z0 + 1.0)));
            }
            if x >= rmax - 1e-9 {
                break;
            }
        }
    }
    let rough = Op { name: "rough bore (G71-style passes, internal)".into(), tool: tool.clone(), rpm: 0.0, css: Some(vc), moves: mv };
    let mut fm = vec![Mv::Rapid(v3(prof[0].1 - 1.0, 0.0, z0 + 1.0)), Mv::Rapid(v3(prof[0].1, 0.0, z0 + 1.0))];
    for &(u, r) in prof.iter().filter(|p| p.0 <= u_end) {
        fm.push(Mv::Feed(v3(r, 0.0, z0 - u), rules::TURN_FINISH_FPR));
    }
    let r_end = radius_at(prof, u_end);
    fm.push(Mv::Feed(v3(r_end, 0.0, z0 - u_end), rules::TURN_FINISH_FPR));
    fm.push(Mv::Rapid(v3(r_end - 0.5, 0.0, z0 - u_end + 0.5)));
    fm.push(Mv::Rapid(v3(r_end - 0.5, 0.0, z0 + 2.0)));
    let finish = Op { name: "finish bore (G70-style profile pass)".into(), tool, rpm: 0.0, css: Some(vc * 1.2), moves: fm };
    vec![rough, finish]
}

/// Plunge a groove of `width` (wider than the blade: stepped plunges) at
/// part position `u_center`, from `r_surface` down to `r_bottom`.
pub fn groove(tool: Tool, vc: f64, z0: f64, u_center: f64, width: f64, r_surface: f64, r_bottom: f64) -> Op {
    let w = tool.dia;
    let n = ((width - w) / (0.8 * w)).ceil().max(0.0) as usize + 1;
    let mut mv = Vec::new();
    for k in 0..n {
        let off = if n == 1 { 0.0 } else { -(width - w) / 2.0 + (width - w) * k as f64 / (n - 1) as f64 };
        let z = z0 - u_center + off;
        mv.push(Mv::Rapid(v3(r_surface + 2.0, 0.0, z)));
        mv.push(Mv::Feed(v3(r_bottom, 0.0, z), 0.05));
        mv.push(Mv::Rapid(v3(r_surface + 2.0, 0.0, z)));
    }
    Op { name: format!("O-ring groove {width:.2} wide to {:.2} dia", 2.0 * r_bottom), tool, rpm: 0.0, css: Some(vc * 0.6), moves: mv }
}

/// Part off at part position `u`.
pub fn part_off(tool: Tool, vc: f64, z0: f64, u: f64, r_stock: f64, r_in: f64) -> Op {
    let z = z0 - u - tool.dia / 2.0;
    let mv = vec![Mv::Rapid(v3(r_stock + 2.0, 0.0, z)), Mv::Feed(v3((r_in - 0.5).max(-0.2), 0.0, z), 0.05), Mv::Rapid(v3(r_stock + 2.0, 0.0, z))];
    Op { name: format!("part off at {u:.1} mm"), tool, rpm: 0.0, css: Some(vc * 0.6), moves: mv }
}

// ---------------------------------------------------------------- simulation

/// Turned stock as outer and inner radius per slice of u (part coordinates).
pub struct LatheSim {
    pub u0: f64,
    pub res: f64,
    pub outer: Vec<f64>,
    pub inner: Vec<f64>,
    /// Part length (setups flip about it).
    pub len: f64,
}

impl LatheSim {
    /// Stock from u = u0 to u1 (u0 < 0: stock beyond the finished face).
    pub fn new(u0: f64, u1: f64, r_out: f64, r_in: f64, len: f64) -> Self {
        let res = 0.25;
        let n = ((u1 - u0) / res).ceil() as usize;
        LatheSim { u0, res, outer: vec![r_out; n], inner: vec![r_in; n], len }
    }
    pub fn u_of(&self, z: f64, flipped: bool) -> f64 {
        if flipped { z + self.len } else { -z }
    }
    pub fn z_of(&self, u: f64, flipped: bool) -> f64 {
        if flipped { u - self.len } else { -u }
    }
    fn idx(&self, u: f64) -> Option<usize> {
        let i = ((u - self.u0) / self.res).floor();
        (i >= 0.0 && (i as usize) < self.outer.len()).then_some(i as usize)
    }
    fn u_at(&self, i: usize) -> f64 {
        self.u0 + (i as f64 + 0.5) * self.res
    }
    /// Apply the tool at radius r, machine z. Returns mm^3 removed.
    pub fn stamp(&mut self, kind: ToolKind, r: f64, z: f64, flipped: bool) -> f64 {
        let u = self.u_of(z, flipped);
        let mut vol = 0.0;
        let area = |a: f64, b: f64| std::f64::consts::PI * (a * a - b * b).max(0.0);
        match kind {
            ToolKind::Turning(n) | ToolKind::Groove(n) => {
                let half = if matches!(kind, ToolKind::Groove(_)) { n / 2.0 } else { n };
                let (a, b) = (u - half, u + half);
                let mut uu = a;
                while uu <= b {
                    if let Some(i) = self.idx(uu) {
                        let rr = r.max(0.0);
                        if self.outer[i] > rr {
                            vol += area(self.outer[i], rr.max(self.inner[i])) * self.res;
                            self.outer[i] = rr;
                        }
                    }
                    uu += self.res * 0.5;
                }
            }
            ToolKind::Boring(n) => {
                let mut uu = u - n;
                while uu <= u + n {
                    if let Some(i) = self.idx(uu) {
                        if self.inner[i] < r {
                            vol += area(r.min(self.outer[i]), self.inner[i]) * self.res;
                            self.inner[i] = r;
                        }
                    }
                    uu += self.res * 0.5;
                }
            }
            ToolKind::Drill | ToolKind::FlatEnd => {
                // the drill body fills everything from its tip back out the face
                let rd = r;
                for i in 0..self.outer.len() {
                    let ui = self.u_at(i);
                    let toward_face = if flipped { ui >= u } else { ui <= u };
                    if toward_face && self.inner[i] < rd {
                        vol += area(rd.min(self.outer[i]), self.inner[i]) * self.res;
                        self.inner[i] = rd;
                    }
                }
            }
        }
        vol
    }

    /// Revolve into `scene` in machine coordinates of the current setup,
    /// world = (machine z, radial y, radial z), cut in half through the
    /// axis (the +y half removed) so the profile and bore show in section,
    /// as CAM simulators preview turning. Surfaces join
    /// slice centres with sloped bands (a cone reads as a cone, not as
    /// 0.25 mm stairs); runs of material are capped with annular walls.
    pub fn mesh(&self, scene: &mut Scene, flipped: bool, color: [f32; 4], cut: [f32; 4], section: [f32; 4]) {
        let seg = 72;
        let (a0, a1) = (0.0f64, std::f64::consts::PI);
        let ang = |s: usize| a0 + (a1 - a0) * s as f64 / seg as f64 + std::f64::consts::FRAC_PI_2;
        let world = |z: f64, r: f64, t: f64| v3(z, r * t.cos(), r * t.sin());
        let nrm = |t: f64| v3(0.0, t.cos(), t.sin());
        let n = self.outer.len();
        let solid = |i: usize| self.outer[i] > self.inner[i] + 1e-6;
        let rmax = self.outer.iter().cloned().fold(0.0, f64::max);
        let zc = |i: usize| self.z_of(self.u_at(i), flipped);
        let mut i = 0;
        while i < n {
            if !solid(i) {
                i += 1;
                continue;
            }
            let mut k = i;
            while k + 1 < n && solid(k + 1) {
                k += 1;
            }
            // run i..=k: bands between consecutive slices (merge equal ones)
            let mut j = i;
            while j < k {
                let mut m = j + 1;
                while m < k && (self.outer[m] - self.outer[j]).abs() < 1e-9 && (self.inner[m] - self.inner[j]).abs() < 1e-9 && (self.outer[m + 1] - self.outer[j]).abs() < 1e-9 && (self.inner[m + 1] - self.inner[j]).abs() < 1e-9 {
                    m += 1;
                }
                let (za, zb) = (zc(j), zc(m));
                let (oa, ob, ia, ib) = (self.outer[j], self.outer[m], self.inner[j], self.inner[m]);
                let oc = if oa.max(ob) >= rmax - 1e-6 && (oa - ob).abs() < 1e-9 { color } else { cut };
                for s in 0..seg {
                    let (t0, t1) = (ang(s), ang(s + 1));
                    scene.tri_n([world(za, oa, t0), world(zb, ob, t0), world(zb, ob, t1)], [nrm(t0), nrm(t0), nrm(t1)], oc);
                    scene.tri_n([world(za, oa, t0), world(zb, ob, t1), world(za, oa, t1)], [nrm(t0), nrm(t1), nrm(t1)], oc);
                    if ia.max(ib) > 0.0 {
                        scene.tri_n([world(za, ia, t0), world(zb, ib, t1), world(zb, ib, t0)], [-nrm(t0), -nrm(t1), -nrm(t0)], cut);
                        scene.tri_n([world(za, ia, t0), world(za, ia, t1), world(zb, ib, t1)], [-nrm(t0), -nrm(t1), -nrm(t1)], cut);
                    }
                }
                for t in [ang(0), ang(seg)] {
                    scene.quad(world(za, ia, t), world(zb, ib, t), world(zb, ob, t), world(za, oa, t), section);
                }
                j = m;
            }
            // end caps of the run
            for (e, wall) in [(i, true), (k, true)] {
                if !wall {
                    continue;
                }
                let z = zc(e);
                let (ri, ro) = (self.inner[e], self.outer[e]);
                let c = if (e == i && i == 0) || (e == k && k == n - 1) { color } else { cut };
                for s in 0..seg {
                    let (t0, t1) = (ang(s), ang(s + 1));
                    scene.quad(world(z, ri, t0), world(z, ro, t0), world(z, ro, t1), world(z, ri, t1), c);
                }
            }
            i = k + 1;
        }
    }

    pub fn volume(&self) -> f64 {
        self.outer.iter().zip(&self.inner).map(|(o, i)| std::f64::consts::PI * (o * o - i * i).max(0.0) * self.res).sum()
    }
}

pub fn chuck_radius(r_grip: f64) -> f64 {
    r_grip * 1.2 + 25.0
}

/// Chuck and jaws (world coords, axis along x) behind machine z = `z_face`.
pub fn chuck(scene: &mut Scene, z_face: f64, r_grip: f64, grip: f64, spin_deg: f64) {
    let body = rgb([58, 62, 70]);
    let jaw = rgb([110, 116, 126]);
    let rb = chuck_radius(r_grip);
    scene.cylinder(v3(z_face - grip - 70.0, 0.0, 0.0), v3(z_face - grip, 0.0, 0.0), rb, 48, body);
    for k in 0..3 {
        let a = (spin_deg + 120.0 * k as f64).to_radians();
        let (c, s) = (a.cos(), a.sin());
        let r0 = r_grip;
        let r1 = r_grip + 22.0;
        // a jaw: a box in the (radial, tangential) frame
        let corner = |r: f64, t: f64, z: f64| v3(z, r * c - t * s, r * s + t * c);
        let (t0, t1) = (-11.0, 11.0);
        let (za, zb) = (z_face - grip, z_face);
        let p = |r, t, z| corner(r, t, z);
        let faces = [
            [p(r0, t0, za), p(r0, t1, za), p(r0, t1, zb), p(r0, t0, zb)],
            [p(r1, t0, za), p(r1, t0, zb), p(r1, t1, zb), p(r1, t1, za)],
            [p(r0, t0, za), p(r0, t0, zb), p(r1, t0, zb), p(r1, t0, za)],
            [p(r0, t1, za), p(r1, t1, za), p(r1, t1, zb), p(r0, t1, zb)],
            [p(r0, t0, zb), p(r0, t1, zb), p(r1, t1, zb), p(r1, t0, zb)],
        ];
        for f in faces {
            scene.quad(f[0], f[1], f[2], f[3], jaw);
        }
    }
}

/// The cutting tool at machine (r, z): insert + holder, world coords.
pub fn tool_mesh(scene: &mut Scene, kind: ToolKind, r: f64, z: f64, dia: f64) {
    let insert = rgb([230, 190, 60]);
    let holder = rgb([80, 84, 92]);
    // tools come in from +y (towards the camera side) at the radius r, in
    // the world plane y = r (the cut-away shows that side)
    match kind {
        ToolKind::Turning(_) | ToolKind::Groove(_) => {
            let w = if let ToolKind::Groove(w) = kind { w } else { 6.0 };
            scene.cuboid(v3(z - w / 2.0, -2.0, r), v3(z + w / 2.0, 2.0, r + 8.0), insert);
            scene.cuboid(v3(z - 6.0, -10.0, r + 8.0), v3(z + 14.0, 10.0, r + 90.0), holder);
        }
        ToolKind::Boring(_) => {
            let d = dia.max(6.0);
            scene.cuboid(v3(z - 2.0, -2.0, r - 3.0), v3(z + 2.0, 2.0, r), insert);
            scene.cylinder(v3(z, 0.0, r - d / 2.0 - 1.0), v3(z + 220.0, 0.0, r - d / 2.0 - 1.0), d / 2.0, 20, holder);
        }
        _ => {
            scene.cylinder(v3(z, 0.0, 0.0), v3(z + 160.0, 0.0, 0.0), dia / 2.0, 20, rgb([200, 160, 70]));
        }
    }
}

pub fn stock_world_center(len: f64) -> V3 {
    v3(-len / 2.0, 0.0, 0.0)
}
