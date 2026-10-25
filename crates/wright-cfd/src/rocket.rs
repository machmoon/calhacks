//! The rocket in SU2: the nose's pressure (wave) drag against Mach on an
//! axisymmetric Euler mesh, and the nozzle's thrust coefficient.
//!
//! Why these two and not "the whole rocket": an inviscid solver gets
//! pressure drag right and friction and base drag wrong (no boundary layer,
//! no separated wake), so the rocket's CD(M) here is wright-rocket's own
//! OpenRocket build-up (`wright_rocket::aero::AeroBody::cd0`) with ONE term
//! replaced: the nose pressure drag, which `aero.rs` takes from NASA
//! TR-R-100's fineness-3 von Karman data "as-is" for every nose (its own
//! stated deviation). SU2 computes it for the real nose. Fins stay with
//! Barrowman (3D, non-axisymmetric).

use crate::mesh::{self, Grid};
use crate::render::{self, Canvas, Cmap, Series};
use crate::su2;
use crate::Manifest;
use anyhow::Context;
use serde::Serialize;
use std::path::Path;
use wright_rocket::aero::{von_karman_radius, AeroBody};
use wright_rocket::engine::Engine;
use wright_rocket::ideal;

pub const GAMMA_AIR: f64 = 1.4;
pub const P_SL: f64 = 101_325.0;
pub const T_SL: f64 = 288.15;

/// NASA TR-R-100 (NTRS 19630004995, p. 16) free-flight pressure drag of a
/// fineness-3 von Karman nose, as tabulated in OpenRocket
/// `core/.../aerodynamics/barrowman/SymmetricComponentCalc.java`
/// (`vonKarmanInterpolator`) and copied into wright-rocket `aero.rs`.
pub const TR_R_100_VK_FR3: [(f64, f64); 10] = [(0.9, 0.0), (0.95, 0.010), (1.0, 0.027), (1.05, 0.055), (1.1, 0.070), (1.2, 0.081), (1.4, 0.095), (1.6, 0.097), (2.0, 0.091), (3.0, 0.083)];

pub fn interp(t: &[(f64, f64)], x: f64) -> f64 {
    if x <= t[0].0 {
        return t[0].1;
    }
    for w in t.windows(2) {
        if x <= w[1].0 {
            return w[0].1 + (x - w[0].0) / (w[1].0 - w[0].0) * (w[1].1 - w[0].1);
        }
    }
    t[t.len() - 1].1
}

/// OpenRocket's fineness extrapolation of the FR-3 table
/// (`SymmetricComponentCalc.calculateNoseInterpolator`), as implemented in
/// wright-rocket (`aero::von_karman_pressure_cd`).
pub fn openrocket_vk(m: f64, fineness: f64) -> f64 {
    wright_rocket::aero::von_karman_pressure_cd(m, fineness)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum NoseShape {
    VonKarman,
    Cone,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Nose {
    pub shape: NoseShape,
    pub length: f64,
    pub radius: f64,
}

impl Nose {
    pub fn r(&self, x: f64) -> f64 {
        match self.shape {
            NoseShape::VonKarman => von_karman_radius(x.clamp(0.0, self.length), self.length, self.radius),
            NoseShape::Cone => self.radius * (x / self.length).clamp(0.0, 1.0),
        }
    }
    pub fn fineness(&self) -> f64 {
        self.length / (2.0 * self.radius)
    }
}

pub struct NoseGrid {
    pub grid: Grid,
    /// i range of the nose surface nodes (tip ..= shoulder) on j = 0.
    pub nose: (usize, usize),
    pub outline: Vec<[f64; 2]>,
    pub view: [f64; 4],
}

/// Axis + nose + 3-calibre cylinder below, far field above; vertical grid
/// lines, clustered to the tip, the shoulder and the wall.
pub fn nose_grid(n: &Nose, fine: f64) -> NoseGrid {
    let (ln, r) = (n.length, n.radius);
    let after = 6.0 * r;
    let a = 0.6 * ln + 4.0 * r;
    let h = 1.6 * (ln + after);
    let k = |v: f64| ((v * fine).round() as usize).max(8);
    let (na, nn, nc, nj) = (k(40.0), k(140.0), k(50.0), k(110.0));
    let mut lower = vec![];
    // axis: -a .. 0, clustered at the tip
    let s = mesh::stretched(na + 1, 0.004 * ln / a);
    for q in s.iter().rev().take(na) {
        lower.push([-a * q, 0.0]);
    }
    let i_tip = lower.len();
    for kk in 0..=nn {
        let t = kk as f64 / nn as f64;
        // dense at the tip (r ~ sqrt(x) there) and at the shoulder
        let u = 0.5 * (1.0 - (std::f64::consts::PI * t).cos());
        let x = ln * (0.6 * u * u + 0.4 * u);
        lower.push([x, n.r(x)]);
    }
    let i_sh = lower.len() - 1;
    let s = mesh::stretched(nc + 1, 0.01);
    for q in s.iter().skip(1) {
        lower.push([ln + after * q, r]);
    }
    let upper: Vec<[f64; 2]> = lower.iter().map(|p| [p[0], h]).collect();
    let dist = mesh::stretched(nj, 0.0015 * ln / h);
    let mut g = mesh::ruled(&lower, &upper, &dist);
    let ni = g.ni;
    let nj = g.nj;
    g.markers = vec![
        ("axis".into(), g.row_edges(0, 0, i_tip)),
        ("body".into(), g.row_edges(0, i_tip, ni - 1)),
        ("farfield".into(), [g.col_edges(0, 0, nj - 1), g.row_edges(nj - 1, 0, ni - 1), g.col_edges(ni - 1, 0, nj - 1)].concat()),
    ];
    let outline = lower[i_tip..].to_vec();
    let (x0, x1) = (-0.2 * ln, ln + after);
    // the picture box is 1120 x 600 and mirrored: r1 = (x1 - x0) 600 / 1120 / 2
    NoseGrid { grid: g, nose: (i_tip, i_sh), outline, view: [x0, x1, 0.0, (x1 - x0) * 600.0 / 1120.0 / 2.0] }
}

/// Pressure drag of the nose surface on the base area pi R^2:
/// sum over surface edges of Cp * d(pi r^2), Cp = (p - p_inf) / q_inf.
pub fn nose_cd(ng: &NoseGrid, p: &[f64], mach: f64, p_inf: f64, g: f64) -> f64 {
    let q = 0.5 * g * p_inf * mach * mach;
    let r = ng.grid.at(ng.nose.1, 0)[1];
    let mut d = 0.0;
    for i in ng.nose.0..ng.nose.1 {
        let (a, b) = (ng.grid.at(i, 0), ng.grid.at(i + 1, 0));
        let pa = p[ng.grid.id(i, 0)];
        let pb = p[ng.grid.id(i + 1, 0)];
        d += (0.5 * (pa + pb) - p_inf) * (b[1] * b[1] - a[1] * a[1]);
    }
    d / (q * r * r)
}

#[derive(Debug, Clone, Serialize)]
pub struct NosePoint {
    pub mach: f64,
    pub cd_pressure: f64,
    pub iterations: usize,
    pub log10_rms_density: f64,
    pub seconds: f64,
    /// SU2's own CD on the whole body (nose + cylinder, pressure only): a
    /// check on the integration here.
    pub cd_su2_body: f64,
}

/// SU2 Euler on `nose` at each Mach; one Mach-contour frame per point.
pub fn nose_sweep(nose: &Nose, machs: &[f64], dir: &Path, tag: &str, threads: usize) -> anyhow::Result<(Vec<NosePoint>, Vec<Canvas>)> {
    let fine: f64 = std::env::var("WRIGHT_CFD_FINE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.8);
    let ng = nose_grid(nose, fine);
    anyhow::ensure!(ng.grid.min_area() > 0.0, "nose mesh folded");
    // independent Mach cases run side by side (WRIGHT_CFD_JOBS, default 3)
    let jobs: usize = std::env::var("WRIGHT_CFD_JOBS").ok().and_then(|s| s.parse().ok()).unwrap_or(3).max(1);
    let mut runs: Vec<Option<anyhow::Result<su2::Run>>> = (0..machs.len()).map(|_| None).collect();
    for chunk in (0..machs.len()).collect::<Vec<_>>().chunks(jobs) {
        let done: Vec<(usize, anyhow::Result<su2::Run>)> = std::thread::scope(|sc| {
            let hs: Vec<_> = chunk
                .iter()
                .map(|&k| {
                    let m = machs[k];
                    let d = dir.join(format!("{tag}_M{m:.2}"));
                    let g = &ng.grid;
                    sc.spawn(move || {
                        let iters = if (0.95..=1.15).contains(&m) { 3000 } else { 1500 };
                        let r = su2::run(g, &su2::external_cfg(m, P_SL, T_SL, iters, std::f64::consts::PI * nose.radius * nose.radius), &d, threads, GAMMA_AIR).with_context(|| format!("{tag} at M {m}"));
                        let _ = std::fs::remove_file(d.join("mesh.su2"));
                        (k, r)
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("SU2 thread panicked")).collect()
        });
        for (k, r) in done {
            runs[k] = Some(r);
        }
    }
    let mut pts = vec![];
    let mut frames = vec![];
    for (k, &m) in machs.iter().enumerate() {
        let run = runs[k].take().expect("every case ran")?;
        let p = run.field.col("Pressure")?;
        let cd = nose_cd(&ng, p, m, P_SL, GAMMA_AIR);
        let mach = run.field.col("Mach")?;
        let mut cv = Canvas::new(1280, 720, render::BG);
        let (lo, hi) = ((m - 0.6).max(0.0), m + 0.5);
        let map = su2::draw_field(&mut cv, &ng.grid, mach, (20.0, 100.0, 1120.0, 600.0), Some(ng.view), lo, hi, Cmap::Turbo, true);
        su2::draw_outline(&mut cv, &ng.outline, map, ng.view, true, render::BLACK);
        cv.clip_outside(20.0, 100.0, 1120.0, 600.0, render::BG);
        cv.colorbar(1170.0, 160.0, 22.0, 480.0, lo, hi, Cmap::Turbo, "Mach");
        cv.text(30.0, 44.0, &format!("{}: SU2 axisymmetric Euler, free-stream Mach {m:.2}", tag.replace('_', " ")), 26.0, render::BLACK, 0.0);
        cv.text(30.0, 74.0, &format!("nose pressure drag CD {cd:.4} (on base area)   {} iterations, log10 rms(rho) {:.1}, {:.0} s", run.iterations, run.rms_density, run.seconds), 17.0, render::GREY, 0.0);
        frames.push(cv);
        pts.push(NosePoint { mach: m, cd_pressure: cd, iterations: run.iterations, log10_rms_density: run.rms_density, seconds: run.seconds, cd_su2_body: run.cd });
    }
    Ok((pts, frames))
}

// ---------------------------------------------------------------------------
// Nozzle

/// Round the corner at `p[k]` of a polyline with a circular arc of radius `rad`.
fn fillet(p: &[[f64; 2]], k: usize, rad: f64, n: usize) -> Vec<[f64; 2]> {
    let (a, b, c) = (p[k - 1], p[k], p[k + 1]);
    let u = norm2([a[0] - b[0], a[1] - b[1]]);
    let v = norm2([c[0] - b[0], c[1] - b[1]]);
    let phi = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0).acos();
    let t = rad / (phi / 2.0).tan();
    let p1 = [b[0] + u[0] * t, b[1] + u[1] * t];
    let p2 = [b[0] + v[0] * t, b[1] + v[1] * t];
    let bis = norm2([u[0] + v[0], u[1] + v[1]]);
    let dc = rad / (phi / 2.0).sin();
    let ctr = [b[0] + bis[0] * dc, b[1] + bis[1] * dc];
    let a1 = (p1[1] - ctr[1]).atan2(p1[0] - ctr[0]);
    let mut a2 = (p2[1] - ctr[1]).atan2(p2[0] - ctr[0]);
    if (a2 - a1).abs() > std::f64::consts::PI {
        a2 += if a2 < a1 { 2.0 * std::f64::consts::PI } else { -2.0 * std::f64::consts::PI };
    }
    let mut out: Vec<[f64; 2]> = p[..k].to_vec();
    for s in 0..=n {
        let th = a1 + (a2 - a1) * s as f64 / n as f64;
        out.push([ctr[0] + rad * th.cos(), ctr[1] + rad * th.sin()]);
    }
    out.extend_from_slice(&p[k + 1..]);
    out
}

fn norm2(v: [f64; 2]) -> [f64; 2] {
    let l = v[0].hypot(v[1]);
    [v[0] / l, v[1] / l]
}

/// The engine's contour as wright-rocket sizes it (cylindrical chamber end,
/// conical convergence at the design's angle, 15 deg conical divergence to
/// the design's exit radius: `engine.rs` `Engine`), with the throat made
/// the usual way: a circular arc of 1.5 r_t upstream and 0.382 r_t
/// downstream, both centred above the throat so the minimum radius stays
/// exactly r_t (Sutton & Biblarz 9th ed. sect. 3.4, the Rao-contour throat
/// radii; a sharp throat is neither machined nor well-posed for a solver).
/// The cones keep their angles, so their lengths move by a few mm; Ae/At
/// is exact.
pub fn nozzle_contour(e: &Engine) -> Vec<[f64; 2]> {
    let (rc, rt, re) = (e.chamber_d / 2.0, e.throat_d / 2.0, e.exit_d / 2.0);
    let th_c = ((rc - rt) / e.converging_len).atan();
    let th_d = ((re - rt) / e.diverging_len).atan();
    let (ru, rd) = (1.5 * rt, 0.382 * rt);
    let xt = 0.0;
    // upstream arc: angle from -90 deg back to -90 - th_c
    let up: Vec<[f64; 2]> = (0..=24).rev().map(|k| {
        let a = -std::f64::consts::FRAC_PI_2 - th_c * k as f64 / 24.0;
        [xt + ru * a.cos(), rt + ru + ru * a.sin()]
    }).collect();
    let dn: Vec<[f64; 2]> = (1..=12).map(|k| {
        let a = -std::f64::consts::FRAC_PI_2 + th_d * k as f64 / 12.0;
        [xt + rd * a.cos(), rt + rd + rd * a.sin()]
    }).collect();
    let (ua, db) = (up[0], dn[dn.len() - 1]);
    // cones to the chamber radius and to the exit radius
    let x_c = ua[0] - (rc - ua[1]) / th_c.tan();
    let x_e = db[0] + (re - db[1]) / th_d.tan();
    let mut p = vec![[x_c - 0.5 * rc, rc], [x_c, rc]];
    p.extend(up);
    p.extend(dn);
    p.push([x_e, re]);
    // round the chamber-to-cone corner (radius r_t)
    p = fillet(&p, 1, rt, 12);
    let x0 = p[0][0];
    p.iter().map(|q| [q[0] - x0, q[1]]).collect()
}

fn wall_r(contour: &[[f64; 2]], x: f64) -> f64 {
    for w in contour.windows(2) {
        if x >= w[0][0] && x <= w[1][0] {
            let t = (x - w[0][0]) / (w[1][0] - w[0][0]).max(1e-300);
            return w[0][1] + t * (w[1][1] - w[0][1]);
        }
    }
    contour.last().unwrap()[1]
}

pub struct NozzleGrid {
    pub grid: Grid,
    pub contour: Vec<[f64; 2]>,
}

pub fn nozzle_grid(e: &Engine, ni: usize, nj: usize) -> NozzleGrid {
    let contour = nozzle_contour(e);
    let (xa, xb) = (contour[0][0], contour.last().unwrap()[0]);
    // cluster x around the throat: map through the cumulative of 1/r^2 weight
    let n_s = 2000;
    let xs: Vec<f64> = (0..=n_s).map(|k| xa + (xb - xa) * k as f64 / n_s as f64).collect();
    let w: Vec<f64> = xs.iter().map(|&x| 1.0 / wall_r(&contour, x).powf(1.0)).collect();
    let mut cum = vec![0.0];
    for k in 1..=n_s {
        cum.push(cum[k - 1] + 0.5 * (w[k] + w[k - 1]));
    }
    let tot = cum[n_s];
    let xi: Vec<f64> = (0..ni)
        .map(|i| {
            let s = tot * i as f64 / (ni - 1) as f64;
            let k = cum.partition_point(|&c| c < s).clamp(1, n_s);
            let t = (s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-300);
            xs[k - 1] + t * (xs[k] - xs[k - 1])
        })
        .collect();
    let lower: Vec<[f64; 2]> = xi.iter().map(|&x| [x, 0.0]).collect();
    let upper: Vec<[f64; 2]> = xi.iter().map(|&x| [x, wall_r(&contour, x)]).collect();
    let dist: Vec<f64> = (0..nj).map(|j| j as f64 / (nj - 1) as f64).collect();
    let mut g = mesh::ruled(&lower, &upper, &dist);
    let (ni, nj) = (g.ni, g.nj);
    g.markers = vec![
        ("axis".into(), g.row_edges(0, 0, ni - 1)),
        ("wall".into(), g.row_edges(nj - 1, 0, ni - 1)),
        ("inlet".into(), g.col_edges(0, 0, nj - 1)),
        ("outlet".into(), g.col_edges(ni - 1, 0, nj - 1)),
    ];
    NozzleGrid { grid: g, contour: upper }
}

/// Gamma for which the ideal frozen expansion to the design area ratio
/// gives the equilibrium solution's pe/pc (bisection).
pub fn equivalent_gamma(eps: f64, pe_pc: f64) -> f64 {
    let (mut lo, mut hi) = (1.05, 1.67);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        // at fixed eps a higher gamma expands further: pe/pc falls
        if ideal::exit_pressure_ratio(eps, mid) > pe_pc {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[derive(Debug, Clone, Serialize)]
pub struct NozzleResult {
    pub gamma_equivalent: f64,
    pub r_gas: f64,
    pub pc: f64,
    pub tc: f64,
    pub area_ratio_geometric: f64,
    /// The meshed wall's smallest radius over the design's r_t.
    pub throat_radius_mesh_over_design: f64,
    pub divergence_half_angle_deg: f64,
    /// Vacuum thrust coefficient from SU2 (exit-plane momentum + pressure).
    pub cf_vac_su2: f64,
    /// Same gas, quasi-1D ideal (Sutton eq. 3-30).
    pub cf_vac_1d: f64,
    /// wright-rocket's model on the same gas: lambda x momentum + pressure term.
    pub cf_vac_lambda_model: f64,
    pub lambda_wright: f64,
    /// SU2 mass flow over the ideal choked flow (discharge coefficient).
    pub discharge_coefficient: f64,
    /// SU2 vacuum Cf over the quasi-1D one.
    pub efficiency_2d: f64,
    /// At the design ambient pressure.
    pub cf_design_su2: f64,
    pub cf_design_wright: f64,
    pub iterations: usize,
    pub log10_rms_density: f64,
    pub seconds: f64,
}

/// SU2 on the engine's nozzle; draws a Mach contour.
pub fn nozzle(e: &Engine, dir: &Path, threads: usize) -> anyhow::Result<(NozzleResult, Canvas, Canvas)> {
    let ng = nozzle_grid(e, 260, 70);
    anyhow::ensure!(ng.grid.min_area() > 0.0, "nozzle mesh folded");
    let ch = &e.ideal.chamber;
    let pc = e.spec.pc;
    let tc = ch.t_k;
    let r_gas = 8.314462618 / ch.molar_mass_kg_mol;
    let eps = e.area_ratio;
    let g = equivalent_gamma(eps, e.ideal.exit.p_pa / pc);
    // outlet static pressure only matters if the exit went subsonic
    let p_out = 0.5 * pc * ideal::exit_pressure_ratio(eps, g);
    let run = su2::run(&ng.grid, &su2::nozzle_cfg(pc, tc, g, r_gas, p_out, 4000), &dir.join("nozzle"), threads, g)?;
    let f = &run.field;
    let (rho, mx, p) = (f.col("Density")?, f.col("Momentum_x")?, f.col("Pressure")?);
    let gr = &ng.grid;
    let i = gr.ni - 1;
    let (mut mom, mut pres, mut mdot) = (0.0, 0.0, 0.0);
    for j in 0..gr.nj - 1 {
        let (a, b) = (gr.id(i, j), gr.id(i, j + 1));
        let (ra, rb) = (gr.pts[a][1], gr.pts[b][1]);
        let da = std::f64::consts::PI * (rb * rb - ra * ra);
        mom += 0.5 * (mx[a] * mx[a] / rho[a] + mx[b] * mx[b] / rho[b]) * da;
        pres += 0.5 * (p[a] + p[b]) * da;
        mdot += 0.5 * (mx[a] + mx[b]) * da;
    }
    let rt = e.throat_d / 2.0;
    let at = std::f64::consts::PI * rt * rt;
    let ae = std::f64::consts::PI * gr.pts[gr.id(i, gr.nj - 1)][1].powi(2);
    let rt_mesh = gr.pts.iter().map(|p| p[1]).enumerate().filter(|(k, _)| k / gr.ni == gr.nj - 1).map(|(_, r)| r).fold(f64::INFINITY, f64::min);
    let cf_vac_su2 = (mom + pres) / (pc * at);
    let pe_pc = ideal::exit_pressure_ratio(eps, g);
    let cf_vac_1d = ideal::thrust_coefficient(g, pe_pc, 0.0, eps);
    let mom_1d = ideal::thrust_coefficient(g, pe_pc, pe_pc, eps);
    let lambda = e.lambda;
    let cf_vac_lambda = lambda * mom_1d + pe_pc * eps;
    let cstar = ideal::c_star(tc, ch.molar_mass_kg_mol, g);
    let mdot_ideal = pc * at / cstar;
    let pa = e.spec.pa_design;
    let res = NozzleResult {
        gamma_equivalent: g,
        r_gas,
        pc,
        tc,
        area_ratio_geometric: ae / at,
        throat_radius_mesh_over_design: rt_mesh / rt,
        divergence_half_angle_deg: ((e.exit_d - e.throat_d) / 2.0 / e.diverging_len).atan().to_degrees(),
        cf_vac_su2,
        cf_vac_1d,
        cf_vac_lambda_model: cf_vac_lambda,
        lambda_wright: lambda,
        discharge_coefficient: mdot / mdot_ideal,
        efficiency_2d: cf_vac_su2 / cf_vac_1d,
        cf_design_su2: cf_vac_su2 - pa * eps / pc,
        cf_design_wright: e.cf(pa),
        iterations: run.iterations,
        log10_rms_density: run.rms_density,
        seconds: run.seconds,
    };
    let _ = std::fs::remove_file(dir.join("nozzle").join("mesh.su2"));
    // pictures: Mach and pressure
    let mach = f.col("Mach")?;
    let view = {
        let xe = gr.pts[gr.id(gr.ni - 1, 0)][0];
        let rmax = gr.pts.iter().map(|p| p[1]).fold(0.0, f64::max);
        [0.0, xe, 0.0, rmax]
    };
    let draw = |vals: &[f64], lo: f64, hi: f64, cmap: Cmap, label: &str, title: &str| {
        let mut cv = Canvas::new(1280, 720, render::BG);
        let map = su2::draw_field(&mut cv, gr, vals, (30.0, 110.0, 1110.0, 560.0), Some(view), lo, hi, cmap, true);
        su2::draw_outline(&mut cv, &ng.contour, map, view, true, render::BLACK);
        cv.colorbar(1170.0, 140.0, 22.0, 480.0, lo, hi, cmap, label);
        cv.text(30.0, 44.0, title, 26.0, render::BLACK, 0.0);
        cv.text(
            30.0,
            74.0,
            &format!(
                "Cf_vac SU2 {:.4} vs 1-D {:.4} (x{:.4}); Wright's lambda model {:.4}; Cd {:.3}; gamma {:.3}, Pc {:.0} bar",
                res.cf_vac_su2,
                res.cf_vac_1d,
                res.efficiency_2d,
                res.cf_vac_lambda_model,
                res.discharge_coefficient,
                g,
                pc / 1e5
            ),
            17.0,
            render::GREY,
            0.0,
        );
        cv
    };
    let mmax = mach.iter().cloned().fold(0.0, f64::max).ceil();
    let c1 = draw(mach, 0.0, mmax, Cmap::Turbo, "Mach", "Engine nozzle: SU2 axisymmetric Euler, Mach number");
    let pr: Vec<f64> = p.iter().map(|x| x / pc).collect();
    let c2 = draw(&pr, 0.0, 1.0, Cmap::Turbo, "p / pc", "Engine nozzle: SU2 axisymmetric Euler, pressure / chamber pressure");
    Ok((res, c1, c2))
}

#[derive(Debug, Clone, Serialize)]
pub struct DragRow {
    pub mach: f64,
    pub cd_wright: f64,
    /// The nose term wright-rocket uses today (`AeroBody::nose_pressure_cd`).
    pub nose_wright: f64,
    pub nose_openrocket: f64,
    pub nose_su2: f64,
    pub cd_with_su2_nose: f64,
}

/// The rocket: nose drag sweep on the design's nose, its effect on CD(M),
/// and the nozzle. Writes media and manifest items.
pub fn run(aero: &AeroBody, engine: &Engine, machs: &[f64], cfd_dir: &Path, man: &mut Manifest, threads: usize) -> anyhow::Result<(Vec<DragRow>, NozzleResult)> {
    let nose = Nose { shape: NoseShape::VonKarman, length: aero.nose_length, radius: aero.diameter / 2.0 };
    let dir = cfd_dir.join("rocket_su2");
    let (pts, frames) = nose_sweep(&nose, machs, &dir, "rocket_nose", threads)?;
    let nu = 1.46e-5;
    let a = (GAMMA_AIR * 287.053 * T_SL).sqrt();
    let rows: Vec<DragRow> = pts
        .iter()
        .map(|p| {
            let cdw = aero.cd0(p.mach, p.mach * a, nu, false, 0.0);
            let t = aero.nose_pressure_cd(p.mach);
            DragRow { mach: p.mach, cd_wright: cdw, nose_wright: t, nose_openrocket: openrocket_vk(p.mach, nose.fineness()), nose_su2: p.cd_pressure, cd_with_su2_nose: cdw - t + p.cd_pressure }
        })
        .collect();
    let mp4 = cfd_dir.join("rocket_mach_sweep.mp4");
    crate::video::encode(&frames, 2, 1, &mp4)?;
    man.add(cfd_dir, &mp4, "Rocket nose, Mach sweep (SU2)", &format!("Mach field round the design's {:.1}:1 von Karman nose, M {:.2} to {:.2}, SU2 axisymmetric Euler", nose.fineness(), machs[0], machs[machs.len() - 1]), &[("cases", frames.len() as f64)]);
    if let Some((k, _)) = pts.iter().enumerate().min_by(|a, b| (a.1.mach - 1.2).abs().total_cmp(&(b.1.mach - 1.2).abs())) {
        let png = cfd_dir.join("rocket_nose_mach.png");
        frames[k].save_png(&png)?;
        man.add(cfd_dir, &png, &format!("Rocket nose at M {:.2} (SU2)", pts[k].mach), "Bow shock and expansion round the nose, mirrored about the axis", &[("mach", pts[k].mach), ("cd_nose", pts[k].cd_pressure)]);
    }
    // chart
    let mut cv = Canvas::new(1280, 560, render::BG);
    let col = |f: &dyn Fn(&DragRow) -> f64| rows.iter().map(|r| (r.mach, f(r))).collect::<Vec<_>>();
    render::chart(&mut cv, 10.0, 10.0, 620.0, 540.0, &format!("Nose pressure drag, {:.1}:1 von Karman", nose.fineness()), "Mach", "CD (base area)", &[
        Series::line("SU2 Euler, this nose", col(&|r| r.nose_su2), render::SERIES[0]),
        Series::line("wright-rocket today", col(&|r| r.nose_wright), render::SERIES[1]).dashed(),
        Series::line("OpenRocket FR extrapolation", col(&|r| r.nose_openrocket), render::SERIES[2]).dashed(),
    ], None);
    render::chart(&mut cv, 650.0, 10.0, 620.0, 540.0, "Rocket CD vs Mach (power off)", "Mach", "CD", &[
        Series::line("Wright build-up today", col(&|r| r.cd_wright), render::SERIES[1]),
        Series::line("with SU2 nose pressure drag", col(&|r| r.cd_with_su2_nose), render::SERIES[0]),
    ], None);
    let png = cfd_dir.join("rocket_cd_mach.png");
    cv.save_png(&png)?;
    let at = |m: f64| rows.iter().min_by(|a, b| (a.mach - m).abs().total_cmp(&(b.mach - m).abs())).cloned();
    let r12 = at(1.1).context("no rows")?;
    man.add(cfd_dir, &png, "Rocket CD vs Mach", &format!("At M {:.2}: CD {:.3} today, {:.3} with the SU2 nose term (nose {:.3} vs wright-rocket {:.3})", r12.mach, r12.cd_wright, r12.cd_with_su2_nose, r12.nose_su2, r12.nose_wright), &[("cd_wright", r12.cd_wright), ("cd_su2_nose", r12.cd_with_su2_nose)]);
    let csv = cfd_dir.join("rocket_cd_mach.csv");
    crate::write_csv(&csv, &["mach", "cd_wright", "nose_wright", "nose_openrocket", "nose_su2", "cd_with_su2_nose"], &rows.iter().map(|r| vec![r.mach, r.cd_wright, r.nose_wright, r.nose_openrocket, r.nose_su2, r.cd_with_su2_nose]).collect::<Vec<_>>())?;
    man.add(cfd_dir, &csv, "Rocket CD(M) table", "Wright's power-off CD with and without the SU2 nose term, per Mach", &[]);
    // nozzle
    let (nz, c1, c2) = nozzle(engine, &dir, threads)?;
    let p1 = cfd_dir.join("nozzle_mach.png");
    c1.save_png(&p1)?;
    let p2 = cfd_dir.join("nozzle_pressure.png");
    c2.save_png(&p2)?;
    let summary = format!("Vacuum Cf {:.4} (SU2) vs {:.4} (Wright's lambda {:.4} model) on the same gas; discharge coefficient {:.3}", nz.cf_vac_su2, nz.cf_vac_lambda_model, nz.lambda_wright, nz.discharge_coefficient);
    let metrics = [("cf_vac_su2", nz.cf_vac_su2), ("cf_vac_lambda_model", nz.cf_vac_lambda_model), ("cf_vac_1d", nz.cf_vac_1d), ("discharge_coefficient", nz.discharge_coefficient)];
    man.add(cfd_dir, &p1, "Nozzle Mach number (SU2)", &summary, &metrics);
    man.add(cfd_dir, &p2, "Nozzle pressure (SU2)", "Static pressure over chamber pressure through the conical nozzle", &[]);
    let mp4 = cfd_dir.join("nozzle.mp4");
    crate::video::encode(&[c1, c2], 1, 3, &mp4)?;
    man.add(cfd_dir, &mp4, "Nozzle flow (SU2)", &summary, &metrics);
    let json = cfd_dir.join("rocket_su2.json");
    std::fs::write(&json, serde_json::to_vec_pretty(&serde_json::json!({"nose": nose, "nose_points": pts, "cd_rows": rows, "nozzle": nz}))?)?;
    man.add(cfd_dir, &json, "Rocket SU2 numbers", "Nose drag points with convergence, CD(M) rows, nozzle Cf comparison", &[]);
    Ok((rows, nz))
}

#[derive(Debug, Clone, Serialize)]
pub struct ApogeeEffect {
    pub max_mach: f64,
    pub apogee_today_m: f64,
    /// Flown with the whole drag scaled by CD_su2 / CD_today at the
    /// flight's peak Mach (where the two differ most): a bound on the
    /// change, not a Mach-by-Mach re-flight (wright-rocket's `fly` takes
    /// one drag scale; see docs/verification.md for the proposed table).
    pub drag_scale_at_max_mach: f64,
    pub apogee_bound_m: f64,
}

/// Re-fly a full `RocketDesign` with wright-rocket's own trajectory model.
pub fn apogee_effect(d: &wright_rocket::RocketDesign, rows: &[DragRow]) -> ApogeeEffect {
    let spec = d.launch_spec();
    let mut l = spec.launch(0.0);
    let today = wright_rocket::trajectory::fly(&d.vehicle, &l);
    let mm = today.max_mach;
    let pts: Vec<(f64, f64)> = rows.iter().map(|r| (r.mach, r.cd_with_su2_nose / r.cd_wright)).collect();
    let k = interp(&pts, mm);
    l.drag_scale = k;
    let alt = wright_rocket::trajectory::fly(&d.vehicle, &l);
    ApogeeEffect { max_mach: mm, apogee_today_m: today.apogee_agl, drag_scale_at_max_mach: k, apogee_bound_m: alt.apogee_agl }
}
