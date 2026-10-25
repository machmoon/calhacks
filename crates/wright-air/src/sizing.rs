//! Parts chosen by rule, not searched: spar and joiner, rear spar, booms,
//! legs, ESCs, servo and avionics.
//!
//! Each rule uses a SIMPLER model than the check that later judges the part
//! (rule 3 of `wright_core::check`: a check must not restate the sizing):
//! spars are sized on an elliptic lift distribution's root moment in the
//! manoeuvre case alone, booms on thrust alone, ESCs on one motor at full
//! throttle on a stiff full pack. The checks run Schrenk loading, the
//! full-thrust hover case, landing (14 CFR 23.473) and tail loads, the
//! four-motor sagging-pack solve, and the geometry. When a check rejects a
//! rule's pick, [`crate::search`] steps the part up and tries again.

use crate::catalog::{catalog, Tube};
use crate::spec::StockPolicy;
use crate::layout::{self, joiner_fit, Layout};
use crate::perf;
use crate::powertrain::max_thrust;
use crate::rules::*;
use crate::spec::Design;
use wright_core::units::{Density, Length, Speed, G0};

/// Fraction of allowable a rule sizes to.
pub const RULE_RESERVE: f64 = 0.9;

fn tubes(hollow: bool, policy: StockPolicy) -> Vec<&'static Tube> {
    let mut v: Vec<&Tube> = catalog()
        .tubes
        .values()
        .filter(|t| t.row.orderable(policy) && (t.id.is_some() == hollow))
        .collect();
    v.sort_by(|a, b| a.kg_per_m.total_cmp(&b.kg_per_m).then(a.row.id.cmp(&b.row.id)));
    v
}

fn allowable(t: &Tube) -> f64 {
    match t.kind {
        crate::catalog::TubeKind::Pultruded => CF_PULTRUDED.allowable_pa,
        crate::catalog::TubeKind::RollWrapped => CF_ROLLED.allowable_pa,
    }
}

/// The wing's depth at the main spar's outer end, minus two skins, m.
fn spar_room(lay: &Layout, y: f64) -> f64 {
    lay.wing_surface(lay.main_spar_x, Length::m(y))
        .map(|(u, l)| (u - l).as_m() - 2.0 * SKIN_WALL_M)
        .unwrap_or(0.0)
}

/// Re-size the structure, ESCs and servo of `d` (avionics untouched).
pub fn size(mut d: Design, bump: &Bumps) -> Result<Design, String> {
    let c = catalog();
    let policy = d.requirements.stock;
    let mut lay = layout::solve(&d)?;
    for _ in 0..3 {
        let semi = lay.semi_span().as_m();
        let w = lay.weight_n();
        // main spar + joiner: the lightest fitting pair whose joiner carries
        // the elliptic root moment and whose spar carries what is left where
        // the joiner ends (elliptic centroid 4 s / 3 pi)
        let lift_half = LIMIT_LOAD_FACTOR * ULTIMATE_FACTOR * w / 2.0;
        let root = lift_half * 4.0 * semi / (3.0 * std::f64::consts::PI);
        let y_j = JOINER_HALF_FRAC * semi;
        let at_j = lift_half * ((semi - y_j) / semi).powi(2) * (semi - y_j) * 4.0 / (3.0 * std::f64::consts::PI);
        let room = spar_room(&lay, MAIN_SPAR_REACH * semi);
        let room_j = spar_room(&lay, y_j);
        let mut pairs = vec![];
        for s in tubes(true, policy) {
            for j in c.tubes.values().filter(|t| t.row.orderable(policy)) {
                let Some(kind) = joiner_fit(s, j) else { continue };
                let outer = if kind == "sleeve" { j.od.as_m() } else { s.od.as_m() };
                let ok = perf::bending_stress(j, root) <= RULE_RESERVE * allowable(j)
                    && perf::bending_stress(s, at_j) <= RULE_RESERVE * allowable(s)
                    && s.od.as_m() <= room
                    && outer <= room_j;
                if ok {
                    let mass = s.kg_per_m * 2.0 * MAIN_SPAR_REACH * semi + j.kg_per_m * 2.0 * y_j;
                    pairs.push((mass, s.row.id.clone(), j.row.id.clone()));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        if let Some((_, s, j)) = pairs.get(bump.spar.min(pairs.len().saturating_sub(1))) {
            d.structure.main_spar = s.clone();
            d.structure.joiner = j.clone();
        } else {
            return Err("no spar and joiner pair carries the root moment and fits the wing".into());
        }
        // rear spar: the lightest 3-5 mm rod
        if let Some(r) = tubes(false, policy).into_iter().find(|t| (2.9..=5.1).contains(&t.od.as_mm())) {
            d.structure.rear_spar = r.row.id.clone();
        }
        // booms: thrust x 1.5 on the overhang from the nearest clamp
        let pk = &lay.battery.pack;
        let (tmax, _) = max_thrust(lay.parts.lift_motor, &lay.parts.lift_prop.data, Speed::mps(0.0), Density::kg_m3(1.225), pk.full_voltage());
        let cb = lay.chord_at(lay.boom_y).as_m();
        let over = (0.10 * cb - lay.x_front.as_m()).max(lay.x_rear.as_m() - 0.75 * cb).max(0.0);
        let booms: Vec<&Tube> = tubes(true, policy)
            .into_iter()
            .filter(|t| t.od.as_mm() >= 10.0 && t.stock_length.as_m() >= (lay.boom_x1 - lay.boom_x0).as_m())
            .filter(|t| perf::bending_stress(t, tmax.value() * ULTIMATE_FACTOR * over) <= RULE_RESERVE * allowable(t))
            .collect();
        match booms.get(bump.boom.min(booms.len().saturating_sub(1))) {
            Some(t) => d.structure.boom_tube = t.row.id.clone(),
            None => return Err("no boom tube is long and strong enough".into()),
        }
        // legs: the thinnest strut whose RECTANGULAR envelope (t x 2.75 t)
        // carries the landing load and its side load; the check uses the
        // faired NACA section, which is weaker, and bumps when it fails
        let p_leg = LANDING_LOAD_FACTOR * ULTIMATE_FACTOR * w / LANDING_LEGS_LOADED;
        let lh = lay.leg_height.as_m();
        let mut t_mm = 8.0;
        while t_mm < 30.0 {
            let (t, c) = (t_mm / 1000.0, t_mm * LEG_CHORD_OVER_THICKNESS / 1000.0);
            let i = c * t.powi(3) / 12.0;
            let pcr = std::f64::consts::PI.powi(2) * PETG.modulus_pa * i / (2.0 * lh).powi(2);
            let stress = p_leg / (c * t) + FOOT_FRICTION * p_leg * lh * (t / 2.0) / i;
            if pcr * RULE_RESERVE >= p_leg && stress <= RULE_RESERVE * PETG.allowable_pa {
                break;
            }
            t_mm += 1.0;
        }
        d.structure.leg_thickness_mm = t_mm + 2.0 * bump.legs as f64;
        // ESCs: one motor at full throttle on a stiff full pack
        let s = pk.s;
        let (_, lift_a) = max_thrust(lay.parts.lift_motor, &lay.parts.lift_prop.data, Speed::mps(0.0), Density::kg_m3(1.225), pk.full_voltage());
        let (_, push_a) = max_thrust(lay.parts.cruise_motor, &lay.parts.cruise_prop.data, Speed::mps(0.0), Density::kg_m3(1.225), pk.full_voltage());
        let rated = |e: &crate::catalog::Esc| e.cells_max.is_some_and(|h| h >= s) && e.cells_min.unwrap_or(1) <= s && e.row.orderable(policy);
        // When the lift motors flat out with no limit would draw more than
        // the motor's rating or a quarter of the pack's burst (less 10 A for
        // the rest), each lift ESC's firmware limits it: AM32 `limits.current`
        // in 2 A steps, enforced by a 1 kHz PID on the ESC's own current
        // sensor (am32-firmware/AM32 Src/main.c, `use_current_limit`), so
        // four single ESCs with their own sensors. The checks judge thrust
        // at that limit.
        let free = crate::powertrain::free_full_throttle(lay.parts.lift_motor, &lay.parts.lift_prop.data, pk, 4, Speed::mps(0.0), Density::kg_m3(1.225), wright_core::units::Power::w(36.0), crate::powertrain::Factors::NOMINAL, None);
        let per_motor_budget = lay.parts.lift_motor.max_current.value().min((pk.burst.value() - 10.0) / 4.0);
        let limiting = c
            .escs
            .values()
            .filter(|e| e.channels == 1 && e.current_limit_source.is_some() && rated(e))
            .min_by(|a, b| a.mass.value().total_cmp(&b.mass.value()).then(a.row.id.cmp(&b.row.id)));
        let limit = (2.0 * (per_motor_budget / 2.0).floor()).min(lift_a.value());
        if free.motor_a.value() > per_motor_budget && limit >= 2.0 && limiting.is_some_and(|e| e.cont_current.value() >= limit) {
            d.propulsion.lift_esc = limiting.unwrap().row.id.clone();
            d.propulsion.lift_current_limit_a = Some(limit);
        } else {
            d.propulsion.lift_current_limit_a = None;
            if let Some(e) = c
                .escs
                .values()
                .filter(|e| e.channels >= 4 && e.cont_current.value() >= lift_a.value() && rated(e))
                .min_by(|a, b| a.mass.value().total_cmp(&b.mass.value()).then(a.row.id.cmp(&b.row.id)))
            {
                d.propulsion.lift_esc = e.row.id.clone();
            }
        }
        if let Some(e) = c
            .escs
            .values()
            .filter(|e| e.channels == 1 && e.cont_current.value() >= push_a.value() && rated(e))
            .min_by(|a, b| a.mass.value().total_cmp(&b.mass.value()).then(a.row.id.cmp(&b.row.id)))
        {
            d.propulsion.cruise_esc = e.row.id.clone();
        }
        let _ = G0;
        lay = layout::solve(&d)?;
    }
    Ok(d)
}

/// The servo by rule: the cheapest that fits the wing at the aileron with
/// the spare depth, then the lightest.
pub fn choose_servo(d: &mut Design) -> Result<(), String> {
    let c = catalog();
    let mut best: Option<(f64, f64, String)> = None;
    let policy = d.requirements.stock;
    for sv in c.servos.values().filter(|s| s.row.orderable(policy)) {
        let mut t = d.clone();
        t.avionics.servo = sv.row.id.clone();
        let lay = layout::solve(&t)?;
        if crate::geometry::servo_margin(&lay) < SERVO_SPARE_M {
            continue;
        }
        let key = (sv.row.price.unwrap().as_usd(), sv.mass.as_g(), sv.row.id.clone());
        if best.as_ref().is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
            best = Some(key);
        }
    }
    match best {
        Some((_, _, id)) => {
            d.avionics.servo = id;
            Ok(())
        }
        None => Err("no servo fits the wing at the aileron".into()),
    }
}

/// Avionics by rule, the cheapest that do the job: a flight controller on
/// sale with an ArduPilot target, at least 10 outputs, a servo BEC covering
/// five stalled servos and rated for the pack; a GPS with a compass; an
/// ExpressLRS receiver; an airspeed sensor when required (not an I2C DLVR on
/// an F405: ArduPilot dropped it from 1 MB-flash boards, Matek ASPD-DLVR note).
pub fn choose_avionics(d: &mut Design) {
    let c = catalog();
    let policy = d.requirements.stock;
    let s = d.requirements.cells_s;
    let cost = |r: &crate::catalog::Row, g: f64| (r.price.map(|p| p.as_usd()).unwrap_or(1e9), g, r.id.clone());
    let need = 5.0 * SERVO_STALL_A;
    if let Some(fc) = c
        .fcs
        .values()
        .filter(|f| f.row.orderable(policy) && f.ardupilot_target.is_some())
        .filter(|f| f.pwm_outputs >= 10 && f.servo_bec.as_a() >= need)
        .filter(|f| f.cells.is_some_and(|(lo, hi)| lo <= s && s <= hi))
        .min_by(|a, b| cost(&a.row, a.mass.as_g()).partial_cmp(&cost(&b.row, b.mass.as_g())).unwrap())
    {
        d.avionics.fc = fc.row.id.clone();
    }
    let f405 = c.fcs.get(&d.avionics.fc).is_some_and(|f| f.mcu.to_lowercase().contains("f405"));
    if let Some(g) = c
        .gps
        .values()
        .filter(|g| g.row.orderable(policy))
        .filter(|g| ["compass", "5883", "qmc", "ist8310", "magnet"].iter().any(|k| (g.key_spec.to_lowercase() + &g.row.id).contains(k)))
        .min_by(|a, b| cost(&a.row, a.mass.as_g()).partial_cmp(&cost(&b.row, b.mass.as_g())).unwrap())
    {
        d.avionics.gps = g.row.id.clone();
    }
    if let Some(r) = c
        .receivers
        .values()
        .filter(|r| r.row.orderable(policy))
        .filter(|r| {
            let t = (r.row.name.clone() + &r.key_spec).to_lowercase();
            t.contains("elrs") || t.contains("expresslrs")
        })
        .min_by(|a, b| cost(&a.row, a.mass.as_g()).partial_cmp(&cost(&b.row, b.mass.as_g())).unwrap())
    {
        d.avionics.receiver = r.row.id.clone();
    }
    d.avionics.airspeed = if d.requirements.airspeed_sensor {
        c.airspeed
            .values()
            .filter(|a| a.row.orderable(policy))
            .filter(|a| !(f405 && (a.key_spec.to_lowercase() + &a.row.id).contains("dlvr")))
            .min_by(|a, b| cost(&a.row, a.mass.as_g()).partial_cmp(&cost(&b.row, b.mass.as_g())).unwrap())
            .map(|a| a.row.id.clone())
    } else {
        None
    };
}

/// Steps up from each rule's first pick, set when a check rejected it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bumps {
    pub spar: usize,
    pub boom: usize,
    pub legs: usize,
}

/// Size by rule, then let the checks step up any structural part they
/// reject (the rule proposes, the independent check decides).
pub fn size_and_repair(d: Design) -> Result<Design, String> {
    let mut bump = Bumps::default();
    let mut d = size(d, &bump)?;
    for _ in 0..12 {
        let e = crate::checks::evaluate(&d, crate::powertrain::Factors::NOMINAL)?;
        let f = crate::checks::physics_only(&e);
        let bad = |n: &str| f.get(n).is_some_and(|o| !o.passed());
        let before = bump;
        if bad("spar_stress") || bad("spar_fit") {
            bump.spar += 1;
        }
        if bad("boom_stress") || bad("boom_resonance") {
            bump.boom += 1;
        }
        if bad("landing_legs") {
            bump.legs += 1;
        }
        if bump == before {
            break;
        }
        d = size(d, &bump)?;
    }
    Ok(d)
}
