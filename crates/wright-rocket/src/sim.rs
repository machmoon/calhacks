//! 6-DOF flight in RocketPy (RocketPy-Team/RocketPy 1.13, pip), fed with
//! Wright's own design: mass properties, a liquid motor with its two tanks
//! draining at Wright's flows, the CD(M) curve from `aero::cd0`, the fin
//! set, recovery and the competition's launch conditions.
//!
//! The mapping follows RocketPy's own liquid-motor example
//! (`docs/examples/` and `rocketpy/motors/liquid_motor.py`: `LiquidMotor`
//! with `MassFlowRateBasedTank` on `CylindricalTank`; `Rocket.add_motor`,
//! `add_nose`, `add_trapezoidal_fins`, `add_parachute`;
//! `Environment.set_atmospheric_model("custom_atmosphere", wind_u=...)`;
//! `Flight(rail_length, inclination, heading)`), read from the installed
//! source. The thrust curve is the vacuum thrust with
//! `reference_pressure = 0`, so RocketPy subtracts p_a A_e itself
//! (`Motor.pressure_thrust`), the same F = F_vac - p_a A_e Wright flies.
//!
//! What RocketPy computes independently of Wright: the 6-DOF equations of
//! motion (quaternions, `Flight.u_dot_generalized`), Barrowman lift and
//! centre of pressure of nose and fins (incl. its own fin-body interference
//! and pitch damping), time-varying inertia from the tanks, rail
//! departure, Earth rotation, and the descent under parachutes. What it
//! takes from Wright: the masses, the thrust and flow curves, and CD(M).
//!
//! This module only writes the input JSON; the runner is
//! `crates/wright-rocket/sim/rocketpy_sim.py`, run by `run` when a Python
//! with RocketPy is found (env `WRIGHT_PYTHON`, else none: the manifest then
//! says the simulation was not run and why).

use crate::catalog::Catalog;
use crate::domain::{reflown, RocketDesign, RocketSpec};
use crate::rules;
use crate::vehicle::Vehicle;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Approximate launch site coordinates (for RocketPy's Earth rotation and
/// gravity model only; Wright's model has neither). FAR: Friends of Amateur
/// Rocketry, Randsburg CA (https://www.friendsofamateurrocketry.org/ ,
/// site map). IREC 2025: Midland TX (Spaceport America Cup moved there in
/// 2025, https://www.soundingrocket.org/).
fn site(spec: &RocketSpec) -> (f64, f64) {
    match spec.competition {
        crate::domain::Competition::FarOut2027 => (35.347, -117.808),
        crate::domain::Competition::Irec2025 => (31.94, -102.20),
    }
}

/// Propellant left in each tank (ox, fuel) at time `t`, kg, from Wright's
/// own mass model (`Vehicle::mass_at`).
fn tank_masses(v: &Vehicle, t: f64) -> (f64, f64) {
    match &v.profile {
        None => {
            let used = (t / v.burn_time).clamp(0.0, 1.0);
            let frac = 1.0 - used / (1.0 + rules::RESIDUAL);
            (v.ox_tank.propellant_kg * frac, v.fuel_tank.propellant_kg * frac)
        }
        Some(p) => (v.ox_tank.propellant_kg - p.at(&p.ox_used, t), v.fuel_tank.propellant_kg - p.at(&p.f_used, t)),
    }
}

/// The RocketPy input for a design, re-flown from its parameters. Includes
/// Wright's own nominal and dispersion apogees for the comparison.
pub fn sim_input(spec: &RocketSpec, d: &RocketDesign) -> anyhow::Result<Value> {
    let cat = Catalog::load()?;
    let r = reflown(spec, d)?;
    let v = &r.vehicle;
    let lim = spec.competition.limits();
    let launch = spec.launch(0.0);
    let n = 200;
    let tb = v.burn_time;
    let times: Vec<f64> = (0..=n).map(|i| tb * i as f64 / n as f64).collect();
    let thrust_vac: Vec<[f64; 2]> = times.iter().map(|&t| [t, v.thrust_at(t.min(tb * (1.0 - 1e-9)), 0.0)]).collect();
    let ox_mass: Vec<[f64; 2]> = times.iter().map(|&t| [t, tank_masses(v, t).0]).collect();
    let fuel_mass: Vec<[f64; 2]> = times.iter().map(|&t| [t, tank_masses(v, t).1]).collect();
    // Dry mass properties about the dry CG (same sums as Vehicle::mass_at).
    let m: f64 = v.dry.iter().map(|i| i.mass).sum();
    let cg = v.dry.iter().map(|i| i.mass * i.x).sum::<f64>() / m;
    let rr = v.diameter / 2.0;
    let i_pitch = v.dry.iter().map(|i| i.mass * ((i.x - cg).powi(2) + i.len * i.len / 12.0)).sum::<f64>() + m * rr * rr / 4.0;
    // Roll: the dry mass as a thin shell at 0.8 of the radius (stated; roll
    // inertia does not enter the planar comparison).
    let i_roll = m * (0.8 * rr).powi(2);
    // CD(M) from Wright's buildup at the site's air (the Reynolds number at
    // each Mach is taken at site density; RocketPy's drag is a function of
    // Mach only, stated).
    let air = crate::atmosphere::at(lim.elevation_m);
    let exit_frac = v.engine.exit_area() / v.aero.ref_area();
    let machs: Vec<f64> = (0..=60).map(|i| 0.01 + i as f64 * 0.05).collect();
    let cd = |on: bool| -> Vec<[f64; 2]> { machs.iter().map(|&mm| [mm, v.aero.cd0(mm, mm * air.a, air.mu / air.rho, on, exit_frac)]).collect() };
    let chute = |id: &str| -> anyhow::Result<f64> {
        let c = cat.get(id).ok_or_else(|| anyhow::anyhow!("chute {id} not in catalog"))?;
        let dia = c.f("diameter_in").unwrap_or(0.0) * 0.0254;
        Ok(c.f("cd").unwrap_or(0.0) * std::f64::consts::PI * dia * dia / 4.0)
    };
    let tank = |t: &crate::vehicle::Tank, mass: &[[f64; 2]]| {
        json!({
            "name": t.name, "inner_radius_m": t.inner_d / 2.0, "length_m": t.length,
            "center_from_nose_m": t.x_fwd + t.bulkhead_t + t.length / 2.0,
            "density_kg_m3": t.density, "mass_kg": mass,
        })
    };
    let winds = [0.0, lim.wind_max_mps / 2.0, lim.wind_max_mps];
    let wright_disp: Vec<Value> = r.dispersion.iter().map(|(k, a)| json!({"case": k, "apogee_agl_m": a})).collect();
    let (lat, lon) = site(spec);
    Ok(json!({
        "name": format!("{}/{} {:.0} ft", d.params.engine.oxidizer, d.params.engine.fuel, d.contract_m / rules::FT),
        "site": {"latitude": lat, "longitude": lon, "elevation_m": lim.elevation_m, "gravity_m_s2": wright_core::units::G0},
        "rail": {"length_m": launch.rail_length, "inclination_deg": 90.0 - launch.rail_tilt.to_degrees(), "heading_deg": 90.0},
        // Wright's wind: 1/7 power law from 10 m AGL, blowing against the
        // rail tilt (toward -x = west when the rail leans east).
        "wind": {"speeds_10m_m_s": winds, "power_law_exponent": 1.0 / 7.0, "toward_heading_deg": 270.0},
        "dispersion": [[1.0, 1.0], [0.95, 1.10], [1.05, 0.90]],
        "rocket": {
            "radius_m": rr, "length_m": v.length, "nose_length_m": v.nose_length, "nose_kind": "von karman",
            "dry_mass_kg": m, "dry_cg_from_nose_m": cg, "dry_inertia": [i_pitch, i_pitch, i_roll],
        },
        "fins": {"n": v.aero.fin_count, "root_m": v.aero.fin_root, "tip_m": v.aero.fin_tip, "span_m": v.aero.fin_span, "sweep_m": v.aero.fin_sweep, "le_from_nose_m": v.aero.fin_le_x},
        "drag": {"source": "Wright aero::cd0 (OpenRocket-style buildup)", "power_off": cd(false), "power_on": cd(true)},
        "motor": {
            "thrust_vac_n": thrust_vac, "burn_time_s": tb,
            "nozzle_exit_radius_m": (v.engine.exit_area() / std::f64::consts::PI).sqrt(),
            "nozzle_exit_from_nose_m": v.length,
            "tanks": [tank(&v.ox_tank, &ox_mass), tank(&v.fuel_tank, &fuel_mass)],
        },
        "recovery": {
            "drogue_cd_s_m2": chute(&d.params.drogue)?, "main_cd_s_m2": chute(&d.params.main)?,
            "main_deploy_agl_m": rules::MAIN_DEPLOY_AGL_M,
        },
        "contract_agl_m": d.contract_m,
        "wright": {
            "nominal_apogee_agl_m": r.nominal.apogee_agl, "nominal_max_mach": r.nominal.max_mach,
            "rail_exit_m_s": r.nominal.rail_exit_speed, "margin_at_rail_exit_cal": r.nominal.margin_at_rail_exit,
            "dispersion": wright_disp,
            // [t, x, z, speed, Mach, static margin cal] (trajectory.rs trace)
            "trace": r.nominal.trace,
        },
    }))
}

/// The Python that runs RocketPy: `WRIGHT_PYTHON`, if set and it imports
/// rocketpy.
pub fn python() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("WRIGHT_PYTHON")?);
    let ok = std::process::Command::new(&p).args(["-I", "-c", "import rocketpy"]).output().map(|o| o.status.success()).unwrap_or(false);
    ok.then_some(p)
}

pub fn runner() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("sim/rocketpy_sim.py")
}

/// Write `<dir>/input.json` and run RocketPy into `<dir>` (plots, an mp4
/// animation, CSV, `result.json`, `manifest.json`). Errors, saying why,
/// when no Python with RocketPy is configured.
pub fn run(spec: &RocketSpec, d: &RocketDesign, dir: &Path) -> anyhow::Result<wright_core::Manifest> {
    std::fs::create_dir_all(dir)?;
    let input = dir.join("input.json");
    std::fs::write(&input, serde_json::to_string_pretty(&sim_input(spec, d)?)?)?;
    let py = python().ok_or_else(|| anyhow::anyhow!("RocketPy not run: set WRIGHT_PYTHON to a Python with rocketpy 1.13 installed (pip install rocketpy==1.13.0); the input is at {}", input.display()))?;
    let o = std::process::Command::new(py).arg("-I").arg(runner()).arg(&input).arg(dir).env("MPLBACKEND", "Agg").output()?;
    anyhow::ensure!(o.status.success(), "RocketPy run failed: {}", String::from_utf8_lossy(&o.stderr).lines().rev().take(8).collect::<Vec<_>>().join(" | "));
    let mut m = wright_core::Manifest::read(dir).ok_or_else(|| anyhow::anyhow!("RocketPy runner wrote no manifest"))?;
    let r: Value = serde_json::from_slice(&std::fs::read(dir.join("result.json"))?)?;
    let n = &r["nominal"];
    let f = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    let diff = f(&n["apogee_diff_pct"]);
    m.check(
        "6-DOF apogee within 5 % of trajectory.rs",
        diff.abs() <= 5.0,
        &format!("RocketPy {:.0} ft vs Wright {:.0} ft ({diff:+.1} %), nominal mid wind", f(&n["rocketpy_apogee_agl_m"]) / rules::FT, f(&n["wright_apogee_agl_m"]) / rules::FT),
    );
    let lim = spec.competition.limits();
    let sm = f(&n["rocketpy_stability_at_rail_exit_cal"]);
    m.check("6-DOF stability at rail exit", sm >= lim.stability_min_cal, &format!("RocketPy {sm:.2} cal vs {:.1} cal minimum (Wright {:.2} cal)", lim.stability_min_cal, f(&n["wright_stability_at_rail_exit_cal"])));
    let rail = f(&n["rocketpy_rail_exit_m_s"]);
    m.check("6-DOF rail exit speed", rail >= lim.rail_exit_min_mps, &format!("RocketPy {:.0} ft/s vs {:.0} ft/s minimum", rail / rules::FT, lim.rail_exit_min_mps / rules::FT));
    m.write(dir)?;
    Ok(m)
}

/// Pipeline stage "sim": the 6-DOF RocketPy flight of a built rocket.
pub struct SimStage;

impl wright_core::Stage for SimStage {
    fn name(&self) -> &'static str {
        "sim"
    }
    fn applies_to(&self, domain: &str) -> bool {
        domain == <crate::Rocket as wright_core::Domain>::NAME
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn run(&self, ctx: &wright_core::StageCtx) -> anyhow::Result<wright_core::Manifest> {
        let spec: RocketSpec = serde_json::from_value(ctx.spec.clone())?;
        let d: RocketDesign = serde_json::from_value(ctx.design.clone())?;
        run(&spec, &d, &ctx.dir)
    }
}
