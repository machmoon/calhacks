//! A 6-DOF flight simulation of the design in JSBSim (JSBSim-Team/jsbsim,
//! the flight dynamics model ArduPilot's SITL also drives), with the
//! aerodynamic tables from AeroSandbox's AeroBuildup (peterdsharpe/
//! AeroSandbox): neither is written here.
//!
//! This module only exports what the simulator needs, as JSON
//! ([`inputs`]): geometry in the layout's frame (x aft, y right, z up, m, the
//! same axes as JSBSim's structural frame), mass and inertia from the mass
//! items, the props' measured CT/CP tables, the motors' full-throttle shaft
//! power, and the analytic numbers the sim is checked against. The Python
//! side (`crates/wright-air/sim/vtol_sim.py`) builds the AeroSandbox
//! airplane, tabulates the aerodynamics, writes the JSBSim aircraft XML,
//! flies hover, transition, cruise, back-transition and landing under an
//! ArduPilot-QuadPlane-shaped controller, and writes `sim/manifest.json`.
//!
//! [`run`] calls it with the interpreter in `WRIGHT_PYTHON` (default
//! `python3`); when that interpreter lacks `jsbsim` or `aerosandbox`, the
//! manifest says the sim was not run and why, and nothing fails silently.

use crate::checks::Evaluation;
use crate::layout::Layout;
use serde_json::{json, Value};
use std::path::Path;
use wright_core::{Manifest, Stage, StageCtx};
use wright_core::units::{Force, Length, Speed};

/// The Python side, shipped in the crate.
pub const SCRIPT: &str = include_str!("../sim/vtol_sim.py");

/// Where each mass item sits across the span, as (radius of gyration about
/// the x axis)^2 / item mass, m^2. The mass items carry x and z only; the
/// spanwise placement below follows the layout: spars and wing skin spread
/// evenly over their reach (r^2 = reach^2 / 3), boom-borne items at the boom
/// line, the tail between the booms, everything else on the centreline.
fn y2(lay: &Layout, name: &str) -> f64 {
    let semi = lay.semi_span().as_m();
    let by = lay.boom_y.as_m();
    match name {
        "wing (printed)" | "main spar" | "rear spar" | "hardware and glue" => semi * semi / 3.0,
        "spar joiner" => (semi * crate::rules::JOINER_HALF_FRAC).powi(2) / 3.0,
        "booms" | "lift motors" | "lift props" | "landing legs" | "lift motor mounts" | "boom clamps" | "tail joints" => by * by,
        "tail (printed)" | "tail servos" => 0.6 * by * by,
        "aileron servos" => lay.aileron_servo[1].as_m().powi(2),
        _ => 0.0,
    }
}

/// (Ixx, Iyy, Izz) about the CG, kg m^2: the items as point masses at
/// their (x, y, z), plus each spread item's own spanwise term.
pub fn inertia(lay: &Layout) -> (f64, f64, f64) {
    let (xc, zc) = (lay.x_cg.as_m(), lay.z_cg.as_m());
    let (mut ixx, mut iyy, mut izz) = (0.0, 0.0, 0.0);
    for it in &lay.items {
        let m = it.mass.as_kg();
        let dx = it.x.as_m() - xc;
        let dz = it.z.as_m() - zc;
        let yy = y2(lay, it.name);
        ixx += m * (yy + dz * dz);
        iyy += m * (dx * dx + dz * dz);
        izz += m * (dx * dx + yy);
    }
    (ixx, iyy, izz)
}

/// CT and CP over advance ratio at one rpm, from the prop's measured data.
fn prop_table(p: &crate::prop::PropData, rpm: f64) -> Value {
    let js: Vec<f64> = (0..=24).map(|k| k as f64 * 0.05).collect();
    let rows: Vec<[f64; 3]> = js.iter().map(|&j| {
        let (ct, cp) = p.coefficients(j, rpm);
        [j, ct, cp]
    }).collect();
    json!({"rpm": rpm, "j_ct_cp": rows, "data_id": p.data_id, "diameter_m": p.diameter.as_m()})
}

/// Shaft power, W, at the full-throttle thrust `t` (static): what JSBSim's
/// electric engine is given as its maximum (`FGElectric`: power = max x
/// throttle).
fn shaft_at(p: &crate::prop::PropData, t: Force, rho: wright_core::units::Density) -> Option<(f64, f64)> {
    let rpm = p.rpm_for_thrust(t, Speed::mps(0.0), rho, 60_000.0)?;
    Some((p.thrust_power(rpm, Speed::mps(0.0), rho).1.value(), rpm))
}

/// Everything the simulator reads, as JSON.
pub fn inputs(e: &Evaluation) -> Value {
    let lay = &e.layout;
    let perf = &e.perf;
    let p = &lay.parts;
    let d = &lay.design;
    let air = crate::atmosphere::isa(Length::m(d.requirements.altitude_m), 0.0);
    let (ixx, iyy, izz) = inertia(lay);
    let (a0, a1) = lay.aileron_span();
    // lift units at full throttle: the hover check's all-four case (hot day,
    // low pack, current limits): conservative on a standard day
    let lift_max = shaft_at(&p.lift_prop.data, perf.hover.full.thrust_each, air.density);
    let pack_v = lay.battery.pack.full_voltage();
    let (pusher_t, _) = crate::powertrain::max_thrust(p.cruise_motor, &p.cruise_prop.data, Speed::mps(0.0), air.density, pack_v);
    let pusher_max = shaft_at(&p.cruise_prop.data, pusher_t, air.density);
    let hover_op = perf.hover.std_full.as_ref().ok();
    let br = perf.best_range.as_ref();
    let lift_rpm = hover_op.map(|o| o.rpm).unwrap_or(6000.0);
    let cruise_rpm = br.map(|c| c.point.rpm).unwrap_or(6000.0);
    let rotors: Vec<Value> = [(lay.x_front, 1.0, 1), (lay.x_front, -1.0, -1), (lay.x_rear, 1.0, -1), (lay.x_rear, -1.0, 1)]
        .iter()
        .map(|&(x, side, sense)| json!({"x": x.as_m(), "y": side * lay.boom_y.as_m(), "z": lay.lift_disc_z.as_m(), "sense": sense}))
        .collect();
    let legs: Vec<Value> = [(lay.x_front, 1.0), (lay.x_front, -1.0), (lay.x_rear, 1.0), (lay.x_rear, -1.0)]
        .iter()
        .map(|&(x, side)| json!({"x": x.as_m(), "y": side * lay.boom_y.as_m(), "z": lay.ground_z.as_m()}))
        .collect();
    json!({
        "name": d.name,
        "frame": "x aft, y right, z up, m (JSBSim structural axes); origin at the wing root leading edge",
        "air": {"altitude_m": d.requirements.altitude_m, "rho": air.density.value()},
        "mass": {
            "kg": lay.mass().as_kg(),
            "cg": [lay.x_cg.as_m(), 0.0, lay.z_cg.as_m()],
            "ixx": ixx, "iyy": iyy, "izz": izz,
            "items": lay.items.iter().map(|i| json!({"name": i.name, "kg": i.mass.as_kg(), "x": i.x.as_m(), "z": i.z.as_m()})).collect::<Vec<_>>(),
        },
        "wing": {
            "span": lay.span.as_m(), "root_chord": lay.root_chord.as_m(), "tip_chord": lay.tip_chord.as_m(),
            "incidence_deg": d.wing.incidence_deg, "washout_deg": d.wing.washout_deg,
            "airfoil": d.wing.airfoil, "area": lay.area.as_m2(), "mac": lay.mac.as_m(),
            "aileron_y": [a0.as_m(), a1.as_m()], "aileron_chord_frac": 0.25,
        },
        "htail": {
            "x_le": lay.x_tail_le.as_m(), "span": lay.h_span.as_m(), "chord": lay.h_chord.as_m(),
            "z": lay.boom_z.as_m() + p.boom.od.as_m() / 2.0 + lay.v_height.as_m(),
            "airfoil": p.tail_airfoil.name, "elevator_chord_frac": 0.3,
        },
        "vtails": {
            "x_le": lay.x_tail_le.as_m(), "height": lay.v_height.as_m(), "chord": lay.v_chord.as_m(),
            "y": lay.boom_y.as_m(), "z_root": lay.boom_z.as_m() + p.boom.od.as_m() / 2.0,
            "airfoil": p.tail_airfoil.name, "rudder_chord_frac": 0.3,
        },
        "pod": {"x0": lay.pod_x0.as_m(), "x1": lay.pod_x1.as_m(), "width": lay.pod_width.as_m(), "height": lay.pod_height.as_m(), "zc": lay.pod_zc.as_m()},
        "booms": {"x0": lay.boom_x0.as_m(), "x1": lay.boom_x1.as_m(), "y": lay.boom_y.as_m(), "z": lay.boom_z.as_m(), "od": p.boom.od.as_m()},
        "extra_drag_area_m2": {
            "what": "Wright's drag areas for parts AeroBuildup has no component for (motor bells, stopped lift props, legs) and its excrescence allowance, at best-range speed",
            "motors": br.map(|c| c.trim.areas.motors).unwrap_or(0.0),
            "stopped_props": br.map(|c| c.trim.areas.stopped_props).unwrap_or(0.0),
            "legs": br.map(|c| c.trim.areas.legs).unwrap_or(0.0),
            "excrescence": br.map(|c| c.trim.areas.excrescence).unwrap_or(0.0),
        },
        "lift": {
            "rotors": rotors,
            "prop": prop_table(&p.lift_prop.data, lift_rpm),
            "blades": p.lift_prop.blades,
            "max_shaft_w": lift_max.map(|m| m.0),
            "max_rpm": lift_max.map(|m| m.1),
            "motor": p.lift_motor.row.id,
        },
        "pusher": {
            "x": lay.pusher_x.as_m(), "y": 0.0, "z": lay.pusher_z.as_m(),
            "prop": prop_table(&p.cruise_prop.data, cruise_rpm),
            "blades": p.cruise_prop.blades,
            "max_shaft_w": pusher_max.map(|m| m.0),
            "max_rpm": pusher_max.map(|m| m.1),
            "motor": p.cruise_motor.row.id,
        },
        "legs": legs,
        "ardupilot": {
            "AIRSPEED_MIN": e.params.get("AIRSPEED_MIN"),
            "AIRSPEED_CRUISE": e.params.get("AIRSPEED_CRUISE"),
            "Q_ASSIST_SPEED": e.params.get("Q_ASSIST_SPEED"),
            "Q_TRANSITION_MS": 5000, "Q_TRANS_DECEL": 2.0, "Q_BACKTRANS_MS": 3000, "Q_LAND_SPEED_CMS": 50,
            "source": "Wright's vtol.param where it sets them; Q_ defaults from ArduPlane/quadplane.cpp AP_GROUPINFO",
        },
        "analytic": {
            "hover_thrust_each_n": perf.hover.thrust_each.value(),
            "hover_shaft_each_w": hover_op.map(|o| o.shaft.value()),
            "hover_battery_w": perf.hover.power.value(),
            "hover_rpm": hover_op.map(|o| o.rpm),
            "stall_eas_ms": perf.stall_eas.as_mps(),
            "best_range_ms": br.map(|c| c.speed.as_mps()),
            "cruise_alpha_deg": br.map(|c| c.trim.alpha_deg),
            "cruise_cl_wing": br.map(|c| c.trim.cl_wing),
            "cruise_drag_n": br.map(|c| c.trim.drag.value()),
            "cruise_l_over_d": br.map(|c| c.trim.lift_to_drag()),
            "cruise_drag_areas_m2": br.map(|c| serde_json::to_value(c.trim.areas).unwrap_or_default()),
            "cruise_thrust_n": br.map(|c| c.point.thrust.value()),
            "cruise_shaft_w": br.map(|c| c.point.shaft.value()),
            "cruise_rpm": br.map(|c| c.point.rpm),
            "cruise_prop_eff": br.map(|c| c.point.prop_efficiency),
            "cruise_motor_eff": br.map(|c| c.point.motor_efficiency),
            "cruise_battery_w": br.map(|c| c.power.value()),
            "usable_wh": perf.usable.as_wh(),
            "vtol_wh": perf.vtol_energy.as_wh(),
            "climb_wh": perf.climb_energy.as_wh(),
            "range_km": perf.range_km,
            "endurance_min": perf.endurance_min,
            "avionics_w": crate::rules::AVIONICS_POWER_W,
        },
    })
}

/// Write `dir/inputs.json`, run the simulator into `dir`, and return its
/// manifest (`dir/manifest.json`, written by the Python side). A sim that
/// cannot run is an error that says why.
pub fn run(e: &Evaluation, dir: &Path) -> anyhow::Result<Manifest> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("inputs.json"), serde_json::to_string_pretty(&inputs(e))?)?;
    let script = dir.join("vtol_sim.py");
    std::fs::write(&script, SCRIPT)?;
    let py = std::env::var("WRIGHT_PYTHON").unwrap_or_else(|_| "python3".into());
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/air");
    let o = std::process::Command::new(&py)
        .arg("-I")
        .arg(&script)
        .arg(dir.join("inputs.json"))
        .arg(dir)
        .arg(&data)
        .output()
        .map_err(|err| anyhow::anyhow!("could not start {py} (set WRIGHT_PYTHON to an interpreter with jsbsim and aerosandbox): {err}"))?;
    if !o.status.success() {
        let tail: Vec<String> = String::from_utf8_lossy(&o.stderr).lines().rev().take(8).map(str::to_string).collect();
        anyhow::bail!("{py} vtol_sim.py exited {}: {}", o.status, tail.into_iter().rev().collect::<Vec<_>>().join(" | "));
    }
    let raw: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    let mut m = Manifest::new("sim", raw["title"].as_str().unwrap_or("6-DOF flight simulation"));
    for it in raw["items"].as_array().into_iter().flatten() {
        let metrics: Vec<(String, f64)> = it["metrics"].as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_f64().map(|x| (k.clone(), x))).collect()).unwrap_or_default();
        let refs: Vec<(&str, f64)> = metrics.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let path = dir.join(it["path"].as_str().unwrap_or(""));
        m.add(dir, &path, it["label"].as_str().unwrap_or(""), it["summary"].as_str().unwrap_or(""), &refs);
    }
    let summary: Value = serde_json::from_slice(&std::fs::read(dir.join("sim_summary.json"))?)?;
    let done = summary["completed"].as_bool().unwrap_or(false);
    let events = &summary["events_s"];
    m.check("sim_mission_completed", done, &if done {
        "takeoff, hover, transition, cruise, back-transition and landing flown to touchdown".to_string()
    } else {
        format!("did not complete: {}", events["aborted"].as_str().unwrap_or("no touchdown"))
    });
    if let Some(r) = summary["cruise"]["battery_vs_analytic"].as_f64() {
        m.check(
            "sim_cruise_power_vs_analytic",
            r.abs() <= SIM_CRUISE_TOLERANCE,
            &format!("cruise battery power {:+.1} % against the analytic model (tolerance {:.0} %)", 100.0 * r, 100.0 * SIM_CRUISE_TOLERANCE),
        );
    }
    Ok(m)
}

/// How far the sim's cruise power may sit from the analytic model before
/// the sim says the model is off: AeroBuildup and Wright's port of it share
/// method but not every component (ASSUMPTION: 15 %). The sim's tables keep
/// AeroBuildup's k_e,D0 in the induced drag, which `aero::trim` drops as a
/// double count (see aero.rs), so the sim reads a few percent high.
pub const SIM_CRUISE_TOLERANCE: f64 = 0.15;

/// The pipeline stage: `<out_dir>/sim/`.
pub struct SimStage;

impl Stage for SimStage {
    fn name(&self) -> &'static str {
        "sim"
    }
    fn applies_to(&self, domain: &str) -> bool {
        domain == "aircraft"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
        let d: crate::spec::Design = serde_json::from_value(ctx.design.clone())?;
        let mut d = d;
        if let Ok(spec) = serde_json::from_value::<crate::spec::Spec>(ctx.spec.clone()) {
            d.requirements = spec.requirements;
        }
        let e = crate::checks::evaluate(&d, crate::powertrain::Factors::NOMINAL).map_err(anyhow::Error::msg)?;
        run(&e, &ctx.dir)
    }
}
