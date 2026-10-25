//! Student liquid rocket: each propellant tank's wall at MEOP and proof,
//! each bulkhead at proof, and a fin's first modes.
//!
//! Geometry from the design's own vehicle (`RocketDesign.vehicle`: tank
//! inner diameter, wall, bulkhead thickness, MEOP) and fin parameters;
//! materials are the rows wright-rocket uses: 6061-T6 (rules
//! `AL6061_YIELD_PA`, `AL6061_ULT_PA`, `AL6061_DENSITY`) and the G10 sheet
//! from the catalog row the design names (thickness), with the in-plane
//! shear modulus wright-rocket uses for flutter (`G10_SHEAR_PA`).
//!
//! * tank: a closed-end S8R cylinder (three diameters long, so the
//!   mid-section is free of end effects), internal pressure + end-cap force;
//!   hoop at the bore against Lame, von Mises against yield (proof) and
//!   ultimate (burst, by linearity at 2 x MEOP);
//! * bulkhead: a gmsh 20-node-brick disk of the bulkhead's radius and
//!   thickness, pressure on the tank face, held axially on the mid-thickness
//!   edge circle (the radial screws), against the finite-difference plate
//!   solution and Roark 11.2 case 10a it is checked and sized with;
//! * fin: an S8R trapezoid clamped at the root, first modes.

use super::Run;
use crate::deck::{Analysis, Elastic, Material, Model, Section};
use crate::mesh;
use crate::results::{solve, von_mises, Solved};
use crate::validate::{al6061, extreme_stress, hoop_z, mean_disp, plate_supports, pressure_cylinder};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;
use wright_rocket::domain::{flutter_speed, lame_hoop, Competition, G10_SHEAR_PA};
use wright_rocket::rules as rr;
use wright_rocket::vehicle::{plate_stress_fd, Tank};

/// G-10/FR4 in-plane Young's modulus. ASSUMPTION: 18.6 GPa (2.7 Msi), the
/// lengthwise flexural modulus typical of NEMA G-10 datasheets (e.g. Gund
/// "G-10/FR4" and matweb's G-10 entries list 2.7-2.8 Msi); no row in
/// data/rocket/catalog.json states one. In-plane shear is wright-rocket's
/// own 4 GPa (`G10_SHEAR_PA`), so torsion here matches its flutter model.
pub const G10_E_PA: f64 = 18.6e9;
/// G-10 in-plane Poisson's ratio. ASSUMPTION: 0.12 (woven E-glass/epoxy).
pub const G10_NU: f64 = 0.12;

fn g10() -> Material {
    Material {
        name: "G10".into(),
        elastic: Elastic::Ortho { e: [G10_E_PA, G10_E_PA, 0.6 * G10_E_PA], nu: [G10_NU, 0.3, 0.3], g: [G10_SHEAR_PA, 0.75 * G10_SHEAR_PA, 0.75 * G10_SHEAR_PA] },
        density: rr::FIBERGLASS_DENSITY,
        allowable_pa: f64::NAN,
        source: "G-10: E 18.6 GPa (assumed), G12 4 GPa (wright-rocket G10_SHEAR_PA), 1850 kg/m^3 (wright-rocket rules)".into(),
    }
}

/// Von Mises, ignoring nodes within `skip` of the support circle at radius a.
fn vm_off_support(s: &Solved, a: f64, skip: f64) -> (f64, f64) {
    let b = s.block("STRESS").expect("STRESS");
    let (mut away, mut all) = (0.0f64, 0.0f64);
    for (n, st) in &b.data {
        let v = von_mises(st);
        all = all.max(v);
        let p = s.frd.nodes[n];
        if (p[0].hypot(p[1]) - a).abs() > skip {
            away = away.max(v);
        }
    }
    (away, all)
}

pub fn run(design: &Value, dir: &Path, media: bool) -> Result<wright_core::Manifest> {
    let mut run = Run::new(dir, "FEA: rocket tanks, bulkheads and fin (CalculiX)");
    run.media = media;
    std::fs::create_dir_all(dir)?;
    let al = al6061();
    let sf = rr::FAR_BURST_SF_METAL; // 2.0; IREC's 6.17.4.1 is also 2.0
    let mut worst_wall = (f64::INFINITY, String::new());
    let mut worst_bh = (f64::INFINITY, String::new());

    for key in ["ox_tank", "fuel_tank"] {
        let tank: Tank = serde_json::from_value(design["vehicle"][key].clone()).with_context(|| format!("design.vehicle.{key}"))?;
        let slug = if key == "ox_tank" { "ox" } else { "fuel" };
        let (ri, ro) = (tank.inner_d / 2.0, tank.inner_d / 2.0 + tank.wall);
        let r = ri + tank.wall / 2.0;
        let l = (3.0 * tank.inner_d).min(tank.length.max(tank.inner_d));
        let nc = 64;
        let st = mesh::stations(0.0, l, 2.0 * std::f64::consts::PI * r / nc as f64 * 1.5, &[l / 2.0]);
        let tb = mesh::tube([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], r, &st, nc, |_| "SHELL".into());
        let mid = |q: [f64; 3]| (q[2] - l / 2.0).abs() < 1e-6;
        for (case, p) in [("MEOP", tank.meop), ("proof 1.5 x MEOP", 1.5 * tank.meop)] {
            let s = pressure_cylinder(&tb, &al, tank.wall, p, ri, &run.work(&format!("{slug}_tank")))?;
            let hoop = extreme_stress(&s, mid, hoop_z);
            let axial = extreme_stress(&s, mid, |st, _| st[2]);
            let vm = crate::validate::extreme_stress(&s, mid, |st, _| von_mises(st));
            run.cmp(&tank.name, case, "hoop stress at the bore, MPa", lame_hoop(p, ri, ro) / 1e6, hoop / 1e6, "closed form: Lame thick-wall (wright-rocket lame_hoop)");
            run.cmp(&tank.name, case, "axial stress, MPa", p * ri * ri / (ro * ro - ri * ri) / 1e6, axial / 1e6, "closed-end axial");
            run.cmp(&tank.name, case, "von Mises (mid-section), MPa", lame_hoop(p, ri, ro) / 1e6, vm / 1e6, "the check compares hoop (max principal) to yield; von Mises is the yield criterion");
            if case.starts_with("proof") {
                let margin_proof = rr::AL6061_YIELD_PA / vm - 1.0;
                // linear: burst at sf x MEOP = sf / 1.5 x proof
                let vm_burst = vm * sf / 1.5;
                let margin_burst = rr::AL6061_ULT_PA / vm_burst - 1.0;
                run.picture(&s, "von_mises", &format!("{slug}_tank_proof_von_mises.png"), &format!("{}: proof {:.1} bar, von Mises", tank.name, p / 1e5), &format!("closed-end 6061-T6 cylinder ID {:.1} mm wall {:.2} mm; {} S8R over {:.0} mm (3 diameters); burst {sf} x MEOP by linearity", tank.inner_d * 1e3, tank.wall * 1e3, tb.mesh.elements.len(), l * 1e3), [20.0, -60.0], &[("max_von_mises_mpa", vm / 1e6), ("hoop_mpa", hoop / 1e6), ("lame_hoop_mpa", lame_hoop(p, ri, ro) / 1e6), ("margin", margin_proof), ("burst_margin", margin_burst)])?;
                if margin_proof.min(margin_burst) < worst_wall.0 {
                    worst_wall = (margin_proof.min(margin_burst), format!("{}: von Mises {:.0} MPa at proof (Fty {:.0}), {:.0} MPa at {sf} x MEOP (Ftu {:.0})", tank.name, vm / 1e6, rr::AL6061_YIELD_PA / 1e6, vm_burst / 1e6, rr::AL6061_ULT_PA / 1e6));
                }
            }
        }

        // bulkhead at proof
        let pp = 1.5 * tank.meop;
        let (a, t) = (ri, tank.bulkhead_t);
        let pm = crate::gmsh::disk(&run.work(&format!("{slug}_bulkhead")), a, t, (a / 14.0).min(t), 4, "PLATE")?;
        let mut m = Model::new(pm.clone(), Analysis::Static);
        m.materials.push(al.clone());
        m.sections.push(Section::Solid { set: "PLATE".into(), material: "AL6061".into() });
        plate_supports(&mut m, a, t, false);
        for (ei, f, _) in pm.faces_where(|q| q[2].abs() < 1e-9) {
            m.face_pressure.push((ei, f, pp));
        }
        let s = solve(&m, &run.work(&format!("{slug}_bulkhead")), "proof")?;
        let nu = 0.33;
        let d = 68.9e9 * t.powi(3) / (12.0 * (1.0 - nu * nu));
        let centre = |q: [f64; 3]| q[0].hypot(q[1]) < 0.04 * a;
        let s_face = extreme_stress(&s, |q| centre(q) && (q[2] - t).abs() < 1e-6, |st, _| st[0]).abs();
        let w = mean_disp(&s, 2, |q| centre(q) && (q[2] - t / 2.0).abs() < 1e-6);
        let (vm_away, vm_all) = vm_off_support(&s, a, 0.5 * t);
        let fd = plate_stress_fd(a, t, pp);
        let roark = 3.0 * (3.0 + nu) * pp * a * a / (8.0 * t * t);
        run.cmp(&format!("{} bulkhead", tank.name), "proof 1.5 x MEOP", "centre bending stress (dry face), MPa", fd / 1e6, s_face / 1e6, &format!("closed form: wright-rocket plate_stress_fd (Roark 10a {:.0} MPa); a/t = {:.1}, so a thick plate", roark / 1e6, a / t));
        run.cmp(&format!("{} bulkhead", tank.name), "proof 1.5 x MEOP", "centre deflection, mm", (5.0 + nu) * pp * a.powi(4) / (64.0 * d * (1.0 + nu)) * 1e3, w * 1e3, "Roark 10a (Kirchhoff: no shear deformation)");
        run.cmp(&format!("{} bulkhead", tank.name), "proof 1.5 x MEOP", "max von Mises off the support line, MPa", fd / 1e6, vm_away / 1e6, &format!("peak at the idealised line support {:.0} MPa", vm_all / 1e6));
        let margin = rr::AL6061_YIELD_PA / vm_away - 1.0;
        run.picture(&s, "von_mises", &format!("{slug}_bulkhead_proof_von_mises.png"), &format!("{} bulkhead: proof {:.1} bar, von Mises", tank.name, pp / 1e5), &format!("6061-T6 disk r {:.1} mm t {:.1} mm on its screw circle; {} C3D20R (gmsh)", a * 1e3, t * 1e3, pm.elements.len()), [30.0, -60.0], &[("max_von_mises_mpa", vm_away / 1e6), ("peak_at_support_mpa", vm_all / 1e6), ("closed_form_mpa", fd / 1e6), ("centre_deflection_mm", w * 1e3), ("margin", margin)])?;
        if margin < worst_bh.0 {
            worst_bh = (margin, format!("{} bulkhead: von Mises {:.0} MPa at proof vs Fty {:.0} MPa", tank.name, vm_away / 1e6, rr::AL6061_YIELD_PA / 1e6));
        }
    }
    run.manifest.check("tank_wall", worst_wall.0 > 0.0, &format!("worst tank wall margin {:+.2}: {}", worst_wall.0, worst_wall.1));
    run.manifest.check("bulkhead", worst_bh.0 > 0.0, &format!("worst bulkhead margin {:+.2}: {}", worst_bh.0, worst_bh.1));

    // fin modes
    let pr = &design["params"];
    let (root, tip, span, sweep) = (pr["fin_root"].as_f64(), pr["fin_tip"].as_f64(), pr["fin_span"].as_f64(), pr["fin_sweep"].as_f64().or(Some(0.0)));
    if let (Some(root), Some(tip), Some(span), Some(sweep)) = (root, tip, span, sweep) {
        let cat = wright_rocket::catalog::Catalog::load()?;
        let fin_id = pr["fin_material"].as_str().unwrap_or("");
        let t = cat.get(fin_id).and_then(|r| r.f("thickness_mm")).with_context(|| format!("fin material {fin_id} has no thickness"))? * 1e-3;
        let (fm, _) = mesh::quad_plate([[0.0, 0.0, 0.0], [root, 0.0, 0.0], [sweep + tip, span, 0.0], [sweep, span, 0.0]], 16, 8, "FIN");
        let mut m = Model::new(fm.clone(), Analysis::Frequency { modes: 4 });
        m.materials.push(g10());
        m.sections.push(Section::Shell { set: "FIN".into(), material: "G10".into(), thickness: t });
        let rootn = fm.select(|q| q[1].abs() < 1e-9);
        m.fix(&rootn, 1, 6);
        let s = solve(&m, &run.work("fin"), "modes")?;
        let f = &s.frequencies_hz;
        let (f1, f2) = (f.first().copied().unwrap_or(f64::NAN), f.get(1).copied().unwrap_or(f64::NAN));
        // the closed-form flutter speed the check uses, for context
        let comp: Competition = serde_json::from_value(design["competition"].clone()).unwrap_or_default();
        let lim = comp.limits();
        let (max_q, alt) = (design["nominal"]["max_q"].as_f64().unwrap_or(f64::NAN), design["nominal"]["max_q_alt_agl"].as_f64().unwrap_or(0.0));
        let air = wright_rocket::atmosphere::at(lim.elevation_m + alt);
        let vq = (2.0 * max_q / air.rho).sqrt();
        let vf = flutter_speed(air.a, air.p, G10_SHEAR_PA, span, root, tip, t);
        run.cmp("fin", "modes", "first (bending) mode, Hz", f64::NAN, f1, "no closed-form mode in wright-rocket; TN 4197 uses G and t/c directly");
        run.cmp("fin", "modes", "second (torsion) mode, Hz", f64::NAN, f2, "");
        let label = format!("Fin first mode {f1:.0} Hz, second {f2:.0} Hz");
        run.film(&s, 0, "fin_mode1.mp4", &label, &format!("G10 {:.2} mm, root {:.0} tip {:.0} span {:.0} mm, clamped at the root; TN 4197 flutter {vf:.0} m/s vs {vq:.0} m/s at max-Q", t * 1e3, root * 1e3, tip * 1e3, span * 1e3), [35.0, -50.0], &[("f1_hz", f1), ("f2_hz", f2), ("tn4197_flutter_mps", vf), ("max_q_speed_mps", vq), ("margin", vf / (1.2 * vq) - 1.0)])?;
        run.film(&s, 1, "fin_mode2.mp4", &format!("Fin second mode {f2:.0} Hz"), "the torsion-dominated mode that flutter couples with bending", [35.0, -50.0], &[("f2_hz", f2)])?;
        run.manifest.check("fin_modes", f1.is_finite() && f2 > f1, &format!("fin modes {f1:.0} / {f2:.0} Hz (torsion/bending ratio {:.2}); TN 4197 flutter {vf:.0} m/s vs 1.2 x {vq:.0} m/s at max-Q", f2 / f1));
    }
    run.finish()
}
