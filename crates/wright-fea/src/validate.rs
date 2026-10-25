//! The pipeline against closed-form solutions it must reproduce before any
//! of its numbers feed a check:
//!
//! 1. cantilever tube (S8R): tip deflection P L^3 / 3EI and first bending
//!    mode (1.8751^2 / 2 pi) sqrt(EI / m L^4) (Euler-Bernoulli; Blevins,
//!    Formulas for Natural Frequency and Mode Shape, table 8-1 case 3), and
//!    the bending stress at mid-span M c / I;
//! 2. closed-end thin cylinder under internal pressure (S8R): hoop stress at
//!    the bore vs Lame p (ro^2 + ri^2) / (ro^2 - ri^2), axial p ri^2 / (ro^2 - ri^2);
//! 3. clamped circular plate under uniform pressure (gmsh C3D20R): centre
//!    deflection p a^4 / 64 D and centre stress 3 (1 + nu) p a^2 / 8 t^2
//!    (Roark, 7th ed., table 11.2 case 10b);
//! 4. simply supported circular plate (the bulkhead's edge model):
//!    centre stress 3 (3 + nu) p a^2 / 8 t^2 and deflection
//!    (5 + nu) p a^4 / (64 D (1 + nu)) (Roark table 11.2 case 10a);
//! 5. axially compressed cylinder (S8R, *BUCKLE): classical
//!    sigma_cr = E t / (R sqrt(3 (1 - nu^2))) (Timoshenko & Gere, Theory of
//!    Elastic Stability, 2nd ed., eq. 11-13).

use crate::deck::{Analysis, Material, Model, Section};
use crate::mesh::{self, V3};
use crate::results::{solve, Solved};
use anyhow::Result;
use serde::Serialize;
use std::f64::consts::PI;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub case: String,
    pub quantity: String,
    pub fea: f64,
    pub reference: f64,
    pub error_pct: f64,
    pub reference_source: String,
    pub mesh: String,
    pub seconds: f64,
}

fn row(case: &str, q: &str, fea: f64, reference: f64, src: &str, mesh: String, s: f64) -> Row {
    Row { case: case.into(), quantity: q.into(), fea, reference, error_pct: 100.0 * (fea - reference) / reference, reference_source: src.into(), mesh, seconds: s }
}

pub fn al6061() -> Material {
    // 6061-T6: E 68.9 GPa, nu 0.33, 2700 kg/m^3, Fty 276 MPa (MMPDS / ASM
    // Aerospace Specification Metals; wright-rocket rules AL6061_YIELD_PA,
    // AL6061_DENSITY)
    Material::iso("AL6061", 68.9e9, 0.33, wright_rocket::rules::AL6061_DENSITY, wright_rocket::rules::AL6061_YIELD_PA, "6061-T6, ASM/MMPDS E 68.9 GPa nu 0.33; wright-rocket rules")
}

/// Mean of a DISP component over frd nodes where `f` holds.
pub fn mean_disp(s: &Solved, comp: usize, f: impl Fn(V3) -> bool) -> f64 {
    let b = s.block("DISP").expect("DISP");
    let v: Vec<f64> = s.frd.nodes.iter().filter(|(_, p)| f(**p)).filter_map(|(n, _)| b.data.get(n).map(|d| d[comp])).collect();
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// Extreme (by absolute value) of `g(stress, position)` over nodes where `f` holds.
pub fn extreme_stress(s: &Solved, f: impl Fn(V3) -> bool, g: impl Fn(&[f64], V3) -> f64) -> f64 {
    let b = s.block("STRESS").expect("STRESS");
    s.frd.nodes.iter().filter(|(_, p)| f(**p)).filter_map(|(n, p)| b.data.get(n).map(|st| g(st, *p))).fold(0.0, |a: f64, x| if x.abs() > a.abs() { x } else { a })
}

/// Hoop stress about the z axis at a node.
pub fn hoop_z(st: &[f64], p: V3) -> f64 {
    let th = p[1].atan2(p[0]);
    let (s, c) = th.sin_cos();
    st[0] * s * s + st[1] * c * c - 2.0 * st[3] * s * c
}

pub fn run(dir: &Path) -> Result<Vec<Row>> {
    let mut rows = vec![];
    let al = al6061();
    let (e, nu) = (68.9e9, 0.33);

    // 1. cantilever tube along x, clamped at x = 0, tip load -z at x = L
    {
        let (l, r, t, p) = (0.5, 0.010, 0.001, 100.0);
        let st = mesh::stations(0.0, l, 0.005, &[l / 2.0]);
        let tb = mesh::tube([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], r, &st, 24, |_| "TUBE".into());
        let (ro, ri) = (r + t / 2.0, r - t / 2.0);
        let i = PI * (ro.powi(4) - ri.powi(4)) / 4.0;
        let a = PI * (ro * ro - ri * ri);
        let mk = |an: Analysis| {
            let mut m = Model::new(tb.mesh.clone(), an);
            m.materials.push(al.clone());
            m.sections.push(Section::Shell { set: "TUBE".into(), material: "AL6061".into(), thickness: t });
            m.fix(&tb.nodes_in_rows(&[0]), 1, 6);
            m
        };
        let mut m = mk(Analysis::Static);
        m.forces = tb.ring_force(tb.rows.len() - 1, [0.0, 0.0, -p]);
        let s = solve(&m, &dir.join("cantilever"), "static")?;
        let tip = -mean_disp(&s, 2, |q| (q[0] - l).abs() < 1e-6);
        let mesh_d = format!("{} S8R ({} around, {} along)", tb.mesh.elements.len(), 24, st.len() - 1);
        rows.push(row("cantilever tube", "tip deflection, mm", tip * 1e3, p * l.powi(3) / (3.0 * e * i) * 1e3, "Euler-Bernoulli P L^3 / 3 E I", mesh_d.clone(), s.seconds));
        let sx = extreme_stress(&s, |q| (q[0] - l / 2.0).abs() < 1e-6, |st, _| st[0]);
        rows.push(row("cantilever tube", "bending stress at mid-span, MPa", sx.abs() / 1e6, p * (l / 2.0) * ro / i / 1e6, "M c / I at the outer fibre", mesh_d.clone(), s.seconds));
        let f = solve(&mk(Analysis::Frequency { modes: 4 }), &dir.join("cantilever"), "modes")?;
        let f_ref = 1.875_104_f64.powi(2) / (2.0 * PI) * (e * i / (al.density * a * l.powi(4))).sqrt();
        rows.push(row("cantilever tube", "first bending mode, Hz", f.frequencies_hz.first().copied().unwrap_or(f64::NAN), f_ref, "Euler-Bernoulli (1.8751^2 / 2 pi) sqrt(EI / m L^4)", mesh_d, f.seconds));
        crate::render::contour(&s, "von_mises", &crate::render::Picture { title: "Validation: cantilever tube, 100 N tip load", note: "CalculiX S8R", view: [15.0, -70.0], scale: None }, &dir.join("cantilever.png"))?;
        crate::render::mode_animation(&f, 0, &crate::render::Picture { title: &format!("Validation: cantilever first mode {:.1} Hz", f.frequencies_hz[0]), note: "", view: [15.0, -70.0], scale: None }, &dir.join("cantilever_mode1.mp4"), 24)?;
    }

    // 2. closed-end cylinder under internal pressure, axis z
    {
        let (r, t, l, p) = (0.1, 0.002, 0.6, 1.0e6);
        let st = mesh::stations(0.0, l, 0.01, &[l / 2.0]);
        let tb = mesh::tube([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], r, &st, 48, |_| "SHELL".into());
        let (ro, ri) = (r + t / 2.0, r - t / 2.0);
        let s = pressure_cylinder(&tb, &al, t, p, ri, &dir.join("cylinder"))?;
        let hoop = extreme_stress(&s, |q| (q[2] - l / 2.0).abs() < 1e-6, hoop_z);
        let axial = extreme_stress(&s, |q| (q[2] - l / 2.0).abs() < 1e-6, |st, _| st[2]);
        let mesh_d = format!("{} S8R", tb.mesh.elements.len());
        rows.push(row("pressurised cylinder", "hoop stress at the bore, MPa", hoop / 1e6, p * (ro * ro + ri * ri) / (ro * ro - ri * ri) / 1e6, "Lame thick-wall hoop at r = ri", mesh_d.clone(), s.seconds));
        rows.push(row("pressurised cylinder", "axial stress, MPa", axial / 1e6, p * ri * ri / (ro * ro - ri * ri) / 1e6, "closed-end axial p ri^2 / (ro^2 - ri^2)", mesh_d, s.seconds));
        crate::render::contour(&s, "von_mises", &crate::render::Picture { title: "Validation: closed-end cylinder, 1 MPa", note: "CalculiX S8R", view: [20.0, -60.0], scale: None }, &dir.join("cylinder.png"))?;
    }

    // 3 and 4. circular plates
    {
        let (a, t, p): (f64, f64, f64) = (0.1, 0.005, 1.0e5);
        let d = e * t.powi(3) / (12.0 * (1.0 - nu * nu));
        let pm = crate::gmsh::disk(&dir.join("plate"), a, t, 0.005, 4, "PLATE")?;
        let mesh_d = format!("{} C3D20R (gmsh, 4 layers)", pm.elements.len());
        for clamped in [true, false] {
            let mut m = Model::new(pm.clone(), Analysis::Static);
            m.materials.push(al.clone());
            m.sections.push(Section::Solid { set: "PLATE".into(), material: "AL6061".into() });
            plate_supports(&mut m, a, t, clamped);
            for (ei, f, _) in pm.faces_where(|q| q[2].abs() < 1e-9) {
                m.face_pressure.push((ei, f, p));
            }
            let name = if clamped { "clamped" } else { "simply_supported" };
            let s = solve(&m, &dir.join("plate"), name)?;
            let w = mean_disp(&s, 2, |q| q[0].hypot(q[1]) < 1e-6 && (q[2] - t / 2.0).abs() < 1e-6);
            let w = if w == 0.0 { mean_disp(&s, 2, |q| q[0].hypot(q[1]) < 0.02 * a) } else { w };
            let sr = extreme_stress(&s, |q| q[0].hypot(q[1]) < 0.03 * a && (q[2] - t).abs() < 1e-6, |st, _| st[0]);
            if clamped {
                rows.push(row("clamped circular plate", "centre deflection, mm", w * 1e3, p * a.powi(4) / (64.0 * d) * 1e3, "Roark table 11.2 case 10b: p a^4 / 64 D", mesh_d.clone(), s.seconds));
                rows.push(row("clamped circular plate", "centre stress, MPa", sr.abs() / 1e6, 3.0 * (1.0 + nu) * p * a * a / (8.0 * t * t) / 1e6, "Roark 10b: 3 (1 + nu) p a^2 / 8 t^2", mesh_d.clone(), s.seconds));
                crate::render::contour(&s, "von_mises", &crate::render::Picture { title: "Validation: clamped plate, 0.1 MPa", note: "gmsh C3D20R + CalculiX", view: [30.0, -60.0], scale: None }, &dir.join("plate_clamped.png"))?;
            } else {
                rows.push(row("simply supported circular plate", "centre deflection, mm", w * 1e3, (5.0 + nu) * p * a.powi(4) / (64.0 * d * (1.0 + nu)) * 1e3, "Roark table 11.2 case 10a", mesh_d.clone(), s.seconds));
                rows.push(row("simply supported circular plate", "centre stress, MPa", sr.abs() / 1e6, 3.0 * (3.0 + nu) * p * a * a / (8.0 * t * t) / 1e6, "Roark 10a: 3 (3 + nu) p a^2 / 8 t^2", mesh_d.clone(), s.seconds));
            }
        }
    }

    // 5. axially compressed cylinder, linear buckling
    {
        let (r, t, l) = (0.05, 0.001, 0.12);
        let st = mesh::stations(0.0, l, 0.0025, &[]);
        let nc = 128;
        let tb = mesh::tube([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], r, &st, nc, |_| "SHELL".into());
        let mut m = Model::new(tb.mesh.clone(), Analysis::Buckle { factors: 3 });
        m.materials.push(al.clone());
        m.sections.push(Section::Shell { set: "SHELL".into(), material: "AL6061".into(), thickness: t });
        let last = tb.rows.len() - 1;
        // bottom clamped; top ring free to move axially only (radial and
        // rotations held: the classical clamped-clamped boundary)
        m.fix(&tb.nodes_in_rows(&[0]), 1, 6);
        for n in tb.nodes_in_rows(&[last]) {
            m.fixed.push((n, 1, 2));
            m.fixed.push((n, 4, 6));
        }
        let unit = 1000.0;
        m.forces = tb.ring_force(last, [0.0, 0.0, -unit]);
        let s = solve(&m, &dir.join("buckle"), "buckle")?;
        let sigma_cr = e * t / (r * (3.0 * (1.0 - nu * nu)).sqrt());
        let p_cr = sigma_cr * 2.0 * PI * r * t;
        rows.push(row("axially compressed cylinder", "buckling load, kN", s.buckling_factors.first().copied().unwrap_or(f64::NAN) * unit / 1e3, p_cr / 1e3, "classical E t / (R sqrt(3 (1 - nu^2))) (Timoshenko & Gere eq. 11-13)", format!("{} S8R ({} around)", tb.mesh.elements.len(), nc), s.seconds));
        crate::render::mode_animation(&s, 0, &crate::render::Picture { title: "Validation: cylinder buckling mode 1", note: "CalculiX *BUCKLE", view: [20.0, -60.0], scale: None }, &dir.join("buckle_mode1.mp4"), 24)?;
    }
    Ok(rows)
}

/// Internal pressure `p` on a closed-end S8R cylinder along z (bottom ring
/// held axially, two nodes stop rigid rotation/translation; the end-cap
/// force p pi ri^2 on the top ring).
pub fn pressure_cylinder(tb: &mesh::Tube, mat: &Material, t: f64, p: f64, ri: f64, dir: &Path) -> Result<Solved> {
    let mut m = Model::new(tb.mesh.clone(), Analysis::Static);
    m.materials.push(mat.clone());
    m.sections.push(Section::Shell { set: "SHELL".into(), material: mat.name.clone(), thickness: t });
    let r0 = &tb.rows[0];
    for &n in r0 {
        m.fixed.push((n, 3, 3));
    }
    // node at theta = 0 (on +x): stop y; node at theta = 90 deg: stop x
    let q = r0.len() / 4;
    m.fixed.push((r0[0], 2, 2));
    m.fixed.push((r0[q], 1, 1));
    let last = tb.rows.len() - 1;
    m.forces = tb.ring_force(last, [0.0, 0.0, p * PI * ri * ri]);
    // S8R normals point inward (mesh::tube); ccx pressure acts against the
    // normal, so internal pressure is negative here (checked by validation 2)
    m.shell_pressure.push(("SHELL".into(), -p));
    solve(&m, dir, &format!("pressure_{:.0}kPa", p / 1e3))
}

/// Edge supports for a gmsh disk (z from 0 to t, radius a): clamped = every
/// edge-face node fixed; simply supported = uz held on the mid-thickness
/// edge circle, with two nodes stopping in-plane rigid motion.
pub fn plate_supports(m: &mut Model, a: f64, t: f64, clamped: bool) {
    let on_edge = |q: V3| (q[0].hypot(q[1]) - a).abs() < 1e-6 * a.max(1.0) + 1e-7;
    if clamped {
        let ns = m.mesh.select(on_edge);
        m.fix(&ns, 1, 3);
    } else {
        let ns = m.mesh.select(|q| on_edge(q) && (q[2] - t / 2.0).abs() < 1e-6);
        m.fix(&ns, 3, 3);
        let near = |m: &Model, target: V3| ns.iter().copied().min_by(|&x, &y| mesh::norm(mesh::sub(m.mesh.node(x), target)).total_cmp(&mesh::norm(mesh::sub(m.mesh.node(y), target))));
        if let Some(n) = near(m, [a, 0.0, t / 2.0]) {
            m.fixed.push((n, 2, 2));
        }
        if let Some(n) = near(m, [0.0, a, t / 2.0]) {
            m.fixed.push((n, 1, 1));
        }
    }
}
