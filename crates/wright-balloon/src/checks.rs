//! The closed set of balloon checks. Each reads a number from the analysis
//! (an independent model of the flight, not the sizing rule) and compares
//! it with a limit from `rules.rs` or the spec.

use crate::analysis::{self, Analysis};
use crate::catalog::{Catalog, LinkKind};
use crate::geometry;
use crate::model::{Design, Spec, TrackerClass};
use crate::rules;
use wright_core::units::{Force, Mass};
use wright_core::{CheckId, Finished, Report, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Check {
    BurstAltitude,
    AscentRate,
    LandingSpeed,
    LiftBudget,
    GasSupply,
    Faa101Package,
    Faa101PackageMax,
    Faa101TotalPayload,
    Faa101Suspension,
    SuspensionStrength,
    PowerMargin,
    SupplyVoltage,
    SupplyCurrent,
    PartTemperature,
    LaunchWindow,
    LinkPrimary,
    LinkSecondary,
    GpsAltitude,
    GpsCoexistence,
    Licensing,
    Imaging,
    PayloadFit,
    PrintedParts,
    RackShelfLanding,
    RecoveryProbability,
    Cost,
}

impl CheckId for Check {
    const ALL: &'static [Self] = &[
        Check::BurstAltitude,
        Check::AscentRate,
        Check::LandingSpeed,
        Check::LiftBudget,
        Check::GasSupply,
        Check::Faa101Package,
        Check::Faa101PackageMax,
        Check::Faa101TotalPayload,
        Check::Faa101Suspension,
        Check::SuspensionStrength,
        Check::PowerMargin,
        Check::SupplyVoltage,
        Check::SupplyCurrent,
        Check::PartTemperature,
        Check::LaunchWindow,
        Check::LinkPrimary,
        Check::LinkSecondary,
        Check::GpsAltitude,
        Check::GpsCoexistence,
        Check::Licensing,
        Check::Imaging,
        Check::PayloadFit,
        Check::PrintedParts,
        Check::RackShelfLanding,
        Check::RecoveryProbability,
        Check::Cost,
    ];
    fn name(self) -> &'static str {
        match self {
            Check::BurstAltitude => "burst_altitude",
            Check::AscentRate => "ascent_rate",
            Check::LandingSpeed => "landing_speed",
            Check::LiftBudget => "lift_budget",
            Check::GasSupply => "gas_supply",
            Check::Faa101Package => "faa101_package",
            Check::Faa101PackageMax => "faa101_package_max",
            Check::Faa101TotalPayload => "faa101_total_payload",
            Check::Faa101Suspension => "faa101_suspension",
            Check::SuspensionStrength => "suspension_strength",
            Check::PowerMargin => "power_margin",
            Check::SupplyVoltage => "supply_voltage",
            Check::SupplyCurrent => "supply_current",
            Check::PartTemperature => "part_temperature",
            Check::LaunchWindow => "launch_window",
            Check::LinkPrimary => "link_primary",
            Check::LinkSecondary => "link_secondary",
            Check::GpsAltitude => "gps_altitude",
            Check::GpsCoexistence => "gps_coexistence",
            Check::Licensing => "licensing",
            Check::Imaging => "imaging",
            Check::PayloadFit => "payload_fit",
            Check::PrintedParts => "printed_parts",
            Check::RackShelfLanding => "rack_shelf_landing",
            Check::RecoveryProbability => "recovery_probability",
            Check::Cost => "cost",
        }
    }
    fn describe(self) -> &'static str {
        match self {
            Check::BurstAltitude => "US76-integrated burst altitude >= the spec's minimum (m)",
            Check::AscentRate => "launch and mean ascent rate inside 4..6 m/s (m/s)",
            Check::LandingSpeed => "descent rate at the ground under the chute <= 5 m/s (m/s)",
            Check::LiftBudget => "free lift > 0 with the measured neck lift (g)",
            Check::GasSupply => "gas bought covers the fill after cylinder residue (m^3)",
            Check::Faa101Package => "14 CFR 101.1(a)(4)(i): package <= 4 lb or <= 3 oz/in^2 on every face (lb)",
            Check::Faa101PackageMax => "14 CFR 101.1(a)(4)(ii): package <= 6 lb (lb)",
            Check::Faa101TotalPayload => "14 CFR 101.1(a)(4)(iii): payload total <= 12 lb (lb)",
            Check::Faa101Suspension => "14 CFR 101.1(a)(4)(iv): suspension line breaks at <= 50 lbf, allowing for a spool stronger than its rating (lbf)",
            Check::SuspensionStrength => "line breaking strength >= 5x the descent weight (N)",
            Check::PowerMargin => "every battery covers flight + recovery window at its coldest cell temperature (Wh)",
            Check::SupplyVoltage => "every load's input range holds its bus voltage over the battery's range (V)",
            Check::SupplyCurrent => "regulator and battery current ratings vs the load (A)",
            Check::PartTemperature => "box temperature over flight + window inside every part's rating at both ends of the launch temperature range (K)",
            Check::LaunchWindow => "declared go/no-go launch window covers the spec's ground-temperature range and lies inside the modelled one (K)",
            Check::LinkPrimary => "primary tracker link margin over every flight sample above 1 km AGL (dB)",
            Check::LinkSecondary => "secondary tracker link margin over every flight sample above 1 km AGL (dB)",
            Check::GpsAltitude => "each tracker GPS's dynamic-model altitude limit above burst (m)",
            Check::GpsCoexistence => "every onboard transmitter at every tracker GPS under the front-end limits (dB)",
            Check::Licensing => "every transmitter legal for this team (count)",
            Check::Imaging => "a camera records from launch to burst at the cold and sees out of its port unobstructed (h)",
            Check::PayloadFit => "all-pairs interference of boards, rack and walls (pairs)",
            Check::PrintedParts => "printed rack sliced; plates fit the bed (mm)",
            Check::RackShelfLanding => "printed rack shelf bending at touchdown (stop over the foam wall on soft ground) <= PETG allowable (MPa)",
            Check::RecoveryProbability => "recovery probability >= the spec's minimum",
            Check::Cost => "every BOM line priced; total <= budget when the request states one (USD)",
        }
    }
}

const LB: f64 = 0.45359237;

pub fn evaluate(spec: &Spec, d: &Design, cat: &Catalog) -> (Finished, Option<Analysis>) {
    let mut r = Report::<Check>::new();
    let a = match analysis::analyse(spec, d, cat) {
        Ok(a) => a,
        Err(e) => {
            // Nothing could be measured: every check reports it (and fails).
            for &c in Check::ALL {
                r.missing(c, Source::Physics, format!("analysis failed: {e:#}"));
            }
            return (r.finish(), None);
        }
    };
    let fl = &a.flight;
    let z0 = spec.launch.elevation_m;

    // ---- flight
    match fl.burst_altitude_m {
        Some(b) => r.margin(Check::BurstAltitude, b - spec.min_burst_altitude_m, "m", Source::Physics, format!("burst at {:.0} m (CUSF closed form says {:.0} m), target {:.0} m", b, fl.cusf_burst_altitude_m, spec.min_burst_altitude_m)),
        None => r.missing(Check::BurstAltitude, Source::Physics, format!("no burst: ascent stalled at {:.0} m (floater or no lift)", fl.samples.iter().map(|s| s.z_m).fold(0.0, f64::max))),
    }
    let m_launch = (fl.launch_ascent_mps - rules::ASCENT_MIN_MPS).min(rules::ASCENT_MAX_MPS - fl.launch_ascent_mps);
    let m_mean = (fl.mean_ascent_mps - rules::ASCENT_MIN_MPS).min(rules::ASCENT_MAX_MPS - fl.mean_ascent_mps);
    r.margin(Check::AscentRate, m_launch.min(m_mean), "m/s", Source::Physics, format!("launch {:.2} m/s, mean {:.2} m/s (band 4-6)", fl.launch_ascent_mps, fl.mean_ascent_mps));
    if fl.burst_altitude_m.is_some() {
        r.margin(Check::LandingSpeed, rules::LANDING_MAX_MPS - fl.landing_speed_mps, "m/s", Source::Physics, format!("{:.2} m/s at {:.0} m with {:.0} g under the chute", fl.landing_speed_mps, z0, a.masses.descent().as_g()));
    } else {
        r.missing(Check::LandingSpeed, Source::Physics, "no descent: the balloon never burst");
    }
    r.margin(Check::LiftBudget, fl.free_lift_kg * 1e3, "g", Source::Physics, format!("neck lift {:.0} g vs flight train {:.0} g", d.neck_lift_g, a.masses.flight_train().as_g()));
    r.margin(Check::GasSupply, a.gas_have_m3 - a.gas_needed_m3, "m^3", Source::Physics, format!("need {:.2} m^3 at 21 C/1 atm, have {:.2} usable in {} cylinder(s)", a.gas_needed_m3, a.gas_have_m3, d.cylinders));

    // ---- FAA Part 101
    let pkg_lb = a.masses.package().as_kg() / LB;
    let o = crate::mass::outer_mm(d);
    let mut faces = [o[0] * o[1], o[1] * o[2], o[0] * o[2]];
    faces.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let min_face_in2 = faces[0] / 645.16;
    let ratio = pkg_lb * 16.0 / min_face_in2;
    let headroom_by_ratio = (rules::FAA_PKG_OZ_PER_IN2 - ratio) * min_face_in2 / 16.0;
    r.margin(Check::Faa101Package, (rules::FAA_PKG_LB - pkg_lb).max(headroom_by_ratio), "lb", Source::Requirement, format!("package {:.2} lb, {:.2} oz/in^2 on its smallest face ({:.0} in^2)", pkg_lb, ratio, min_face_in2));
    r.margin(Check::Faa101PackageMax, rules::FAA_PKG_MAX_LB - pkg_lb, "lb", Source::Requirement, format!("package {:.2} lb", pkg_lb));
    let total_lb = a.masses.flight_train().as_kg() / LB;
    r.margin(Check::Faa101TotalPayload, rules::FAA_TOTAL_LB - total_lb, "lb", Source::Requirement, format!("package + chute + line {:.2} lb (counted as payload, conservative)", total_lb));
    match cat.line(&d.line) {
        Ok(line) => {
            let lbf = Force::n(line.breaking_n).as_lbf();
            let worst = lbf * rules::LINE_OVERSTRENGTH;
            r.margin(Check::Faa101Suspension, rules::FAA_LINE_LBF - worst, "lbf", Source::Rating, format!("{} rated {:.0} lbf; up to {:.0} lbf allowing {}x over the rating", line.name, lbf, worst, rules::LINE_OVERSTRENGTH));
            let need = a.masses.descent().as_kg() * wright_core::units::G0 * rules::LINE_SHOCK_FACTOR;
            r.margin(Check::SuspensionStrength, line.breaking_n - need, "N", Source::Rating, format!("{:.0} N rated vs {:.0} N ({}x descent weight)", line.breaking_n, need, rules::LINE_SHOCK_FACTOR));
        }
        Err(e) => {
            r.missing(Check::Faa101Suspension, Source::Rating, format!("{e}"));
            r.missing(Check::SuspensionStrength, Source::Rating, format!("{e}"));
        }
    }

    // ---- power
    if a.buses.is_empty() && a.self_powered.is_empty() {
        r.missing(Check::PowerMargin, Source::Physics, "no battery in the design");
    }
    for b in a.buses.iter().chain(&a.self_powered) {
        r.margin(Check::PowerMargin, b.have_wh - b.need_wh, "Wh", Source::Physics, format!("{}: need {:.1} Wh, have {:.1} Wh at {:.0} C cell ({:.0}% capacity)", b.name, b.need_wh, b.have_wh, b.cell_min_c, b.capacity_fraction * 100.0));
    }
    supply_checks(&mut r, d, cat);

    // ---- temperature: every part inside the box, over the launch range
    let [rlo, rhi] = a.launch_range_c;
    let forecast = match spec.ground_temperature_c {
        Some(g) if analysis::forecast_is_launch_day(spec, d, g).unwrap_or(false) => format!("; forecast {g:.0} C included"),
        Some(g) => format!("; forecast {g:.0} C is outside the launch range and the declared window: a no-go day"),
        None => String::new(),
    };
    let cases = a.thermal_cases.iter().map(|c| format!("{:.1} C{}", c.ground_c, if c.warmers_on { " (warmers)" } else { "" })).collect::<Vec<_>>().join(", ");
    for id in analysis::rated_parts(d) {
        match cat.part(id) {
            Ok(p) => {
                let m = (a.box_min_c - p.temp_min_c).min(p.temp_max_c - a.box_max_c);
                let tag = if p.temp_published { "" } else { " (rating assumed)" };
                r.margin(Check::PartTemperature, m, "K", Source::Rating, format!("{}: box {:.0}..{:.0} C vs rated {:.0}..{:.0} C{tag}, ground {rlo:.0}..{rhi:.0} C (cases {cases}){forecast}", p.id, a.box_min_c, a.box_max_c, p.temp_min_c, p.temp_max_c));
            }
            Err(e) => r.missing(Check::PartTemperature, Source::Rating, format!("{e}")),
        }
    }
    match (d.launch_window_c, analysis::launch_window(spec, d, cat)) {
        (_, Err(e)) => r.missing(Check::LaunchWindow, Source::Physics, format!("launch window could not be computed: {e:#}")),
        (None, Ok(Some([clo, chi]))) => r.missing(Check::LaunchWindow, Source::Physics, format!("the design declares no launch window (the model allows {clo:.1}..{chi:.1} C ground)")),
        (_, Ok(None)) => r.missing(Check::LaunchWindow, Source::Physics, format!("no launch window: the thermal checks fail at {:.1} C, the middle of the {rlo:.0}..{rhi:.0} C range", 0.5 * (rlo + rhi))),
        (Some([dlo, dhi]), Ok(Some([clo, chi]))) => {
            let m = (rlo - dlo).min(dhi - rhi).min(dlo - clo).min(chi - dhi);
            r.margin(
                Check::LaunchWindow,
                m,
                "K",
                Source::Physics,
                format!("declared go {dlo:.1}..{dhi:.1} C ground; modelled {clo:.1}..{chi:.1} C; required {rlo:.0}..{rhi:.0} C{forecast}"),
            );
        }
    }

    // ---- links
    for (class, check) in [(TrackerClass::Primary, Check::LinkPrimary), (TrackerClass::Secondary, Check::LinkSecondary)] {
        let ts: Vec<_> = a.trackers.iter().filter(|t| t.class == class).collect();
        if ts.is_empty() {
            r.missing(check, Source::Physics, format!("no {class:?} tracker"));
        }
        for t in ts {
            match t.worst_margin_db {
                Some(m) => r.margin(check, m, "dB", Source::Physics, format!("{}: worst {:.1} dB at {:.0} km (max range {:.0} km), {}", t.name, m, t.worst_at_range_km, t.max_range_km, t.why)),
                None => r.missing(check, Source::Physics, format!("{}: {}", t.name, t.why)),
            }
        }
    }

    // ---- GPS
    match fl.burst_altitude_m {
        None => r.missing(Check::GpsAltitude, Source::Rating, "no burst altitude to compare"),
        Some(b) => {
            let mut any = false;
            for tr in &d.trackers {
                any = true;
                let Ok(p) = cat.part(&tr.gps) else {
                    r.missing(Check::GpsAltitude, Source::Rating, format!("{}: unknown gps {}", tr.name, tr.gps));
                    continue;
                };
                if !d.items.iter().any(|it| it.part == tr.gps) {
                    r.missing(Check::GpsAltitude, Source::Rating, format!("{}: GPS {} is not in the payload", tr.name, tr.gps));
                    continue;
                }
                let Some(g) = &p.gps else {
                    r.missing(Check::GpsAltitude, Source::Rating, format!("{}: {} has no GPS (no altitude limit to read)", tr.name, tr.gps));
                    continue;
                };
                let cellular = cat.part(&tr.radio).ok().and_then(|x| x.radio.as_ref()).is_some_and(|x| x.link == LinkKind::Cellular);
                let lim = if tr.gps_airborne { g.alt_limit_airborne_m } else { g.alt_limit_default_m };
                if cellular {
                    // a phone reports through the cell network after landing:
                    // its fix is needed on the ground, not at burst
                    r.margin(Check::GpsAltitude, lim - z0, "m", Source::Rating, format!("{}: {} limit {:.0} m vs the ground at {:.0} m (reports after landing only)", tr.name, g.chip, lim, z0));
                } else {
                    r.margin(Check::GpsAltitude, lim - b, "m", Source::Rating, format!("{}: {} limit {:.0} m ({}) vs burst {:.0} m", tr.name, g.chip, lim, if tr.gps_airborne { "airborne model" } else { "default model" }, b));
                }
            }
            if !any {
                r.missing(Check::GpsAltitude, Source::Rating, "no tracker has a GPS");
            }
        }
    }
    if a.coexistence.is_empty() {
        r.missing(Check::GpsCoexistence, Source::Physics, "no GPS/transmitter pair to evaluate");
    }
    for c in &a.coexistence {
        r.margin(Check::GpsCoexistence, c.limit_dbm - c.at_gnss_dbm, "dB", Source::Physics, format!("{} -> {} at {:.2} m: {:.1} dBm vs {:.1} dBm ({})", c.tx, c.gps, c.separation_m, c.at_gnss_dbm, c.limit_dbm, c.mechanism));
    }

    // ---- licensing
    let mut n_tx = 0;
    for it in &d.items {
        let Ok(p) = cat.part(&it.part) else { continue };
        let Some(rs) = &p.radio else { continue };
        n_tx += 1;
        if rs.needs_amateur_licence && !spec.licensed_ham {
            r.margin(Check::Licensing, -1.0, "count", Source::Requirement, format!("{} needs a US amateur licence; nobody on the team holds one", p.id));
        } else if rs.link == LinkKind::Video && !(rules::HAM_23CM_MHZ.0..=rules::HAM_23CM_MHZ.1).contains(&rs.freq_mhz) {
            r.margin(Check::Licensing, -1.0, "count", Source::Requirement, format!("{} set to {:.0} MHz, outside the 1240-1300 MHz amateur band", p.id, rs.freq_mhz));
        } else {
            r.margin(Check::Licensing, 0.0, "count", Source::Requirement, format!("{} legal ({})", p.id, if rs.needs_amateur_licence { "amateur, licensed operator" } else { "unlicensed / Part 15 or carrier" }));
        }
    }
    if n_tx == 0 {
        r.margin(Check::Licensing, 0.0, "count", Source::Requirement, "no transmitters");
    }

    // ---- imaging: the best camera's endurance against time to burst
    let to_burst_h = fl.burst_t_s / 3600.0;
    let mut best: Option<(f64, String)> = None;
    for it in &d.items {
        let Ok(p) = cat.part(&it.part) else { continue };
        let Some(cam) = &p.camera else { continue };
        if (spec.needs_video && !cam.video) || (spec.needs_photos && !cam.photo && !cam.video) {
            continue;
        }
        let temp_ok = a.box_min_c >= p.temp_min_c && a.box_max_c <= p.temp_max_c;
        let endurance = if p.self_powered {
            a.self_powered.iter().find(|b| b.name == p.id).map(|b| b.flight_endurance_h).unwrap_or(0.0)
        } else {
            it.bus.and_then(|bi| a.buses.get(bi)).map(|b| b.flight_endurance_h).unwrap_or(0.0)
        };
        // the picture: the lens must see out of its port, unobstructed
        let view = geometry::camera_view(d, &a.layout, &format!("{}#1", p.id));
        let view_ok = view.as_ref().is_some_and(|v| v.blocked_by.is_empty() && v.clear_half_deg >= rules::VIEW_HALF_DEG - 1e-9);
        let view_txt = match &view {
            None => "not placed in the box".to_string(),
            Some(v) if !v.blocked_by.is_empty() => format!("port view blocked by {}", v.blocked_by.join(", ")),
            Some(v) => format!("lens {:.0} mm behind the port, clear cone +-{:.0} deg (need +-{:.0})", v.gap_mm, v.clear_half_deg, rules::VIEW_HALF_DEG),
        };
        let e = if temp_ok && view_ok { endurance } else { 0.0 };
        if best.as_ref().is_none_or(|(x, _)| e > *x) {
            best = Some((e, format!("{} records {:.1} h at the cold{}; {view_txt}", p.id, endurance, if temp_ok { "" } else { " but leaves its temperature range" })));
        }
    }
    match best {
        Some((e, why)) => r.margin(Check::Imaging, e - to_burst_h, "h", Source::Physics, format!("{why}; burst at {:.1} h", to_burst_h)),
        None => r.missing(Check::Imaging, Source::Requirement, "no camera meeting the mission"),
    }

    // ---- geometry
    let solids = geometry::solids(d, cat, &a.layout);
    let clashes = wright_geom::interference(&solids, 1.0);
    let outside: Vec<_> = a
        .layout
        .placed
        .iter()
        .filter(|p| (0..3).any(|k| p.min[k] < -1e-6 || p.min[k] + p.size[k] > a.layout.inner_mm[k] + 1e-6))
        .map(|p| p.name.clone())
        .collect();
    let detail = if clashes.is_empty() && outside.is_empty() {
        format!("{} solids, no overlapping pair", solids.len())
    } else {
        format!(
            "{} clashes: {}; outside the box: {}",
            clashes.len(),
            clashes.iter().take(4).map(|(x, y, v)| format!("{x}/{y} {v:.0} mm^3")).collect::<Vec<_>>().join(", "),
            outside.join(", ")
        )
    };
    let n_bad = clashes.len() + outside.len();
    r.margin(Check::PayloadFit, if n_bad == 0 { 0.0 } else { -(n_bad as f64) }, "pairs", Source::Geometry, detail);
    if d.enclosure.printed_tray {
        match &d.printed {
            Some(p) => {
                let bed = wright_geom::slice::printer(geometry::PRINTER).map(|p| p.bed[0].min(p.bed[1])).unwrap_or(0.0);
                let biggest = d.enclosure.inner_mm[0].max(d.enclosure.inner_mm[1]);
                r.margin(Check::PrintedParts, bed - biggest, "mm", Source::Slicer, format!("rack {:.0} g, {:.1} h on {}; largest plate {:.0} mm vs bed {:.0} mm", p.grams, p.seconds / 3600.0, p.printer, biggest, bed));
            }
            None => r.missing(Check::PrintedParts, Source::Slicer, "the printed rack was never sliced (its mass is only an allowance)"),
        }
    } else {
        r.margin(Check::PrintedParts, 0.0, "mm", Source::Slicer, "no printed parts in this design");
    }
    match rack_shelf_landing(d, cat, &a) {
        Some(s) => r.margin(Check::RackShelfLanding, rules::PETG_ALLOWABLE_MPA - s.field_mpa, "MPa", Source::Physics, s.detail()),
        None => r.margin(Check::RackShelfLanding, 0.0, "MPa", Source::Physics, "no shelf carried by posts (no printed rack, or a single shelf on the floor)"),
    }

    r.margin(Check::RecoveryProbability, a.recovery_probability - spec.min_recovery_probability, "", Source::Physics, format!("P = {:.3}: {}", a.recovery_probability, a.trackers.iter().map(|t| format!("{} {:.2}", t.name, t.p_recover)).collect::<Vec<_>>().join(", ")));

    // ---- cost
    let bom = crate::bom(d, cat);
    let unpriced = bom.unpriced();
    if !unpriced.is_empty() {
        r.missing(Check::Cost, Source::Requirement, format!("unpriced: {}", unpriced.iter().map(|l| l.id.as_str()).collect::<Vec<_>>().join(", ")));
    } else {
        // no budget stated: cost is information only (wright-air checks.rs, Check::Budget)
        let total = bom.total().as_usd();
        match spec.budget_usd {
            Some(b) => r.margin(Check::Cost, b - total, "USD", Source::Requirement, format!("BOM ${total:.2} vs budget ${b:.2}")),
            None => r.margin(Check::Cost, 0.0, "USD", Source::Requirement, format!("BOM ${total:.2}; no budget stated (information only)")),
        }
    }
    let _ = Mass::ZERO;
    (r.finish(), Some(a))
}

fn supply_checks(r: &mut Report<Check>, d: &Design, cat: &Catalog) {
    let mut any = false;
    for (bi, b) in d.buses.iter().enumerate() {
        let Ok(bat) = cat.part(&b.battery) else {
            r.missing(Check::SupplyVoltage, Source::Rating, format!("bus {}: unknown battery {}", b.name, b.battery));
            continue;
        };
        let Some(bs) = bat.battery.as_ref() else {
            r.missing(Check::SupplyVoltage, Source::Rating, format!("bus {}: {} is not a battery", b.name, b.battery));
            continue;
        };
        let (lo, hi) = (bs.v_min * b.series as f64, bs.v_max * b.series as f64);
        // rails: regulator input vs battery range, current vs rating
        let mut rails = Vec::new();
        for id in &b.rails {
            match cat.part(id).ok().and_then(|p| p.regulator.as_ref().map(|s| (p, s))) {
                Some((p, s)) => {
                    any = true;
                    r.margin(Check::SupplyVoltage, (lo - s.vin_min).min(s.vin_max - hi), "V", Source::Rating, format!("{}: {} takes {:.1}-{:.1} V from {:.1}-{:.1} V", b.name, p.id, s.vin_min, s.vin_max, lo, hi));
                    rails.push(Some((p, s)));
                }
                None => {
                    r.missing(Check::SupplyVoltage, Source::Rating, format!("{}: rail {id} is not a known regulator", b.name));
                    rails.push(None);
                }
            }
        }
        let mut rail_load = vec![0.0; rails.len()];
        let mut i_bat = 0.0;
        for it in d.items.iter().filter(|it| it.bus == Some(bi)) {
            let Ok(p) = cat.part(&it.part) else { continue };
            let pw = p.power_w * it.qty as f64;
            let (out_lo, out_hi) = match it.rail {
                None => {
                    i_bat += pw / lo;
                    (lo, hi)
                }
                Some(k) => match rails.get(k) {
                    Some(Some((_, s))) => {
                        rail_load[k] += pw;
                        i_bat += pw / s.efficiency / lo;
                        (s.vout, s.vout)
                    }
                    _ => {
                        r.missing(Check::SupplyVoltage, Source::Rating, format!("{}: {} on a rail that does not exist", b.name, p.id));
                        continue;
                    }
                },
            };
            match &p.supply {
                Some(s) => {
                    any = true;
                    r.margin(Check::SupplyVoltage, (out_lo - s.vin_min).min(s.vin_max - out_hi), "V", Source::Rating, format!("{}: {} takes {:.1}-{:.1} V, gets {:.1}-{:.1} V", b.name, p.id, s.vin_min, s.vin_max, out_lo, out_hi));
                }
                None if pw > 0.0 => r.missing(Check::SupplyVoltage, Source::Rating, format!("{} has no input range", p.id)),
                None => {}
            }
        }
        for (k, rail) in rails.iter().enumerate() {
            if let Some((p, s)) = rail {
                r.margin(Check::SupplyCurrent, s.imax_a - rail_load[k] / s.vout, "A", Source::Rating, format!("{}: {} rated {:.1} A, load {:.2} A", b.name, p.id, s.imax_a, rail_load[k] / s.vout));
            }
        }
        r.margin(Check::SupplyCurrent, bs.max_current_a * b.parallel as f64 - i_bat, "A", Source::Rating, format!("{}: battery {:.2} A at {:.1} V vs {:.1} A rated", b.name, i_bat, lo, bs.max_current_a * b.parallel as f64));
    }
    if !any {
        r.margin(Check::SupplyVoltage, 0.0, "V", Source::Rating, "no bus-powered loads (all self-powered)");
    }
    if d.buses.is_empty() {
        r.margin(Check::SupplyCurrent, 0.0, "A", Source::Rating, "no buses (all self-powered)");
    }
}

/// The printed rack's most loaded post-carried shelf at touchdown.
#[derive(Debug, Clone)]
pub struct ShelfLanding {
    pub shelf: usize,
    pub load_g: f64,
    pub span_mm: f64,
    pub width_mm: f64,
    pub landing_mps: f64,
    /// Soft ground: stopped over the foam wall (the judged case).
    pub field_stop_mm: f64,
    pub field_g: f64,
    pub field_mpa: f64,
    /// Hard ground: the bottom wall crushes at XPS_CRUSH_KPA.
    pub hard_stop_mm: f64,
    pub hard_g: f64,
    pub hard_mpa: f64,
}

impl ShelfLanding {
    pub fn detail(&self) -> String {
        format!(
            "shelf {} carries {:.0} g on four corner posts, span {:.0} mm x {:.0} mm wide, {:.0} mm PETG; touchdown {:.2} m/s. Soft ground: stopped over {:.1} mm (the foam wall) = {:.0} g -> {:.1} MPa peak vs {:.0} MPa allowable. Hard ground (not judged): XPS crushes at {:.1} kPa, stop {:.1} mm = {:.0} g -> {:.1} MPa{}",
            self.shelf + 1,
            self.load_g,
            self.span_mm,
            self.width_mm,
            geometry::SHELF_MM,
            self.landing_mps,
            self.field_stop_mm,
            self.field_g,
            self.field_mpa,
            rules::PETG_ALLOWABLE_MPA,
            rules::XPS_CRUSH_KPA,
            self.hard_stop_mm,
            self.hard_g,
            self.hard_mpa,
            if self.hard_mpa > rules::PETG_ALLOWABLE_MPA { " (over the allowable: land away from roads)" } else { "" }
        )
    }
}

/// Closed form: the shelf rests on corner posts `POST_MARGIN_MM / 2` in from
/// each side. Cutting it at mid-span, statics gives the total moment on the
/// section exactly, M = W L / 8 (the half-shelf's corner reactions W/2 at
/// L/2 against its load W/2 at L/4), over the full shelf width; the peak is
/// `rules::SHELF_PEAK_FACTOR` times that section average (6 M / b t^2).
/// The long span governs. Load factor n = a/g + 1 with a = v^2 / 2 d.
pub fn rack_shelf_landing(d: &Design, cat: &Catalog, a: &Analysis) -> Option<ShelfLanding> {
    let l = &a.layout;
    if !d.enclosure.printed_tray || l.shelves.len() < 2 {
        return None;
    }
    let mut per = vec![0.0_f64; l.shelves.len()];
    for p in &l.placed {
        let k = l.shelves.iter().rposition(|s| s + geometry::SHELF_MM <= p.min[2] + 1e-6).unwrap_or(0);
        per[k] += cat.part(&p.part).map(|x| x.mass_g).unwrap_or(f64::NAN);
    }
    let (k, load_g) = per.iter().copied().enumerate().skip(1).fold((1, f64::NEG_INFINITY), |b, (i, g)| if g > b.1 { (i, g) } else { b });
    let [w, dd, _] = l.inner_mm;
    let (span, width) = (w.max(dd) - geometry::POST_MARGIN_MM, w.min(dd));
    let t = geometry::SHELF_MM;
    let v = a.flight.landing_speed_mps;
    let g0 = wright_core::units::G0;
    // MPa for a load factor n (N, mm -> N/mm^2 = MPa)
    let mpa = |n: f64| {
        let w_n = load_g / 1e3 * g0 * n;
        rules::SHELF_PEAK_FACTOR * 6.0 * (w_n * span / 8.0) / (width * t * t)
    };
    let field_stop = d.enclosure.wall_mm * rules::LANDING_STOP_WALL_FRACTION;
    let field_g = v * v / (2.0 * field_stop / 1e3) / g0 + 1.0;
    // hard ground: crush force on the bottom face over the package mass
    let o = crate::mass::outer_mm(d);
    let force = rules::XPS_CRUSH_KPA * 1e3 * o[0] * o[1] * 1e-6;
    let m = a.masses.package().as_kg();
    let hard_g = force / (m * g0);
    let hard_stop = 0.5 * m * v * v / (force - m * g0).max(1e-9) * 1e3;
    Some(ShelfLanding {
        shelf: k,
        load_g,
        span_mm: span,
        width_mm: width,
        landing_mps: v,
        field_stop_mm: field_stop,
        field_g,
        field_mpa: mpa(field_g),
        hard_stop_mm: hard_stop,
        hard_g,
        hard_mpa: mpa(hard_g),
    })
}
