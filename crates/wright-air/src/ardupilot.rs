//! An ArduPilot QuadPlane parameter file, validated against ArduPlane's own
//! metadata (`data/air/ardupilot_params.json`, from apm.pdef.json; ArduPilot
//! is GPL-3.0 and is read for its interface only).
//!
//! Outputs: lift motors on SERVO1-4 (Motor1-4, DShot600), pusher SERVO5,
//! ailerons 6-7, elevator 8, rudders 9-10. ArduPilot's Quad X numbering
//! (AP_MotorsMatrix, the documented frame diagram): Motor1 front right CCW,
//! Motor2 rear left CCW, Motor3 front left CW, Motor4 rear right CW.

use crate::electrical::Electrical;
use crate::layout::Layout;
use crate::perf::Performance;
use crate::powertrain::{max_thrust, operate, Factors};
use crate::rules::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use wright_core::units::{Current, Density, Force, Speed};

/// (output, ArduPilot motor, geometry tag, spin).
pub const MOTOR_ORDER: [(u32, &str, &str); 4] = [(1, "FR", "CCW"), (2, "RL", "CCW"), (3, "FL", "CW"), (4, "RR", "CW")];

fn meta() -> &'static BTreeMap<String, Value> {
    static M: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    M.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../../../data/air/ardupilot_params.json")).expect("params json");
        v["params"].as_object().expect("params object").iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    })
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Params {
    pub values: Vec<(String, f64, String)>,
}

impl Params {
    fn set(&mut self, k: &str, v: f64, why: &str) {
        self.values.push((k.into(), v, why.into()));
    }
    pub fn get(&self, k: &str) -> Option<f64> {
        self.values.iter().find(|p| p.0 == k).map(|p| p.1)
    }
    pub fn text(&self, header: &str) -> String {
        let mut s: String = header.lines().map(|l| format!("# {l}\n")).collect();
        for (k, v, why) in &self.values {
            let vs = if v.fract() == 0.0 { format!("{}", *v as i64) } else { format!("{v}") };
            if why.is_empty() {
                s += &format!("{k},{vs}\n");
            } else {
                s += &format!("{k},{vs}  # {why}\n");
            }
        }
        s
    }
}

/// Problems against the metadata: unknown names, values out of range, or
/// not one of the published labels.
pub fn validate(p: &Params) -> Vec<String> {
    let m = meta();
    let mut out = vec![];
    for (k, v, _) in &p.values {
        let Some(e) = m.get(k) else {
            out.push(format!("{k} is not an ArduPlane parameter"));
            continue;
        };
        if !v.is_finite() {
            out.push(format!("{k}={v} is not a number"));
            continue;
        }
        if let Some(r) = e.get("range").and_then(Value::as_array) {
            let (lo, hi) = (r[0].as_f64().unwrap_or(f64::MIN), r[1].as_f64().unwrap_or(f64::MAX));
            if *v < lo || *v > hi {
                out.push(format!("{k}={v} is outside {lo}..{hi}"));
            }
        }
        if let Some(vals) = e.get("values").and_then(Value::as_object) {
            if e.get("bitmask").is_none() && v.fract() == 0.0 && !vals.contains_key(&format!("{}", *v as i64)) {
                out.push(format!("{k}={v} is not one of the published values"));
            }
        }
    }
    out
}

/// Q_M_THST_EXPO fitted to the lift unit: ArduPilot maps thrust fraction t
/// to actuator u by t = (1 - e) u + e u^2 (AP_MotorsMulticopter
/// thrust_to_actuator); least squares on the motor model's duty-thrust pairs.
pub fn thrust_expo(lay: &Layout) -> f64 {
    let p = &lay.parts;
    let v = lay.battery.pack.voltage(Current::a(0.0), false);
    let rho = Density::kg_m3(1.225);
    let (tmax, _) = max_thrust(p.lift_motor, &p.lift_prop.data, Speed::mps(0.0), rho, v);
    let mut pairs = vec![];
    for f in [0.15, 0.3, 0.45, 0.6, 0.75, 0.9] {
        if let Ok(op) = operate(p.lift_motor, &p.lift_prop.data, Force::n(f * tmax.value()), Speed::mps(0.0), rho, v, Factors::NOMINAL) {
            pairs.push((op.throttle, f));
        }
    }
    if pairs.len() < 3 {
        return 0.65;
    }
    let num: f64 = pairs.iter().map(|(u, t)| (t - u) * (u * u - u)).sum();
    let den: f64 = pairs.iter().map(|(u, _)| (u * u - u).powi(2)).sum();
    ((num / den).clamp(0.0, 1.0) * 100.0).round() / 100.0
}

pub fn build(lay: &Layout, perf: &Performance, e: &Electrical) -> Params {
    let mut p = Params::default();
    let pack = &lay.battery.pack;
    p.set("Q_ENABLE", 1.0, "");
    p.set("Q_FRAME_CLASS", 1.0, "Quad");
    p.set("Q_FRAME_TYPE", 1.0, "X: Motor1 FR CCW, Motor2 RL CCW, Motor3 FL CW, Motor4 RR CW");
    p.set("Q_M_PWM_TYPE", 6.0, "DShot600 to the 4-in-1 ESC");
    for n in 1..=4 {
        p.set(&format!("SERVO{n}_FUNCTION"), 32.0 + n as f64, "");
    }
    p.set("SERVO5_FUNCTION", 70.0, "pusher throttle");
    p.set("SERVO6_FUNCTION", 4.0, "left aileron");
    p.set("SERVO7_FUNCTION", 4.0, "right aileron: set SERVO7_REVERSED after the surface check");
    p.set("SERVO8_FUNCTION", 19.0, "elevator");
    p.set("SERVO9_FUNCTION", 21.0, "left rudder");
    p.set("SERVO10_FUNCTION", 21.0, "right rudder");
    // ArduPilot's airspeed limits are EAS (AP_TECS scales them by EAS2TAS);
    // the stall is the CLmax-corrected one (`Performance::cl_max_used`)
    let stall = perf.stall_eas.as_mps();
    p.set("AIRSPEED_MIN", (1.25 * stall).ceil(), "1.25 x predicted stall");
    if let Some(br) = &perf.best_range {
        p.set("AIRSPEED_CRUISE", (br.speed.as_mps() * 10.0).round() / 10.0, "best-range speed");
        p.set("TRIM_THROTTLE", (100.0 * br.point.throttle).round(), "pusher duty at best range");
        p.set("SCALING_SPEED", (br.speed.as_mps() * 10.0).round() / 10.0, "");
        if let Some(be) = &perf.best_endurance {
            let sink = be.speed.as_mps() / be.trim.lift_to_drag().max(1.0);
            p.set("TECS_SINK_MIN", (sink.clamp(0.5, 10.0) * 10.0).round() / 10.0, "power-off sink at min-power speed");
        }
    }
    let amin = p.get("AIRSPEED_MIN").unwrap_or(10.0);
    p.set("AIRSPEED_MAX", (0.9 * perf.max_speed.as_mps()).floor().max(amin + 3.0), "90 % of predicted top speed");
    p.set("Q_ASSIST_SPEED", ((1.1 * stall) * 10.0).round() / 10.0, "quad assist below 1.1 x stall");
    let hover = perf.hover.std_full.as_ref().map(|o| o.throttle).unwrap_or(0.5);
    p.set("Q_M_THST_HOVER", ((hover.clamp(0.125, 0.6875)) * 1000.0).round() / 1000.0, "predicted; Q_M_HOVER_LEARN refines it");
    p.set("Q_M_HOVER_LEARN", 2.0, "learn and save");
    p.set("Q_M_THST_EXPO", thrust_expo(lay), "fitted to the lift unit's thrust curve");
    p.set("Q_M_BAT_VOLT_MAX", (pack.s as f64 * pack.cell_max.value() * 10.0).round() / 10.0, "");
    p.set("Q_M_BAT_VOLT_MIN", (pack.s as f64 * pack.cell_min.value().max(3.0) * 10.0).round() / 10.0, "");
    p.set("Q_M_BAT_CURR_MAX", e.current_cap_a.floor().min(200.0), "pack and connector burst; hover T/W is computed under it");
    p.set("Q_TRANSITION_MS", (TRANSITION_S * 1000.0 / 3.0).round(), "");
    p.set("Q_RTL_MODE", 1.0, "");
    p.set("RTL_AUTOLAND", 0.0, "");
    if lay.parts.airspeed.is_some() {
        p.set("ARSPD_TYPE", 1.0, "MS4525 on I2C");
        p.set("ARSPD_USE", 1.0, "");
    } else {
        p.set("ARSPD_TYPE", 0.0, "no airspeed sensor");
        p.set("ARSPD_USE", 0.0, "");
    }
    p.set("BATT_MONITOR", 4.0, "the board's analog voltage and current sensor");
    let cap_mah = (pack.capacity().as_mah()).round();
    p.set("BATT_CAPACITY", cap_mah, "");
    let li = pack.chemistry == crate::catalog::Chemistry::LiIon;
    p.set("BATT_ARM_VOLT", (pack.s as f64 * (pack.cell_nominal.value() + 0.1) * 10.0).round() / 10.0, "");
    p.set("BATT_LOW_VOLT", (pack.s as f64 * if li { 3.3 } else { 3.5 } * 10.0).round() / 10.0, "");
    p.set("BATT_CRT_VOLT", (pack.s as f64 * if li { 3.1 } else { 3.3 } * 10.0).round() / 10.0, "");
    let v_nom = pack.s as f64 * pack.cell_nominal.value();
    let landing_mah = perf.hover.power_low.as_w() * HOVER_LANDING_S / 3600.0 / v_nom * 1000.0;
    p.set("BATT_LOW_MAH", (cap_mah * ENERGY_RESERVE + 2.0 * landing_mah).round(), "planned reserve plus two landing hovers");
    p.set("BATT_CRT_MAH", (cap_mah * 0.05 + landing_mah).round(), "");
    p.set("BATT_FS_LOW_ACT", 1.0, "RTL");
    p.set("BATT_FS_CRT_ACT", 4.0, "QLand");
    p.set("RSSI_TYPE", 3.0, "from the receiver protocol");
    p.set("FLTMODE_CH", 5.0, "");
    for (i, m) in [18.0, 19.0, 5.0, 7.0, 10.0, 21.0].iter().enumerate() {
        p.set(&format!("FLTMODE{}", i + 1), *m, "QHOVER QLOITER FBWA CRUISE AUTO QRTL");
    }
    p.set("ROLL_LIMIT_DEG", 45.0, "");
    p.set("PTCH_LIM_MAX_DEG", 20.0, "");
    p.set("PTCH_LIM_MIN_DEG", -25.0, "");
    p.set("STALL_PREVENTION", 1.0, "");
    p.set("TKOFF_ALT", 20.0, "");
    p
}
