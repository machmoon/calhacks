//! High-altitude (weather) balloons for Wright: lift gas and envelope,
//! ascent to burst through the US Standard Atmosphere 1976, descent under a
//! chute, drift, payload thermal and power at the cold, tracker link
//! budgets, GNSS limits and coexistence, FAA 14 CFR 101.1(a)(4), and a
//! printable payload enclosure.
//!
//! Sources for every constant are in [`rules`]; catalog rows carry their
//! URL and retrieval date (`data/balloon/catalog.json`).

pub mod analysis;
pub mod atmosphere;
pub mod build;
pub mod catalog;
pub mod checks;
pub mod design;
pub mod flight;
pub mod geometry;
pub mod mass;
pub mod model;
pub mod options;
pub mod physics;
pub mod predict;
pub mod radio;
pub mod reference;
pub mod rules;
pub mod sim;

pub use checks::Check;
pub use model::{Design, Spec};

use catalog::{catalog, Catalog};
use std::path::Path;
use wright_core::units::Money;
use wright_core::{Artifacts, Bom, BomLine, Domain, Finished};

/// The balloon domain (`wright balloon design|check|schema`).
pub struct Balloon;

/// PETG filament price, USD/g. Bambu Lab PETG Basic 1 kg spool, $19.99
/// (us.store.bambulab.com, read 2026-10-24).
pub const PETG_USD_PER_G: f64 = 19.99 / 1000.0;

#[allow(clippy::too_many_arguments)] // one BOM row, named fields in call order
fn line(category: &str, id: &str, name: &str, qty: f64, unit: &str, price: Option<f64>, url: &str, note: &str) -> BomLine {
    BomLine { category: category.into(), id: id.into(), name: name.into(), qty, unit: unit.into(), unit_price: price.map(Money::usd), url: url.into(), note: note.into() }
}

/// The bill of materials; an id the catalog does not know is an unpriced line.
pub fn bom(d: &Design, cat: &Catalog) -> Bom {
    let mut b = Bom::default();
    let mut add = |l: BomLine| b.lines.push(l);
    match cat.balloon(&d.balloon) {
        Ok(r) => add(line("envelope", &r.id, &r.name, 1.0, "ea", r.row.price_usd, &r.row.url, "")),
        Err(_) => add(line("envelope", &d.balloon, "unknown balloon", 1.0, "ea", None, "", "")),
    }
    match cat.gas(&d.gas) {
        Ok(r) => add(line("gas", &r.id, &r.name, d.cylinders as f64, "cylinder", r.row.price_usd, &r.row.url, &format!("{:.1} m^3 per cylinder", r.cylinder_m3))),
        Err(_) => add(line("gas", &d.gas, "unknown gas", d.cylinders as f64, "cylinder", None, "", "")),
    }
    match cat.chute(&d.parachute) {
        Ok(r) => add(line("recovery", &r.id, &r.name, 1.0, "ea", r.row.price_usd, &r.row.url, "")),
        Err(_) => add(line("recovery", &d.parachute, "unknown chute", 1.0, "ea", None, "", "")),
    }
    match cat.line(&d.line) {
        Ok(r) => add(line("recovery", &r.id, &r.name, d.line_m, "m", r.price_usd_per_m, &r.url, "")),
        Err(_) => add(line("recovery", &d.line, "unknown line", d.line_m, "m", None, "", "")),
    }
    match cat.insulation(&d.enclosure.insulation) {
        Ok(r) => {
            let o = mass::outer_mm(d);
            let area = 2.0 * (o[0] * o[1] + o[1] * o[2] + o[0] * o[2]) * 1e-6;
            let layers = (d.enclosure.wall_mm / r.sheet_thickness_mm - 1e-6).ceil().max(1.0);
            add(line("enclosure", &r.id, &r.name, area * layers, "m^2", r.price_usd_per_m2, &r.url, &format!("{layers} layer(s) of {} mm sheet", r.sheet_thickness_mm)));
        }
        Err(_) => add(line("enclosure", &d.enclosure.insulation, "unknown insulation", 1.0, "ea", None, "", "")),
    }
    if d.enclosure.printed_tray {
        let g = d.printed.as_ref().map(|p| p.grams).unwrap_or(d.printed_allowance_g);
        add(line("enclosure", "petg-rack", "printed PETG rack (shelves + posts)", g, "g", Some(PETG_USD_PER_G), "https://us.store.bambulab.com/products/petg-basic", if d.printed.is_some() { "sliced grams" } else { "allowance (not sliced)" }));
    }
    let mut part = |id: &str, qty: f64, cat_name: &str| match cat.part(id) {
        Ok(p) => b.lines.push(line(cat_name, &p.id, &p.name, qty, "ea", p.row.price_usd, &p.row.url, &p.row.note)),
        Err(_) => b.lines.push(line(cat_name, id, "unknown part", qty, "ea", None, "", "")),
    };
    for it in &d.items {
        part(&it.part, it.qty as f64, "payload");
    }
    for bus in &d.buses {
        part(&bus.battery, (bus.series * bus.parallel) as f64, "power");
        for r in &bus.rails {
            part(r, 1.0, "power");
        }
    }
    // what every flight also buys: fill gear, cylinder rental, the chute
    // harness, a memory card per recording part
    let n_sd: u32 = d.items.iter().filter(|it| cat.part(&it.part).is_ok_and(|p| p.microsd)).map(|it| it.qty).sum();
    for e in &cat.extras {
        let qty = match e.per {
            catalog::ExtraPer::Flight => 1.0,
            catalog::ExtraPer::Cylinder => d.cylinders as f64,
            catalog::ExtraPer::Microsd => n_sd as f64,
        };
        if qty > 0.0 {
            b.lines.push(line(if e.per == catalog::ExtraPer::Microsd { "payload" } else { "launch" }, &e.id, &e.name, qty, "ea", e.row.price_usd, &e.row.url, &e.row.note));
        }
    }
    for g in &d.buy_ground {
        match cat.station(g) {
            Ok(s) => b.lines.push(line("ground", &s.id, &s.name, 1.0, "ea", s.row.price_usd, &s.row.url, &s.row.note)),
            Err(_) => b.lines.push(line("ground", g, "unknown ground station", 1.0, "ea", None, "", "")),
        }
    }
    b
}

/// The checklist's go/no-go launch criterion: the design's declared window.
pub fn go_no_go(d: &Design) -> String {
    match d.launch_window_c {
        Some([lo, hi]) => format!(
            "GO / NO-GO: measure the ground air temperature at the launch site. Launch only between {lo:.0} C and {hi:.0} C ({:.0} F to {:.0} F); outside that range the payload box leaves a part's rated temperature (the `launch_window` check), so scrub",
            lo * 9.0 / 5.0 + 32.0,
            hi * 9.0 / 5.0 + 32.0
        ),
        None => "GO / NO-GO: this design declares no launch temperature window; do not launch until `wright balloon check` passes `launch_window`".into(),
    }
}

/// The hand-warmer step, when the design carries warmers.
fn warmer_step(d: &Design) -> Option<String> {
    let n: u32 = d.items.iter().filter(|i| catalog().part(&i.part).is_ok_and(|p| p.warmer.is_some())).map(|i| i.qty).sum();
    if n == 0 {
        return None;
    }
    let when = match d.warmers_below_c {
        Some(t) => format!("if the ground temperature is below {t:.0} C ({:.0} F); above it leave them sealed in their wrappers", t * 9.0 / 5.0 + 32.0),
        None => "on every launch day".into(),
    };
    Some(format!(
        "Open the {n} HotHands hand warmers and lay them in the box away from the boards at sealing (they take 15-30 min to heat) {when}. Leave a vent in the box: they need air, and a sealed box with warmers fell to ~5 % oxygen in the Red Rocks CC 2023 flight; bag the electronics with desiccant against their moisture (same paper)"
    ))
}

impl Domain for Balloon {
    const NAME: &'static str = "balloon";
    type Spec = Spec;
    type Design = Design;
    type Check = Check;

    fn spec_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["mission", "launch", "min_burst_altitude_m"],
            "properties": {
                "mission": {"type": "string"},
                "launch": {"type": "object", "required": ["name", "lat_deg", "lon_deg", "elevation_m"],
                    "properties": {
                        "name": {"type": "string"},
                        "lat_deg": {"type": "number", "minimum": -90, "maximum": 90, "description": "launch latitude, deg (Tawhiri predictor API range)"},
                        "lon_deg": {"type": "number", "minimum": -180, "maximum": 180, "description": "launch longitude, deg east"},
                        "elevation_m": {"type": "number", "minimum": rules::LAUNCH_ELEVATION_M[0], "maximum": rules::LAUNCH_ELEVATION_M[1], "description": "launch ground elevation, m; bounds: Badwater Basin -86 m to Mount Whitney 4421 m (USGS, lowest/highest US ground)"}}},
                "min_burst_altitude_m": {"type": "number", "minimum": rules::BURST_TARGET_M[0], "maximum": rules::BURST_TARGET_M[1], "description": "burst altitude the design must reach, m; bounds are the burst targets the CUSF burst calculator accepts (cusf-burst-calc js/calc.js sanity_check_inputs); at least 1 km above the launch"},
                "budget_usd": {"type": ["number", "null"], "exclusiveMinimum": 0, "description": "total spend allowed, USD; omit or null for no budget (cost is then information only); no upper limit"},
                "recovery_window_h": {"type": "number", "default": 4, "minimum": 0, "maximum": rules::RECOVERY_WINDOW_MAX_H, "description": "hours the team will search after landing; upper bound Wright's allowance (ASSUMPTION, 3 days)"},
                "min_recovery_probability": {"type": "number", "default": 0.90, "minimum": 0, "maximum": rules::P_SITE, "description": "required recovery probability; capped at rules::P_SITE, the assumed share of recoverable landing spots"},
                "needs_video": {"type": "boolean", "default": true},
                "needs_photos": {"type": "boolean", "default": false},
                "licensed_ham": {"type": "boolean", "default": false},
                "allow_hydrogen": {"type": "boolean", "default": false},
                "ground_temperature_c": {"type": ["number", "null"], "minimum": rules::GROUND_TEMP_RECORD_C[0], "maximum": rules::GROUND_TEMP_RECORD_C[1], "description": "forecast ground temperature, C; judged when inside launch_temp_range_c or the design's declared launch window, otherwise a no-go day. Bounds: San Joaquin Valley records, NWS Hanford (Bakersfield 12 F 1908-01-03, 118 F 1908-07-28)"},
                "launch_temp_range_c": {"type": ["array", "null"], "items": {"type": "number", "minimum": rules::GROUND_TEMP_RECORD_C[0], "maximum": rules::GROUND_TEMP_RECORD_C[1]}, "minItems": 2, "maxItems": 2, "description": "ground temperatures [low, high] C the payload must be launchable at; default [-10, 35]; thermal checks run at both ends. Bounds: San Joaquin Valley records, NWS Hanford"},
                "wind": {"type": "array", "description": "wind profile, increasing altitude", "items": {"type": "object", "properties": {
                    "alt_m": {"type": "number", "minimum": 0, "maximum": rules::WIND_ALT_MAX_M, "description": "altitude, m; up to 10 km over the highest CUSF burst target"},
                    "speed_mps": {"type": "number", "minimum": 0, "maximum": rules::WIND_MAX_MPS, "description": "m/s; NOAA JetStream: jet streams exceed 275 mph (123 m/s)"},
                    "toward_deg": {"type": "number", "minimum": 0, "maximum": 360, "description": "compass bearing the wind blows toward"}}}},
                "owned_ground": {"type": "array", "items": {"type": "string"}},
                "must_fly": {"type": "array", "items": {"type": "string"}, "description": "catalog part ids that must be on the flight train (copy ids from the catalog tool, e.g. lightaprs-2 or heltec-lora32-v3); never a free-text name"}
            }
        })
    }

    /// The envelope and implausible combinations: [`Spec::validate`].
    fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
        spec.validate()?;
        // must_fly names catalog part ids; a free-text name ("APRS tracker")
        // would silently screen out every candidate (design.rs), so refuse
        // it here with the nearest ids (the elicitation analyst re-reads this)
        let cat = catalog();
        let unknown: Vec<String> = spec
            .must_fly
            .iter()
            .filter(|m| cat.part(m).is_err())
            .map(|m| {
                let key = m.to_ascii_lowercase();
                let words: Vec<&str> = key.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| w.len() > 2).collect();
                let mut near: Vec<&str> = cat
                    .parts
                    .iter()
                    .filter(|p| {
                        let hay = format!("{} {} {:?}", p.id, p.name, p.role).to_ascii_lowercase();
                        words.iter().any(|w| hay.contains(w))
                    })
                    .map(|p| p.id.as_str())
                    .take(6)
                    .collect();
                near.sort();
                format!("must_fly `{m}` is not a catalog part id{}", if near.is_empty() { String::new() } else { format!("; nearest: {}", near.join(", ")) })
            })
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{} (use the catalog tool at `must_fly` and copy an id)", unknown.join("; "))
        }
    }

    fn design(spec: &Spec) -> anyhow::Result<Design> {
        spec.validate()?;
        design::search(spec, catalog())
    }

    /// The search, also screened on every confirmed attack (a cold morning,
    /// a hot day): see [`design::search_under`]. A condition outside the
    /// envelope is refused, naming it.
    fn design_under(spec: &Spec, conditions: &[wright_core::Condition<Spec>]) -> anyhow::Result<Design> {
        spec.validate()?;
        for c in conditions {
            c.spec.validate().map_err(|e| anyhow::anyhow!("condition {}: {e}", c.id))?;
        }
        design::search_under(spec, catalog(), conditions)
    }

    fn check(spec: &Spec, d: &Design) -> Finished {
        checks::evaluate(spec, d, catalog()).0
    }

    fn bom(d: &Design) -> Bom {
        bom(d, catalog())
    }

    fn build(d: &Design, out: &Path) -> anyhow::Result<Artifacts> {
        let mut art = build::build(d, catalog(), out)?;
        // the guide's last section (wright_guide::from_artifacts reads
        // checklist.md): the fill, the trackers' settings, the rules
        let items = [
            format!(
                "Fill the {} with {} ({} cylinder{}) until the fill scale reads {:.0} g of neck lift (sized for {:.1} m/s ascent); tie off, then weigh again",
                d.balloon, d.gas, d.cylinders, if d.cylinders == 1 { "" } else { "s" }, d.neck_lift_g, d.fill_target_mps
            ),
            "Set every u-blox GPS to airborne mode (UBX-CFG-NAV5 dynModel 6, Airborne <1g, good to 50 km) and confirm it on the bench; in the default mode many receivers stop reporting above about 12 km".into(),
            "Power every tracker from fresh lithium cells and watch a full position report reach the ground station before release".into(),
            format!(
                "Rig the {} parachute on {:.0} m of {} line ({:.0} lb test) and the payload under it on the swivel harness; the line is the weak link of the train, so the payload separates at under 50 lbf (14 CFR 101.1(a)(4)(iv))",
                d.parachute,
                d.line_m,
                d.line,
                catalog().line(&d.line).map(|l| wright_core::units::Force::n(l.breaking_n).as_lbf()).unwrap_or(f64::NAN)
            ),
            "Weigh each payload package at the launch site: under 4 lb each and 12 lb total keep the flight exempt from Part 101 Subpart D (101.1(a)(4))".into(),
            "Run a landing prediction for the day (e.g. the CUSF/SondeHub predictor) and do not launch if it lands in water, in a city or near an airport".into(),
        ];
        let mut c = String::from("# Before release\n\n");
        // go/no-go first: the launch temperature window this design declares
        c.push_str(&format!("- {}\n", go_no_go(d)));
        c.push_str("- Assemble, power and seal the payload in the heated or air-conditioned car (about 20 C) and carry it out no more than 30 min before release; the thermal model starts the box there (rules::PACK_TEMP_C, rules::PRELAUNCH_MIN)\n");
        if let Some(w) = warmer_step(d) {
            c.push_str(&format!("- {w}\n"));
        }
        for it in items {
            c.push_str(&format!("- {it}\n"));
        }
        let cpath = out.join("checklist.md");
        std::fs::write(&cpath, c)?;
        art.add("pre-release checklist", cpath);
        Ok(art)
    }

    /// Read off the same analysis the checks use.
    fn headlines(spec: &Spec, d: &Design) -> Vec<wright_core::Headline> {
        use wright_core::{Bound, Headline};
        let cat = catalog();
        let h = |label: &str, value: f64, unit: &str, limit: Option<f64>, bound: Bound, check: &str| Headline {
            label: label.into(),
            value,
            unit: unit.into(),
            limit,
            bound,
            check: Some(check.into()),
        };
        let mut out = Vec::new();
        let Ok(a) = analysis::analyse(spec, d, cat) else { return out };
        out.push(h("Burst altitude", a.flight.burst_altitude_m.unwrap_or(f64::NAN), "m", Some(spec.min_burst_altitude_m), Bound::AtLeast, "burst_altitude"));
        out.push(h("Ascent rate (mean)", a.flight.mean_ascent_mps, "m/s", None, Bound::Info, "ascent_rate"));
        out.push(h("Neck lift to fill to", d.neck_lift_g, "g", None, Bound::Info, "lift_budget"));
        out.push(h("Payload package", a.masses.package().as_g(), "g", Some(rules::FAA_PKG_MAX_LB * 453.59237), Bound::AtMost, "faa101_package_max"));
        out.push(h("Cost", bom(d, cat).total().as_usd(), "USD", spec.budget_usd, if spec.budget_usd.is_some() { Bound::AtMost } else { Bound::Info }, "cost"));
        // the go/no-go launch window: its low edge must reach the range's
        // low end and its high edge the high end
        let [rlo, rhi] = a.launch_range_c;
        let [wlo, whi] = d.launch_window_c.unwrap_or([f64::NAN, f64::NAN]);
        out.push(h("Launch window, coldest ground", wlo, "C", Some(rlo), Bound::AtMost, "launch_window"));
        out.push(h("Launch window, hottest ground", whi, "C", Some(rhi), Bound::AtLeast, "launch_window"));
        out
    }

    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        options::PATHS.to_vec()
    }

    /// The catalog rows the field at `path` accepts (`options::all`),
    /// filtered on id or label; the design does not narrow them.
    fn options(_d: &Design, path: &str, query: Option<&str>) -> Vec<wright_core::CatalogOption> {
        options::options(catalog(), path, query)
    }

    /// The balloon catalog does not depend on the design, so the agents can
    /// read it while the spec is still being written.
    fn options_before_design(path: &str, query: Option<&str>) -> Option<Vec<wright_core::CatalogOption>> {
        Some(options::options(catalog(), path, query))
    }

    /// The burst altitude the CUSF rule predicts for this fill (the design
    /// carries no launch site; `check` reports the US76-integrated value).
    fn objective(d: &Design) -> (f64, &'static str) {
        let cat = catalog();
        let v = (|| -> anyhow::Result<f64> {
            let b = cat.balloon(&d.balloon)?;
            let g = cat.gas(&d.gas)?;
            let rho_g = match g.gas {
                catalog::LiftGas::Helium => rules::CUSF_RHO_HE,
                catalog::LiftGas::Hydrogen => rules::CUSF_RHO_H2,
            };
            let v = (d.neck_lift_g + b.mass_g) * 1e-3 / (rules::CUSF_RHO_AIR - rho_g);
            Ok(physics::cusf_burst_altitude(wright_core::units::Volume::m3(v), wright_core::units::Length::m(b.burst_diameter_m)).as_m())
        })();
        // design only (no spec, so no winds or flight simulation): the CUSF
        // closed form; the checks and headlines use the simulated burst
        (v.unwrap_or(f64::NAN), "burst altitude m (CUSF closed-form estimate)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::CheckId;

    #[test]
    fn all_lists_every_check() {
        // every variant appears once in ALL (a new variant must be added there)
        let names: std::collections::BTreeSet<_> = Check::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), Check::ALL.len());
        assert_eq!(Check::ALL.len(), 26);
    }

    #[test]
    fn must_fly_must_name_catalog_ids() {
        use wright_core::Domain;
        let mut spec = reference::mission_spec(30_000.0, None);
        spec.must_fly = vec!["APRS tracker (primary)".into()];
        let e = Balloon::validate_spec(&spec).unwrap_err().to_string();
        assert!(e.contains("APRS tracker (primary)") && e.contains("not a catalog part id"), "{e}");
        assert!(e.contains("nearest:"), "should suggest ids: {e}");
        let first = e.split("nearest: ").nth(1).unwrap().split([',', ' ', ')']).next().unwrap().to_string();
        spec.must_fly = vec![first.clone()];
        assert!(Balloon::validate_spec(&spec).is_ok(), "{first} should be accepted");
    }

    #[test]
    fn catalog_and_kits_load() {
        let c = catalog();
        assert!(!c.balloons.is_empty());
        for k in design::kits() {
            for it in &k.items {
                c.part(&it.part).unwrap();
            }
            for b in &k.buses {
                c.part(&b.battery).unwrap();
            }
        }
    }
}
