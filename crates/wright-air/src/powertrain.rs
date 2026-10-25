//! Motors, ESCs and the pack: where a propeller's shaft power comes from.
//!
//! The motor is Mark Drela's first-order model (MIT, "First-Order DC
//! Electric Motor Model", motor1_theory.pdf), the equations AeroSandbox's
//! `motor_electric_performance` iterates:
//!
//! ```text
//! I = Q Kv' + I0          (Kv' in rad/s per volt)
//! V = omega / Kv' + I R
//! ```
//!
//! The ESC duty (ArduPilot's throttle) is motor terminal volts over pack
//! volts; the pack supplies P_elec / eta_esc. The pack's terminal voltage is
//! `S (V_ocv - I R_cell / P)`.
//!
//! [`full_throttle`] is the system solve the prototype lacked: four motors at
//! full throttle on ONE sagging pack, capped where a motor reaches the
//! sagged voltage or the motor's, the ESC's or the pack's burst rating binds.

use crate::catalog::{Cell, Chemistry, Motor};
use crate::prop::PropData;
use crate::rules;
use wright_core::units::{Charge, Current, Density, Energy, Force, Mass, Power, Speed, Voltage};

/// Scales that make the whole powertrain worse for the pessimistic model.
#[derive(Debug, Clone, Copy)]
pub struct Factors {
    /// Multiplies every zero-lift drag component (1.0 nominal).
    pub parasite: f64,
    /// Multiplies the ESC efficiency (1.0 nominal).
    pub powertrain: f64,
}

impl Factors {
    pub const NOMINAL: Factors = Factors { parasite: 1.0, powertrain: 1.0 };
    /// Parasite drag +20 %, powertrain 10 % worse.
    pub const PESSIMISTIC: Factors = Factors { parasite: 1.2, powertrain: 0.9 };
    pub fn esc_eff(&self) -> f64 {
        rules::ESC_EFFICIENCY * self.powertrain
    }
}

impl Motor {
    fn kv_rad(&self) -> f64 {
        self.kv * std::f64::consts::PI / 30.0
    }
    /// The most terminal voltage the motor makes good on `supply` volts at
    /// full throttle: `voltage_factor` < 1 where the maker's own 100 % row
    /// shows less rpm than Kv and R predict (only the full-throttle ceiling;
    /// part-throttle efficiency is the model's).
    pub fn ceiling(&self, supply: f64) -> f64 {
        supply * self.voltage_factor
    }
    /// (terminal volts, amps) at `rpm` and shaft torque `torque_nm`.
    pub fn terminal(&self, rpm: f64, torque_nm: f64) -> (f64, f64) {
        let k = self.kv_rad();
        let i = torque_nm * k + self.no_load_current.value();
        let w = rpm * std::f64::consts::PI / 30.0;
        (w / k + i * self.resistance_ohm, i)
    }
}

/// The pack's electrical model: cells S x P (a bought pack's weighed mass
/// and ratings beat the cells').
#[derive(Debug, Clone)]
pub struct Pack {
    pub label: String,
    pub chemistry: Chemistry,
    pub s: u32,
    pub p: u32,
    pub cell_capacity: Charge,
    pub cell_nominal: Voltage,
    pub cell_max: Voltage,
    pub cell_min: Voltage,
    pub cell_ocv_low: Voltage,
    pub cell_resistance_ohm: f64,
    pub mass: Mass,
    pub max_cont: Current,
    pub burst: Current,
}

impl Pack {
    pub fn from_cells(cell: &Cell, s: u32, p: u32) -> Pack {
        let burst = cell.pulse.unwrap_or(cell.max_cont) * p as f64;
        Pack {
            label: format!("hand-built {s}S{p}P {}", cell.row.name),
            chemistry: cell.chemistry,
            s,
            p,
            cell_capacity: cell.capacity,
            cell_nominal: cell.nominal,
            cell_max: cell.max_v,
            cell_min: cell.min_v,
            cell_ocv_low: cell.ocv_low,
            cell_resistance_ohm: cell.resistance_ohm,
            mass: cell.mass * (s * p) as f64 * rules::PACK_OVERHEAD,
            max_cont: cell.max_cont * p as f64,
            burst,
        }
    }
    pub fn capacity(&self) -> Charge {
        self.cell_capacity * self.p as f64
    }
    pub fn energy(&self) -> Energy {
        Energy::wh(self.capacity().as_ah() * self.cell_nominal.value() * self.s as f64)
    }
    pub fn usable(&self) -> Energy {
        self.energy()
            * match self.chemistry {
                Chemistry::LiIon => rules::USABLE_FRACTION_LIION,
                Chemistry::LiPo => rules::USABLE_FRACTION_LIPO,
            }
    }
    fn ocv(&self, low: bool) -> f64 {
        if low { self.cell_ocv_low.value() } else { self.cell_nominal.value() }
    }
    /// Terminal voltage at `current` (end of flight when `low`).
    pub fn voltage(&self, current: Current, low: bool) -> Voltage {
        Voltage::v(self.s as f64 * (self.ocv(low) - current.value() / self.p as f64 * self.cell_resistance_ohm))
    }
    /// Terminal voltage and current when drawing `power` (V = s ocv - (s R/p) I,
    /// I = P/V: the larger root). None when the pack cannot deliver it.
    pub fn deliver(&self, power: Power, low: bool) -> Option<(Voltage, Current)> {
        let (s, r, p) = (self.s as f64, self.cell_resistance_ohm, self.p as f64);
        let e = s * self.ocv(low);
        let disc = e * e - 4.0 * (s * r / p) * power.value();
        if disc < 0.0 {
            return None;
        }
        let v = (e + disc.sqrt()) / 2.0;
        Some((Voltage::v(v), Current::a(power.value() / v)))
    }
    pub fn full_voltage(&self) -> Voltage {
        self.cell_max * self.s as f64
    }
}

/// One motor and prop at one condition.
#[derive(Debug, Clone)]
pub struct OperatingPoint {
    pub thrust: Force,
    pub rpm: f64,
    pub shaft: Power,
    pub motor_v: Voltage,
    pub motor_a: Current,
    pub electrical: Power,
    /// Drawn from the pack through the ESC.
    pub battery: Power,
    /// ESC duty, 0..1.
    pub throttle: f64,
    pub prop_efficiency: f64,
    pub motor_efficiency: f64,
}

/// Why an operating point does not exist.
#[derive(Debug, Clone)]
pub struct Infeasible(pub String);

/// The point making `thrust` at airspeed `v` on `pack_v`.
pub fn operate(
    m: &Motor,
    p: &PropData,
    thrust: Force,
    v: Speed,
    rho: Density,
    pack_v: Voltage,
    f: Factors,
) -> Result<OperatingPoint, Infeasible> {
    let rpm_cap = m.kv * pack_v.value();
    let Some(rpm) = p.rpm_for_thrust(thrust, v, rho, rpm_cap) else {
        return Err(Infeasible(format!(
            "{:.2} N at {:.1} m/s needs more than {:.0} rpm (Kv x pack volts)",
            thrust.as_n(),
            v.as_mps(),
            rpm_cap
        )));
    };
    let (t, shaft) = p.thrust_power(rpm, v, rho);
    let torque = shaft.value() / (rpm * std::f64::consts::PI / 30.0);
    let (volts, amps) = m.terminal(rpm, torque);
    if volts > pack_v.value() {
        return Err(Infeasible(format!("needs {volts:.1} V at the motor, the pack gives {:.1} V", pack_v.value())));
    }
    if amps > m.max_current.value() {
        return Err(Infeasible(format!("draws {amps:.1} A, past {}'s {:.0} A", m.row.id, m.max_current.value())));
    }
    let elec = volts * amps;
    Ok(OperatingPoint {
        thrust: t,
        rpm,
        shaft,
        motor_v: Voltage::v(volts),
        motor_a: Current::a(amps),
        electrical: Power::w(elec),
        battery: Power::w(elec / f.esc_eff()),
        throttle: (volts / pack_v.value()).min(1.0),
        prop_efficiency: if shaft.value() > 0.0 { t.value() * v.value() / shaft.value() } else { 0.0 },
        motor_efficiency: if elec > 0.0 { shaft.value() / elec } else { 0.0 },
    })
}

/// `n` identical units at full throttle on one pack, solved together.
#[derive(Debug, Clone)]
pub struct FullThrottle {
    pub thrust_each: Force,
    pub motor_a: Current,
    pub pack_a: Current,
    pub pack_v: Voltage,
    /// What stopped it: "voltage", "pack minimum voltage" (the pack sagged to
    /// its cells' minimum), "motor rating", "ESC rating", "pack burst".
    pub limit: &'static str,
}

#[allow(clippy::too_many_arguments)]
pub fn full_throttle(
    m: &Motor,
    p: &PropData,
    pack: &Pack,
    n: u32,
    v: Speed,
    rho: Density,
    low: bool,
    other: Power,
    esc_limit: Option<Current>,
    f: Factors,
) -> FullThrottle {
    let state = |rpm: f64| {
        let (t, shaft) = p.thrust_power(rpm, v, rho);
        let torque = shaft.value() / (rpm * std::f64::consts::PI / 30.0);
        let (volts, amps) = m.terminal(rpm, torque);
        let power = Power::w(n as f64 * volts * amps / f.esc_eff()) + other;
        let Some((pv, pi)) = pack.deliver(power, low) else {
            return (t, amps, Current::a(f64::INFINITY), Voltage::v(0.0), "pack burst");
        };
        let lim = if volts > m.ceiling(pv.value()) {
            "voltage"
        } else if pv.value() < pack.s as f64 * pack.cell_min.value() {
            "pack minimum voltage"
        } else if amps > m.max_current.value() {
            "motor rating"
        } else if esc_limit.is_some_and(|e| amps > e.value()) {
            "ESC rating"
        } else if pi > pack.burst {
            "pack burst"
        } else {
            ""
        };
        (t, amps, pi, pv, lim)
    };
    let mut lo = 300.0;
    let mut hi = m.kv * pack.s as f64 * pack.ocv(low);
    let mut binding = "voltage";
    if state(hi).4.is_empty() {
        lo = hi;
    } else {
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            let l = state(mid).4;
            if l.is_empty() {
                lo = mid;
            } else {
                hi = mid;
                binding = l;
            }
        }
    }
    let (t, amps, pi, pv, _) = state(lo);
    FullThrottle { thrust_each: t, motor_a: Current::a(amps), pack_a: pi, pack_v: pv, limit: binding }
}

/// `n` units at full throttle on a FULL pack with nothing but the motors'
/// own voltage to stop them: no current limit, no rating. What a motor,
/// ESC, pack and connector must survive when the autopilot saturates the
/// throttles (a yaw or attitude correction can drive any one motor to 100 %;
/// the battery-current limit caps only the total). "pack collapse" when the
/// pack cannot deliver the power at all.
#[allow(clippy::too_many_arguments)]
pub fn free_full_throttle(m: &Motor, p: &PropData, pack: &Pack, n: u32, v: Speed, rho: Density, other: Power, f: Factors, esc_limit: Option<Current>) -> FullThrottle {
    let state = |rpm: f64| {
        let (t, shaft) = p.thrust_power(rpm, v, rho);
        let torque = shaft.value() / (rpm * std::f64::consts::PI / 30.0);
        let (volts, amps) = m.terminal(rpm, torque);
        let power = Power::w(n as f64 * volts * amps / f.esc_eff()) + other;
        match pack.deliver(power, false) {
            None => (t, amps, Current::a(f64::INFINITY), Voltage::v(0.0), "pack collapse"),
            Some((pv, pi)) => (
                t,
                amps,
                pi,
                pv,
                if volts > m.ceiling(pv.value()) {
                    "voltage"
                } else if esc_limit.is_some_and(|l| amps > l.value()) {
                    "ESC current limit"
                } else {
                    ""
                },
            ),
        }
    };
    let (mut lo, mut hi) = (300.0, m.kv * pack.full_voltage().value());
    let mut binding = "voltage";
    if state(hi).4.is_empty() {
        lo = hi;
    } else {
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            let l = state(mid).4;
            if l.is_empty() {
                lo = mid;
            } else {
                hi = mid;
                binding = l;
            }
        }
    }
    let (t, amps, pi, pv, _) = state(lo);
    FullThrottle { thrust_each: t, motor_a: Current::a(amps), pack_a: pi, pack_v: pv, limit: binding }
}

/// One unit flat out on a stiff `supply_v` with no current limit:
/// (thrust, amps) where the motor's terminal voltage meets the supply.
pub fn free_running(m: &Motor, p: &PropData, rho: Density, supply_v: f64) -> (Force, f64) {
    let (mut lo, mut hi) = (100.0, m.kv * supply_v * 1.2);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let (_, sh) = p.thrust_power(mid, Speed::mps(0.0), rho);
        let (v, _) = m.terminal(mid, sh.value() / (mid * std::f64::consts::PI / 30.0));
        if v > m.ceiling(supply_v) {
            hi = mid
        } else {
            lo = mid
        }
    }
    let (t, sh) = p.thrust_power(lo, Speed::mps(0.0), rho);
    (t, m.terminal(lo, sh.value() / (lo * std::f64::consts::PI / 30.0)).1)
}

/// One unit at full throttle on a stiff `pack_v`: the most thrust before
/// the motor reaches that voltage or its current rating.
pub fn max_thrust(m: &Motor, p: &PropData, v: Speed, rho: Density, pack_v: Voltage) -> (Force, Current) {
    let limited = |rpm: f64| {
        let (_, shaft) = p.thrust_power(rpm, v, rho);
        let (volts, amps) = m.terminal(rpm, shaft.value() / (rpm * std::f64::consts::PI / 30.0));
        (volts > m.ceiling(pack_v.value()) || amps > m.max_current.value(), amps)
    };
    let (mut lo, mut hi) = (300.0, m.kv * pack_v.value());
    if !limited(hi).0 {
        lo = hi;
    } else {
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if limited(mid).0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
    }
    (p.thrust_power(lo, v, rho).0, Current::a(limited(lo).1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::catalog;
    #[test]
    fn reference_lift_unit_hovers_near_reported_throttle() {
        let c = catalog();
        let m = &c.motors["aos_supernova_2807_1400kv"];
        let p = &c.props["gemfan_flash_7042_2blade"].data;
        // 2.55 kg / 4 = 6.25 N each on a 6S Li-ion pack at ~21 V
        let op = operate(m, p, Force::n(6.4), Speed::mps(0.0), Density::kg_m3(1.225), Voltage::v(20.5), Factors::NOMINAL)
            .unwrap();
        assert!(op.throttle > 0.35 && op.throttle < 0.6, "throttle {}", op.throttle);
    }
    #[test]
    fn four_on_one_pack_make_less_than_four_alone() {
        let c = catalog();
        let m = &c.motors["aos_supernova_2807_1400kv"];
        let p = &c.props["gemfan_flash_7042_2blade"].data;
        let pk = &c.packs["upgrade_energy_gold_v1_6s2p_22ah_sa08"];
        let cell = &c.cells["amprius_sa08"];
        let mut pack = Pack::from_cells(cell, 6, 2);
        pack.burst = pk.burst.unwrap();
        let rho = Density::kg_m3(1.17);
        let alone = max_thrust(m, p, Speed::mps(0.0), rho, pack.voltage(Current::a(0.0), true)).0;
        let ft = full_throttle(m, p, &pack, 4, Speed::mps(0.0), rho, true, Power::w(36.0), None, Factors::NOMINAL);
        assert!(ft.thrust_each < alone, "{} vs {}", ft.thrust_each.as_n(), alone.as_n());
    }
}

#[cfg(test)]
mod thrust_tables {
    use super::*;
    use crate::catalog::catalog;
    use crate::rules::{THRUST_AT_CURRENT_OVER, THRUST_CALIBRATED_OVER, THRUST_VOLTAGE_LIMITED_OVER};

    /// Every maker 100 % row whose prop is in the catalog, three ways: the
    /// model's thrust at the maker's current; free-running at the maker's
    /// voltage uncalibrated; and calibrated, on rows the fit did not use.
    #[test]
    fn makers_full_throttle_rows_bound_the_model() {
        let c = catalog();
        let rho = Density::kg_m3(1.2);
        let (mut at_i, mut free, mut cal) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let (mut n, mut n_cal) = (0, 0);
        for m in c.motors.values() {
            for t in m.table.iter().filter(|t| (t.throttle - 1.0).abs() < 1e-9) {
                let Some(p) = c.props.values().find(|p| p.table_names.iter().any(|x| t.prop.contains(x.as_str()))) else { continue };
                let published = t.thrust_g * 9.80665e-3;
                let mut raw = m.clone();
                raw.voltage_factor = 1.0;
                let (mut lo, mut hi) = (300.0, raw.kv * t.voltage);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    let (_, sh) = p.data.thrust_power(mid, Speed::mps(0.0), rho);
                    let (_, a) = raw.terminal(mid, sh.value() / (mid * std::f64::consts::PI / 30.0));
                    if a > t.current_a { hi = mid } else { lo = mid }
                }
                at_i = at_i.max(p.data.thrust_power(lo, Speed::mps(0.0), rho).0.value() / published - 1.0);
                free = free.max(free_running(&raw, &p.data, rho, t.voltage).0.value() / published - 1.0);
                n += 1;
                let fitted_here = m.calibrated_on.as_ref().is_some_and(|(pid, v)| *pid == p.row.id && (v - t.voltage).abs() < 1e-9);
                if m.calibrated_on.is_some() && !fitted_here {
                    let e = free_running(m, &p.data, rho, t.voltage).0.value() / published - 1.0;
                    println!("{} {} {:.1} V: calibrated {:+.3}", m.row.id, p.row.id, t.voltage, e);
                    cal = cal.max(e);
                    n_cal += 1;
                }
            }
        }
        println!("rows {n}, cross-validated {n_cal}: at current {at_i:+.3}, free raw {free:+.3}, calibrated {cal:+.3}");
        assert!(at_i <= THRUST_AT_CURRENT_OVER && free <= THRUST_VOLTAGE_LIMITED_OVER && cal <= THRUST_CALIBRATED_OVER);
        assert!(n_cal >= 2, "the calibration is validated on rows it was not fitted on");
    }
}
