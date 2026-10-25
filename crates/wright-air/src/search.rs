//! Design search: the VTOL that flies furthest within the requirements.
//!
//! A requirements-only spec is seeded by rule (avionics by `sizing`, the
//! most energy-dense priced pack, the reference's own 7-inch lift and
//! pusher units, a mid-size SD7037 wing), so it always evaluates. Then,
//! instead of the prototype's one-dimension-at-a-time descent over string
//! ids, every candidate is the WHOLE model (`checks::evaluate`) with the
//! structure and ESCs re-sized by rule, and the discrete choices are
//! enumerated wide:
//!
//! 1. every pusher motor x prop, every lift motor x prop pair, every
//!    battery, each against the incumbent rest; the best few of each kept;
//! 2. their full cross product against the incumbent airframe;
//! 3. the airframe (airfoil x root chord x taper) for the best combinations;
//! 4. rounds repeat until nothing moves; the finalists are repaired by the
//!    checks and judged by EVERY check, meshes included, and the furthest
//!    one that passes wins.
//!
//! Scoring: range with reserve when every physics check passes; otherwise
//! below every passing design, ordered by how far it fails. Deterministic:
//! candidates are generated in a fixed order, evaluated in parallel, and
//! reduced in that order with ties broken by index.

use crate::battery;
use crate::catalog::catalog;
use crate::checks::{self, MESH_CHECKS};
use crate::powertrain::Factors;
use crate::rules::*;
use crate::sizing;
use crate::spec::*;
use std::sync::Mutex;

pub const WING_AIRFOILS: &[&str] =
    &["sd7037", "sd7032", "sd7062", "sd7080", "e387", "e205", "mh32", "mh60", "s4083", "ag35", "rg15", "s3021", "hq259", "clarky", "naca2412", "naca4412"];

#[derive(Debug, Clone)]
pub struct Scored {
    pub design: Design,
    pub score: f64,
    pub range_km: f64,
    pub why: String,
}

pub fn violation_scale(unit: &str) -> f64 {
    match unit {
        "A" => 10.0,
        "MPa" => 100.0,
        "mm" => 5.0,
        "m/s" => 2.0,
        "%MAC" => 5.0,
        "km" => 50.0,
        "USD" => 100.0,
        "min" => 30.0,
        "C" => 50.0,
        "cells" => 1.0,
        _ => 0.2,
    }
}

/// What the search holds back beyond the checks: the checks already take
/// off the thrust model's and the airfoil table's measured error (rules:
/// `THRUST_*_OVER`, `CLMAX_OVER`); the search also asks for T/W >= 1.8 with
/// the pessimistic powertrain on top.
#[derive(Debug, Clone)]
pub struct Allowance {
    /// Hover T/W under the pessimistic powertrain, with the thrust model's
    /// worst over-prediction for what stopped the motors taken off.
    pub tw: f64,
    pub tw_limit: &'static str,
    /// Stall speed with the section CLmax cut by the airfoil's worst measured
    /// over-prediction.
    pub stall_ms: f64,
}

impl Allowance {
    pub fn of(e: &checks::Evaluation) -> Allowance {
        let h = crate::perf::hover(&e.layout, Factors::PESSIMISTIC);
        let over = match (h.full.limit, e.layout.parts.lift_motor.calibrated_on.is_some()) {
            ("voltage", true) => THRUST_CALIBRATED_OVER,
            ("voltage", false) => THRUST_VOLTAGE_LIMITED_OVER,
            _ => THRUST_AT_CURRENT_OVER,
        };
        Allowance { tw: h.thrust_to_weight / (1.0 + over), tw_limit: h.full.limit, stall_ms: e.perf.stall_eas.as_mps() }
    }
    /// (what, signed margin, unit) for each allowance; negative is short.
    pub fn margins(&self, req: &Requirements) -> [(&'static str, f64, &'static str); 2] {
        [("hover_thrust_to_weight with model allowance", self.tw - HOVER_TW_MIN, ""), ("stall_speed with CLmax allowance", req.max_stall_ms - self.stall_ms, "m/s")]
    }
}

/// A confirmed attack the search must also survive: `check` judged with
/// the attack's requirements (e.g. a 1000 m field for `stall_speed`).
#[derive(Debug, Clone)]
pub struct Screen {
    pub id: String,
    pub check: String,
    pub requirements: Requirements,
}

/// Score a design (sized by rule first).
pub fn score(d: &Design) -> Scored {
    score_with(d, &[])
}

/// The screens' shortfall on a sized design (fast analysis, no meshes):
/// (violation, why); a screen on a mesh or slicer check is left to the
/// finalists' full check.
fn screen_shortfall(sized: &Design, screens: &[Screen]) -> (f64, Vec<String>) {
    let (mut bad, mut why) = (0.0, vec![]);
    for sc in screens {
        if MESH_CHECKS.contains(&sc.check.as_str()) {
            continue;
        }
        let mut d = sized.clone();
        d.requirements = sc.requirements.clone();
        let o = checks::evaluate(&d, Factors::NOMINAL).ok().and_then(|e| checks::physics_only(&e).get(&sc.check).cloned());
        match o {
            Some(o) if o.passed() => {}
            Some(o) => {
                bad += match &o.evidence {
                    wright_core::Evidence::Margin { value, unit } => -value / violation_scale(unit),
                    _ => 10.0,
                };
                why.push(format!("{} under {}: {:+.2}", sc.check, sc.id, o.margin().unwrap_or(f64::NAN)));
            }
            None => {
                bad += 10.0;
                why.push(format!("{} under {}: not evaluated", sc.check, sc.id));
            }
        }
    }
    (bad, why)
}

/// Score a design under the screens too.
pub fn score_with(d: &Design, screens: &[Screen]) -> Scored {
    let sized = match sizing::size_and_repair(d.clone()) {
        Ok(s) => s,
        Err(e) => return Scored { design: d.clone(), score: -1e9, range_km: 0.0, why: e },
    };
    let e = match checks::evaluate(&sized, Factors::NOMINAL) {
        Ok(e) => e,
        Err(e) => return Scored { design: sized, score: -1e9, range_km: 0.0, why: e },
    };
    let f = checks::physics_only(&e);
    let fails: Vec<&wright_core::Outcome> = f.failures().filter(|o| !MESH_CHECKS.contains(&o.name.as_str())).collect();
    let range = e.perf.range_km;
    let (screen_bad, screen_why) = screen_shortfall(&sized, screens);
    if screen_bad > 0.0 {
        let bad: f64 = screen_bad
            + fails
                .iter()
                .map(|o| match &o.evidence {
                    wright_core::Evidence::Margin { value, unit } => -value / violation_scale(unit),
                    _ => 10.0,
                })
                .sum::<f64>();
        let mut why: Vec<String> = fails.iter().map(|o| format!("{} {:+.2}", o.name, o.margin().unwrap_or(f64::NAN))).collect();
        why.extend(screen_why);
        return Scored { design: sized, score: -1000.0 - 100.0 * bad, range_km: range, why: why.join("; ") };
    }
    let short: Vec<(&str, f64, &str)> = if fails.is_empty() {
        Allowance::of(&e).margins(&sized.requirements).into_iter().filter(|m| m.1 < 0.0).collect()
    } else {
        vec![]
    };
    if fails.is_empty() && short.is_empty() {
        Scored { design: sized, score: range, range_km: range, why: String::new() }
    } else if fails.is_empty() {
        // passes every check but without the model allowance: below every
        // design that keeps it, above every design that fails a check
        let bad: f64 = short.iter().map(|m| -m.1 / violation_scale(m.2)).sum();
        let why = short.iter().map(|m| format!("{} {:+.2}", m.0, m.1)).collect::<Vec<_>>().join("; ");
        Scored { design: sized, score: -500.0 - 100.0 * bad, range_km: range, why }
    } else {
        let bad: f64 = fails
            .iter()
            .map(|o| match &o.evidence {
                wright_core::Evidence::Margin { value, unit } => -value / violation_scale(unit),
                _ => 10.0,
            })
            .sum();
        let why = fails.iter().map(|o| format!("{} {:+.2}", o.name, o.margin().unwrap_or(f64::NAN))).collect::<Vec<_>>().join("; ");
        Scored { design: sized, score: -1000.0 - 100.0 * bad, range_km: range, why }
    }
}

/// Score many in parallel; results in input order.
pub fn score_all(ds: &[Design]) -> Vec<Scored> {
    score_all_with(ds, &[])
}

pub fn score_all_with(ds: &[Design], screens: &[Screen]) -> Vec<Scored> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(12);
    let next = Mutex::new(0usize);
    let out: Mutex<Vec<Option<Scored>>> = Mutex::new(vec![None; ds.len()]);
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = {
                    let mut g = next.lock().unwrap();
                    let i = *g;
                    *g += 1;
                    i
                };
                if i >= ds.len() {
                    break;
                }
                let r = score_with(&ds[i], screens);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|o| o.unwrap()).collect()
}

/// The best `k` (stable: ties keep input order).
fn top(mut v: Vec<(usize, Scored)>, k: usize) -> Vec<Scored> {
    v.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(a.0.cmp(&b.0)));
    v.into_iter().take(k).map(|x| x.1).collect()
}

fn ranked(ds: Vec<Design>, k: usize, screens: &[Screen]) -> Vec<Scored> {
    let s = score_all_with(&ds, screens);
    top(s.into_iter().enumerate().collect(), k)
}

/// The seed: every open choice filled by rule.
pub fn seed(spec: &Spec) -> Design {
    let ch = &spec.design;
    let req = spec.requirements.clone();
    let c = catalog();
    let battery = ch.battery.clone().unwrap_or_else(|| {
        battery::options(req.cells_s, req.stock)
            .into_iter()
            .filter_map(|b| battery::resolve(&b).ok().map(|r| (r.pack.energy().as_wh() / r.pack.mass.as_kg(), b)))
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|x| x.1)
            .unwrap_or(BatteryChoice::Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08".into(), count: 1 })
    });
    let span = ch.span_mm.unwrap_or(req.max_span_mm);
    let root = ch.root_chord_mm.unwrap_or(200.0);
    let mut d = Design {
        name: spec.name.clone(),
        requirements: req,
        wing: Wing {
            span_mm: span,
            root_chord_mm: root,
            tip_chord_mm: ch.tip_chord_mm.unwrap_or((root * 0.7).round()),
            airfoil: ch.airfoil.clone().unwrap_or("sd7037".into()),
            incidence_deg: 2.0,
            washout_deg: 2.0,
            dihedral_deg: 0.0,
            aileron_chord_frac: 0.25,
        },
        tail: Tail { airfoil: "naca0009".into(), vh: TAIL_VH, vv: TAIL_VV, aspect_ratio_v: 1.6 },
        propulsion: Propulsion {
            lift_motor: ch.lift_motor.clone().unwrap_or("aos_supernova_2807_1400kv".into()),
            lift_prop: ch.lift_prop.clone().unwrap_or("gemfan_flash_7042_2blade".into()),
            lift_esc: ch.lift_esc.clone().unwrap_or("holybro_tekko32_f4_4in1_50a_am32".into()),
            cruise_motor: ch.cruise_motor.clone().unwrap_or("aos_supernova_2807_1400kv".into()),
            cruise_prop: ch.cruise_prop.clone().unwrap_or("gemfan_flash_7042_2blade".into()),
            cruise_esc: ch.cruise_esc.clone().unwrap_or("flycolor_raptor5_blheli32_45a".into()),
            lift_tilt_deg: 3.0,
            lift_current_limit_a: ch.lift_current_limit_a,
        },
        battery,
        avionics: Avionics {
            fc: "corewing_f405_wing_v2".into(),
            gps: "hglrc_m100_5883".into(),
            receiver: "radiomaster_rp1_v2".into(),
            airspeed: None,
            servo: ch.servo.clone().unwrap_or("emax_es08ma_ii".into()),
        },
        structure: Structure {
            main_spar: ch.main_spar.clone().unwrap_or("rockwest_t_rnd_314_l39".into()),
            rear_spar: "cst_cs328_1m_rod".into(),
            joiner: "cst_cs525_1m".into(),
            boom_tube: ch.boom_tube.clone().unwrap_or("goodwinds_wct375048".into()),
            leg_thickness_mm: 10.0,
            shell_filament: "bambu_pla_aero".into(),
            structural_filament: "bambu_petg_basic".into(),
        },
        hardware: Default::default()
    };
    sizing::choose_avionics(&mut d);
    if let Some(v) = &ch.fc {
        d.avionics.fc = v.clone();
    }
    if let Some(v) = &ch.gps {
        d.avionics.gps = v.clone();
    }
    if let Some(v) = &ch.receiver {
        d.avionics.receiver = v.clone();
    }
    if let Some(v) = &ch.airspeed {
        d.avionics.airspeed = v.clone();
    }
    let _ = c;
    d
}

fn motor_ok(id: &str, s: u32, policy: StockPolicy) -> bool {
    catalog().motors.get(id).is_some_and(|m| m.row.orderable(policy) && m.cells_max.is_some_and(|h| h >= s) && m.cells_min.unwrap_or(1) <= s)
}

pub fn lift_pairs(spec: &Spec) -> Vec<(String, String)> {
    let c = catalog();
    let s = spec.requirements.cells_s;
    let mut v = vec![];
    for m in c.motors.keys().filter(|m| motor_ok(m, s, spec.requirements.stock)) {
        if spec.design.lift_motor.as_ref().is_some_and(|x| x != m) {
            continue;
        }
        for (pid, p) in &c.props {
            if spec.design.lift_prop.as_ref().is_some_and(|x| x != pid) {
                continue;
            }
            // the prop must mount on the motor by published dimensions (the
            // lift_prop_mount check judges the same thing)
            let mounts = match (&p.hub, &c.motors[m].prop_interface) {
                (Some(h), Some(i)) => crate::catalog::mount_fit(h, i).is_ok(),
                _ => false,
            };
            if mounts && p.rotation.as_deref() == Some("pair") && p.row.orderable(spec.requirements.stock) && (7.0..=16.0).contains(&p.diameter_in) {
                v.push((m.clone(), pid.clone()));
            }
        }
    }
    v
}

pub fn cruise_pairs(spec: &Spec) -> Vec<(String, String)> {
    let c = catalog();
    let s = spec.requirements.cells_s;
    let mut v = vec![];
    for m in c.motors.keys().filter(|m| motor_ok(m, s, spec.requirements.stock)) {
        if spec.design.cruise_motor.as_ref().is_some_and(|x| x != m) {
            continue;
        }
        for (pid, p) in &c.props {
            if spec.design.cruise_prop.as_ref().is_some_and(|x| x != pid) {
                continue;
            }
            let mounts = match (&p.hub, &c.motors[m].prop_interface) {
                (Some(h), Some(i)) => crate::catalog::mount_fit(h, i).is_ok(),
                _ => false,
            };
            if mounts && p.row.orderable(spec.requirements.stock) && (7.0..=16.0).contains(&p.diameter_in) && p.blades == 2 {
                v.push((m.clone(), pid.clone()));
            }
        }
    }
    v
}

pub struct SearchLog {
    pub lines: Vec<String>,
    pub evaluations: usize,
}

fn chords(spec: &Spec) -> Vec<f64> {
    match spec.design.root_chord_mm {
        Some(c) => vec![c],
        None => (0..=10).map(|k| 150.0 + 10.0 * k as f64).collect(),
    }
}

/// Coordinate rounds from `start` with the battery held: every pusher pair,
/// every lift pair, their top few crossed, then the airframe.
fn rounds(spec: &Spec, start: Design, lifts: &[(String, String)], cruises: &[(String, String)], log: &mut SearchLog, progress: &dyn Fn(&str), screens: &[Screen]) -> Vec<Scored> {
    let airfoils: Vec<String> = match &spec.design.airfoil {
        Some(a) => vec![a.clone()],
        None => WING_AIRFOILS.iter().map(|s| s.to_string()).collect(),
    };
    let tapers: Vec<f64> = if spec.design.tip_chord_mm.is_some() { vec![] } else { vec![0.5, 0.6, 0.75, 1.0] };
    let mut best = score_with(&start, screens);
    let mut finalists = vec![best.clone()];
    for round in 0..4 {
        let before = best.score;
        let inc = best.design.clone();
        let cr = ranked(
            cruises.iter().map(|(m, p)| {
                let mut d = inc.clone();
                d.propulsion.cruise_motor = m.clone();
                d.propulsion.cruise_prop = p.clone();
                d
            }).collect(),
            4,
            screens,
        );
        let li = ranked(
            lifts.iter().map(|(m, p)| {
                let mut d = inc.clone();
                d.propulsion.lift_motor = m.clone();
                d.propulsion.lift_prop = p.clone();
                d
            }).collect(),
            4,
            screens,
        );
        log.evaluations += cruises.len() + lifts.len();
        let mut combos = vec![];
        let mut seen = std::collections::BTreeSet::new();
        for (lm, lp) in li.iter().map(|s| (s.design.propulsion.lift_motor.clone(), s.design.propulsion.lift_prop.clone())).chain([(inc.propulsion.lift_motor.clone(), inc.propulsion.lift_prop.clone())]) {
            for (cm, cp) in cr.iter().map(|s| (s.design.propulsion.cruise_motor.clone(), s.design.propulsion.cruise_prop.clone())).chain([(inc.propulsion.cruise_motor.clone(), inc.propulsion.cruise_prop.clone())]) {
                if !seen.insert((lm.clone(), lp.clone(), cm.clone(), cp.clone())) {
                    continue;
                }
                let mut d = inc.clone();
                d.propulsion.lift_motor = lm.clone();
                d.propulsion.lift_prop = lp.clone();
                d.propulsion.cruise_motor = cm.clone();
                d.propulsion.cruise_prop = cp.clone();
                combos.push(d);
            }
        }
        log.evaluations += combos.len();
        let co = ranked(combos, 3, screens);
        let mut frames = vec![];
        for base in &co {
            for a in &airfoils {
                for c in chords(spec) {
                    let taper_list: Vec<f64> = if tapers.is_empty() { vec![base.design.wing.tip_chord_mm / base.design.wing.root_chord_mm] } else { tapers.clone() };
                    for t in taper_list {
                        let mut d = base.design.clone();
                        d.wing.airfoil = a.clone();
                        d.wing.root_chord_mm = c;
                        d.wing.tip_chord_mm = (c * t).round();
                        frames.push(d);
                    }
                }
            }
        }
        log.evaluations += frames.len();
        let fr = ranked(frames, 6, screens);
        if fr[0].score > best.score {
            best = fr[0].clone();
        }
        if co[0].score > best.score {
            best = co[0].clone();
        }
        progress(&format!("  round {}: {:.1} km", round + 1, best.score));
        finalists.extend(fr);
        finalists.extend(co);
        if best.score <= before + 0.05 {
            break;
        }
    }
    finalists
}

/// The search; returns the ranked finalists (sized, not yet repaired).
pub fn search(spec: &Spec, log: &mut SearchLog, progress: &dyn Fn(&str)) -> Vec<Scored> {
    search_under(spec, &[], log, progress)
}

pub fn search_under(spec: &Spec, screens: &[Screen], log: &mut SearchLog, progress: &dyn Fn(&str)) -> Vec<Scored> {
    let s0 = seed(spec);
    let lifts = lift_pairs(spec);
    let cruises = cruise_pairs(spec);
    // the battery shortlist: every orderable option, ranked by its energy
    // over its mass plus a 1.2 kg airframe (a cheap screen; the rounds then
    // judge each with the whole model), priced inside the budget less $300
    let mut batts: Vec<(f64, BatteryChoice)> = match &spec.design.battery {
        Some(b) => vec![(0.0, b.clone())],
        None => battery::options(spec.requirements.cells_s, spec.requirements.stock)
            .into_iter()
            .filter_map(|b| {
                let r = battery::resolve(&b).ok()?;
                let price = r.price?.as_usd();
                if spec.requirements.budget_usd.is_some_and(|bu| price > bu - 300.0) {
                    return None;
                }
                Some((-r.pack.usable().as_wh() / (r.pack.mass.as_kg() + 1.2), b))
            })
            .collect(),
    };
    batts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    // the best few bought packs AND the best few hand-built packs: ranked
    // together, the many P counts of a dense cell crowd out every product
    let (packs, cells): (Vec<_>, Vec<_>) = batts.into_iter().partition(|b| matches!(b.1, BatteryChoice::Pack { .. }));
    let mut batts: Vec<(f64, BatteryChoice)> = packs.into_iter().take(BATTERY_SHORTLIST / 2).chain(cells.into_iter().take(BATTERY_SHORTLIST / 2)).collect();
    batts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut finalists = vec![];
    for (k, (_, b)) in batts.iter().enumerate() {
        let mut start = s0.clone();
        start.battery = b.clone();
        progress(&format!("battery {}/{}: {:?}", k + 1, batts.len(), b));
        let f = rounds(spec, start, &lifts, &cruises, log, progress, screens);
        let top = f.iter().map(|s| s.score).fold(f64::NEG_INFINITY, f64::max);
        log.lines.push(format!("battery {b:?}: best {top:.1}"));
        finalists.extend(f);
    }
    finalists.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut uniq: Vec<Scored> = vec![];
    for f in finalists {
        if !uniq.iter().any(|u| u.design == f.design) {
            uniq.push(f);
        }
    }
    if let Some(b) = uniq.first() {
        log.lines.push(format!(
            "best: {:.1} km -- {} / {} lift, {} / {} pusher, {:?}, {} {}/{} mm",
            b.score, b.design.propulsion.lift_motor, b.design.propulsion.lift_prop, b.design.propulsion.cruise_motor, b.design.propulsion.cruise_prop, b.design.battery, b.design.wing.airfoil, b.design.wing.root_chord_mm, b.design.wing.tip_chord_mm
        ));
    }
    uniq
}

pub const BATTERY_SHORTLIST: usize = 10;

/// Search, then repair the finalists and judge each by EVERY check; the
/// furthest-flying one that passes them all (or, if none does, the closest
/// to passing) is the design.
pub fn design(spec: &Spec, log: &mut SearchLog, progress: &dyn Fn(&str)) -> anyhow::Result<Design> {
    design_under(spec, &[], log, progress)
}

/// The search, every candidate also screened on each confirmed attack's
/// check under that attack's requirements. The first finalist passing every
/// check and every screen is the design; if none does, the closest one is
/// returned (fewest confirmed attacks failed, then smallest check
/// violation) with a warning in the log and on stderr naming what it
/// fails (review 5, F6): the harness's own checks then show the failures.
pub fn design_under(spec: &Spec, screens: &[Screen], log: &mut SearchLog, progress: &dyn Fn(&str)) -> anyhow::Result<Design> {
    spec.requirements.validate().map_err(|e| anyhow::anyhow!(e))?;
    let finalists = search_under(spec, screens, log, progress);
    let mut screen_failures: Vec<String> = vec![];
    let mut best_fail: Option<(f64, Design)> = None;
    // finalists failing a confirmed attack: (attacks failed, violation)
    let mut best_screened: Option<((usize, f64), Design, String)> = None;
    for (k, f) in finalists.iter().take(24).enumerate() {
        let mut d = match sizing::size_and_repair(f.design.clone()) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if sizing::choose_servo(&mut d).is_err() {
            continue;
        }
        let d = sizing::size_and_repair(d).unwrap_or_else(|_| f.design.clone());
        let fin = checks::check(&d);
        log.evaluations += 1;
        progress(&format!("finalist {}: {:.1} km, {} failing", k + 1, f.range_km, fin.failures().count()));
        let keeps = checks::evaluate(&d, Factors::NOMINAL).map(|e| Allowance::of(&e).margins(&d.requirements).iter().all(|m| m.1 >= 0.0)).unwrap_or(false);
        let screened: Vec<String> = screens.iter().filter_map(|sc| passes_screen(&d, sc).err()).collect();
        if !screened.is_empty() {
            log.lines.push(format!("finalist {} fails a confirmed attack: {}", k + 1, screened.join("; ")));
            screen_failures.push(screened.join("; "));
            let key = (screened.len(), fin.violation(violation_scale));
            if best_screened.as_ref().is_none_or(|b| key.0 < b.0 .0 || (key.0 == b.0 .0 && key.1 < b.0 .1)) {
                best_screened = Some((key, d, screened.join("; ")));
            }
            continue;
        }
        if fin.passed() && keeps {
            log.lines.push(format!("finalist {} passes every check and keeps the model allowance", k + 1));
            return Ok(d);
        }
        if fin.passed() {
            log.lines.push(format!("finalist {} passes every check but not the model allowance", k + 1));
            if best_fail.as_ref().is_none_or(|b| b.0 > 0.0) {
                best_fail = Some((0.0, d));
            }
            continue;
        }
        let v = fin.violation(violation_scale);
        log.lines.push(format!("finalist {} fails: {}", k + 1, fin.failures().map(|o| o.name.clone()).collect::<Vec<_>>().join(", ")));
        if best_fail.as_ref().is_none_or(|b| v < b.0) {
            best_fail = Some((v, d));
        }
    }
    if !screens.is_empty() {
        // nothing passes every check AND every confirmed attack: return the
        // closest, and say so (not an error: the harness judges it)
        let warn = |what: String| {
            let w = format!("WARNING: no design in the search passes every check and the confirmed attacks ({}); returning the closest: {what}", screens.iter().map(|s| format!("{} under {}", s.check, s.id)).collect::<Vec<_>>().join(", "));
            eprintln!("{w}");
            w
        };
        if let Some((_, d)) = best_fail.take() {
            let w = warn("it passes the attacks but fails a check".into());
            log.lines.push(w);
            return Ok(d);
        }
        if let Some((_, d, why)) = best_screened {
            let w = warn(format!("it fails {why}"));
            log.lines.push(w);
            return Ok(d);
        }
        anyhow::bail!("no finalist could be sized under the confirmed attacks: {}", screen_failures.first().cloned().unwrap_or_else(|| "none could be sized".into()));
    }
    best_fail.map(|b| b.1).ok_or_else(|| anyhow::anyhow!("no finalist could be sized"))
}

/// Ok when `d` passes `sc.check` under the screen's requirements (the fast
/// analysis, or the full check for a mesh/slicer check); Err says how not.
pub fn passes_screen(d: &Design, sc: &Screen) -> Result<(), String> {
    let o = if MESH_CHECKS.contains(&sc.check.as_str()) {
        checks::check_against(&sc.requirements, d).get(&sc.check).cloned()
    } else {
        let mut e = d.clone();
        e.requirements = sc.requirements.clone();
        checks::evaluate(&e, Factors::NOMINAL).ok().and_then(|ev| checks::physics_only(&ev).get(&sc.check).cloned())
    };
    match o {
        Some(o) if o.passed() => Ok(()),
        Some(o) => Err(format!("{} under {}: {o}", sc.check, sc.id)),
        None => Err(format!("{} under {}: no such check or not evaluated", sc.check, sc.id)),
    }
}
