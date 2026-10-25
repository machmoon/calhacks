//! Validation of the two solvers as this crate drives them, against
//! published data:
//!
//! * AVL on the Warren-12 wing (LE sweep 53.54 deg, root chord 1.5, tip 0.5,
//!   semispan sqrt 2, flat): CL_alpha 2.743 /rad, Cm_alpha -3.10 /rad, the
//!   values OpenVSP's own VSPAERO verification uses
//!   (`examples/scripts/python_scripts/WarrenTest.py`,
//!   `m_Cl_alpha_expected`, `m_Cm_alpha_expected`; from the NASA SP-405
//!   vortex-lattice workshop, 1976).
//! * AVL on an elliptic flat wing: lifting-line exact CL_alpha =
//!   2 pi AR / (AR + 2) and span efficiency 1 (Prandtl).
//! * SU2 on sharp cones against the exact Taylor-Maccoll solution
//!   ([`crate::taylor_maccoll`]).
//! * SU2 on a fineness-3 von Karman nose against NASA TR-R-100's
//!   free-flight pressure drag ([`crate::rocket::TR_R_100_VK_FR3`]).

use crate::avl::{self, Camber, Geometry, Section, Surface};
use crate::render::{self, Canvas, Series};
use crate::rocket::{self, Nose, NoseShape};
use crate::taylor_maccoll;
use crate::Manifest;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub case: String,
    pub quantity: String,
    pub reference: f64,
    pub computed: f64,
    pub error_pct: f64,
    pub source: String,
}

fn line(case: &str, q: &str, reference: f64, computed: f64, source: &str) -> Line {
    Line { case: case.into(), quantity: q.into(), reference, computed, error_pct: 100.0 * (computed - reference) / reference.abs().max(1e-12), source: source.into() }
}

fn flat(xle: f64, y: f64, c: f64) -> Section {
    Section { xle, yle: y, zle: 0.0, chord: c, ainc_deg: 0.0, camber: Camber::Flat, claf: None }
}

pub fn warren12() -> Geometry {
    let s = 2f64.sqrt();
    Geometry {
        title: "Warren-12".into(),
        mach: 0.0,
        sref: 2.0 * s,
        cref: 1.0,
        bref: 2.0 * s,
        xref: [0.0; 3],
        cdp: 0.0,
        surfaces: vec![Surface {
            name: "Wing".into(),
            n_chord: 12,
            c_space: 1.0,
            n_span: 24,
            s_space: -2.0,
            y_duplicate: Some(0.0),
            angle_deg: 0.0,
            sections: vec![flat(0.0, 0.0, 1.5), flat(53.54f64.to_radians().tan() * s, s, 0.5)],
        }],
        bodies: vec![],
    }
}

pub fn elliptic(ar: f64) -> Geometry {
    let b = ar; // with root chord c0: S = pi b c0 / 4 -> c0 = 4 b / (pi AR)
    let c0 = 4.0 * b / (std::f64::consts::PI * ar);
    let n = 24;
    let sections = (0..=n)
        .map(|k| {
            let th = std::f64::consts::FRAC_PI_2 * k as f64 / n as f64;
            let y = b / 2.0 * th.sin();
            let c = (c0 * th.cos()).max(c0 * 0.01);
            flat(-c / 4.0, y, c)
        })
        .collect();
    Geometry {
        title: "Elliptic".into(),
        mach: 0.0,
        sref: std::f64::consts::PI * b * c0 / 4.0,
        cref: c0,
        bref: b,
        xref: [0.0; 3],
        cdp: 0.0,
        surfaces: vec![Surface { name: "Wing".into(), n_chord: 8, c_space: 1.0, n_span: 48, s_space: 1.0, y_duplicate: Some(0.0), angle_deg: 0.0, sections }],
        bodies: vec![],
    }
}

pub fn run_avl(dir: &Path) -> anyhow::Result<Vec<Line>> {
    let mut out = vec![];
    let w = avl::run_sweep(&warren12(), &dir.join("warren12"), "warren12", &[0.0, 1.0, 2.0], false)?;
    let r = &w.rows[1];
    let src = "OpenVSP examples/scripts/python_scripts/WarrenTest.py (NASA SP-405)";
    out.push(line("Warren-12 (AVL)", "CL_alpha /rad", 2.743, r.CLa.unwrap_or(f64::NAN), src));
    out.push(line("Warren-12 (AVL)", "Cm_alpha /rad (about apex)", -3.10, r.Cma.unwrap_or(f64::NAN), src));
    for ar in [6.0, 10.0] {
        let e = avl::run_sweep(&elliptic(ar), &dir.join("elliptic"), &format!("elliptic{ar}"), &[0.0, 2.0, 4.0], false)?;
        let r = &e.rows[1];
        let src = "Prandtl lifting line, elliptic loading: 2 pi AR/(AR+2), e = 1";
        out.push(line(&format!("elliptic AR {ar} (AVL)"), "CL_alpha /rad", 2.0 * std::f64::consts::PI * ar / (ar + 2.0), r.CLa.unwrap_or(f64::NAN), src));
        out.push(line(&format!("elliptic AR {ar} (AVL)"), "span efficiency e", 1.0, e.rows[2].e.unwrap_or(f64::NAN), src));
    }
    Ok(out)
}

pub struct SuValidation {
    pub lines: Vec<Line>,
    pub vk: Vec<(f64, f64, f64)>,
    pub cone: Vec<(f64, f64, f64)>,
}

pub fn run_su2(dir: &Path, man: &mut Manifest, threads: usize) -> anyhow::Result<SuValidation> {
    let mut lines = vec![];
    let mut cone = vec![];
    let cone_nose = Nose { shape: NoseShape::Cone, length: 0.1 / 15f64.to_radians().tan(), radius: 0.1 };
    let machs = [1.5, 2.0, 3.0];
    let (pts, frames) = rocket::nose_sweep(&cone_nose, &machs, &dir.join("val_cone15"), "cone_15deg", threads)?;
    for p in &pts {
        let tm = taylor_maccoll::cone(p.mach, 15.0, 1.4).map(|c| c.cp).unwrap_or(f64::NAN);
        lines.push(line(&format!("15 deg cone, M {:.1} (SU2)", p.mach), "surface Cp = CD_pressure", tm, p.cd_pressure, "Taylor-Maccoll exact (NACA 1135 charts 5-6)"));
        cone.push((p.mach, tm, p.cd_pressure));
    }
    if let Some(f) = frames.get(1) {
        let png = dir.join("validation_cone_M2.png");
        f.save_png(&png)?;
        man.add(dir, &png, "Validation: 15 deg cone at M 2 (SU2)", "Attached conical shock; surface pressure vs Taylor-Maccoll", &[]);
    }
    let vk_nose = Nose { shape: NoseShape::VonKarman, length: 0.3, radius: 0.05 };
    let vm = [1.1, 1.2, 1.4, 1.6, 2.0, 3.0];
    let (pts, frames) = rocket::nose_sweep(&vk_nose, &vm, &dir.join("val_vk3"), "von_karman_FR3", threads)?;
    let mut vk = vec![];
    for p in &pts {
        let r = rocket::interp(&rocket::TR_R_100_VK_FR3, p.mach);
        lines.push(line(&format!("von Karman FR 3, M {:.1} (SU2)", p.mach), "nose pressure CD", r, p.cd_pressure, "NASA TR-R-100 p. 16 (free flight), via OpenRocket SymmetricComponentCalc.java"));
        vk.push((p.mach, r, p.cd_pressure));
    }
    if let Some(f) = frames.get(2) {
        let png = dir.join("validation_vk3_M1.4.png");
        f.save_png(&png)?;
        man.add(dir, &png, "Validation: FR 3 von Karman nose at M 1.4 (SU2)", "The NASA TR-R-100 free-flight shape", &[]);
    }
    let mut cv = Canvas::new(1280, 560, render::BG);
    render::chart(&mut cv, 10.0, 10.0, 620.0, 540.0, "15 deg cone: SU2 vs Taylor-Maccoll", "Mach", "Cp on the cone", &[
        Series::line("Taylor-Maccoll (exact)", cone.iter().map(|p| (p.0, p.1)).collect(), render::SERIES[1]),
        Series::points("SU2 Euler", cone.iter().map(|p| (p.0, p.2)).collect(), render::SERIES[0]),
    ], None);
    render::chart(&mut cv, 650.0, 10.0, 620.0, 540.0, "FR 3 von Karman: SU2 vs NASA TR-R-100", "Mach", "nose pressure CD", &[
        Series::line("TR-R-100 free flight", vk.iter().map(|p| (p.0, p.1)).collect(), render::SERIES[1]),
        Series::points("SU2 Euler", vk.iter().map(|p| (p.0, p.2)).collect(), render::SERIES[0]),
    ], Some((0.0, 0.14)));
    let png = dir.join("validation_su2.png");
    cv.save_png(&png)?;
    man.add(dir, &png, "Validation: SU2 against exact and flight data", "Cone surface pressure vs Taylor-Maccoll; von Karman FR 3 nose drag vs NASA TR-R-100", &[]);
    Ok(SuValidation { lines, vk, cone })
}

pub fn write(dir: &Path, lines: &[Line], man: &mut Manifest) -> anyhow::Result<()> {
    let json = dir.join("validation.json");
    std::fs::write(&json, serde_json::to_vec_pretty(lines)?)?;
    let worst = lines.iter().map(|l| l.error_pct.abs()).fold(0.0, f64::max);
    man.add(dir, &json, "Validation table", &format!("{} comparisons with published or exact data; worst {:.1} %", lines.len(), worst), &[("cases", lines.len() as f64), ("worst_error_pct", worst)]);
    Ok(())
}
