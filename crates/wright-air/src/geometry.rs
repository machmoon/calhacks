//! The parts as solids: one description derived from [`Layout`], turned into
//! `wright_geom` meshes for the CAD, the all-pairs interference check, the
//! prop clearances and the slicer.
//!
//! Each part declares what it is joined to (`touches`): its mount, the tube
//! it is bonded to. The interference check runs over ALL pairs minus those;
//! the attachment check runs over those and demands contact, so a part
//! declared joined and drawn floating fails too. The pod is a hollow shell:
//! the battery and avionics touch nothing, so a battery outside the pod
//! clashes with its wall or fails containment -- the prototype passed one
//! sitting wholly outside.
//!
//! Containment (battery and bay in the pod's cavity, spars and servos in the
//! wing's skin) is measured analytically on the exact sections, in metres.

use crate::layout::Layout;
use crate::rules::*;
use wright_geom::{shapes, v3, Mesh, Part, PartKind, V3};

/// Points per airfoil ring and sides per cylinder: wright-geom's booleans
/// are O(n^2)-ish (its notes: 60-120 points, 32-64 sides).
const RING_POINTS: usize = 64;
const CYL_SIDES: usize = 40;

#[derive(Debug, Clone)]
pub enum Shape {
    Box { min: [f64; 3], max: [f64; 3] },
    Cyl { p0: [f64; 3], p1: [f64; 3], r: f64 },
    Tube { p0: [f64; 3], p1: [f64; 3], r_out: f64, r_in: f64 },
    Loft { rings: Vec<Vec<[f64; 3]>> },
    Shell { outer: Vec<Vec<[f64; 3]>>, inner: Vec<Vec<[f64; 3]>> },
}

#[derive(Debug, Clone)]
pub struct PartGeometry {
    pub name: String,
    pub shape: Shape,
    /// Filament id when printed; None when bought.
    pub filament: Option<String>,
    pub touches: Vec<String>,
    /// A spinning prop's swept disc (clearance applies, not a solid).
    pub disc: bool,
}

fn mm(p: [f64; 3]) -> V3 {
    v3(p[0] * 1000.0, p[1] * 1000.0, p[2] * 1000.0)
}

fn ring_area(r: &[[f64; 3]]) -> f64 {
    // Newell's method: the area of a planar polygon in 3D
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    for i in 0..r.len() {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        x += (a[1] - b[1]) * (a[2] + b[2]);
        y += (a[2] - b[2]) * (a[0] + b[0]);
        z += (a[0] - b[0]) * (a[1] + b[1]);
    }
    0.5 * (x * x + y * y + z * z).sqrt()
}

fn centroid(r: &[[f64; 3]]) -> [f64; 3] {
    let n = r.len() as f64;
    let s = r.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
    [s[0] / n, s[1] / n, s[2] / n]
}

fn loft_volume(rings: &[Vec<[f64; 3]>]) -> f64 {
    rings
        .windows(2)
        .map(|w| {
            let (a, b) = (centroid(&w[0]), centroid(&w[1]));
            let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            // prismatoid between similar sections: (A0 + sqrt(A0 A1) + A1) / 3
            let (a0, a1) = (ring_area(&w[0]), ring_area(&w[1]));
            d * (a0 + (a0 * a1).sqrt() + a1) / 3.0
        })
        .sum()
}

impl Shape {
    /// Enclosed volume, m^3 (analytic; the mesh's agrees to tessellation).
    pub fn volume_m3(&self) -> f64 {
        let len = |p0: &[f64; 3], p1: &[f64; 3]| ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2) + (p1[2] - p0[2]).powi(2)).sqrt();
        match self {
            Shape::Box { min, max } => (max[0] - min[0]) * (max[1] - min[1]) * (max[2] - min[2]),
            Shape::Cyl { p0, p1, r } => std::f64::consts::PI * r * r * len(p0, p1),
            Shape::Tube { p0, p1, r_out, r_in } => std::f64::consts::PI * (r_out * r_out - r_in * r_in) * len(p0, p1),
            Shape::Loft { rings } => loft_volume(rings),
            Shape::Shell { outer, inner } => loft_volume(outer) - loft_volume(inner),
        }
    }
    pub fn mesh(&self) -> Mesh {
        match self {
            Shape::Box { min, max } => shapes::cuboid(mm(*min), mm(*max)),
            Shape::Cyl { p0, p1, r } => shapes::cylinder(mm(*p0), mm(*p1), r * 1000.0, CYL_SIDES),
            Shape::Tube { p0, p1, r_out, r_in } => shapes::tube(mm(*p0), mm(*p1), r_out * 1000.0, r_in * 1000.0, CYL_SIDES),
            Shape::Loft { rings } => shapes::loft(&rings.iter().map(|r| r.iter().map(|p| mm(*p)).collect()).collect::<Vec<_>>()),
            Shape::Shell { outer, inner } => {
                let o = shapes::loft(&outer.iter().map(|r| r.iter().map(|p| mm(*p)).collect()).collect::<Vec<_>>());
                let i = shapes::loft(&inner.iter().map(|r| r.iter().map(|p| mm(*p)).collect()).collect::<Vec<_>>());
                o.subtract(&i)
            }
        }
    }
}

/// The airfoil outline (x/c, z/c), resampled to about RING_POINTS, closed
/// implicitly, without a duplicate trailing-edge point.
fn section(lay: &Layout) -> Vec<[f64; 2]> {
    let c = &lay.parts.wing_airfoil.coordinates;
    let step = (c.len() as f64 / RING_POINTS as f64).max(1.0);
    let mut out: Vec<[f64; 2]> = (0..RING_POINTS).map(|k| c[((k as f64 * step) as usize).min(c.len() - 1)]).collect();
    out.dedup();
    if out.len() > 2 && (out[0][0] - out[out.len() - 1][0]).abs() < 1e-9 && (out[0][1] - out[out.len() - 1][1]).abs() < 1e-9 {
        out.pop();
    }
    out
}

fn naca00(t: f64, n: usize) -> Vec<[f64; 2]> {
    let mut up = vec![];
    for i in 0..=n {
        let xc = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / n as f64).cos());
        let yt = 5.0 * t * (0.2969 * xc.sqrt() - 0.1260 * xc - 0.3516 * xc * xc + 0.2843 * xc.powi(3) - 0.1015 * xc.powi(4));
        up.push([xc, yt]);
    }
    let mut ring: Vec<[f64; 2]> = up.iter().rev().cloned().collect();
    for p in up.iter().skip(1).take(n - 1) {
        ring.push([p[0], -p[1]]);
    }
    ring
}

fn superellipse(a: f64, b: f64, n: f64, k: usize) -> Vec<[f64; 2]> {
    (0..k)
        .map(|i| {
            let t = 2.0 * std::f64::consts::PI * i as f64 / k as f64;
            let (c, s) = (t.cos(), t.sin());
            [a * c.signum() * c.abs().powf(2.0 / n), b * s.signum() * s.abs().powf(2.0 / n)]
        })
        .collect()
}

/// The pod's cavity half-sizes at x (zero outside it), m.
pub fn pod_cavity(lay: &Layout, x: f64, inner: bool) -> Option<(f64, f64)> {
    let (x0, x1) = (lay.pod_x0.as_m(), lay.pod_x1.as_m());
    let wall = if inner { POD_WALL_M } else { 0.0 };
    let (f0, f1) = (lay.pod_full_x0.as_m(), lay.pod_full_x1.as_m());
    if x < x0 + wall || x > x1 - wall {
        return None;
    }
    let scale = if x < f0 {
        0.5 + 0.5 * (x - x0) / (f0 - x0)
    } else if x > f1 {
        1.0 - 0.4 * (x - f1) / (x1 - f1)
    } else {
        1.0
    };
    Some((scale * lay.pod_width.as_m() / 2.0 - wall, scale * lay.pod_height.as_m() / 2.0 - wall))
}

fn pod_rings(lay: &Layout, inner: bool) -> Vec<Vec<[f64; 3]>> {
    let w = if inner { POD_WALL_M } else { 0.0 };
    let xs = [lay.pod_x0.as_m() + w, lay.pod_full_x0.as_m(), lay.pod_full_x1.as_m(), lay.pod_x1.as_m() - w];
    xs.iter()
        .map(|&x| {
            let (a, b) = pod_cavity(lay, x, inner).unwrap_or((0.01, 0.01));
            superellipse(a, b, POD_EXPONENT, 48).iter().map(|p| [x, p[0], lay.pod_zc.as_m() + p[1]]).collect()
        })
        .collect()
}

/// Every part, named, placed, with what it may touch.
pub fn parts(lay: &Layout) -> Vec<PartGeometry> {
    let p = &lay.parts;
    let shell = Some(lay.design.structure.shell_filament.clone());
    let fit = Some(lay.design.structure.structural_filament.clone());
    let mut v: Vec<PartGeometry> = vec![];
    let mut add = |name: &str, shape: Shape, filament: Option<String>, touches: &[&str], disc: bool| {
        v.push(PartGeometry { name: name.into(), shape, filament, touches: touches.iter().map(|s| s.to_string()).collect(), disc });
    };
    let semi = lay.semi_span().as_m();
    let sec = section(lay);
    let dih = lay.design.wing.dihedral_deg.to_radians().tan();
    let wing_ring = |y: f64| -> Vec<[f64; 3]> {
        let c = lay.chord_at(wright_core::units::Length::m(y)).as_m();
        sec.iter().map(|q| [q[0] * c, y, q[1] * c + y.abs() * dih]).collect()
    };
    for (side, sgn) in [("R", 1.0), ("L", -1.0)] {
        let rings: Vec<Vec<[f64; 3]>> = (0..=6).map(|k| wing_ring(sgn * semi * k as f64 / 6.0)).collect();
        let other = if side == "R" { "L" } else { "R" };
        add(
            &format!("wing_{side}"),
            Shape::Loft { rings },
            shell.clone(),
            &[
                &format!("wing_{other}"),
                "pod",
                &format!("main_spar_{side}"),
                &format!("rear_spar_{side}"),
                &format!("joiner_{side}"),
                &format!("joiner_{other}"),
                &format!("servo_aileron_{side}"),
                &format!("boom_clamp_{side}0"),
                &format!("boom_clamp_{side}1"),
            ],
            false,
        );
        let (msx, msz, msr) = (lay.main_spar_x.as_m(), lay.main_spar_z.as_m(), p.main_spar.od.as_m() / 2.0);
        let msr_in = p.main_spar.id.map(|l| l.as_m() / 2.0).unwrap_or(0.0);
        let reach = MAIN_SPAR_REACH * semi;
        let spar_shape = |y0: f64, y1: f64, ro: f64, ri: f64, x: f64, z: f64| {
            if ri > 0.0 {
                Shape::Tube { p0: [x, y0, z], p1: [x, y1, z], r_out: ro, r_in: ri }
            } else {
                Shape::Cyl { p0: [x, y0, z], p1: [x, y1, z], r: ro }
            }
        };
        add(&format!("main_spar_{side}"), spar_shape(0.0005 * sgn, sgn * reach, msr, msr_in, msx, msz), None, &[&format!("wing_{side}"), &format!("joiner_{side}")], false);
        let jr = p.joiner.od.as_m() / 2.0;
        let jri = p.joiner.id.map(|l| l.as_m() / 2.0).unwrap_or(0.0);
        let yj = JOINER_HALF_FRAC * semi;
        add(
            &format!("joiner_{side}"),
            spar_shape(0.0, sgn * yj, jr, jri, msx, msz),
            None,
            &[&format!("main_spar_{side}"), &format!("joiner_{other}"), &format!("wing_{side}"), &format!("wing_{other}")],
            false,
        );
        let rr = p.rear_spar.od.as_m() / 2.0;
        add(
            &format!("rear_spar_{side}"),
            spar_shape(0.0005 * sgn, sgn * REAR_SPAR_REACH * semi, rr, 0.0, lay.rear_spar_x.as_m(), lay.rear_spar_z.as_m()),
            None,
            &[&format!("wing_{side}")],
            false,
        );
        let s = &lay.aileron_servo;
        let z = &lay.servo_size;
        let (cx, cy, cz) = (s[0].as_m(), sgn * s[1].as_m(), s[2].as_m());
        add(
            &format!("servo_aileron_{side}"),
            Shape::Box { min: [cx - z[0].as_m() / 2.0, cy - z[1].as_m() / 2.0, cz - z[2].as_m() / 2.0], max: [cx + z[0].as_m() / 2.0, cy + z[1].as_m() / 2.0, cz + z[2].as_m() / 2.0] },
            None,
            &[&format!("wing_{side}")],
            false,
        );
        // boom and everything on it
        let by = sgn * lay.boom_y.as_m();
        let bz = lay.boom_z.as_m();
        let br = p.boom.od.as_m() / 2.0;
        let bri = p.boom.id.map(|l| l.as_m() / 2.0).unwrap_or(0.0);
        let boom = format!("boom_{side}");
        let st = |k: &str| format!("{k}_{side}");
        add(
            &boom,
            Shape::Tube { p0: [lay.boom_x0.as_m(), by, bz], p1: [lay.boom_x1.as_m(), by, bz], r_out: br, r_in: bri },
            None,
            &[
                &format!("boom_clamp_{side}0"),
                &format!("boom_clamp_{side}1"),
                &format!("lift_mount_F{side}"),
                &format!("lift_mount_R{side}"),
                &format!("leg_F{side}"),
                &format!("leg_R{side}"),
                &st("tail_joint"),
                &st("fin"),
                &format!("servo_tail_{side}"),
            ],
            false,
        );
        let c_b = lay.chord_at(lay.boom_y).as_m();
        for (k, xc) in [(0, 0.10), (1, 0.75)] {
            let x = xc * c_b;
            let low = lay.wing_surface(wright_core::units::Length::m(x), lay.boom_y).map(|s| s.1.as_m()).unwrap_or(0.0);
            add(
                &format!("boom_clamp_{side}{k}"),
                Shape::Box { min: [x - 0.01, by - br - 0.003, bz - br - 0.003], max: [x + 0.01, by + br + 0.003, low + 0.002] },
                fit.clone(),
                &[&boom, &format!("wing_{side}")],
                false,
            );
        }
        for (st_, x) in [("F", lay.x_front.as_m()), ("R", lay.x_rear.as_m())] {
            let tag = format!("{st_}{side}");
            add(
                &format!("lift_mount_{tag}"),
                Shape::Box { min: [x - 0.02, by - br - 0.004, bz], max: [x + 0.02, by + br + 0.004, lay.mount_top_z.as_m()] },
                fit.clone(),
                &[&boom, &format!("lift_motor_{tag}")],
                false,
            );
            add(
                &format!("lift_motor_{tag}"),
                Shape::Cyl { p0: [x, by, lay.mount_top_z.as_m()], p1: [x, by, lay.motor_top_z.as_m()], r: p.lift_motor.body_diameter.as_m() / 2.0 },
                None,
                &[&format!("lift_mount_{tag}"), &format!("lift_hub_{tag}")],
                false,
            );
            let dz = lay.lift_disc_z.as_m();
            // the shaft and prop hub: overlaps both, so contact is a real overlap
            add(
                &format!("lift_hub_{tag}"),
                Shape::Cyl { p0: [x, by, lay.motor_top_z.as_m() - 0.001], p1: [x, by, dz + 0.001], r: 0.004 },
                None,
                &[&format!("lift_motor_{tag}"), &format!("lift_prop_disc_{tag}")],
                false,
            );
            add(
                &format!("lift_prop_disc_{tag}"),
                Shape::Cyl { p0: [x, by, dz], p1: [x, by, dz + LIFT_DISC_THICK_M], r: lay.lift_radius.as_m() },
                None,
                &[&format!("lift_hub_{tag}")],
                true,
            );
            let (lt, lc) = (lay.leg_thickness.as_m(), lay.leg_chord.as_m());
            // a faired strut: NACA 00xx section, chord along the flow
            let strut = naca00(lt / lc, 16);
            let ring = |z: f64| strut.iter().map(|q| [x - lc / 2.0 + q[0] * lc, by + q[1] * lc, z]).collect::<Vec<_>>();
            add(&format!("leg_{tag}"), Shape::Loft { rings: vec![ring(lay.ground_z.as_m()), ring(bz)] }, fit.clone(), &[&boom], false);
        }
        // tail joint round the boom end, a fin on it
        let xt = lay.x_tail_le.as_m();
        let hc = lay.h_chord.as_m();
        let top = bz + br;
        let ht = hc * p.tail_airfoil.t_max;
        add(
            &st("tail_joint"),
            Shape::Box { min: [xt, by - br - 0.003, bz - br - 0.003], max: [xt + hc.max(lay.v_chord.as_m()), by + br + 0.003, top + 0.002] },
            fit.clone(),
            &[&boom, "h_stab", &st("fin")],
            false,
        );
        let vc = lay.v_chord.as_m();
        let fin_sec = naca00(p.tail_airfoil.t_max, 24);
        let fin_ring = |z: f64| fin_sec.iter().map(|q| [xt + q[0] * vc, by + q[1] * vc, z]).collect::<Vec<_>>();
        add(&st("fin"), Shape::Loft { rings: vec![fin_ring(top - 0.001), fin_ring(top + ht + lay.v_height.as_m())] }, shell.clone(), &[&boom, "h_stab", &st("tail_joint")], false);
        // tail servos on the boom's outboard side, ahead of the tail
        let sz = &lay.servo_size;
        let (sx, sy, sth) = (sz[1].as_m(), sz[0].as_m(), sz[2].as_m());
        let yo = by + sgn * (br - 0.0005 + sth / 2.0);
        add(
            &format!("servo_tail_{side}"),
            Shape::Box { min: [xt - 0.035 - sx / 2.0, yo - sth / 2.0, bz - sy / 2.0], max: [xt - 0.035 + sx / 2.0, yo + sth / 2.0, bz + sy / 2.0] },
            None,
            &[&boom],
            false,
        );
    }
    // horizontal stabiliser across the booms, on top of them
    let xt = lay.x_tail_le.as_m();
    let hc = lay.h_chord.as_m();
    let top = lay.boom_z.as_m() + lay.parts.boom.od.as_m() / 2.0;
    let hsec = naca00(lay.parts.tail_airfoil.t_max, 24);
    let ht = hc * lay.parts.tail_airfoil.t_max;
    let hring = |y: f64| hsec.iter().map(|q| [xt + q[0] * hc, y, top + ht / 2.0 + 0.001 + q[1] * hc]).collect::<Vec<_>>();
    let by = lay.boom_y.as_m();
    add("h_stab", Shape::Loft { rings: vec![hring(-by), hring(by)] }, shell.clone(), &["tail_joint_R", "tail_joint_L", "fin_R", "fin_L"], false);
    // the pod and what is in it
    add("pod", Shape::Shell { outer: pod_rings(lay, false), inner: pod_rings(lay, true) }, shell.clone(), &["wing_R", "wing_L", "pusher_mount"], false);
    for b in &lay.bay {
        let c = [b.center[0].as_m(), b.center[1].as_m(), b.center[2].as_m()];
        let h = [b.size[0].as_m() / 2.0, b.size[1].as_m() / 2.0, b.size[2].as_m() / 2.0];
        add(b.name, Shape::Box { min: [c[0] - h[0], c[1] - h[1], c[2] - h[2]], max: [c[0] + h[0], c[1] + h[1], c[2] + h[2]] }, None, &[], false);
    }
    // the pusher: mount on the pod's tail, motor, disc
    let x1 = lay.pod_x1.as_m();
    let pz = lay.pusher_z.as_m();
    let cm = lay.parts.cruise_motor;
    let mr = cm.body_diameter.as_m() / 2.0;
    let px = lay.pusher_x.as_m();
    let motor_x0 = px - 0.012 - cm.bell_length().as_m();
    add("pusher_mount", Shape::Box { min: [x1 - 0.01, -mr - 0.003, pz - mr - 0.003], max: [motor_x0, mr + 0.003, pz + mr + 0.003] }, fit.clone(), &["pod", "pusher_motor"], false);
    add("pusher_motor", Shape::Cyl { p0: [motor_x0, 0.0, pz], p1: [px - 0.012, 0.0, pz], r: mr }, None, &["pusher_mount", "pusher_hub"], false);
    add("pusher_hub", Shape::Cyl { p0: [px - 0.013, 0.0, pz], p1: [px + 0.001, 0.0, pz], r: 0.005 }, None, &["pusher_motor", "pusher_disc"], false);
    add("pusher_disc", Shape::Cyl { p0: [px, 0.0, pz], p1: [px + PUSHER_DISC_THICK_M, 0.0, pz], r: lay.pusher_radius.as_m() }, None, &["pusher_hub"], true);
    v
}

// ---------------------------------------------------------------------------
// Analytic containment, exact on the sections.

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Signed distance from `p` to the polygon's boundary: positive inside.
pub fn signed_distance(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let n = poly.len();
    let mut inside = false;
    let mut d = f64::INFINITY;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        d = d.min(seg_dist(p, a, b));
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    if inside { d } else { -d }
}

/// The wing's outline at span station y, (x, z), m.
pub fn wing_outline(lay: &Layout, y: f64) -> Vec<[f64; 2]> {
    let c = lay.chord_at(wright_core::units::Length::m(y)).as_m();
    let dz = y.abs() * lay.design.wing.dihedral_deg.to_radians().tan();
    lay.parts.wing_airfoil.coordinates.iter().map(|q| [q[0] * c, q[1] * c + dz]).collect()
}

/// How far a round tube at (x, z) of radius r stays inside the wing's skin
/// from y0 to y1 (sampled every 10 mm), m. Negative: it breaks out.
pub fn tube_in_wing(lay: &Layout, x: f64, z: f64, r: f64, y0: f64, y1: f64) -> (f64, f64) {
    let n = (((y1 - y0).abs() / 0.01).ceil() as usize).max(1);
    let mut worst = (f64::INFINITY, y0);
    for k in 0..=n {
        let y = y0 + (y1 - y0) * k as f64 / n as f64;
        let m = signed_distance(&wing_outline(lay, y), [x, z]) - r - SKIN_WALL_M;
        if m < worst.0 {
            worst = (m, y);
        }
    }
    worst
}

/// The aileron servo's room in the wing (inside the skins), m: the least
/// over its corners and over any part of the outline poking into it.
pub fn servo_margin(lay: &Layout) -> f64 {
    let s = &lay.aileron_servo;
    let z = &lay.servo_size;
    let (cx, cy, cz) = (s[0].as_m(), s[1].as_m(), s[2].as_m());
    let (hx, hy, hz) = (z[0].as_m() / 2.0, z[1].as_m() / 2.0, z[2].as_m() / 2.0);
    let mut worst = f64::INFINITY;
    for y in [cy - hy, cy, cy + hy] {
        let poly = wing_outline(lay, y);
        for (dx, dz) in [(-hx, -hz), (hx, -hz), (-hx, hz), (hx, hz)] {
            worst = worst.min(signed_distance(&poly, [cx + dx, cz + dz]) - SKIN_WALL_M);
        }
        for q in &poly {
            if q[0] > cx - hx && q[0] < cx + hx && q[1] > cz - hz && q[1] < cz + hz {
                worst = worst.min(-((q[1] - (cz - hz)).min(cz + hz - q[1])));
            }
        }
    }
    worst
}

/// How far a box stays inside the pod's cavity, m (negative: through the
/// wall or outside). The cavity is a loft of homothetic convex sections, so
/// a box is inside exactly when its corners are.
pub fn box_in_pod(lay: &Layout, min: [f64; 3], max: [f64; 3]) -> f64 {
    let n = POD_EXPONENT;
    let zc = lay.pod_zc.as_m();
    let mut worst = f64::INFINITY;
    for x in [min[0], max[0]] {
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                let m = match pod_cavity(lay, x, true) {
                    None => -0.01,
                    Some((a, b)) if a > 0.0 && b > 0.0 => {
                        let f = (y.abs() / a).powf(n) + ((z - zc).abs() / b).powf(n);
                        (1.0 - f.powf(1.0 / n)) * a.min(b)
                    }
                    _ => -0.01,
                };
                worst = worst.min(m);
            }
        }
    }
    worst
}

/// Meshes for every part (mm), in the order of [`parts`].
pub fn meshes(geo: &[PartGeometry]) -> Vec<Part> {
    geo.iter()
        .map(|g| Part {
            name: g.name.clone(),
            kind: if g.filament.is_some() { PartKind::Printed } else { PartKind::Bought },
            material: g.filament.clone().unwrap_or_default(),
            mesh: g.shape.mesh(),
            touches: g.touches.clone(),
        })
        .collect()
}

/// Distance from point `q` to a solid cylinder (any axis), m.
pub fn point_cylinder(q: [f64; 3], p0: [f64; 3], p1: [f64; 3], r: f64) -> f64 {
    let ax = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let len = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
    let u = [ax[0] / len, ax[1] / len, ax[2] / len];
    let w = [q[0] - p0[0], q[1] - p0[1], q[2] - p0[2]];
    let t = w[0] * u[0] + w[1] * u[1] + w[2] * u[2];
    let radial = ((w[0] - t * u[0]).powi(2) + (w[1] - t * u[1]).powi(2) + (w[2] - t * u[2]).powi(2)).sqrt();
    let dr = (radial - r).max(0.0);
    let da = (-t).max(t - len).max(0.0);
    dr.hypot(da)
}

/// A disc's distance to a mesh (mm): the mesh's triangles sampled every
/// `step` mm (vertices, edges and interiors) against the exact cylinder,
/// less half a step so the sampling can only understate it.
pub fn disc_to_mesh_mm(shape: &Shape, mesh: &wright_geom::Mesh, step: f64) -> f64 {
    let Shape::Cyl { p0, p1, r } = shape else { return f64::NAN };
    let (a, b) = (p0.map(|v| v * 1000.0), p1.map(|v| v * 1000.0));
    let rr = r * 1000.0;
    let mut best = f64::INFINITY;
    for t in mesh.triangles() {
        let e1 = t[1] - t[0];
        let e2 = t[2] - t[0];
        let n = ((e1.norm().max(e2.norm()) / step).ceil() as usize).clamp(1, 400);
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (s, u) = (i as f64 / n as f64, j as f64 / n as f64);
                let q = t[0] + e1 * s + e2 * u;
                best = best.min(point_cylinder([q.x, q.y, q.z], a, b, rr));
            }
        }
    }
    best - step / 2.0
}
