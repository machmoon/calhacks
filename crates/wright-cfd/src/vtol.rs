//! The VTOL quadplane in AVL: wing + H-tail + twin fins + pod, from the
//! one geometric model in `wright_air::layout` (the same numbers its CAD
//! and checks read).
//!
//! What AVL is given, and why:
//! * wing sections carry the design's real airfoil (camber from
//!   `Airfoil::surfaces_at`, AVL `AIRFOIL` block), incidence and washout
//!   (`Ainc`), dihedral (`Zle`);
//! * `CLAF` = the NeuralFoil section lift slope at the cruise Reynolds number
//!   over 2 pi (AVL's documented way to give a viscous section slope:
//!   avl_doc.txt "CLAF"), so the 3D answer is not credited with a thin-
//!   airfoil 2 pi section the low-Re section does not have;
//! * the pod as an AVL slender BODY (source line), which carries the pod's
//!   destabilising moment;
//! * booms, motors and props are left out: they are drag, not lift, and the
//!   drag build-up keeps them.
//!
//! What comes back is compared with `wright_air`'s analytic models at the
//! same points: the wing's lift slope (`aero::WingModel`), induced drag
//! (`aero::oswald`, Nita and Scholz), and the neutral point
//! (`stability::neutral_point`, Etkin & Reid + Raymer).

use crate::avl::{self, Camber, Geometry, Section, Surface, Sweep};
use crate::render::{self, Canvas, Cmap, Series};
use crate::Manifest;
use anyhow::Context;
use serde::Serialize;
use std::path::Path;
use wright_air::aero;
use wright_air::atmosphere;
use wright_air::layout::{self, Layout};
use wright_air::powertrain::Factors;
use wright_air::spec::Design;
use wright_core::units::{Length, Speed};

fn tail_camber(name: &str) -> Camber {
    let n = name.trim_start_matches("naca").trim_start_matches("NACA");
    if n.len() == 4 && n.chars().all(|c| c.is_ascii_digit()) {
        Camber::Naca(n.to_string())
    } else {
        Camber::Naca("0009".into())
    }
}

/// The AVL model of a laid-out VTOL. `full` adds tail and pod; otherwise the
/// wing alone (for the lift-slope and induced-drag comparison).
pub fn geometry(lay: &Layout, v_cruise: f64, full: bool) -> Geometry {
    let w = &lay.design.wing;
    let air = atmosphere::isa(Length::m(lay.design.requirements.altitude_m), 0.0);
    let af = lay.parts.wing_airfoil;
    let re = air.reynolds(Speed::mps(v_cruise), lay.mac);
    let claf = af.lift_slope_per_rad(re) / (2.0 * std::f64::consts::PI);
    let coords = avl::coords_from(40, |x| af.surfaces_at(x));
    let half = lay.span.as_m() / 2.0;
    let dz = half * w.dihedral_deg.to_radians().tan();
    let sec = |y: f64, z: f64, c: f64, ainc: f64| Section { xle: 0.0, yle: y, zle: z, chord: c, ainc_deg: ainc, camber: Camber::Coords(coords.clone()), claf: Some(claf) };
    let mut surfaces = vec![Surface {
        name: "Wing".into(),
        n_chord: 10,
        c_space: 1.0,
        n_span: 18,
        s_space: 1.0,
        y_duplicate: Some(0.0),
        angle_deg: 0.0,
        sections: vec![
            sec(0.0, 0.0, lay.root_chord.as_m(), w.incidence_deg),
            sec(half, dz, lay.tip_chord.as_m(), w.incidence_deg - w.washout_deg),
        ],
    }];
    let mut bodies = vec![];
    if full {
        let tc = tail_camber(&lay.parts.tail_airfoil.name);
        let xt = lay.x_tail_le.as_m();
        let hc = lay.h_chord.as_m();
        let top = lay.boom_z.as_m() + lay.parts.boom.od.as_m() / 2.0;
        let z_h = top + hc * lay.parts.tail_airfoil.t_max / 2.0 + 0.001;
        let by = lay.boom_y.as_m();
        let tsec = |y: f64, z: f64, c: f64| Section { xle: xt, yle: y, zle: z, chord: c, ainc_deg: 0.0, camber: tc.clone(), claf: None };
        surfaces.push(Surface {
            name: "Htail".into(),
            n_chord: 6,
            c_space: 1.0,
            n_span: 8,
            s_space: 1.0,
            y_duplicate: Some(0.0),
            angle_deg: 0.0,
            sections: vec![tsec(0.0, z_h, hc), tsec(by, z_h, hc)],
        });
        let vc = lay.v_chord.as_m();
        let ht = hc * lay.parts.tail_airfoil.t_max;
        surfaces.push(Surface {
            name: "Fin".into(),
            n_chord: 6,
            c_space: 1.0,
            n_span: 6,
            s_space: 1.0,
            y_duplicate: Some(0.0),
            angle_deg: 0.0,
            sections: vec![tsec(by, top, vc), tsec(by, top + ht + lay.v_height.as_m(), vc)],
        });
        // pod: elliptic nose to the full section, straight, cone to the tail
        let (x0, x1) = (lay.pod_x0.as_m(), lay.pod_x1.as_m());
        let (f0, f1) = (lay.pod_full_x0.as_m().max(x0 + 0.01), lay.pod_full_x1.as_m().min(x1 - 0.01));
        let r = (lay.pod_width.as_m() * lay.pod_height.as_m()).sqrt() / 2.0;
        let mut prof = vec![];
        for k in 0..=40 {
            let x = x0 + (x1 - x0) * k as f64 / 40.0;
            let rr = if x < f0 {
                let t = (f0 - x) / (f0 - x0);
                r * (1.0 - t * t).max(0.0).sqrt()
            } else if x > f1 {
                r * (1.0 - 0.8 * (x - f1) / (x1 - f1))
            } else {
                r
            };
            prof.push((x - x0, rr));
        }
        bodies.push(avl::Body { name: "Pod".into(), n_body: 20, b_space: 1.0, translate: [x0, 0.0, lay.pod_zc.as_m()], profile: prof });
    }
    Geometry {
        title: format!("Wright VTOL {}", lay.design.name),
        mach: v_cruise / (1.4 * 287.053 * air.temperature.value()).sqrt(),
        sref: lay.area.as_m2(),
        cref: lay.mac.as_m(),
        bref: lay.span.as_m(),
        xref: [lay.x_cg.as_m(), 0.0, lay.z_cg.as_m()],
        cdp: 0.0,
        surfaces,
        bodies,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub cruise_speed_ms: f64,
    pub trim_cl_total: f64,
    pub claf: f64,
    /// Wing alone, per rad, fitted over -2..4 deg.
    pub cl_alpha_wing_analytic: f64,
    pub cl_alpha_wing_avl: f64,
    /// Whole aircraft (AVL, wing + tail + pod), per rad.
    pub cl_alpha_aircraft_avl: f64,
    pub oswald_analytic: f64,
    pub span_efficiency_avl_wing: f64,
    /// Induced drag coefficient at the trimmed CL (on Sref).
    pub cdi_analytic: f64,
    pub cdi_avl: f64,
    /// Induced drag force at cruise, N.
    pub induced_drag_analytic_n: f64,
    pub induced_drag_avl_n: f64,
    pub total_drag_analytic_n: f64,
    /// The analytic drag with its wing+tail induced part replaced by AVL's.
    pub total_drag_with_avl_n: f64,
    pub x_cg_m: f64,
    pub x_np_analytic_m: f64,
    pub x_np_avl_m: f64,
    pub static_margin_analytic: f64,
    pub static_margin_avl: f64,
    pub cm_alpha_avl_per_rad: f64,
    pub range_km_analytic: f64,
    /// Range scaled by the drag ratio at the same speed (power ~ drag x V):
    /// an estimate, not a re-run of the range model.
    pub range_km_with_avl_estimate: f64,
}

fn interp_by(rows: &[(f64, f64)], x: f64) -> f64 {
    for w in rows.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if (x0 <= x && x <= x1) || (x1 <= x && x <= x0) {
            return y0 + (x - x0) / (x1 - x0) * (y1 - y0);
        }
    }
    f64::NAN
}

/// Run AVL on `design` and write media + manifest items under `cfd_dir`.
pub fn run(design: &Design, range_km: f64, v_cruise: f64, cfd_dir: &Path, man: &mut Manifest) -> anyhow::Result<Comparison> {
    let lay = layout::solve(design).map_err(|e| anyhow::anyhow!(e)).context("laying out the VTOL")?;
    let case = cfd_dir.join("vtol_avl");
    let alphas: Vec<f64> = (-4..=14).map(|a| a as f64).collect();
    let full = run_geom(&lay, v_cruise, true, &case, "vtol", &alphas, true)?;
    let wing = run_geom(&lay, v_cruise, false, &case, "wing", &alphas, false)?;
    // analytic, at the same speed and air
    let air = atmosphere::isa(Length::m(design.requirements.altitude_m), 0.0);
    let v = Speed::mps(v_cruise);
    let wm = aero::WingModel::new(&lay, v, &air);
    let an_pts: Vec<(f64, f64)> = [-2.0, 0.0, 2.0, 4.0].iter().map(|&a| (f64::to_radians(a), wm.coefficients(a).0)).collect();
    let cla_an = avl::slope(&an_pts);
    let e_an = aero::oswald(lay.taper, lay.aspect_ratio, lay.pod_width / lay.span);
    let tr = aero::trim(&lay, v, &air, 1.0, Factors::NOMINAL);
    let q = air.q(v).value();
    let s = lay.area.as_m2();
    let cl_total = tr.weight.value() / (q * s);
    // analytic induced = wing (Oswald) + tail induced, both on Sref
    let h_area = lay.h_area.as_m2();
    let ar_h = if h_area > 0.0 { lay.h_span.as_m().powi(2) / h_area } else { 4.0 };
    let cdi_tail = tr.cl_tail.powi(2) / (std::f64::consts::PI * ar_h * wright_air::rules::TAIL_SPAN_EFFICIENCY) * h_area / s;
    let cdi_an = tr.cl_wing.powi(2) / (std::f64::consts::PI * lay.aspect_ratio * e_an) + cdi_tail;
    let rows: Vec<(f64, f64)> = full.sweep.rows.iter().map(|r| (r.cl(), r.cdi())).collect();
    let cdi_avl = interp_by(&rows, cl_total);
    let cm_rows: Vec<(f64, f64)> = full.sweep.rows.iter().map(|r| (r.cl(), r.Xnp.unwrap_or(f64::NAN))).collect();
    let x_np_avl = interp_by(&cm_rows, cl_total);
    let d_an = tr.drag.value();
    let di_an = cdi_an * q * s;
    let di_avl = cdi_avl * q * s;
    let d_new = d_an - di_an + di_avl;
    let mac = lay.mac.as_m();
    let cmp = Comparison {
        cruise_speed_ms: v_cruise,
        trim_cl_total: cl_total,
        claf: full.claf,
        cl_alpha_wing_analytic: cla_an,
        cl_alpha_wing_avl: wing.sweep.cl_alpha_fit(4.0),
        cl_alpha_aircraft_avl: full.sweep.cl_alpha_fit(4.0),
        oswald_analytic: e_an,
        span_efficiency_avl_wing: wing.sweep.rows.iter().find(|r| r.alpha == 4.0).and_then(|r| r.e).unwrap_or(f64::NAN),
        cdi_analytic: cdi_an,
        cdi_avl,
        induced_drag_analytic_n: di_an,
        induced_drag_avl_n: di_avl,
        total_drag_analytic_n: d_an,
        total_drag_with_avl_n: d_new,
        x_cg_m: lay.x_cg.as_m(),
        x_np_analytic_m: lay.x_np.as_m(),
        x_np_avl_m: x_np_avl,
        static_margin_analytic: (lay.x_np.as_m() - lay.x_cg.as_m()) / mac,
        static_margin_avl: (x_np_avl - lay.x_cg.as_m()) / mac,
        cm_alpha_avl_per_rad: full.sweep.rows.iter().find(|r| r.alpha == 2.0).and_then(|r| r.Cma).unwrap_or(f64::NAN),
        range_km_analytic: range_km,
        range_km_with_avl_estimate: range_km * d_an / d_new,
    };
    media(&lay, &full.sweep, &wing.sweep, &wm, &cmp, cfd_dir, man)?;
    let json = cfd_dir.join("vtol_avl.json");
    std::fs::write(&json, serde_json::to_vec_pretty(&serde_json::json!({ "comparison": cmp, "geometry_avl": "vtol_avl/vtol.avl" }))?)?;
    man.add(
        cfd_dir,
        &json,
        "VTOL: AVL vs Wright's analytic aero",
        &format!(
            "AVL (vortex lattice, wing+tail+pod) at the {:.1} m/s cruise: induced drag {:.2} N vs {:.2} N analytic; neutral point {:.3} m vs {:.3} m (static margin {:.1} % vs {:.1} %)",
            v_cruise,
            di_avl,
            di_an,
            x_np_avl,
            lay.x_np.as_m(),
            100.0 * cmp.static_margin_avl,
            100.0 * cmp.static_margin_analytic
        ),
        &[
            ("cl_alpha_wing_avl", cmp.cl_alpha_wing_avl),
            ("cl_alpha_wing_analytic", cmp.cl_alpha_wing_analytic),
            ("cdi_avl", cdi_avl),
            ("cdi_analytic", cdi_an),
            ("static_margin_avl", cmp.static_margin_avl),
            ("static_margin_analytic", cmp.static_margin_analytic),
            ("range_km_with_avl_estimate", cmp.range_km_with_avl_estimate),
        ],
    );
    let csv = cfd_dir.join("vtol_sweep.csv");
    crate::write_csv(
        &csv,
        &["alpha_deg", "CL", "CDi_trefftz", "Cm", "e", "Xnp_m", "CL_wing_analytic"],
        &full
            .sweep
            .rows
            .iter()
            .map(|r| vec![r.alpha, r.cl(), r.cdi(), r.cm(), r.e.unwrap_or(f64::NAN), r.Xnp.unwrap_or(f64::NAN), wm.coefficients(r.alpha).0])
            .collect::<Vec<_>>(),
    )?;
    man.add(cfd_dir, &csv, "VTOL alpha sweep (AVL)", "CL, Trefftz-plane CDi, Cm about the CG, span efficiency and neutral point per degree of alpha", &[]);
    Ok(cmp)
}

struct GeomRun {
    sweep: Sweep,
    claf: f64,
}

fn run_geom(lay: &Layout, v: f64, full: bool, dir: &Path, stem: &str, alphas: &[f64], cp: bool) -> anyhow::Result<GeomRun> {
    let g = geometry(lay, v, full);
    let claf = g.surfaces[0].sections[0].claf.unwrap_or(1.0);
    let sweep = avl::run_sweep(&g, dir, stem, alphas, cp)?;
    Ok(GeomRun { sweep, claf })
}

/// Orthographic 3D camera: from ahead, left and above.
pub struct Camera {
    right: [f64; 3],
    up: [f64; 3],
    dir: [f64; 3],
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Camera {
    /// `c` points from the scene towards the viewer (AVL axes: x aft, y right, z up).
    pub fn new(c: [f64; 3]) -> Self {
        let dir = norm(c);
        let right = norm(cross([0.0, 0.0, 1.0], dir));
        let up = cross(dir, right);
        Camera { right, up, dir }
    }
    pub fn project(&self, p: [f64; 3]) -> (f64, f64, f64) {
        (dot(p, self.right), dot(p, self.up), -dot(p, self.dir))
    }
}

/// Draw AVL's panel Cp on every surface into the box (x, y, w, h).
pub fn draw_cp(cv: &mut Canvas, surfaces: &[avl::CpSurface], cam: &Camera, bx: (f64, f64, f64, f64), lo: f64, hi: f64) {
    let pts: Vec<(f64, f64, f64)> = surfaces.iter().flat_map(|s| s.xyz.iter().flatten().map(|p| cam.project(*p))).collect();
    if pts.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in &pts {
        x0 = x0.min(p.0);
        x1 = x1.max(p.0);
        y0 = y0.min(p.1);
        y1 = y1.max(p.1);
    }
    let sc = ((bx.2) / (x1 - x0)).min(bx.3 / (y1 - y0)) * 0.94;
    let (cx, cy) = (bx.0 + bx.2 / 2.0, bx.1 + bx.3 / 2.0);
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let to = |p: (f64, f64, f64)| (cx + (p.0 - mx) * sc, cy - (p.1 - my) * sc, p.2);
    let mut zb = vec![f64::INFINITY; cv.w * cv.h];
    for s in surfaces {
        for (i, row) in s.cp.iter().enumerate() {
            for (j, &cp) in row.iter().enumerate() {
                let (Some(a), Some(b)) = (s.xyz.get(i), s.xyz.get(i + 1)) else { continue };
                let (Some(p00), Some(p01), Some(p10), Some(p11)) = (a.get(j), a.get(j + 1), b.get(j), b.get(j + 1)) else { continue };
                let n = norm(cross(sub(*p01, *p00), sub(*p10, *p00)));
                let shade = 0.78 + 0.22 * dot(n, cam.dir).abs();
                let q: Vec<(f64, f64, f64)> = [*p00, *p01, *p11, *p10].iter().map(|p| to(cam.project(*p))).collect();
                for t in [[0, 1, 2], [0, 2, 3]] {
                    cv.tri_scalar([(q[t[0]].0, q[t[0]].1), (q[t[1]].0, q[t[1]].1), (q[t[2]].0, q[t[2]].1)], [cp; 3], lo, hi, Cmap::CoolWarm, Some((&mut zb, [q[t[0]].2, q[t[1]].2, q[t[2]].2])), shade);
                }
            }
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn media(lay: &Layout, sw: &Sweep, wing: &Sweep, wm: &aero::WingModel, cmp: &Comparison, dir: &Path, man: &mut Manifest) -> anyhow::Result<()> {
    let cam = Camera::new([0.9, -0.3, 1.0]);
    let (lo, hi) = (-1.6, 1.0);
    let alpha_trim = interp_by(&sw.rows.iter().map(|r| (r.cl(), r.alpha)).collect::<Vec<_>>(), cmp.trim_cl_total);
    let cl_avl: Vec<(f64, f64)> = sw.rows.iter().map(|r| (r.alpha, r.cl())).collect();
    let cl_an: Vec<(f64, f64)> = sw.rows.iter().map(|r| (r.alpha, wm.coefficients(r.alpha).0)).collect();
    let cl_wing: Vec<(f64, f64)> = wing.rows.iter().map(|r| (r.alpha, r.cl())).collect();
    let cm_avl: Vec<(f64, f64)> = sw.rows.iter().map(|r| (r.alpha, r.cm())).collect();
    let frame = |r: &avl::Row, note: &str| -> Canvas {
        let mut cv = Canvas::new(1280, 720, render::BG);
        cv.text(30.0, 44.0, "VTOL surface pressure, AVL vortex lattice (via OptVL)", 26.0, render::BLACK, 0.0);
        cv.text(30.0, 74.0, &format!("{}   alpha {:+.0} deg   CL {:.3}   CDi {:.4}   Cm {:+.3}   {}", lay.design.name, r.alpha, r.cl(), r.cdi(), r.cm(), note), 17.0, render::GREY, 0.0);
        draw_cp(&mut cv, &r.surfaces, &cam, (20.0, 90.0, 760.0, 600.0), lo, hi);
        cv.colorbar(790.0, 140.0, 22.0, 480.0, lo, hi, Cmap::CoolWarm, "Cp");
        let ax = render::chart(&mut cv, 870.0, 90.0, 400.0, 330.0, "Lift and moment vs alpha", "alpha, deg", "CL / Cm", &[
            Series::line("CL AVL (aircraft)", cl_avl.clone(), render::SERIES[0]),
            Series::line("CL wing alone, AVL", cl_wing.clone(), render::SERIES[4]),
            Series::line("CL wing, Wright analytic", cl_an.clone(), render::SERIES[1]).dashed(),
            Series::line("Cm AVL about CG", cm_avl.clone(), render::SERIES[2]),
        ], None);
        let (px, py) = ax.map(r.alpha, r.cl());
        cv.disc(px, py, 7.0, render::BLACK);
        let mut y = 440.0;
        for line in [
            format!("trim CL {:.3} at alpha {:.1} deg ({:.1} m/s)", cmp.trim_cl_total, alpha_trim, cmp.cruise_speed_ms),
            format!("CL_alpha wing: AVL {:.2} /rad, analytic {:.2} /rad", cmp.cl_alpha_wing_avl, cmp.cl_alpha_wing_analytic),
            format!("induced drag: AVL {:.2} N, analytic {:.2} N", cmp.induced_drag_avl_n, cmp.induced_drag_analytic_n),
            format!("neutral point: AVL {:.3} m, analytic {:.3} m", cmp.x_np_avl_m, cmp.x_np_analytic_m),
            format!("static margin: AVL {:.1} %, analytic {:.1} %", 100.0 * cmp.static_margin_avl, 100.0 * cmp.static_margin_analytic),
            format!("section slope from NeuralFoil: CLAF {:.2}", cmp.claf),
        ] {
            cv.text(880.0, y, &line, 15.0, render::BLACK, 0.0);
            y += 26.0;
        }
        cv
    };
    let frames: Vec<Canvas> = sw.rows.iter().map(|r| frame(r, "")).collect();
    let mp4 = dir.join("vtol_cp_alpha_sweep.mp4");
    crate::video::encode(&frames, 4, 1, &mp4)?;
    man.add(dir, &mp4, "VTOL pressure, alpha sweep (AVL)", "Cp on every vortex-lattice panel of wing, tail and fins from -4 to 14 deg; the dot walks the lift curve", &[("frames", frames.len() as f64)]);
    // the cruise frame
    let near = sw.rows.iter().min_by(|a, b| (a.alpha - alpha_trim).abs().total_cmp(&(b.alpha - alpha_trim).abs())).context("no rows")?;
    let png = dir.join("vtol_cp_cruise.png");
    frame(near, "(nearest cruise)").save_png(&png)?;
    man.add(dir, &png, "VTOL pressure at cruise (AVL)", &format!("Cp at alpha {:+.0} deg, the sweep point nearest the {:.1} m/s trim", near.alpha, cmp.cruise_speed_ms), &[("alpha_deg", near.alpha), ("CL", near.cl())]);
    // polar panel: CDi vs CL, both models, and span loading at cruise
    let mut cv = Canvas::new(1280, 560, render::BG);
    let e = cmp.oswald_analytic;
    let ar = lay.aspect_ratio;
    let cdi_avl: Vec<(f64, f64)> = sw.rows.iter().map(|r| (r.cl(), r.cdi())).collect();
    let cdi_an: Vec<(f64, f64)> = sw.rows.iter().map(|r| (r.cl(), r.cl().powi(2) / (std::f64::consts::PI * ar * e))).collect();
    render::chart(&mut cv, 10.0, 10.0, 620.0, 540.0, "Induced drag polar", "CL", "CDi", &[
        Series::line("AVL Trefftz plane (aircraft)", cdi_avl, render::SERIES[0]),
        Series::line(format!("analytic CL^2/(pi AR e), e {e:.2}"), cdi_an, render::SERIES[1]).dashed(),
    ], None);
    if let Some(st) = near.strips.get("Wing") {
        let pts: Vec<(f64, f64)> = st.y.iter().zip(&st.lift_dist).filter_map(|(y, l)| Some((y.as_ref().copied()?, l.as_ref().copied()?))).collect();
        let half = lay.span.as_m() / 2.0;
        let area: f64 = {
            let mut a = 0.0;
            for w in pts.windows(2) {
                a += 0.5 * (w[0].1 + w[1].1) * (w[1].0 - w[0].0);
            }
            a
        };
        let ell: Vec<(f64, f64)> = (0..=40).map(|k| {
            let y = half * k as f64 / 40.0;
            (y, 4.0 * area / (std::f64::consts::PI * half) * (1.0 - (y / half).powi(2)).max(0.0).sqrt())
        }).collect();
        render::chart(&mut cv, 650.0, 10.0, 620.0, 540.0, &format!("Span loading at alpha {:+.0} deg", near.alpha), "y, m (starboard)", "cl c / cref", &[
            Series::line("AVL strips", pts, render::SERIES[0]),
            Series::line("elliptic, same lift", ell, render::SERIES[2]).dashed(),
        ], None);
    }
    let png2 = dir.join("vtol_polar_spanload.png");
    cv.save_png(&png2)?;
    man.add(dir, &png2, "VTOL induced drag and span loading", "AVL's Trefftz-plane induced drag against the analytic Oswald model; AVL span loading against elliptic", &[("oswald_analytic", e), ("e_avl_wing", cmp.span_efficiency_avl_wing)]);
    Ok(())
}
