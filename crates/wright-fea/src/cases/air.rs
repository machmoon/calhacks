//! VTOL quadplane: the lift-motor boom and the wing spar.
//!
//! Geometry and loads come from wright-air's own evaluation of the design
//! (`checks::evaluate`: layout, hover, structure), so FEA and the
//! closed-form checks see the same tube rows, clamps, motor stations and
//! load factors:
//!
//! * boom: the whole tube (boom_x0..boom_x1) as an S8R shell, held at the
//!   wing clamps (10 % and 75 % chord, as `perf::structure` assumes), lift
//!   motor + prop + mount masses at both motor stations; static at the
//!   landing load (14 CFR 23.473: 2.67 g x 1.5 on two legs), full thrust
//!   x 1.5, and the tail load; first modes vs the hover rotation rate;
//!   linear buckling at the landing load with NASA SP-8007's bending
//!   knock-down;
//! * spar: the main spar outboard of the joiner and the joiner itself, as
//!   S8R cantilevers under Schrenk's lift distribution (the same
//!   distribution `perf::schrenk_moment` integrates) at 4 g x 1.5.

use super::Run;
use crate::deck::{Analysis, Material, Model, Section};
use crate::mesh::{self, Tube};
use crate::results::{solve, von_mises, Solved};
use anyhow::{bail, Result};
use std::f64::consts::PI;
use std::path::Path;
use wright_air::catalog::{Tube as TubeRow, TubeKind};
use wright_air::powertrain::Factors;
use wright_air::rules::{self as ar, CF_PULTRUDED, CF_ROLLED};

/// Poisson's ratio for the carbon tubes. ASSUMPTION: 0.3 (quasi-isotropic
/// laminate; the catalog rows give E only). Bending is insensitive to it.
pub const CF_NU: f64 = 0.3;

/// NASA SP-8007 (Buckling of Thin-Walled Circular Cylinders, 1968; same in
/// SP-8007-2020 rev. 2) bending knock-down: gamma = 1 - 0.731 (1 - e^-phi),
/// phi = sqrt(R/t) / 16.
pub fn sp8007_bending(r: f64, t: f64) -> f64 {
    1.0 - 0.731 * (1.0 - (-(r / t).sqrt() / 16.0).exp())
}

fn material(t: &TubeRow) -> (Material, &'static str) {
    let m = match t.kind {
        TubeKind::Pultruded => CF_PULTRUDED,
        TubeKind::RollWrapped => CF_ROLLED,
    };
    let (ro, ri) = (t.od.as_m() / 2.0, t.id.map(|l| l.as_m() / 2.0).unwrap_or(0.0));
    // density from the row's weighed kg/m, so the modes carry the real mass
    let rho = t.kg_per_m / (PI * (ro * ro - ri * ri));
    (Material::iso("CF", m.modulus_pa, CF_NU, rho, m.allowable_pa, m.source), m.name)
}

/// (mid-surface radius, wall) of a tube row; a solid rod has no shell model.
fn shell_of(t: &TubeRow) -> Result<(f64, f64)> {
    let Some(id) = t.id else { bail!("{} is a solid rod: no shell model", t.row.id) };
    let (od, id) = (t.od.as_m(), id.as_m());
    Ok(((od + id) / 4.0, (od - id) / 2.0))
}

fn model(tb: &Tube, mat: &Material, t: f64, an: Analysis) -> Model {
    let mut m = Model::new(tb.mesh.clone(), an);
    m.materials.push(mat.clone());
    m.sections.push(Section::Shell { set: "TUBE".into(), material: mat.name.clone(), thickness: t });
    m
}

/// Largest von Mises over nodes farther than `skip` from every point in
/// `clamps` along the axis coordinate `axis` (St. Venant: away from the
/// idealised rigid clamp line), and over all nodes.
fn vm_away(s: &Solved, axis: usize, clamps: &[f64], skip: f64) -> (f64, f64) {
    let b = s.block("STRESS").expect("STRESS");
    let (mut away, mut all) = (0.0f64, 0.0f64);
    for (n, st) in &b.data {
        let v = von_mises(st);
        all = all.max(v);
        let x = s.frd.nodes[n][axis];
        if clamps.iter().all(|c| (x - c).abs() > skip) {
            away = away.max(v);
        }
    }
    (away, all)
}

pub fn run(d: &wright_air::spec::Design, dir: &Path, media: bool) -> Result<wright_core::Manifest> {
    let e = wright_air::checks::evaluate(d, Factors::NOMINAL).map_err(anyhow::Error::msg)?;
    let lay = &e.layout;
    let p = &lay.parts;
    let mut run = Run::new(dir, "FEA: VTOL boom and wing spar (CalculiX)");
    run.media = media;
    std::fs::create_dir_all(dir)?;

    // ---- boom -------------------------------------------------------------
    let (r, t) = shell_of(p.boom)?;
    let (mat, mat_name) = material(p.boom);
    let c_boom = lay.chord_at(lay.boom_y).as_m();
    let clamps = [0.10 * c_boom, 0.75 * c_boom];
    let (x0, x1) = (lay.boom_x0.as_m(), lay.boom_x1.as_m());
    let (xf, xr) = (lay.x_front.as_m(), lay.x_rear.as_m());
    let x_tail = lay.x_ac_tail.as_m().clamp(x0, x1);
    let h = (2.0 * PI * r / 24.0).max(0.002) * 1.5;
    let st = mesh::stations(x0, x1, h, &[clamps[0], clamps[1], xf, xr, x_tail]);
    let origin = [0.0, lay.boom_y.as_m(), lay.boom_z.as_m()];
    let tb = mesh::tube(origin, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], r, &st, 24, |_| "TUBE".into());
    let clamp_rows = [tb.station_row(clamps[0]), tb.station_row(clamps[1])];
    let tip_kg = p.lift_motor.mass.as_kg() + p.lift_prop.mass.as_kg() + ar::MOTOR_MOUNT_KG;
    let (rf, rr) = (tb.station_row(xf), tb.station_row(xr));
    let boom_model = |an: Analysis| {
        let mut m = model(&tb, &mat, t, an);
        m.fix(&tb.nodes_in_rows(&clamp_rows), 1, 6);
        m.masses.extend(tb.ring_mass(rf, tip_kg));
        m.masses.extend(tb.ring_mass(rr, tip_kg));
        m
    };
    let mesh_note = format!("{} S8R, OD {:.1} mm wall {:.2} mm {} ({})", tb.mesh.elements.len(), p.boom.od.as_mm(), t * 1e3, p.boom.row.id, mat_name);
    let over_front = (clamps[0] - xf).max(0.0);
    let over_rear = (xr - clamps[1]).max(0.0);
    let (leg_row, overhang) = if over_front >= over_rear { (rf, over_front) } else { (rr, over_rear) };
    let w = lay.weight_n();
    let p_leg = ar::LANDING_LOAD_FACTOR * ar::ULTIMATE_FACTOR * w / ar::LANDING_LEGS_LOADED;
    let t_ult = e.perf.hover.max_thrust_each.value() * ar::ULTIMATE_FACTOR;
    let closed = |case: &str| e.structure.members.iter().find(|m| m.name == "boom" && m.load_case.starts_with(case)).map(|m| m.stress_pa).unwrap_or(f64::NAN);
    let ro = p.boom.od.as_m() / 2.0;
    let i_sec = PI * (p.boom.od.as_m().powi(4) - p.boom.id.map(|l| l.as_m()).unwrap_or(0.0).powi(4)) / 64.0;
    // tail load per boom, back out of the closed-form stress (M c / I over its arm)
    let tail_arm = (lay.x_ac_tail.as_m() - clamps[1]).max(1e-6);
    let tail_n = closed("tail") * i_sec / ro / tail_arm;
    let skip = 2.0 * p.boom.od.as_m();

    let mut worst = (0.0f64, String::new());
    let landing = {
        let mut m = boom_model(Analysis::Static);
        m.masses.clear();
        m.forces = tb.ring_force(leg_row, [0.0, 0.0, p_leg]);
        solve(&m, &run.work("boom"), "landing")?
    };
    for (case, label, s_opt) in [
        ("landing", "landing 2.67 g x 1.5", Some(&landing)),
        ("full thrust", "full thrust x 1.5", None),
        ("tail", "tail at CLmax, top speed x 1.5", None),
    ] {
        let owned;
        let s = match s_opt {
            Some(s) => s,
            None => {
                let mut m = boom_model(Analysis::Static);
                m.masses.clear();
                m.forces = if case == "full thrust" {
                    [tb.ring_force(rf, [0.0, 0.0, t_ult]), tb.ring_force(rr, [0.0, 0.0, t_ult])].concat()
                } else {
                    tb.ring_force(tb.station_row(x_tail), [0.0, 0.0, tail_n])
                };
                owned = solve(&m, &run.work("boom"), &case.replace(' ', "_"))?;
                &owned
            }
        };
        let (away, all) = vm_away(s, 0, &clamps, skip);
        let cf = closed(case);
        run.cmp("boom", label, "max von Mises, MPa", cf / 1e6, all / 1e6, &format!("peak at the clamp (rigid clamp line); {:.0} MPa beyond 2 OD from a clamp", away / 1e6));
        if all > worst.0 {
            worst = (all, label.to_string());
        }
        if case == "landing" {
            let margin = mat.allowable_pa / all - 1.0;
            run.cmp("boom", label, "tip deflection, mm", f64::NAN, s.max_displacement() * 1e3, "");
            run.picture(s, "von_mises", "boom_landing_von_mises.png", "Boom: landing load (2.67 g x 1.5), von Mises", &format!("{p_leg:.0} N on the leg under the motor {:.0} mm from the clamp; {mesh_note}", overhang * 1e3), [12.0, -75.0], &[("max_von_mises_mpa", all / 1e6), ("beyond_2od_from_clamp_mpa", away / 1e6), ("closed_form_mpa", cf / 1e6), ("allowable_mpa", mat.allowable_pa / 1e6), ("margin", margin)])?;
            run.picture(s, "disp_mm", "boom_landing_displacement.png", "Boom: landing load, displacement", "true displacement magnitude, shape exaggerated", [0.0, -90.0], &[("max_displacement_mm", s.max_displacement() * 1e3)])?;
        }
    }
    let margin = mat.allowable_pa / worst.0 - 1.0;
    run.manifest.check("boom_stress", margin > 0.0, &format!("worst boom von Mises {:.0} MPa ({}) vs {:.0} MPa allowable ({mat_name}): margin {:+.2}", worst.0 / 1e6, worst.1, mat.allowable_pa / 1e6, margin));

    // modes
    let modes = solve(&boom_model(Analysis::Frequency { modes: 6 }), &run.work("boom"), "modes")?;
    let f1 = modes.frequencies_hz.first().copied().unwrap_or(f64::NAN);
    let rot = e.structure.hover_rotation_hz;
    let sep = (f1 / rot - 1.0).abs() - ar::BOOM_FREQUENCY_SEPARATION;
    run.cmp("boom", "first mode", "frequency, Hz", e.structure.boom_frequency_hz, f1, "closed form: tip mass on the longer overhang only; FEA: the whole boom with both motors and both clamps");
    run.film(&modes, 0, "boom_mode1.mp4", &format!("Boom first mode {f1:.0} Hz (hover rotation {rot:.0} Hz)"), &format!("first bending mode of the whole boom with both lift motors; separation from hover rotation {:+.0} % (need 30 %)", 100.0 * (f1 / rot - 1.0)), [12.0, -75.0], &[("f1_hz", f1), ("f2_hz", modes.frequencies_hz.get(1).copied().unwrap_or(f64::NAN)), ("hover_rotation_hz", rot), ("closed_form_hz", e.structure.boom_frequency_hz), ("margin", sep)])?;
    run.manifest.check("boom_resonance", sep > 0.0, &format!("boom first mode {f1:.0} Hz vs hover rotation {rot:.0} Hz: {:.0} % apart (need {:.0} %)", 100.0 * (f1 / rot - 1.0).abs(), 100.0 * ar::BOOM_FREQUENCY_SEPARATION));

    // buckling at the landing load
    let mut bm = boom_model(Analysis::Buckle { factors: 2 });
    bm.masses.clear();
    bm.forces = tb.ring_force(leg_row, [0.0, 0.0, p_leg]);
    let bk = solve(&bm, &run.work("boom"), "buckle")?;
    let lam = bk.buckling_factors.first().copied().unwrap_or(f64::NAN);
    let gamma = sp8007_bending(r, t);
    run.cmp("boom", "landing 2.67 g x 1.5", "linear buckling factor x SP-8007 knock-down", f64::NAN, lam * gamma, &format!("raw factor {lam:.1}, knock-down {gamma:.2}"));
    run.numbers("boom_buckling", "Boom: buckling at the landing load", &format!("first linear buckling factor {lam:.1} x NASA SP-8007 bending knock-down {gamma:.2} = {:.1}", lam * gamma), &[("buckling_factor", lam), ("knockdown", gamma), ("margin", lam * gamma - 1.0)])?;
    run.manifest.check("boom_buckling", lam * gamma > 1.0, &format!("boom buckles at {:.1} x the ultimate landing load (after SP-8007 knock-down {gamma:.2})", lam * gamma));

    // ---- spar -------------------------------------------------------------
    let semi = lay.semi_span().as_m();
    let lam_t = lay.taper;
    let n_ult = ar::LIMIT_LOAD_FACTOR * ar::ULTIMATE_FACTOR;
    let lift_half = n_ult * w / 2.0;
    // Schrenk: mean of elliptic and chord-proportional unit distributions
    let wy = |y: f64| {
        let ell = 4.0 / (PI * semi) * (1.0 - (y / semi).powi(2)).max(0.0).sqrt();
        let trap = 2.0 * (1.0 - (1.0 - lam_t) * y / semi) / (semi * (1.0 + lam_t));
        lift_half * 0.5 * (ell + trap)
    };
    // resultant force and moment about y0 of the lift on [a, semi]
    let beyond = |a: f64, y0: f64| {
        let n = 400;
        let (mut f, mut mo) = (0.0, 0.0);
        for i in 0..n {
            let y = a + (semi - a) * (i as f64 + 0.5) / n as f64;
            let df = wy(y) * (semi - a) / n as f64;
            f += df;
            mo += df * (y - y0);
        }
        (f, mo)
    };
    let y_j = ar::JOINER_HALF_FRAC * semi;
    let y_r = ar::MAIN_SPAR_REACH * semi;
    let mut spar_worst = (0.0f64, String::new(), 0.0f64);
    for (part, row, a, b) in [("joiner", p.joiner, 0.0, y_j), ("main_spar", p.main_spar, y_j, y_r)] {
        let (r, t) = shell_of(row)?;
        let (mat, mname) = material(row);
        let st = mesh::stations(a, b, (2.0 * PI * r / 24.0).max(0.002) * 1.5, &[]);
        // along +y; section axes x (chord) and z (lift)
        let tb = mesh::tube([lay.main_spar_x.as_m(), 0.0, lay.main_spar_z.as_m()], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r, &st, 24, |_| "TUBE".into());
        let mut m = model(&tb, &mat, t, Analysis::Static);
        m.fix(&tb.nodes_in_rows(&[0]), 1, 6);
        // lift on [a, b] lumped to each station row, plus everything outboard of b at the tip
        for k in (0..tb.rows.len()).step_by(2) {
            let y = tb.row_s[k];
            let lo = if k == 0 { y } else { 0.5 * (tb.row_s[k - 2] + y) };
            let hi = if k + 2 >= tb.rows.len() { y } else { 0.5 * (tb.row_s[k + 2] + y) };
            if hi > lo {
                m.forces.extend(tb.ring_force(k, [0.0, 0.0, wy(y) * (hi - lo)]));
            }
        }
        let last = tb.rows.len() - 1;
        let (f_out, m_out) = beyond(b, b);
        m.forces.extend(tb.ring_force(last, [0.0, 0.0, f_out]));
        m.forces.extend(tb.ring_moment(last, m_out, [1.0, 0.0, 0.0]));
        let s = solve(&m, &run.work("spar"), part)?;
        let (away, all) = vm_away(&s, 1, &[a], 2.0 * row.od.as_m());
        // closed form at the clamp; FEA just outboard of the clamp zone,
        // compared with the closed-form moment there
        let ro = row.od.as_m() / 2.0;
        let i_sec = PI * (row.od.as_m().powi(4) - row.id.map(|l| l.as_m()).unwrap_or(0.0).powi(4)) / 64.0;
        let cf_root = e.structure.members.iter().find(|x| x.name == part && x.load_case.starts_with("manoeuvre")).map(|x| x.stress_pa).unwrap_or(f64::NAN);
        let y_s = a + 2.0 * row.od.as_m();
        let (_, m_s) = beyond(y_s, y_s);
        let cf_s = m_s * ro / i_sec;
        let fea_s = crate::validate::extreme_stress(&s, |q| (q[1] - tb.row_s[tb.station_row(y_s)]).abs() < 1e-6, |st, _| st[1]).abs();
        let label = if part == "joiner" { "Joiner" } else { "Main spar" };
        run.cmp(part, "manoeuvre 4 g x 1.5", &format!("bending stress {:.0} mm from its root, MPa", (tb.row_s[tb.station_row(y_s)] - a) * 1e3), cf_s / 1e6, fea_s / 1e6, "closed form: Schrenk moment x c / I at the same station");
        run.cmp(part, "manoeuvre 4 g x 1.5", "max von Mises (at the root), MPa", cf_root / 1e6, all / 1e6, "FEA includes the rigid-clamp peak; closed form is M c / I");
        let margin = mat.allowable_pa / all - 1.0;
        let file = format!("{part}_manoeuvre_von_mises.png");
        run.picture(&s, "von_mises", &file, &format!("{label}: 4 g x 1.5 manoeuvre, von Mises"), &format!("Schrenk lift on {:.0}-{:.0} mm of the half span (+ outboard lift at the tip); {} S8R, {} ({mname})", a * 1e3, b * 1e3, tb.mesh.elements.len(), row.row.id), [15.0, -30.0], &[("max_von_mises_mpa", all / 1e6), ("beyond_2od_from_root_mpa", away / 1e6), ("closed_form_root_mpa", cf_root / 1e6), ("allowable_mpa", mat.allowable_pa / 1e6), ("margin", margin), ("tip_deflection_mm", s.max_displacement() * 1e3)])?;
        if all / mat.allowable_pa > spar_worst.2 {
            spar_worst = (all, format!("{label} ({mname})"), all / mat.allowable_pa);
        }
    }
    run.manifest.check("spar_stress", spar_worst.2 < 1.0, &format!("{} von Mises {:.0} MPa at 4 g x 1.5: {:.0} % of allowable", spar_worst.1, spar_worst.0 / 1e6, 100.0 * spar_worst.2));
    run.finish()
}
