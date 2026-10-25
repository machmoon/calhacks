//! SU2 (su2code/SU2 v8.5.0, LGPL-2.1) as a subprocess: config, run, parse.
//!
//! Config keys are SU2's own (`Common/src/CConfig.cpp`: `SOLVER`,
//! `AXISYMMETRIC`, `MARKER_EULER`, `MARKER_SYM`, `MARKER_FAR`,
//! `MARKER_RIEMANN`, `OUTPUT_FILES`, ...); the internal-nozzle setup follows
//! SU2's own axisymmetric nozzle test case
//! (`TestCases/axisymmetric_rans/air_nozzle/air_nozzle_restart.cfg`: Riemann
//! `TOTAL_CONDITIONS_PT` inlet, `STATIC_PRESSURE` outlet, `MARKER_SYM` axis),
//! made inviscid. Results are read from `RESTART_ASCII`
//! (`restart_flow.csv`, one row per mesh node in node order, with the
//! conservative variables; pressure and Mach are derived here).

use crate::mesh::Grid;
use crate::render::{self, Canvas, Cmap};
use anyhow::{bail, Context};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Numerics shared by every case here: implicit Euler, Roe with MUSCL and
/// Venkatakrishnan limiting, adaptive CFL (`CFL_ADAPT_PARAM` = factor down,
/// factor up, min, max), stopped by a Cauchy criterion on the monitored
/// marker's drag (`CONV_CAUCHY_ELEMS`/`EPS`, as `TestCases/euler/naca0012/
/// inv_NACA0012_Roe.cfg` does), since that integral is what is read.
pub fn numerics(iters: usize, cfl: f64, cfl_max: f64, monitor: &str, conv: &str) -> String {
    format!(
        "NUM_METHOD_GRAD= WEIGHTED_LEAST_SQUARES
CFL_NUMBER= {cfl}
CFL_ADAPT= YES
CFL_ADAPT_PARAM= ( 0.5, 1.2, {cfl}, {cfl_max} )
CONV_NUM_METHOD_FLOW= ROE
ENTROPY_FIX_COEFF= 0.1
MUSCL_FLOW= YES
SLOPE_LIMITER_FLOW= VENKATAKRISHNAN
VENKAT_LIMITER_COEFF= 0.05
TIME_DISCRE_FLOW= EULER_IMPLICIT
LINEAR_SOLVER= FGMRES
LINEAR_SOLVER_PREC= ILU
LINEAR_SOLVER_ERROR= 1E-4
LINEAR_SOLVER_ITER= 10
ITER= {iters}
MARKER_MONITORING= ( {monitor} )
{conv}
MESH_FILENAME= mesh.su2
MESH_FORMAT= SU2
OUTPUT_FILES= ( RESTART_ASCII )
RESTART_FILENAME= restart_flow
VOLUME_OUTPUT= ( COORDINATES, SOLUTION, PRIMITIVE )
SCREEN_OUTPUT= ( INNER_ITER, RMS_DENSITY, RMS_ENERGY )
HISTORY_OUTPUT= ( ITER, RMS_RES, AERO_COEFF )
CONV_FILENAME= history
OUTPUT_WRT_FREQ= 1000000
SCREEN_WRT_FREQ_INNER= 50
"
    )
}

/// External flow: stop when the body's drag stops changing (SU2's Cauchy
/// test is on the coefficient's absolute change, so REF_AREA is the body's
/// base area and CD is O(0.01-0.1): 2e-6 is ~0.01 % per iteration).
pub const CAUCHY_DRAG: &str = "CONV_FIELD= DRAG\nCONV_CAUCHY_ELEMS= 100\nCONV_CAUCHY_EPS= 2E-6\nCONV_STARTITER= 400";
/// Internal flow (the coefficients are scaled by a near-zero free stream,
/// so their Cauchy test is meaningless): density residual down 8 orders.
pub const RESIDUAL: &str = "CONV_FIELD= RMS_DENSITY\nCONV_RESIDUAL_MINVAL= -10\nCONV_STARTITER= 300";

/// Free-stream external flow round an axisymmetric body (Euler).
pub fn external_cfg(mach: f64, p: f64, t: f64, iters: usize, ref_area: f64) -> String {
    format!(
        "SOLVER= EULER
MATH_PROBLEM= DIRECT
AXISYMMETRIC= YES
MACH_NUMBER= {mach}
AOA= 0.0
FREESTREAM_PRESSURE= {p}
FREESTREAM_TEMPERATURE= {t}
REF_DIMENSIONALIZATION= DIMENSIONAL
FLUID_MODEL= STANDARD_AIR
MARKER_EULER= ( body )
MARKER_SYM= ( axis )
MARKER_FAR= ( farfield )
{}REF_AREA= {ref_area}
",
        numerics(iters, 5.0, if mach > 0.95 && mach < 1.15 { 30.0 } else { 100.0 }, "body", CAUCHY_DRAG)
    )
}

/// Internal nozzle flow from chamber stagnation (pc, tc) to `p_out`, ideal
/// gas with `gamma` and `r_gas` (J/kg K), inviscid.
pub fn nozzle_cfg(pc: f64, tc: f64, gamma: f64, r_gas: f64, p_out: f64, iters: usize) -> String {
    format!(
        "SOLVER= EULER
MATH_PROBLEM= DIRECT
AXISYMMETRIC= YES
MACH_NUMBER= 0.05
AOA= 0.0
INIT_OPTION= TD_CONDITIONS
FREESTREAM_OPTION= TEMPERATURE_FS
FREESTREAM_PRESSURE= {pc}
FREESTREAM_TEMPERATURE= {tc}
REF_DIMENSIONALIZATION= DIMENSIONAL
FLUID_MODEL= IDEAL_GAS
GAMMA_VALUE= {gamma}
GAS_CONSTANT= {r_gas}
MARKER_EULER= ( wall )
MARKER_SYM= ( axis )
MARKER_RIEMANN= ( inlet, TOTAL_CONDITIONS_PT, {pc}, {tc}, 1.0, 0.0, 0.0, outlet, STATIC_PRESSURE, {p_out}, 0.0, 0.0, 0.0, 0.0 )
{}",
        numerics(iters, 5.0, 60.0, "wall", RESIDUAL)
    )
}

/// One node's solution.
#[derive(Debug, Clone, Default)]
pub struct Field {
    pub names: Vec<String>,
    /// Column-major: `cols[name][node]`.
    pub cols: BTreeMap<String, Vec<f64>>,
}

impl Field {
    /// Pressure and Mach from the conservative restart variables of an
    /// ideal gas: p = (gamma - 1)(rhoE - |rho u|^2 / 2 rho), a^2 = gamma p / rho
    /// (the restart file holds only the solution vector).
    pub fn add_primitives(&mut self, gamma: f64) -> anyhow::Result<()> {
        if self.cols.contains_key("Pressure") && self.cols.contains_key("Mach") {
            return Ok(());
        }
        let (r, mx, my, e) = (self.col("Density")?.clone(), self.col("Momentum_x")?.clone(), self.col("Momentum_y")?.clone(), self.col("Energy")?.clone());
        let mut p = vec![0.0; r.len()];
        let mut m = vec![0.0; r.len()];
        for k in 0..r.len() {
            let ke = 0.5 * (mx[k] * mx[k] + my[k] * my[k]) / r[k];
            p[k] = (gamma - 1.0) * (e[k] - ke);
            let a = (gamma * p[k] / r[k]).sqrt();
            m[k] = (mx[k] * mx[k] + my[k] * my[k]).sqrt() / r[k] / a;
        }
        self.cols.insert("Pressure".into(), p);
        self.cols.insert("Mach".into(), m);
        self.names.extend(["Pressure".to_string(), "Mach".to_string()]);
        Ok(())
    }
    pub fn col(&self, name: &str) -> anyhow::Result<&Vec<f64>> {
        self.cols.get(name).with_context(|| format!("SU2 output has no {name:?} column (has {:?})", self.names))
    }
}

#[derive(Debug, Clone)]
pub struct Run {
    pub field: Field,
    pub iterations: usize,
    /// Final log10 RMS density residual.
    pub rms_density: f64,
    pub seconds: f64,
    /// SU2's own drag coefficient on the monitored marker (REF_AREA based).
    pub cd: f64,
}

/// Write `grid` + `cfg` to `dir`, run SU2_CFD there and read the result.
pub fn run(grid: &Grid, cfg: &str, dir: &Path, threads: usize, gamma: f64) -> anyhow::Result<Run> {
    std::fs::create_dir_all(dir)?;
    let dir = &dir.canonicalize()?;
    // a finished case with the same config, mesh signature (or none
    // recorded) and node count is read back, not re-run (the stage runner
    // also skips unchanged inputs)
    let sig = grid_signature(grid);
    if std::fs::read_to_string(dir.join("case.cfg")).ok().as_deref() == Some(cfg) && std::fs::read_to_string(dir.join("mesh.sig")).map(|s| s == sig).unwrap_or(true) {
        if let Ok(mut field) = read_restart(&dir.join("restart_flow.csv")) {
            if field.cols.values().next().map(|c| c.len()) == Some(grid.pts.len()) {
                field.add_primitives(gamma)?;
                let (iterations, rms_density, cd) = read_history(&dir.join("history.csv")).unwrap_or((0, f64::NAN, f64::NAN));
                std::fs::write(dir.join("mesh.sig"), &sig)?;
                return Ok(Run { field, iterations, rms_density, seconds: 0.0, cd });
            }
        }
    }
    grid.write_su2(&dir.join("mesh.su2"))?;
    std::fs::write(dir.join("case.cfg"), cfg)?;
    let _ = std::fs::remove_file(dir.join("mesh.sig"));
    let t0 = std::time::Instant::now();
    let exe = crate::su2_cfd();
    let o = Command::new(&exe)
        .args(["-t", &threads.to_string(), "case.cfg"])
        // the machine is shared: OpenMP threads sleep instead of spinning
        .env("OMP_WAIT_POLICY", "PASSIVE")
        .current_dir(dir)
        .output()
        .with_context(|| format!("running {} (set WRIGHT_SU2)", exe.display()))?;
    let seconds = t0.elapsed().as_secs_f64();
    std::fs::write(dir.join("su2.log"), [&o.stdout[..], &o.stderr[..]].concat())?;
    if !o.status.success() {
        let log = String::from_utf8_lossy(&o.stdout);
        let tail: Vec<&str> = log.lines().rev().take(8).collect();
        bail!("SU2_CFD failed ({}) in {}: {}", o.status, dir.display(), tail.into_iter().rev().collect::<Vec<_>>().join(" | "));
    }
    let mut field = read_restart(&dir.join("restart_flow.csv"))?;
    field.add_primitives(gamma)?;
    std::fs::write(dir.join("mesh.sig"), &sig)?;
    if field.cols.values().next().map(|c| c.len()) != Some(grid.pts.len()) {
        bail!("SU2 wrote {} nodes, the mesh has {}", field.cols.values().next().map(|c| c.len()).unwrap_or(0), grid.pts.len());
    }
    let (iterations, rms_density, cd) = read_history(&dir.join("history.csv")).unwrap_or((0, f64::NAN, f64::NAN));
    Ok(Run { field, iterations, rms_density, seconds, cd })
}

/// FNV-1a over the node coordinates and marker edges.
pub fn grid_signature(g: &Grid) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    for p in &g.pts {
        eat(&p[0].to_le_bytes());
        eat(&p[1].to_le_bytes());
    }
    for (name, edges) in &g.markers {
        eat(name.as_bytes());
        for (a, b) in edges {
            eat(&a.to_le_bytes());
            eat(&b.to_le_bytes());
        }
    }
    format!("{h:016x} {}x{}", g.ni, g.nj)
}

pub fn read_restart(path: &Path) -> anyhow::Result<Field> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = text.lines();
    let head = lines.next().context("empty restart file")?;
    let names: Vec<String> = head.split(',').map(|s| s.trim().trim_matches('"').to_string()).collect();
    let mut cols: BTreeMap<String, Vec<f64>> = names.iter().map(|n| (n.clone(), vec![])).collect();
    for l in lines {
        if l.trim().is_empty() {
            continue;
        }
        let vals: Vec<&str> = l.split(',').collect();
        if vals.len() != names.len() {
            continue;
        }
        for (n, v) in names.iter().zip(vals) {
            cols.get_mut(n).unwrap().push(v.trim().parse::<f64>().unwrap_or(f64::NAN));
        }
    }
    // restart rows are in global point order; sort by PointID to be sure
    if let Some(ids) = cols.get("PointID").cloned() {
        let mut order: Vec<usize> = (0..ids.len()).collect();
        order.sort_by(|a, b| ids[*a].total_cmp(&ids[*b]));
        for c in cols.values_mut() {
            *c = order.iter().map(|&k| c[k]).collect();
        }
    }
    Ok(Field { names, cols })
}

fn read_history(path: &Path) -> anyhow::Result<(usize, f64, f64)> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let head: Vec<String> = lines.next().context("empty")?.split(',').map(|s| s.trim().trim_matches('"').to_string()).collect();
    let ki = head.iter().position(|h| h == "Inner_Iter").unwrap_or(0);
    let kr = head.iter().position(|h| h == "rms[Rho]").context("no rms[Rho]")?;
    let last = lines.rfind(|l| !l.trim().is_empty()).context("no rows")?;
    let v: Vec<f64> = last.split(',').map(|s| s.trim().parse().unwrap_or(f64::NAN)).collect();
    let cd = head.iter().position(|h| h == "CD").map(|k| v[k]).unwrap_or(f64::NAN);
    Ok((v[ki] as usize + 1, v[kr], cd))
}

/// Draw `values` on `grid` (and its mirror about the axis) into the box.
#[allow(clippy::too_many_arguments)]
pub fn draw_field(cv: &mut Canvas, grid: &Grid, values: &[f64], bx: (f64, f64, f64, f64), view: Option<[f64; 4]>, lo: f64, hi: f64, cmap: Cmap, mirror: bool) -> (f64, f64, f64) {
    let [x0, x1, r0, r1] = view.unwrap_or_else(|| {
        let (mut a, mut b, mut c, mut d) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in &grid.pts {
            a = a.min(p[0]);
            b = b.max(p[0]);
            c = c.min(p[1]);
            d = d.max(p[1]);
        }
        [a, b, c, d]
    });
    let rmin = if mirror { -r1 } else { r0 };
    let sc = (bx.2 / (x1 - x0)).min(bx.3 / (r1 - rmin));
    let ox = bx.0 + (bx.2 - (x1 - x0) * sc) / 2.0;
    let oy = bx.1 + (bx.3 - (r1 - rmin) * sc) / 2.0;
    let to = |p: [f64; 2], s: f64| (ox + (p[0] - x0) * sc, oy + (r1 - s * p[1]) * sc);
    for q in grid.quads() {
        for sgn in if mirror { vec![1.0, -1.0] } else { vec![1.0] } {
            let p: Vec<(f64, f64)> = q.iter().map(|&k| to(grid.pts[k], sgn)).collect();
            let v: Vec<f64> = q.iter().map(|&k| values[k]).collect();
            cv.tri_scalar([p[0], p[1], p[2]], [v[0], v[1], v[2]], lo, hi, cmap, None, 1.0);
            cv.tri_scalar([p[0], p[2], p[3]], [v[0], v[2], v[3]], lo, hi, cmap, None, 1.0);
        }
    }
    (ox, oy, sc)
}

/// Draw a boundary polyline (x, r) in the same mapping as `draw_field`.
pub fn draw_outline(cv: &mut Canvas, pts: &[[f64; 2]], map: (f64, f64, f64), view: [f64; 4], mirror: bool, c: render::Rgb) {
    let (ox, oy, sc) = map;
    let [x0, _, _, r1] = view;
    for sgn in if mirror { vec![1.0, -1.0] } else { vec![1.0] } {
        let p: Vec<(f64, f64)> = pts.iter().map(|q| (ox + (q[0] - x0) * sc, oy + (r1 - sgn * q[1]) * sc)).collect();
        cv.polyline(&p, 2.0, c);
    }
}
