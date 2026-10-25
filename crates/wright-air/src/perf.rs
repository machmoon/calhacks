//! What the aircraft does: hover, cruise, the mission, and what holds it
//! together.
//!
//! Hover is checked on a hot day at the end of the flight (pack at its low
//! open-circuit voltage): the landing hover has the least in hand. Energy is
//! counted at standard conditions. Mission: take-off hover, transition,
//! climb, cruise, transition back, landing hover, with `ENERGY_RESERVE` of
//! the usable energy left. Range is flown at the best-range speed (most
//! metres per joule), endurance at the minimum-power speed.

use crate::aero::{trim, Trim, WingModel};
use crate::atmosphere::{isa, Air};
use crate::layout::Layout;
use crate::powertrain::{full_throttle, max_thrust, operate, Factors, FullThrottle, OperatingPoint};
use crate::rules::*;
use wright_core::units::{Current, Energy, Force, Length, Power, Speed, Time, Voltage, G0};

#[derive(Debug, Clone)]
pub struct Hover {
    pub thrust_each: Force,
    /// Standard day, full pack: what the builder sees on the first hover.
    pub std_full: Result<OperatingPoint, String>,
    /// Hot day, low pack: the landing.
    pub hot_low: Result<OperatingPoint, String>,
    pub power: Power,
    pub power_low: Power,
    pub pack_current_low: Current,
    /// All four at full throttle together, hot day, low pack.
    pub full: FullThrottle,
    pub thrust_to_weight: f64,
    /// One unit at full throttle on a fresh pack, sea level: the boom load.
    pub max_thrust_each: Force,
    pub max_current_each: Current,
    /// All four at full throttle with no current limit, standard day, full
    /// pack (`powertrain::free_full_throttle`): what the ratings must take.
    pub free: FullThrottle,
}

#[derive(Debug, Clone)]
pub struct CruisePoint {
    pub speed: Speed,
    pub trim: Trim,
    pub point: OperatingPoint,
    /// From the pack, avionics included.
    pub power: Power,
}

#[derive(Debug, Clone)]
pub struct Performance {
    pub factors: Factors,
    pub hover: Hover,
    /// The wing model's CLmax (NeuralFoil polars through the strip model).
    pub cl_max: f64,
    /// CLmax less the airfoil's measured over-prediction
    /// (`rules::clmax_over`): what every stall speed below is computed at.
    pub cl_max_used: f64,
    /// Stall (at `cl_max_used`) in true airspeed at the field (density altitude): what the
    /// ground sees, and what cruise speeds (TAS) are bounded by.
    pub stall: Speed,
    /// Stall in equivalent airspeed (`rules::STALL_IS_EAS`): what the
    /// stall_speed check and ArduPilot's AIRSPEED_MIN / Q_ASSIST_SPEED use.
    pub stall_eas: Speed,
    pub curve: Vec<CruisePoint>,
    pub best_range: Option<CruisePoint>,
    pub best_endurance: Option<CruisePoint>,
    pub max_speed: Speed,
    pub vtol_energy: Energy,
    pub climb_energy: Energy,
    pub usable: Energy,
    pub range_km: f64,
    pub range_to_cutoff_km: f64,
    pub endurance_min: f64,
    pub endurance_to_cutoff_min: f64,
    /// Pusher at full throttle on a full stiff pack, static: its ESC's load.
    pub pusher_full_current: Current,
    pub notes: Vec<String>,
}

fn sea(lay: &Layout) -> Air {
    isa(Length::m(lay.design.requirements.altitude_m), 0.0)
}

/// Where the lift motor's maker published a thrust table WITH this prop
/// (`Motor::rows_with`), the hover point's supply power, current and duty
/// come from that table (interpolated in thrust, at the tested voltage
/// nearest the pack's; below its lowest row down to half that thrust, scaled
/// from that row by momentum theory; power scaled by sqrt(1.225 / rho) for
/// density, and by
/// the pessimistic powertrain factor) instead of the prop model: measured
/// beats a proxy (at the reference's hover thrust the AOS 2807 + Gemfan
/// 7042 table gives 91 W per motor where the APC-proxy model gives 139 W). Outside
/// the table's thrust range the model stands.
fn from_table(m: &crate::catalog::Motor, prop: &crate::catalog::Prop, mut op: OperatingPoint, air: &Air, pack_v: Voltage, f: Factors) -> OperatingPoint {
    let rows = m.rows_with(prop);
    let Some(vt) = rows.iter().map(|r| r.voltage).min_by(|a, b| (a - pack_v.value()).abs().total_cmp(&(b - pack_v.value()).abs())) else { return op };
    let rows: Vec<_> = rows.into_iter().filter(|r| (r.voltage - vt).abs() < 0.5).collect();
    let g = op.thrust.value() / 9.80665e-3;
    let dens = (1.225 / air.density.value()).sqrt();
    let (power, duty, supply_a) = if let Some(k) = rows.windows(2).position(|w| w[0].thrust_g <= g && g <= w[1].thrust_g) {
        let (a, b) = (rows[k], rows[k + 1]);
        let t = if b.thrust_g > a.thrust_g { (g - a.thrust_g) / (b.thrust_g - a.thrust_g) } else { 0.0 };
        let lerp = |x: f64, y: f64| x + t * (y - x);
        (lerp(a.power_w, b.power_w), lerp(a.throttle, b.throttle), lerp(a.current_a, b.current_a))
    } else if let Some(a) = rows.first().filter(|a| g < a.thrust_g && g >= 0.5 * a.thrust_g) {
        // below the lowest row (down to half its thrust): a fixed prop's
        // power goes as thrust^1.5 and its rpm, so its duty, as thrust^0.5
        // (momentum theory); the table's own lowest point is the anchor
        let r = g / a.thrust_g;
        (a.power_w * r.powf(1.5), a.throttle * r.sqrt(), a.current_a * r.powf(1.5))
    } else {
        return op;
    };
    let power = power * dens / f.powertrain;
    let supply_a = supply_a * dens;
    op.battery = Power::w(power);
    op.throttle = (duty * vt / pack_v.value()).min(1.0);
    op.motor_a = Current::a(supply_a / duty.max(0.05));
    op
}

pub fn hover(lay: &Layout, f: Factors) -> Hover {
    let p = &lay.parts;
    let pack = &lay.battery.pack;
    let req = &lay.design.requirements;
    let tilt = lay.design.propulsion.lift_tilt_deg.to_radians();
    let w = lay.weight_n();
    let each = Force::n(w * HOVER_DOWNLOAD / 4.0 / tilt.cos());
    let std = sea(lay);
    let hot = isa(Length::m(req.altitude_m), req.hot_day_dk);
    let avion = Power::w(AVIONICS_POWER_W);
    let at = |air: &Air, low: bool| -> (Result<OperatingPoint, String>, Power, Current) {
        let mut current = Current::a(40.0);
        let mut last = Err("no iteration".to_string());
        for _ in 0..10 {
            let v = pack.voltage(current, low);
            match operate(p.lift_motor, &p.lift_prop.data, each, Speed::mps(0.0), air.density, v, f).map(|op| from_table(p.lift_motor, p.lift_prop, op, air, v, f)) {
                Ok(op) => {
                    let tot = op.battery * 4.0 + avion;
                    current = Current::a(tot.value() / v.value());
                    last = Ok(op);
                }
                Err(e) => {
                    last = Err(e.0);
                    break;
                }
            }
        }
        let power = last.as_ref().map(|op| op.battery * 4.0 + avion).unwrap_or(Power::w(f64::NAN));
        (last, power, current)
    };
    let (full, power, _) = at(&std, false);
    let (low, power_low, cur_low) = at(&hot, true);
    // the ESC's rating, or the firmware current limit when one is set (a
    // limit the ESC cannot apply is judged by `lift_esc_current`)
    let limit = lay.design.propulsion.lift_current_limit_a.map(Current::a);
    let rating = p.lift_esc.burst_current.unwrap_or(p.lift_esc.cont_current);
    let esc = match limit {
        Some(l) if p.lift_esc.current_limit_source.is_some() => if l.value() < rating.value() { l } else { rating },
        _ => rating,
    };
    let free_cap = limit.filter(|_| p.lift_esc.current_limit_source.is_some());
    let servo_peak = Power::w(5.0 * p.servo.stall_current.map(|c| c.as_a()).unwrap_or(SERVO_STALL_A) * SERVO_RAIL_V);
    // the lift motors are held under the pack's and the connector's burst
    // (Q_M_BAT_CURR_MAX), so the thrust-to-weight is computed under it
    let mut capped = pack.clone();
    let free = crate::powertrain::free_full_throttle(p.lift_motor, &p.lift_prop.data, pack, 4, Speed::mps(0.0), std.density, avion + servo_peak, f, free_cap);
    capped.burst = Current::a(crate::electrical::current_cap(lay, cur_low.as_a(), free.pack_a.as_a()));
    let ft = full_throttle(p.lift_motor, &p.lift_prop.data, &capped, 4, Speed::mps(0.0), hot.density, true, avion + servo_peak, Some(esc), f);
    let tw = 4.0 * ft.thrust_each.value() * tilt.cos() / w;
    let (tmax, imax) = max_thrust(p.lift_motor, &p.lift_prop.data, Speed::mps(0.0), std.density, pack.full_voltage());
    Hover {
        thrust_each: each,
        std_full: full,
        hot_low: low,
        power,
        power_low,
        pack_current_low: cur_low,
        full: ft,
        thrust_to_weight: tw,
        max_thrust_each: tmax,
        max_current_each: imax,
        free,
    }
}

pub fn cruise_point(lay: &Layout, v: Speed, air: &Air, f: Factors) -> Option<CruisePoint> {
    let tr = trim(lay, v, air, 1.0, f);
    if tr.stalled {
        return None;
    }
    let p = &lay.parts;
    let pack = &lay.battery.pack;
    let mut current = Current::a(6.0);
    let mut point = None;
    for _ in 0..5 {
        let pv = pack.voltage(current, false);
        let op = operate(p.cruise_motor, &p.cruise_prop.data, tr.drag, v, air.density, pv, f).ok()?;
        current = Current::a((op.battery.value() + AVIONICS_POWER_W) / pv.value());
        point = Some(op);
    }
    let point = point?;
    let power = point.battery + Power::w(AVIONICS_POWER_W);
    Some(CruisePoint { speed: v, trim: tr, point, power })
}

pub fn evaluate(lay: &Layout, f: Factors) -> Performance {
    let air = sea(lay);
    let pack = &lay.battery.pack;
    let hov = hover(lay, f);
    let cl_max = WingModel::new(lay, Speed::mps(12.0), &air).cl_max().max(0.3);
    let w = lay.weight_n();
    // ONE stall, corrected for the airfoil's measured CLmax over-prediction:
    // the stall check, the cruise floor, ArduPilot's AIRSPEED_MIN and
    // Q_ASSIST_SPEED and the checklist all read it (review 5, F7)
    let cl_max_used = cl_max / (1.0 + crate::rules::clmax_over(&lay.design.wing.airfoil));
    let stall = Speed::mps((2.0 * w / (air.density.value() * lay.area.as_m2() * cl_max_used)).sqrt());
    let mut notes = vec![];
    // golden-section on metres per joule (unimodal on the measured range),
    // then the curve on a 1 m/s grid for endurance and top speed
    let v_lo = (stall.value() * CRUISE_OVER_STALL_MIN).max(6.0);
    let mut curve: Vec<CruisePoint> = vec![];
    let mut v = v_lo;
    while v <= 45.0 {
        match cruise_point(lay, Speed::mps(v), &air, f) {
            Some(cp) => curve.push(cp),
            None if !curve.is_empty() => break,
            None => {}
        }
        v += 1.0;
    }
    let mut best_range = None;
    let mut best_end = None;
    let mut max_speed = Speed::mps(0.0);
    if curve.is_empty() {
        notes.push("no cruise speed is feasible: the pusher cannot make the drag".into());
    } else {
        let k = curve
            .iter()
            .enumerate()
            .max_by(|a, b| (a.1.speed.value() / a.1.power.value()).total_cmp(&(b.1.speed.value() / b.1.power.value())))
            .unwrap()
            .0;
        let lo = curve[k.saturating_sub(1)].speed.value().max(v_lo);
        let hi = curve[(k + 1).min(curve.len() - 1)].speed.value();
        let mpj = |v: f64| cruise_point(lay, Speed::mps(v), &air, f).map(|c| c.speed.value() / c.power.value()).unwrap_or(0.0);
        let (mut a, mut b) = (lo, hi);
        let g = 0.618_033_988_75;
        for _ in 0..20 {
            let c1 = b - g * (b - a);
            let c2 = a + g * (b - a);
            if mpj(c1) >= mpj(c2) {
                b = c2;
            } else {
                a = c1;
            }
        }
        best_range = cruise_point(lay, Speed::mps(0.5 * (a + b)), &air, f).or_else(|| Some(curve[k].clone()));
        best_end = curve.iter().min_by(|a, b| a.power.value().total_cmp(&b.power.value())).cloned();
        // top speed: the pusher's full-throttle thrust meets the drag
        let mut vm = curve.last().unwrap().speed.value();
        while vm < 60.0 {
            let tr = trim(lay, Speed::mps(vm + 0.5), &air, 1.0, f);
            let (t, _) = max_thrust(lay.parts.cruise_motor, &lay.parts.cruise_prop.data, Speed::mps(vm + 0.5), air.density, pack.voltage(Current::a(20.0), false));
            if t < tr.drag {
                break;
            }
            vm += 0.5;
        }
        max_speed = Speed::mps(vm);
    }
    let usable = pack.usable();
    let reserve = usable * ENERGY_RESERVE;
    let hover_e = Energy::j(hov.power.value() * HOVER_TAKEOFF_S + hov.power_low.value() * HOVER_LANDING_S);
    let trans_e = Energy::j(2.0 * TRANSITION_S * hov.power.value());
    let vtol = hover_e + trans_e;
    let climb = Energy::j(lay.mass().as_kg() * G0 * CLIMB_ALTITUDE_M / CLIMB_EFFICIENCY);
    let cruise_e = usable - reserve - vtol - climb;
    let cut_e = usable - vtol - climb;
    let (mut rng, mut rng_cut, mut end, mut end_cut) = (0.0, 0.0, 0.0, 0.0);
    if let (Some(br), Some(be)) = (&best_range, &best_end) {
        if cruise_e.value() > 0.0 && hov.power.value().is_finite() {
            rng = cruise_e.value() / br.power.value() * br.speed.value() / 1000.0;
            rng_cut = cut_e.value() / br.power.value() * br.speed.value() / 1000.0;
            let hover_min = vtol.value() / hov.power.value() / 60.0;
            end = cruise_e.value() / be.power.value() / 60.0 + hover_min;
            end_cut = cut_e.value() / be.power.value() / 60.0 + hover_min;
        }
    }
    let (_, pusher_a) = max_thrust(lay.parts.cruise_motor, &lay.parts.cruise_prop.data, Speed::mps(0.0), air.density, pack.full_voltage());
    Performance {
        factors: f,
        hover: hov,
        cl_max,
        cl_max_used,
        stall,
        stall_eas: Speed::mps(stall.value() * (air.density.value() / ISA_RHO0_KG_M3).sqrt()),
        curve,
        best_range,
        best_endurance: best_end,
        max_speed,
        vtol_energy: vtol,
        climb_energy: climb,
        usable,
        range_km: rng,
        range_to_cutoff_km: rng_cut,
        endurance_min: end,
        endurance_to_cutoff_min: end_cut,
        pusher_full_current: pusher_a,
        notes,
    }
}

/// Range and endurance flown at a given speed (the reference was flown at
/// its pilot's speed, not the model's best-range one).
pub fn at_speed(lay: &Layout, perf: &Performance, v: Speed) -> Option<(Power, Time, f64)> {
    let cp = cruise_point(lay, v, &sea(lay), perf.factors)?;
    let cut = perf.usable - perf.vtol_energy - perf.climb_energy;
    let t = Time::s(cut.value() / cp.power.value() + perf.vtol_energy.value() / perf.hover.power.value());
    let km = cut.value() / cp.power.value() * v.value() / 1000.0;
    Some((cp.power, t, km))
}

// ---------------------------------------------------------------------------
// Structure: every load reaches the CG through named members.

/// A member a load passes through, and its worst stress against allowable.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: &'static str,
    pub load_case: &'static str,
    pub stress_pa: f64,
    pub allowable_pa: f64,
    pub detail: String,
}

impl Member {
    pub fn margin_mpa(&self) -> f64 {
        (self.allowable_pa - self.stress_pa) / 1e6
    }
}

/// The chains each load travels, as (load, members in order). The geometric
/// check verifies every consecutive pair is joined and touching.
pub const LOAD_PATHS: &[(&str, &[&str])] = &[
    ("wing lift", &["wing_R", "main_spar_R", "joiner_R", "joiner_L", "main_spar_L", "wing_L", "pod"]),
    ("front lift thrust", &["lift_prop_disc_FR", "lift_hub_FR", "lift_motor_FR", "lift_mount_FR", "boom_R", "boom_clamp_R0", "wing_R", "main_spar_R", "joiner_R", "joiner_L", "wing_L", "pod"]),
    ("rear lift thrust", &["lift_prop_disc_RR", "lift_hub_RR", "lift_motor_RR", "lift_mount_RR", "boom_R", "boom_clamp_R1", "wing_R", "pod"]),
    ("landing", &["leg_FR", "boom_R", "boom_clamp_R0", "wing_R", "pod"]),
    ("tail load", &["h_stab", "tail_joint_R", "boom_R", "boom_clamp_R1", "wing_R", "pod"]),
    ("pusher thrust", &["pusher_disc", "pusher_hub", "pusher_motor", "pusher_mount", "pod"]),
];

#[derive(Debug, Clone)]
pub struct Structure {
    pub members: Vec<Member>,
    pub root_moment_nm: f64,
    pub boom_frequency_hz: f64,
    pub hover_rotation_hz: f64,
    pub leg_load_n: f64,
    pub leg_buckling_n: f64,
    pub boom_deflection_mm: f64,
    /// Local shell buckling of the boom tube in bending (NASA SP-8007):
    /// (critical stress with the knock-down, Pa; knock-down gamma; R/t).
    pub boom_buckling: (f64, f64, f64),
}

fn tube_material(t: &crate::catalog::Tube) -> Material {
    match t.kind {
        crate::catalog::TubeKind::Pultruded => CF_PULTRUDED,
        crate::catalog::TubeKind::RollWrapped => CF_ROLLED,
    }
}

fn tube_i(t: &crate::catalog::Tube) -> f64 {
    let od = t.od.as_m();
    let id = t.id.map(|l| l.as_m()).unwrap_or(0.0);
    std::f64::consts::PI * (od.powi(4) - id.powi(4)) / 64.0
}

/// A thin-walled tube's local buckling stress in bending, NASA SP-8007
/// (Buckling of Thin-Walled Circular Cylinders, rev. 1968), sect. 4.1.2:
/// classical sigma_cl = E t / (R sqrt(3 (1 - nu^2))) times the bending
/// knock-down gamma = 1 - 0.731 (1 - exp(-phi)), phi = sqrt(R / t) / 16
/// (eq. 6), R the mid-wall radius. Isotropic with the tube's axial modulus
/// and nu = 0.3 (the carbon layup's orthotropy is not modelled). A solid rod
/// cannot shell-buckle: infinite. Returns (gamma x sigma_cl, gamma, R/t).
pub fn shell_buckling(t: &crate::catalog::Tube, modulus_pa: f64) -> (f64, f64, f64) {
    let Some(id) = t.id else { return (f64::INFINITY, 1.0, 0.0) };
    let wall = (t.od - id).as_m() / 2.0;
    let r = (t.od + id).as_m() / 4.0;
    if wall <= 0.0 {
        return (0.0, 0.0, f64::INFINITY);
    }
    let nu: f64 = 0.3;
    let classical = modulus_pa * wall / (r * (3.0 * (1.0 - nu * nu)).sqrt());
    let phi = (r / wall).sqrt() / 16.0;
    let gamma = 1.0 - 0.731 * (1.0 - (-phi).exp());
    (gamma * classical, gamma, r / wall)
}

pub fn bending_stress(t: &crate::catalog::Tube, moment_nm: f64) -> f64 {
    moment_nm * (t.od.as_m() / 2.0) / tube_i(t)
}

/// Bending moment at station `y` of one half-wing carrying `lift` by
/// Schrenk's approximation (mean of elliptic and chord-proportional).
pub fn schrenk_moment(lay: &Layout, lift_half_n: f64, y: f64) -> f64 {
    let s = lay.semi_span().as_m();
    let lam = lay.taper;
    let n = 200;
    let mut total = 0.0;
    for i in 0..n {
        let t = y + (s - y) * (i as f64 + 0.5) / n as f64;
        let ell = 4.0 / (std::f64::consts::PI * s) * (1.0 - (t / s).powi(2)).max(0.0).sqrt();
        let trap = 2.0 * (1.0 - (1.0 - lam) * t / s) / (s * (1.0 + lam));
        total += 0.5 * (ell + trap) * (t - y) * (s - y) / n as f64;
    }
    lift_half_n * total
}

/// NACA 00xx section (area, least second moment) from its own outline.
pub fn naca_section(chord: f64, thick: f64) -> (f64, f64) {
    let t = thick / chord;
    let n = 40;
    let mut pts = vec![];
    for i in 0..=n {
        let xc = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / n as f64).cos());
        let yt = 5.0 * t * (0.2969 * xc.sqrt() - 0.1260 * xc - 0.3516 * xc * xc + 0.2843 * xc.powi(3) - 0.1036 * xc.powi(4));
        pts.push((xc * chord, yt * chord));
    }
    let mut poly = pts.clone();
    for k in (1..pts.len() - 1).rev() {
        poly.push((pts[k].0, -pts[k].1));
    }
    let (mut a, mut ixx) = (0.0, 0.0);
    for k in 0..poly.len() {
        let (x0, y0) = poly[k];
        let (x1, y1) = poly[(k + 1) % poly.len()];
        let cr = x0 * y1 - x1 * y0;
        a += cr / 2.0;
        ixx += (y0 * y0 + y0 * y1 + y1 * y1) * cr / 12.0;
    }
    (a.abs(), ixx.abs())
}

pub fn structure(lay: &Layout, hov: &Hover, perf_vmax: Speed) -> Structure {
    let p = &lay.parts;
    let w = lay.weight_n();
    let semi = lay.semi_span().as_m();
    let mut members = vec![];
    let n_ult = LIMIT_LOAD_FACTOR * ULTIMATE_FACTOR;
    let lift_half = n_ult * w / 2.0;
    let root = schrenk_moment(lay, lift_half, 0.0);
    let y_j = JOINER_HALF_FRAC * semi;
    let at_joint = schrenk_moment(lay, lift_half, y_j);
    // hover at full thrust (ultimate): both motors' thrust enters each wing
    // half at the boom clamps and reaches the pod at the root
    let t_ult = hov.max_thrust_each.value() * ULTIMATE_FACTOR;
    let by = lay.boom_y.as_m();
    let hover_root = 2.0 * t_ult * by;
    let hover_joint = 2.0 * t_ult * (by - y_j).max(0.0);
    let jm = tube_material(p.joiner);
    let sm = tube_material(p.main_spar);
    for (case, mr, mj) in [("manoeuvre 4 g x 1.5", root, at_joint), ("full-thrust hover x 1.5", hover_root, hover_joint)] {
        members.push(Member {
            name: "joiner",
            load_case: case,
            stress_pa: bending_stress(p.joiner, mr),
            allowable_pa: jm.allowable_pa,
            detail: format!("{mr:.1} N m at the root"),
        });
        members.push(Member {
            name: "main_spar",
            load_case: case,
            stress_pa: bending_stress(p.main_spar, mj),
            allowable_pa: sm.allowable_pa,
            detail: format!("{mj:.1} N m where the joiner ends, {:.0} mm out", y_j * 1000.0),
        });
    }
    // booms: cantilevers from the nearest clamp (wing 10 % and 75 % chord)
    let c_boom = lay.chord_at(lay.boom_y).as_m();
    let over_front = (0.10 * c_boom - lay.x_front.as_m()).max(0.0);
    let over_rear = (lay.x_rear.as_m() - 0.75 * c_boom).max(0.0);
    let overhang = over_front.max(over_rear);
    let bm = tube_material(p.boom);
    members.push(Member {
        name: "boom",
        load_case: "full thrust x 1.5",
        stress_pa: bending_stress(p.boom, t_ult * overhang),
        allowable_pa: bm.allowable_pa,
        detail: format!("{t_ult:.1} N on a {:.0} mm overhang", overhang * 1000.0),
    });
    // landing (14 CFR 23.473): two legs take 2.67 g x 1.5, each under a
    // motor mount at the end of the boom's overhang
    let p_leg = LANDING_LOAD_FACTOR * ULTIMATE_FACTOR * w / LANDING_LEGS_LOADED;
    members.push(Member {
        name: "boom",
        load_case: "landing 2.67 g x 1.5",
        stress_pa: bending_stress(p.boom, p_leg * overhang),
        allowable_pa: bm.allowable_pa,
        detail: format!("{p_leg:.0} N leg load on a {:.0} mm overhang", overhang * 1000.0),
    });
    // tail: the stabiliser at its section CLmax at top speed, half on each
    // boom, from the rear clamp
    let air = isa(Length::m(lay.design.requirements.altitude_m), 0.0);
    let re_h = air.reynolds(perf_vmax, lay.h_chord.max(Length::m(0.02)));
    let tail_load = air.q(perf_vmax).value() * lay.h_area.as_m2() * p.tail_airfoil.cl_max(re_h).0 * ULTIMATE_FACTOR / 2.0;
    let tail_arm = (lay.x_ac_tail.as_m() - 0.75 * c_boom).max(0.0);
    members.push(Member {
        name: "boom",
        load_case: "tail at CLmax, top speed x 1.5",
        stress_pa: bending_stress(p.boom, tail_load * tail_arm),
        allowable_pa: bm.allowable_pa,
        detail: format!("{tail_load:.0} N per boom at {:.0} mm", tail_arm * 1000.0),
    });
    // legs: Euler buckling with the foot free to slide (K = 2) and the
    // friction side load bending the strut about its thin axis
    let (area, i_min) = naca_section(lay.leg_chord.as_m(), lay.leg_thickness.as_m());
    let lh = lay.leg_height.as_m();
    let p_cr = std::f64::consts::PI.powi(2) * PETG.modulus_pa * i_min / (2.0 * lh).powi(2);
    let side = FOOT_FRICTION * p_leg * lh;
    members.push(Member {
        name: "landing_leg",
        load_case: "landing 2.67 g x 1.5",
        stress_pa: p_leg / area + side * (lay.leg_thickness.as_m() / 2.0) / i_min,
        allowable_pa: PETG.allowable_pa,
        detail: format!("{p_leg:.0} N axial, {:.0} N side at the foot", FOOT_FRICTION * p_leg),
    });
    let ei = bm.modulus_pa * tube_i(p.boom);
    let deflection = hov.thrust_each.value() * overhang.powi(3) / (3.0 * ei) * 1000.0;
    let tip = p.lift_motor.mass.as_kg() + p.lift_prop.mass.as_kg() + MOTOR_MOUNT_KG;
    let beam = p.boom.kg_per_m * overhang;
    let freq = (3.0 * ei / (overhang.max(1e-3).powi(3) * (tip + 0.24 * beam))).sqrt() / (2.0 * std::f64::consts::PI);
    let rot = hov.std_full.as_ref().map(|o| o.rpm / 60.0).unwrap_or(f64::NAN);
    Structure {
        members,
        root_moment_nm: root.max(hover_root),
        boom_frequency_hz: freq,
        hover_rotation_hz: rot,
        leg_load_n: p_leg,
        leg_buckling_n: p_cr,
        boom_deflection_mm: deflection,
        boom_buckling: shell_buckling(p.boom, bm.modulus_pa),
    }
}
