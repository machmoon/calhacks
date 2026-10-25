//! Every claim about an aircraft as a check with a signed margin.
//!
//! The closed set [`Check`]; `wright_core::Report` fails any check nobody
//! evaluated and any non-finite margin. Each check names its source and,
//! where a rule chose the part, measures it with a different model (see
//! `sizing` and `electrical`).

use crate::ardupilot::{self, Params};
use crate::bom;
use crate::electrical::{self, Electrical};
use crate::geometry::{self, PartGeometry};
use crate::layout::{self, Layout};
use crate::perf::{self, Performance, Structure, LOAD_PATHS};
use crate::powertrain::Factors;
use crate::rules::*;
use crate::spec::Design;
use wright_core::{Bom, CheckId, Finished, Report, Source};

macro_rules! checks {
    ($($v:ident => $n:literal, $d:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Check { $($v),* }
        impl CheckId for Check {
            const ALL: &'static [Self] = &[$(Check::$v),*];
            fn name(self) -> &'static str { match self { $(Check::$v => $n),* } }
            fn describe(self) -> &'static str { match self { $(Check::$v => $d),* } }
        }
    };
}

checks! {
    SpecRequirements => "spec_requirements", "the design's requirements are the spec's (every check is judged against the spec; a design that moved one fails)";
    Parts => "parts", "every part is a valid catalog row with a source URL";
    PartsOrderable => "parts_orderable", "every BOM line's catalog stock state is one the spec's stock policy accepts (made to order, pre-order, out of stock refused by default)";
    HoverThrustToWeight => "hover_thrust_to_weight", "four lift motors at full throttle on one sagging pack (never below its cells' minimum voltage), hot day, low battery, capped by motor/ESC/pack/connector ratings, less the thrust model's measured error, over weight, minus 1.8";
    HoverThrottle => "hover_throttle", "0.65 minus the hot-day low-battery hover throttle";
    LiftMotorCurrent => "lift_motor_current", "lift motor rating minus its hover current and its full-throttle current with no current limit";
    LiftEscCurrent => "lift_esc_current", "lift ESC continuous rating minus hover current, and burst minus full-throttle current with no current limit";
    PackCurrent => "pack_current", "pack continuous rating minus hover current";
    PackPeakCurrent => "pack_peak_current", "pack burst rating minus the worst seconds' current (lift motors flat out, no current limit: independent of the cap)";
    VoltageRatings => "voltage_ratings", "every motor, ESC, flight controller and servo rated for its supply (a missing rating fails)";
    PusherEscCurrent => "pusher_esc_current", "pusher ESC rating minus the pusher's full-throttle current";
    Cruise => "cruise", "a trimmed, powered best-range cruise exists; 0.9 minus its pusher throttle";
    MaxSpeed => "max_speed", "top speed over best-range speed minus 1.3";
    StallSpeed => "stall_speed", "required stall speed minus predicted stall with the airfoil's measured CLmax over-prediction taken off";
    StaticMargin => "static_margin", "distance inside the 5-15 % MAC band (INFORMATIONAL: the layout places the battery for 10 %, so this restates that rule; it fails only when the layout could not; the `cfd` stage re-judges the same band from AVL's neutral point as `cfd_static_margin`)";
    TailAuthority => "tail_authority", "tail CL needed to trim at the stall within 0.8 of the tail section's CLmax";
    SparStress => "spar_stress", "joiner at the root and spar where the joiner ends, Schrenk manoeuvre and full-thrust hover loads at ultimate, MPa";
    SparJoint => "spar_joint", "the joiner slides over or into the spar within the bond clearance";
    SparFit => "spar_fit", "main spar, rear spar and joiner inside the wing's skin along their run, mm";
    BoomStress => "boom_stress", "booms under full thrust, landing (14 CFR 23.473) and tail loads at ultimate, MPa";
    BoomBuckling => "boom_buckling", "boom tube local shell buckling (NASA SP-8007 bending, with its knock-down) over the highest boom bending stress at ultimate, ratio minus 1";
    BoomResonance => "boom_resonance", "the boom's first mode at least 30 % from the hover rotation rate";
    LandingLegs => "landing_legs", "legs at 2.67 g x 1.5 with friction side load: buckling and combined stress";
    LoadPaths => "load_paths", "every load reaches the pod through members that exist and touch";
    Interference => "interference", "no two parts overlap, over ALL pairs except declared joints (meshes)";
    Attachments => "attachments", "every declared joint is in contact (meshes)";
    PropClearance => "prop_clearance", "every spinning disc 5 mm clear of every part but its own motor (meshes)";
    Containment => "containment", "battery and avionics inside the pod's cavity, mm";
    ServoFit => "servo_fit", "aileron servo inside the wing's skins with 0.5 mm spare";
    PrintBed => "print_bed", "every printed piece, split to the bed, fits it with the margin";
    GroundClearance => "ground_clearance", "feet 15 mm below every other point, a folding pusher folded (INFORMATIONAL: the legs are sized by this rule; measured on the meshes)";
    ThrustLine => "thrust_line", "pusher thrust line within 15 mm of the CG's height (INFORMATIONAL: restates the layout rule that places the pusher)";
    PropMount => "prop_mount", "a folding pusher's collet has a long plain shaft to grip";
    LiftPropPairs => "lift_prop_pairs", "lift props sold as CW/CCW pairs";
    PusherPropMount => "pusher_prop_mount", "the pusher prop's published hub (bore, bolt circle or collet) fits the pusher motor's published prop mount; unpublished on either side fails";
    LiftPropMount => "lift_prop_mount", "the lift prop's published hub (bolt circle or bore) fits the lift motor's published prop mount; unpublished on either side fails";
    WireTemperature => "wire_temperature", "every run's conductor temperature after a burst, below the insulation rating";
    BatteryConnector => "battery_connector", "connector continuous rating over hover current and burst over the uncapped peak";
    FcOutputs => "fc_outputs", "flight controller outputs minus the 10 the pin map uses";
    ServoPower => "servo_power", "servo BEC peak rating (continuous if no peak is stated) minus five servos stalled";
    ArdupilotParams => "ardupilot_params", "every parameter known to ArduPlane and inside its published range or values";
    Budget => "budget", "budget minus the BOM total, every line priced";
    Span => "span", "allowed span minus span, mm";
    Range => "range", "range with 15 % reserve minus the requirement, km";
    Endurance => "endurance", "endurance with reserve minus the requirement, min";
    AirspeedSensor => "airspeed_sensor", "an airspeed sensor on board when required";
    PrintTime => "print_time", "PrusaSlicer's total print time for every printed piece, against the limit";
    PrintedMass => "printed_mass", "PrusaSlicer's grams per filament no more than the mass model flew with";
}

/// Everything known about one design.
pub struct Evaluation {
    pub layout: Layout,
    pub perf: Performance,
    pub structure: Structure,
    pub elec: Electrical,
    pub params: Params,
    pub param_problems: Vec<String>,
    pub bom: Bom,
    pub bom_problems: Vec<String>,
}

pub fn evaluate(d: &Design, f: Factors) -> Result<Evaluation, String> {
    let layout = layout::solve(d)?;
    let perf = perf::evaluate(&layout, f);
    let structure = perf::structure(&layout, &perf.hover, perf.max_speed);
    let elec = electrical::design(&layout, &perf);
    let params = ardupilot::build(&layout, &perf, &elec);
    let param_problems = ardupilot::validate(&params);
    let (bom, bom_problems) = bom::bom(&layout, Some(&elec));
    Ok(Evaluation { layout, perf, structure, elec, params, param_problems, bom, bom_problems })
}

fn rating_margin(what: &str, lo: Option<u32>, hi: Option<u32>, s: u32) -> (f64, String) {
    match hi {
        None => (-(s as f64), format!("{what} states no cell rating")),
        Some(h) => {
            let m = (h as f64 - s as f64).min(s as f64 - lo.unwrap_or(1) as f64);
            (m, format!("{what} {}-{h}S", lo.map(|l| l.to_string()).unwrap_or("?".into())))
        }
    }
}

/// The physics, electrical, configuration and requirement checks: fast
/// enough for the search. Geometry checks on meshes are added by [`check`].
pub fn physics(e: &Evaluation, r: &mut Report<Check>) {
    let lay = &e.layout;
    let p = &lay.parts;
    let d = &lay.design;
    let req = &d.requirements;
    let h = &e.perf.hover;
    let s = lay.battery.pack.s;
    r.margin(Check::Parts, 0.0, "", Source::Requirement, "every part resolved to a sourced catalog row");
    {
        // every BOM line (the battery and hardware included), against the
        // stock policy the spec states; a line with no catalog row fails
        let c = crate::catalog::catalog();
        let policy = req.stock;
        let (mut refused, mut unstated, mut n) = (vec![], 0usize, 0usize);
        for l in &e.bom.lines {
            if l.category == "printed" {
                continue;
            }
            n += 1;
            let id = l.id.strip_suffix("_hub").filter(|_| l.category == "props").unwrap_or(&l.id);
            match c.row(&l.category, id) {
                Some(row) if row.stock.accepted(policy) => unstated += (row.stock == crate::catalog::Stock::Unstated) as usize,
                Some(row) => refused.push(format!("{} ({})", row.id, row.stock.label())),
                None => refused.push(format!("{} (no catalog row)", l.id)),
            }
        }
        let text = if refused.is_empty() {
            format!("{n} bought lines accepted under stock policy {policy:?}; {} in stock, {unstated} listed with stock unstated", n - unstated)
        } else {
            format!("refused under stock policy {policy:?}: {}", refused.join(", "))
        };
        r.margin(Check::PartsOrderable, -(refused.len() as f64), "parts", Source::Rating, text);
    }
    // the thrust model's worst over-prediction against the makers' tables
    // for what stopped the motors (rules: THRUST_*_OVER) is taken off
    let over = match (h.full.limit, p.lift_motor.calibrated_on.is_some()) {
            ("voltage", true) => THRUST_CALIBRATED_OVER,
            ("voltage", false) => THRUST_VOLTAGE_LIMITED_OVER,
            _ => THRUST_AT_CURRENT_OVER,
        };
    let tw = h.thrust_to_weight / (1.0 + over);
    r.margin(Check::HoverThrustToWeight, tw - HOVER_TW_MIN, "", Source::Physics, format!("T/W {tw:.2} ({:.2} modelled, less {:.1} % thrust-model error), stopped by the {} at {:.0} A / {:.1} V from the pack (hot day +{} K, low battery)", h.thrust_to_weight, 100.0 * over, h.full.limit, h.full.pack_a.as_a(), h.full.pack_v.value(), req.hot_day_dk));
    match (&h.hot_low, &h.std_full) {
        (Ok(lo), Ok(fu)) => {
            r.margin(Check::HoverThrottle, HOVER_THROTTLE_MAX - lo.throttle, "", Source::Physics, format!("hover throttle {:.2} standard day full pack, {:.2} hot day low pack", fu.throttle, lo.throttle));
            let free_a = h.free.motor_a.as_a();
            let rated = p.lift_motor.max_current.as_a();
            r.margin(Check::LiftMotorCurrent, (rated - lo.motor_a.as_a()).min(rated - free_a), "A", Source::Rating, format!("{:.1} A per motor in hover, {free_a:.1} A at full throttle (ESC firmware limit if set, else none); rated {rated:.0} A", lo.motor_a.as_a()));
            let burst = p.lift_esc.burst_current.unwrap_or(p.lift_esc.cont_current).as_a();
            let m = (p.lift_esc.cont_current.as_a() - lo.motor_a.as_a()).min(burst - free_a);
            match (d.propulsion.lift_current_limit_a, &p.lift_esc.current_limit_source) {
                (None, _) => r.margin(Check::LiftEscCurrent, m, "A", Source::Rating, format!("{:.1} A hover / {free_a:.1} A full throttle, no current limit, per channel; ESC {:.0} A cont, {:.0} A burst", lo.motor_a.as_a(), p.lift_esc.cont_current.as_a(), burst)),
                (Some(l), Some(src)) if l > 0.0 && l <= 200.0 && (l / 2.0).fract() == 0.0 => r.margin(Check::LiftEscCurrent, m, "A", Source::Rating, format!("{:.1} A hover / {free_a:.1} A full throttle under the ESC firmware limit {l:.0} A per motor (AM32 limits.current = {:.0}, {src}); ESC {:.0} A cont, {:.0} A burst", lo.motor_a.as_a(), l / 2.0, p.lift_esc.cont_current.as_a(), burst)),
                (Some(l), Some(_)) => r.margin(Check::LiftEscCurrent, -1.0, "A", Source::Rating, format!("current limit {l} A is not a setting AM32 has (1-100 x 2 A)")),
                (Some(l), None) => r.margin(Check::LiftEscCurrent, -l, "A", Source::Rating, format!("{} has no documented per-motor current limit, so the {l} A limit the design relies on cannot be set", p.lift_esc.row.id)),
            }
            r.margin(Check::PackCurrent, lay.battery.pack.max_cont.as_a() - h.pack_current_low.as_a(), "A", Source::Rating, format!("hover {:.1} A from the pack, rated {:.0} A", h.pack_current_low.as_a(), lay.battery.pack.max_cont.as_a()));
        }
        (lo, fu) => {
            let why = lo.as_ref().err().or(fu.as_ref().err()).cloned().unwrap_or_default();
            for c in [Check::HoverThrottle, Check::LiftMotorCurrent, Check::LiftEscCurrent, Check::PackCurrent] {
                r.missing(c, Source::Physics, format!("no hover operating point: {why}"));
            }
        }
    }
    r.margin(Check::PackPeakCurrent, lay.battery.pack.burst.as_a() - e.elec.pack_peak_a, "A", Source::Rating, format!("worst seconds {:.0} A (four lift motors flat out under the ESC firmware limit if set, else none; full pack); {} bursts {:.0} A", e.elec.pack_peak_a, lay.battery.label, lay.battery.pack.burst.as_a()));
    let mut worst = (f64::INFINITY, String::new());
    for (m, w) in [
        rating_margin(&format!("lift motor {}", p.lift_motor.row.id), p.lift_motor.cells_min, p.lift_motor.cells_max, s),
        rating_margin(&format!("pusher motor {}", p.cruise_motor.row.id), p.cruise_motor.cells_min, p.cruise_motor.cells_max, s),
        rating_margin(&format!("lift ESC {}", p.lift_esc.row.id), p.lift_esc.cells_min, p.lift_esc.cells_max, s),
        rating_margin(&format!("pusher ESC {}", p.cruise_esc.row.id), p.cruise_esc.cells_min, p.cruise_esc.cells_max, s),
        rating_margin(&format!("flight controller {}", p.fc.row.id), p.fc.cells.map(|c| c.0), p.fc.cells.map(|c| c.1), s),
    ] {
        if m < worst.0 {
            worst = (m, w);
        }
    }
    r.margin(Check::VoltageRatings, worst.0, "cells", Source::Rating, format!("{s}S pack; tightest {}", worst.1));
    r.margin(Check::PusherEscCurrent, p.cruise_esc.cont_current.as_a() - e.perf.pusher_full_current.as_a(), "A", Source::Rating, format!("pusher {:.1} A at full throttle, ESC {:.0} A", e.perf.pusher_full_current.as_a(), p.cruise_esc.cont_current.as_a()));
    match &e.perf.best_range {
        Some(br) => {
            r.margin(Check::Cruise, CRUISE_THROTTLE_MAX - br.point.throttle, "", Source::Physics, format!("{:.1} m/s at {:.0} W, L/D {:.1}, prop {:.2}, motor {:.2}, throttle {:.2}", br.speed.as_mps(), br.power.as_w(), br.trim.lift_to_drag(), br.point.prop_efficiency, br.point.motor_efficiency, br.point.throttle));
            r.margin(Check::MaxSpeed, e.perf.max_speed / br.speed - MAX_OVER_CRUISE_MIN, "", Source::Physics, format!("top {:.1} m/s, best range {:.1} m/s", e.perf.max_speed.as_mps(), br.speed.as_mps()));
        }
        None => {
            r.missing(Check::Cruise, Source::Physics, e.perf.notes.join("; "));
            r.missing(Check::MaxSpeed, Source::Physics, "no cruise");
        }
    }
    // the section CLmax's measured over-prediction for this airfoil (rules:
    // CLMAX_OVER, wind tunnel vs the polar table) is taken off
    let co = clmax_over(&d.wing.airfoil);
    let stall = e.perf.stall_eas.as_mps();
    r.margin(Check::StallSpeed, req.max_stall_ms - stall, "m/s", Source::Physics, format!("stall {stall:.1} m/s EAS at CLmax {:.2} ({:.2} modelled, less {:.1} % measured CLmax error for {}; {:.1} m/s TAS at the {:.0} m field), at most {} EAS", e.perf.cl_max_used, e.perf.cl_max, 100.0 * co, d.wing.airfoil, e.perf.stall.as_mps(), req.altitude_m, req.max_stall_ms));
    let sm = lay.static_margin;
    r.margin(Check::StaticMargin, 100.0 * (sm - STATIC_MARGIN_MIN).min(STATIC_MARGIN_MAX - sm), "%MAC", Source::Physics, format!("{:.1} % MAC: CG {:.0} mm, neutral point {:.0} mm", 100.0 * sm, lay.x_cg.as_mm(), lay.x_np.as_mm()));
    // tail authority at the stall
    {
        let air = crate::atmosphere::isa(wright_core::units::Length::m(req.altitude_m), 0.0);
        let v = e.perf.stall;
        let wm = crate::aero::WingModel::new(lay, v, &air);
        let (mut amax, mut clm) = (0.0, f64::NEG_INFINITY);
        for k in 0..=44 {
            let a = -4.0 + 0.5 * k as f64;
            let c = wm.coefficients(a).0;
            if c > clm {
                clm = c;
                amax = a;
            }
        }
        let (cl, _, cm) = wm.coefficients(amax);
        let q = air.q(v).value();
        let (sw, c) = (lay.area.as_m2(), lay.mac.as_m());
        let lw = q * sw * cl;
        let lh = (lw * (lay.x_cg - lay.x_ac_wing).as_m() + q * sw * c * cm) / (lay.x_ac_tail - lay.x_cg).as_m();
        let cl_h = lh / (q * lay.h_area.as_m2());
        let re_h = air.reynolds(v, lay.h_chord);
        let (clmax_h, _) = p.tail_airfoil.cl_max(re_h);
        r.margin(Check::TailAuthority, TAIL_CL_USE_MAX * clmax_h - cl_h.abs(), "", Source::Physics, format!("tail CL {cl_h:+.2} to trim at the stall; section CLmax {clmax_h:.2}"));
    }
    let st = &e.structure;
    let mut spar = (f64::INFINITY, String::new());
    let mut boom = (f64::INFINITY, String::new());
    let mut leg = (f64::INFINITY, String::new());
    for m in &st.members {
        let slot = match m.name {
            "joiner" | "main_spar" => &mut spar,
            "boom" => &mut boom,
            _ => &mut leg,
        };
        if m.margin_mpa() < slot.0 {
            *slot = (m.margin_mpa(), format!("{} in {}: {:.0} MPa of {:.0} ({})", m.name, m.load_case, m.stress_pa / 1e6, m.allowable_pa / 1e6, m.detail));
        }
    }
    r.margin(Check::SparStress, spar.0, "MPa", Source::Physics, spar.1);
    r.margin(Check::BoomStress, boom.0, "MPa", Source::Physics, boom.1);
    r.margin(Check::LandingLegs, (st.leg_buckling_n / st.leg_load_n - 1.0).min(leg.0 / (PETG.allowable_pa / 1e6)), "", Source::Physics, format!("buckle at {:.0} N for {:.0} N; {}", st.leg_buckling_n, st.leg_load_n, leg.1));
    // boom shell buckling against the worst boom bending stress (ultimate)
    let worst = st.members.iter().filter(|m| m.name == "boom").max_by(|a, b| a.stress_pa.total_cmp(&b.stress_pa));
    if let Some(m) = worst {
        let (cr, gamma, rt) = st.boom_buckling;
        r.margin(Check::BoomBuckling, cr / m.stress_pa - 1.0, "", Source::Physics, format!("buckles at {:.0} MPa (SP-8007, R/t {rt:.1}, knock-down {gamma:.2}) under {:.0} MPa in {}: {:.1} x", cr / 1e6, m.stress_pa / 1e6, m.load_case, cr / m.stress_pa));
    }
    let ratio = st.boom_frequency_hz / st.hover_rotation_hz;
    r.margin(Check::BoomResonance, (ratio - 1.0).abs() - BOOM_FREQUENCY_SEPARATION, "", Source::Physics, format!("boom mode {:.0} Hz, hover rotation {:.0} Hz", st.boom_frequency_hz, st.hover_rotation_hz));
    match lay.joiner_kind {
        Some(k) => r.margin(Check::SparJoint, 0.0, "mm", Source::Geometry, format!("{} {} on {}", k, p.joiner.row.id, p.main_spar.row.id)),
        None => r.margin(Check::SparJoint, -1.0, "mm", Source::Geometry, format!("{} neither slides over nor into {}", p.joiner.row.id, p.main_spar.row.id)),
    }
    let semi = lay.semi_span().as_m();
    let mut fit = (f64::INFINITY, String::new());
    let jr = if lay.joiner_kind == Some("sleeve") { p.joiner.od.as_m() } else { p.main_spar.od.as_m() } / 2.0;
    for (name, x, z, rr, y1) in [
        ("main spar", lay.main_spar_x.as_m(), lay.main_spar_z.as_m(), p.main_spar.od.as_m() / 2.0, MAIN_SPAR_REACH * semi),
        ("joiner", lay.main_spar_x.as_m(), lay.main_spar_z.as_m(), jr, JOINER_HALF_FRAC * semi),
        ("rear spar", lay.rear_spar_x.as_m(), lay.rear_spar_z.as_m(), p.rear_spar.od.as_m() / 2.0, REAR_SPAR_REACH * semi),
    ] {
        let (m, y) = geometry::tube_in_wing(lay, x, z, rr, 0.0, y1);
        if m < fit.0 {
            fit = (m, format!("{name}: {:.1} mm inside the skin at its tightest, {:.0} mm out", m * 1000.0, y * 1000.0));
        }
    }
    r.margin(Check::SparFit, fit.0 * 1000.0, "mm", Source::Geometry, fit.1);
    let sv = geometry::servo_margin(lay);
    r.margin(Check::ServoFit, (sv - SERVO_SPARE_M) * 1000.0, "mm", Source::Geometry, format!("{} in the wing at {:.0} mm span: {:.1} mm inside the skins", p.servo.row.id, lay.aileron_servo[1].as_mm(), sv * 1000.0));
    let mut cont = (f64::INFINITY, String::new());
    for b in &lay.bay {
        let c = [b.center[0].as_m(), b.center[1].as_m(), b.center[2].as_m()];
        let hh = [b.size[0].as_m() / 2.0, b.size[1].as_m() / 2.0, b.size[2].as_m() / 2.0];
        let m = geometry::box_in_pod(lay, [c[0] - hh[0], c[1] - hh[1], c[2] - hh[2]], [c[0] + hh[0], c[1] + hh[1], c[2] + hh[2]]);
        if m < cont.0 {
            cont = (m, format!("tightest {}: {:.1} mm inside the cavity", b.name, m * 1000.0));
        }
    }
    r.margin(Check::Containment, cont.0 * 1000.0, "mm", Source::Geometry, cont.1);
    let dz = (lay.pusher_z - lay.z_cg).as_m();
    r.margin(Check::ThrustLine, (THRUST_LINE_MAX_M - dz.abs()) * 1000.0, "mm", Source::Geometry, format!("pusher {:+.0} mm from the CG's height", dz * 1000.0));
    let folding_collet = p.cruise_prop.folding && p.cruise_prop.intended.as_deref() != Some("multirotor");
    let ok = !folding_collet || p.cruise_motor.prop_mount.as_deref() == Some("plain_long");
    r.margin(Check::PropMount, if ok { 0.0 } else { -1.0 }, "", Source::Rating, format!("{} on {} ({} shaft)", p.cruise_prop.row.id, p.cruise_motor.row.id, p.cruise_motor.prop_mount.as_deref().unwrap_or("unstated")));
    r.margin(Check::LiftPropPairs, if p.lift_prop.rotation.as_deref() == Some("pair") { 0.0 } else { -1.0 }, "", Source::Rating, format!("lift prop rotation {:?}", p.lift_prop.rotation));
    {
        for (check, lp, lm) in [(Check::LiftPropMount, &p.lift_prop, &p.lift_motor), (Check::PusherPropMount, &p.cruise_prop, &p.cruise_motor)] {
            let fit = match (&lp.hub, &lm.prop_interface) {
                (Some(h), Some(i)) => crate::catalog::mount_fit(h, i),
                (None, _) => Err(format!("{} publishes no hub dimensions", lp.row.id)),
                (_, None) => Err(format!("{} publishes no prop mount", lm.row.id)),
            };
            match fit {
                Ok(how) => r.margin(check, 0.0, "", Source::Rating, format!("{} on {}: {how}", lp.row.id, lm.row.id)),
                Err(why) => r.margin(check, -1.0, "", Source::Rating, format!("{} on {}: {why}", lp.row.id, lm.row.id)),
            }
        }
    }
    let mut wt = (f64::NEG_INFINITY, String::new());
    for run in &e.elec.runs {
        let (tb, ts) = electrical::wire_temperature(run.awg, run.continuous_a, run.peak_a);
        if tb > wt.0 {
            wt = (tb, format!("{}: {} AWG, {:.0} C steady at {:.1} A, {:.0} C after {:.0} s at {:.1} A", run.name, run.awg, ts, run.continuous_a, tb, BURST_S, run.peak_a));
        }
    }
    r.margin(Check::WireTemperature, SILICONE_MAX_C - wt.0, "C", Source::Physics, wt.1);
    let con = e.elec.connector;
    r.margin(Check::BatteryConnector, (con.1 - e.elec.pack_hover_a).min(con.2 - e.elec.pack_peak_a), "A", Source::Rating, format!("{} {:.0}/{:.0} A for {:.1} A hover, {:.0} A peak", con.0, con.1, con.2, e.elec.pack_hover_a, e.elec.pack_peak_a));
    r.margin(Check::FcOutputs, p.fc.pwm_outputs as f64 - 10.0, "outputs", Source::Rating, format!("{} has {} outputs", p.fc.row.id, p.fc.pwm_outputs));
    // five servos stalled at once is a transient: against the BEC's peak
    // rating when the maker states one, else its continuous rating
    let bec = p.fc.servo_bec_peak.unwrap_or(p.fc.servo_bec).as_a();
    let kind = if p.fc.servo_bec_peak.is_some() { "peak" } else { "continuous (no peak stated)" };
    r.margin(Check::ServoPower, bec - e.elec.servo_demand_a, "A", Source::Rating, format!("servo BEC {bec:.1} A {kind}, five servos stall at {:.1} A", e.elec.servo_demand_a));
    r.margin(Check::ArdupilotParams, -(e.param_problems.len() as f64), "params", Source::Rating, if e.param_problems.is_empty() { format!("{} parameters, all inside ArduPlane's metadata", e.params.values.len()) } else { e.param_problems.join("; ") });
    let total = e.bom.total().as_usd();
    let unpriced = e.bom.unpriced();
    if !unpriced.is_empty() || !e.bom_problems.is_empty() {
        let ids: Vec<&str> = unpriced.iter().map(|l| l.id.as_str()).collect();
        r.missing(Check::Budget, Source::Requirement, format!("unpriced: {}; problems: {}", ids.join(", "), e.bom_problems.join("; ")));
    } else {
        let budget = req.budget_usd.unwrap_or(f64::INFINITY);
        let m = if budget.is_finite() { budget - total } else { 0.0 };
        r.margin(Check::Budget, m, "USD", Source::Requirement, format!("BOM ${total:.0} over {} lines; budget {}", e.bom.lines.len(), req.budget_usd.map(|b| format!("${b:.0}")).unwrap_or("none stated".into())));
    }
    r.margin(Check::Span, req.max_span_mm - d.wing.span_mm, "mm", Source::Requirement, format!("span {} mm, allowed {}", d.wing.span_mm, req.max_span_mm));
    r.margin(Check::Range, e.perf.range_km - req.range_km.unwrap_or(0.0), "km", Source::Physics, format!("{:.1} km with 15 % reserve ({:.1} km to cutoff)", e.perf.range_km, e.perf.range_to_cutoff_km));
    r.margin(Check::Endurance, e.perf.endurance_min - req.endurance_min.unwrap_or(0.0), "min", Source::Physics, format!("{:.0} min with reserve", e.perf.endurance_min));
    let has = p.airspeed.is_some();
    r.margin(Check::AirspeedSensor, if has || !req.airspeed_sensor { 0.0 } else { -1.0 }, "", Source::Requirement, if has { "on board".into() } else if req.airspeed_sensor { "required, none on board".to_string() } else { "not required".into() });
}

/// The checks on meshes (wright-geom): interference over all pairs,
/// joints in contact, prop clearance, load paths, print bed, ground.
pub fn geometry_checks(e: &Evaluation, r: &mut Report<Check>) {
    let lay = &e.layout;
    let geo: Vec<PartGeometry> = geometry::parts(lay);
    let meshes = geometry::meshes(&geo);
    let names: Vec<&str> = geo.iter().map(|g| g.name.as_str()).collect();
    // interference: all pairs minus declared joints; discs are checked for
    // clearance below
    let solids: Vec<wright_geom::Part> = meshes.iter().zip(&geo).filter(|(_, g)| !g.disc).map(|(m, _)| m.clone()).collect();
    let clash = wright_geom::interference(&solids, 1.0);
    let n = solids.len();
    if clash.is_empty() {
        r.margin(Check::Interference, 0.0, "mm", Source::Geometry, format!("{n} solids, {} pairs, none overlapping", n * (n - 1) / 2));
    } else {
        let worst = clash.iter().map(|c| c.2).fold(0.0, f64::max);
        let list: Vec<String> = clash.iter().map(|c| format!("{} x {} {:.0} mm^3", c.0, c.1, c.2)).collect();
        r.margin(Check::Interference, -worst.cbrt(), "mm", Source::Geometry, list.join("; "));
    }
    // joints in contact
    let idx = |name: &str| names.iter().position(|n| *n == name);
    let mut broken = vec![];
    let mut contact = std::collections::BTreeSet::new();
    for (i, g) in geo.iter().enumerate() {
        for t in &g.touches {
            let Some(j) = idx(t) else {
                broken.push(format!("{} declares {} which does not exist", g.name, t));
                continue;
            };
            let key = if i < j { (i, j) } else { (j, i) };
            if !contact.insert(key) {
                continue;
            }
            let (a, b) = (&meshes[i].mesh, &meshes[j].mesh);
            let gap = a.bounds().gap(&b.bounds());
            let d = if gap > JOINT_CONTACT_M * 1000.0 { gap } else { wright_geom::clearance(a, b) };
            if d > JOINT_CONTACT_M * 1000.0 {
                broken.push(format!("{} and {} are {:.1} mm apart", g.name, t, d));
            }
        }
    }
    r.margin(Check::Attachments, -(broken.len() as f64), "joints", Source::Geometry, if broken.is_empty() { format!("{} joints, all in contact", contact.len()) } else { broken.join("; ") });
    // load paths: every link a declared joint between existing parts in contact
    let mut bad = vec![];
    for (load, chain) in LOAD_PATHS {
        for w in chain.windows(2) {
            let (Some(i), Some(j)) = (idx(w[0]), idx(w[1])) else {
                bad.push(format!("{load}: {} or {} missing", w[0], w[1]));
                continue;
            };
            let declared = geo[i].touches.iter().any(|t| t == w[1]) || geo[j].touches.iter().any(|t| t == w[0]);
            let failed = broken.iter().any(|b| b.contains(w[0]) && b.contains(w[1]));
            if !declared || failed {
                bad.push(format!("{load}: {} -> {} not joined", w[0], w[1]));
            }
        }
    }
    r.margin(Check::LoadPaths, -(bad.len() as f64), "links", Source::Geometry, if bad.is_empty() { format!("{} load paths, every link joined and touching", LOAD_PATHS.len()) } else { bad.join("; ") });
    // prop discs against everything but their own motor
    let mut least = (f64::INFINITY, String::new());
    for (i, g) in geo.iter().enumerate().filter(|(_, g)| g.disc) {
        // its own hub, and the motor the hub turns with (an outrunner's bell
        // spins with the prop)
        let own: Vec<&String> = g.touches.iter().chain(geo.iter().filter(|h| g.touches.contains(&h.name)).flat_map(|h| h.touches.iter())).collect();
        for (j, o) in geo.iter().enumerate() {
            if i == j || own.contains(&&o.name) || o.touches.contains(&g.name) {
                continue;
            }
            if o.disc && j < i {
                continue;
            }
            let gap = meshes[i].mesh.bounds().gap(&meshes[j].mesh.bounds());
            let dist = if gap > 40.0 || gap > least.0 { gap } else { geometry::disc_to_mesh_mm(&g.shape, &meshes[j].mesh, 2.0) };
            if dist < least.0 {
                least = (dist, format!("{} to {}", g.name, o.name));
            }
        }
    }
    r.margin(Check::PropClearance, least.0 - PROP_CLEARANCE_M * 1000.0, "mm", Source::Geometry, format!("closest: {} at {:.1} mm", least.1, least.0));
    // print bed
    let bed = PRINTERS.iter().find(|p| p.0 == lay.design.requirements.printer).map(|p| p.1);
    match bed {
        None => r.missing(Check::PrintBed, Source::Geometry, format!("unknown printer {}", lay.design.requirements.printer)),
        Some(bed) => {
            let mut lim = bed.map(|b| b - 2.0 * BED_MARGIN_MM);
            lim.sort_by(|a, b| a.total_cmp(b));
            let mut worst = (f64::INFINITY, String::new());
            for (m, g) in meshes.iter().zip(&geo).filter(|(_, g)| g.filament.is_some()) {
                let s = m.mesh.bounds().size();
                let axis = if s.x >= s.y && s.x >= s.z { 0 } else if s.y >= s.z { 1 } else { 2 };
                for piece in wright_geom::split_to_fit(&m.mesh, axis, lim[2]) {
                    let ps = piece.bounds().size();
                    let mut d = [ps.x, ps.y, ps.z];
                    d.sort_by(|a, b| a.total_cmp(b));
                    let mm = (0..3).map(|k| lim[k] - d[k]).fold(f64::INFINITY, f64::min);
                    if mm < worst.0 {
                        worst = (mm, format!("{}: {:.0} x {:.0} x {:.0} mm piece", g.name, d[2], d[1], d[0]));
                    }
                }
            }
            r.margin(Check::PrintBed, worst.0, "mm", Source::Geometry, format!("tightest {} in {:?} less {} mm margins", worst.1, bed, BED_MARGIN_MM));
        }
    }
    // ground: the feet below every other point
    let feet = meshes.iter().zip(&geo).filter(|(_, g)| g.name.starts_with("leg_")).map(|(m, _)| m.mesh.bounds().min.z).fold(f64::INFINITY, f64::min);
    let rest = meshes
        .iter()
        .zip(&geo)
        .filter(|(_, g)| !g.name.starts_with("leg_") && !(g.name == "pusher_disc" && lay.folding_pusher()))
        .map(|(m, _)| m.mesh.bounds().min.z)
        .fold(f64::INFINITY, f64::min);
    r.margin(Check::GroundClearance, rest - feet - GROUND_CLEARANCE_M * 1000.0, "mm", Source::Geometry, format!("lowest non-leg point {:.0} mm above the feet", rest - feet));
}

/// Every check on a resolved design.
pub fn check(d: &Design) -> Finished {
    check_against(&d.requirements, d)
}

/// Every check, judged against `req` (the person's effective spec), never
/// against the requirements the design carries: the design is evaluated with
/// `req` put in, and a design whose own copy differs (an edit that moved a
/// requirement) fails `spec_requirements` as missing evidence.
/// Checks that compare the design's copy of something with the spec's (not
/// physics): a requirement-change attack makes them unpassable by
/// construction, so the harness must not count them as confirmed attacks
/// (review 5, F1). Wire to the Domain's `consistency_checks()` hook.
pub const CONSISTENCY_CHECKS: &[&str] = &["spec_requirements"];

pub fn check_against(req: &crate::spec::Requirements, d: &Design) -> Finished {
    let mut r = Report::<Check>::new();
    if &d.requirements == req {
        r.margin(Check::SpecRequirements, 0.0, "", Source::Requirement, "the design carries the spec's requirements unchanged");
    } else {
        let a = serde_json::to_value(req).unwrap_or_default();
        let b = serde_json::to_value(&d.requirements).unwrap_or_default();
        let keys: Vec<String> = a
            .as_object()
            .map(|o| o.iter().filter(|(k, v)| b.get(k.as_str()) != Some(*v)).map(|(k, v)| format!("{k}: spec {v}, design {}", b.get(k.as_str()).cloned().unwrap_or_default())).collect())
            .unwrap_or_default();
        r.missing(Check::SpecRequirements, Source::Requirement, format!("the design's requirements differ from the spec (judged against the spec): {}", keys.join("; ")));
    }
    let mut d = d.clone();
    d.requirements = req.clone();
    let d = &d;
    match evaluate(d, Factors::NOMINAL) {
        Ok(e) => {
            physics(&e, &mut r);
            geometry_checks(&e, &mut r);
            slicer_checks(&e, &mut r);
        }
        Err(why) => r.missing(Check::Parts, Source::Requirement, why),
    }
    r.finish()
}

/// The physics checks only, as a finished report whose geometry checks are
/// absent (and so failing): for ranking in the search, never for a verdict.
pub fn physics_only(e: &Evaluation) -> Finished {
    let mut r = Report::<Check>::new();
    r.margin(Check::SpecRequirements, 0.0, "", Source::Requirement, "ranking only");
    physics(e, &mut r);
    r.finish()
}

/// Checks the search skips (meshes and the slicer); `check` always runs them.
pub const MESH_CHECKS: &[&str] = &["interference", "attachments", "load_paths", "prop_clearance", "print_bed", "ground_clearance", "print_time", "printed_mass"];

/// The slicer's evidence: print time and grams (cached per design).
pub fn slicer_checks(e: &Evaluation, r: &mut Report<Check>) {
    match crate::print::slice_cached(&e.layout) {
        Err(why) => {
            r.missing(Check::PrintTime, Source::Slicer, why.to_string());
            r.missing(Check::PrintedMass, Source::Slicer, why.to_string());
        }
        Ok(pieces) => {
            let hours = pieces.iter().map(|p| p.seconds).sum::<f64>() / 3600.0;
            let limit = e.layout.design.requirements.max_print_hours;
            r.margin(Check::PrintTime, limit.map(|l| l - hours).unwrap_or(0.0), "h", Source::Slicer, format!("{} pieces, {hours:.1} h on the {} ({})", pieces.len(), e.layout.design.requirements.printer, limit.map(|l| format!("limit {l} h")).unwrap_or("no limit stated".into())));
            let mut worst = (f64::INFINITY, String::new());
            for (fil, model) in bom::printed_grams(&e.layout) {
                let sliced: f64 = pieces.iter().filter(|p| p.filament == fil).map(|p| p.grams).sum();
                if model - sliced < worst.0 {
                    worst = (model - sliced, format!("{fil}: sliced {sliced:.0} g, mass model {model:.0} g"));
                }
            }
            r.margin(Check::PrintedMass, worst.0, "g", Source::Slicer, worst.1);
        }
    }
}
