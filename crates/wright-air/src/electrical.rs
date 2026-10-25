//! The electrical system: power budget, harness, connector, servo power.
//!
//! The SIZING rule for a wire is the sellers' table (continuous rating at
//! least 1.25 x the run's continuous current and its peak, under 2 % drop),
//! restricted to gauges the catalog sells. The CHECK is a different model:
//! the conductor's temperature from its own resistance -- steady state at the
//! continuous current against natural convection, then the burst heating the
//! copper adiabatically for `BURST_S` -- against the insulation's rating. The
//! prototype's `wire_ampacity` restated the sizing inequality.

use crate::layout::Layout;
use crate::perf::Performance;
use crate::rules::*;

#[derive(Debug, Clone)]
pub struct Run {
    pub name: String,
    pub conductors: u32,
    pub continuous_a: f64,
    pub peak_a: f64,
    pub length_m: f64,
    pub awg: u32,
    pub volts: f64,
}

#[derive(Debug, Clone)]
pub struct Electrical {
    pub runs: Vec<Run>,
    pub connector: (&'static str, f64, f64, &'static str),
    /// The current the lift motors are held under (Q_M_BAT_CURR_MAX).
    pub current_cap_a: f64,
    pub pack_hover_a: f64,
    pub pack_peak_a: f64,
    pub servo_demand_a: f64,
    pub hover_motor_a: f64,
    pub lift_peak_motor_a: f64,
}

pub fn sellable_awg(g: u32) -> Option<String> {
    let id = format!("pyrodrone_silicone_wire_{g}awg_1m_pair");
    crate::catalog::catalog().hardware.contains_key(&id).then_some(id)
}

/// The sizing rule: thinnest sold gauge rated for 1.25 x continuous and the
/// peak, under 2 % drop over the round trip at the continuous current.
pub fn gauge_rule(cont: f64, peak: f64, len: f64, volts: f64) -> u32 {
    let mut sold: Vec<&(u32, f64, f64, f64, f64)> = WIRE_AWG.iter().filter(|w| sellable_awg(w.0).is_some()).collect();
    sold.sort_by_key(|w| std::cmp::Reverse(w.0)); // thinnest first
    for w in &sold {
        let drop = cont * COPPER_RESISTIVITY * 2.0 * len / (w.1 * 1e-6);
        if w.2 >= (1.25 * cont).max(peak) && drop <= WIRE_DROP_MAX * volts {
            return w.0;
        }
    }
    sold.last().map(|w| w.0).unwrap_or(10)
}

/// The check's model: (temperature at the end of a burst, C; steady C).
pub fn wire_temperature(awg: u32, cont: f64, peak: f64) -> (f64, f64) {
    let w = WIRE_AWG.iter().find(|w| w.0 == awg).copied().unwrap_or(WIRE_AWG[0]);
    let area = w.1 * 1e-6;
    let d_out = w.4 * 1e-3;
    let r20 = COPPER_RESISTIVITY / area; // ohm/m
    let r_at = |t: f64| r20 * (1.0 + COPPER_ALPHA * (t - 20.0));
    // steady: I^2 R(T) = h pi D (T - Ta)
    let mut t = AMBIENT_HOT_C;
    for _ in 0..50 {
        t = AMBIENT_HOT_C + cont * cont * r_at(t) / (WIRE_H_W_M2K * std::f64::consts::PI * d_out);
    }
    let steady = t;
    // burst: the copper alone heats (adiabatic, conservative), 1 s steps
    let heat_cap = COPPER_DENSITY * area * COPPER_CP; // J/(m K)
    let mut tb = steady;
    for _ in 0..(BURST_S as usize) {
        tb += peak * peak * r_at(tb) / heat_cap;
    }
    (tb, steady)
}

/// The sizing rule: the smallest connector rated for 1.25 x hover current
/// and, when given, a burst rating over the uncapped peak.
pub fn connector_for(hover_a: f64, peak_a: Option<f64>) -> (&'static str, f64, f64, &'static str) {
    CONNECTORS.iter().copied().find(|c| 1.25 * hover_a <= c.1 && peak_a.is_none_or(|p| p <= c.2)).unwrap_or(CONNECTORS[CONNECTORS.len() - 1])
}

/// The connector by hover current alone (what the current cap is built on).
pub fn connector(hover_a: f64) -> (&'static str, f64, f64, &'static str) {
    connector_for(hover_a, None)
}

/// The current cap for the lift motors: the pack's burst and the
/// connector's, whichever is less (ArduPilot holds it via Q_M_BAT_CURR_MAX).
pub fn current_cap(lay: &Layout, hover_a: f64, peak_a: f64) -> f64 {
    lay.battery.pack.burst.as_a().min(connector_for(hover_a, Some(peak_a)).2)
}

pub fn design(lay: &Layout, perf: &Performance) -> Electrical {
    let h = &perf.hover;
    let pack = &lay.battery.pack;
    let v_nom = pack.s as f64 * pack.cell_nominal.value();
    let hover_a = h.hot_low.as_ref().map(|o| o.motor_a.as_a()).unwrap_or(f64::NAN);
    let lift_peak = h.full.motor_a.as_a();
    let pusher_cont = perf.best_range.as_ref().map(|b| b.point.motor_a.as_a()).unwrap_or(f64::NAN);
    let pusher_peak = perf.pusher_full_current.as_a();
    let stall = lay.parts.servo.stall_current.map(|c| c.as_a()).unwrap_or(SERVO_STALL_A);
    let servos_w = 5.0 * stall * SERVO_RAIL_V;
    let pack_hover = h.pack_current_low.as_a();
    // the worst seconds: the lift motors at full throttle with NO current
    // limit (independent of the cap, which is built from the same ratings
    // this current is checked against), or hover plus the pusher flat out
    let pack_peak = h.free.pack_a.as_a().max(pack_hover + (pusher_peak * v_nom / perf.factors.esc_eff() + servos_w) / v_nom);
    let con = connector_for(pack_hover, Some(pack_peak));
    // routes: Manhattan along the pod, out along the wing, along the boom
    let bay = (lay.pod_full_x0.as_m() + 0.05, 0.0);
    let batt = (lay.battery_x.as_m(), 0.0);
    let path = |a: (f64, f64), b: (f64, f64)| (WIRE_ROUTING * ((a.0 - b.0).abs() + (a.1 - b.1).abs())).max(0.05);
    let mut runs = vec![];
    let mut add = |name: String, n: u32, cont: f64, peak: f64, len: f64, volts: f64| {
        let awg = gauge_rule(cont, peak, len, volts);
        runs.push(Run { name, conductors: n, continuous_a: cont, peak_a: peak.max(cont), length_m: len, awg, volts });
    };
    add("battery lead".into(), 2, pack_hover, pack_peak, path(batt, bay).max(0.15), v_nom);
    add("lift ESC feed".into(), 2, pack_hover, h.full.pack_a.as_a(), 0.08, v_nom);
    let by = lay.boom_y.as_m();
    for (tag, x, y) in [("FR", lay.x_front, by), ("RR", lay.x_rear, by), ("FL", lay.x_front, -by), ("RL", lay.x_rear, -by)] {
        add(format!("lift motor {tag} phases"), 3, hover_a, lift_peak, path(bay, (x.as_m(), y)), v_nom);
    }
    let cone = (lay.pod_full_x1.as_m() + 0.02, 0.0);
    add("pusher ESC feed".into(), 2, pusher_cont, pusher_peak, path(bay, cone), v_nom);
    add("pusher phases".into(), 3, pusher_cont, pusher_peak, path(cone, (lay.pusher_x.as_m(), 0.0)), v_nom);
    let ys = lay.aileron_servo[1].as_m();
    let xt = lay.x_tail_le.as_m();
    for (n, pt) in [("aileron R", (lay.aileron_servo[0].as_m(), ys)), ("aileron L", (lay.aileron_servo[0].as_m(), -ys)), ("elevator", (xt, by)), ("rudder R", (xt, by)), ("rudder L", (xt, -by))] {
        add(format!("servo {n}"), 3, SERVO_AVERAGE_A, stall, path(bay, pt), SERVO_RAIL_V);
    }
    Electrical {
        runs,
        connector: con,
        current_cap_a: current_cap(lay, pack_hover, pack_peak),
        pack_hover_a: pack_hover,
        pack_peak_a: pack_peak,
        servo_demand_a: 5.0 * stall,
        hover_motor_a: hover_a,
        lift_peak_motor_a: lift_peak,
    }
}

pub fn wire_mass_kg(e: &Electrical) -> f64 {
    e.runs
        .iter()
        .map(|r| WIRE_AWG.iter().find(|w| w.0 == r.awg).map(|w| w.3).unwrap_or(0.0) / 1000.0 * r.length_m * r.conductors as f64)
        .sum()
}

