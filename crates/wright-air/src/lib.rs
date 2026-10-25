//! Fixed-wing 4+1 VTOL quadplanes for Wright.
//!
//! [`Aircraft`] implements `wright_core::Domain`: `design` searches from a
//! requirements-only spec (or keeps what the spec names), `check` runs the
//! closed set [`checks::Check`] -- physics, ratings, the ArduPilot file, the
//! budget from the BOM's own rows, all-pairs interference on wright-geom
//! meshes, and PrusaSlicer's time and grams -- and `build` writes the CAD,
//! G-code, BOM, parameters, harness and a first-flight checklist.
//!
//! The physics: ISA ([`atmosphere`]), measured airfoil polars
//! ([`airfoil`]), UIUC propeller data ([`prop`]), Drela's motor model and a
//! sagging pack with burst limits ([`powertrain`]), an AeroBuildup-style
//! drag buildup and trim ([`aero`]), hover, cruise, mission energy and
//! structure with explicit load paths ([`perf`]), the neutral point with
//! per-degree and per-radian kept apart ([`stability`]). Every number that
//! changes an answer is in [`rules`] with its source.

pub mod aero;
pub mod airfoil;
pub mod ardupilot;
pub mod atmosphere;
pub mod battery;
pub mod bom;
pub mod build;
pub mod cad_catalog;
pub mod options;
pub mod catalog;
pub mod checks;
pub mod electrical;
pub mod geometry;
pub mod layout;
pub mod perf;
pub mod powertrain;
pub mod print;
pub mod prop;
pub mod reference;
pub mod rules;
pub mod search;
pub mod sim;
pub mod sizing;
pub mod spec;
pub mod stability;

use std::path::Path;
use wright_core::{Artifacts, Bom, Bound, Domain, Finished, Headline};

/// The aircraft domain (VTOL quadplanes).
pub struct Aircraft;

impl Domain for Aircraft {
    const NAME: &'static str = "aircraft";
    type Spec = spec::Spec;
    type Design = spec::Design;
    type Check = checks::Check;

    fn spec_schema() -> serde_json::Value {
        let num = |d: &str| serde_json::json!({"type": "number", "description": d});
        let opt = |d: &str| serde_json::json!({"type": ["number", "null"], "description": d});
        let id = |d: &str| serde_json::json!({"type": ["string", "null"], "description": d});
        let mut schema = serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {"type": "string"},
                "requirements": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "range_km": opt("range with a 15 % energy reserve, km"),
                        "endurance_min": opt("endurance with reserve, min"),
                        "payload_g": num("payload carried in the bay, g"),
                        "max_span_mm": num("largest wingspan allowed, mm (default 1200)"),
                        "printer": {"type": "string", "enum": rules::PRINTERS.iter().map(|p| p.0).collect::<Vec<_>>()},
                        "budget_usd": opt("total parts cost including the battery, USD"),
                        "altitude_m": num("field altitude, m"),
                        "hot_day_dk": num("hover is checked this many K above ISA (default 15); at most MIL-HDBK-310's 1 % high temperature at the field altitude less ISA there (49/40/30/17 C at 0/1/2/4 km geometric: about +34 K at sea level, +28 K at 2-4.4 km)"),
                        "max_stall_ms": num("highest acceptable stall speed, EQUIVALENT airspeed m/s (default 14, an assumption): it bounds the transition and ArduPilot's airspeed floor, which ArduPilot holds in EAS (AP_TECS scales AIRSPEED_MIN by EAS2TAS); the TAS stall at the field is a headline"),
                        "cells_s": {"type": "integer", "description": "pack cells in series (default 6)"},
                        "airspeed_sensor": {"type": "boolean", "description": "carry an airspeed sensor (default true)"},
                        "max_print_hours": opt("longest acceptable total print time, h")
                    }
                },
                "design": {
                    "type": "object",
                    "description": "optional choices to keep fixed; everything left out is searched",
                    "additionalProperties": false,
                    "properties": {
                        "span_mm": opt("wingspan, mm"), "root_chord_mm": opt("root chord, mm"), "tip_chord_mm": opt("tip chord, mm"),
                        "airfoil": id("wing section id (data/air/airfoils.json)"),
                        "lift_motor": id("catalog motor id"), "lift_prop": id("catalog prop id"), "lift_esc": id("catalog ESC id"),
                        "cruise_motor": id("catalog motor id"), "cruise_prop": id("catalog prop id"), "cruise_esc": id("catalog ESC id"),
                        "lift_current_limit_a": opt("per-motor current limit in each lift ESC's AM32 firmware, A (even, 2-200); needs a lift ESC with a per-motor limit"),
                        "battery": {"description": "{kind: pack, id, count 1-3} or {kind: cells, id, s, p 1-8}"},
                        "fc": id("flight controller id"), "gps": id("GPS id"), "receiver": id("receiver id"),
                        "airspeed": {"description": "airspeed sensor id, or null for none"},
                        "servo": id("servo id"), "main_spar": id("tube id"), "boom_tube": id("tube id")
                    }
                }
            }
        });
        // the requirement envelope, each bound with its source
        if let Some(props) = schema.pointer_mut("/properties/requirements/properties") {
            for &(k, lo, hi, source) in rules::REQUIREMENT_BOUNDS {
                let Some(node) = props.get_mut(k).and_then(|n| n.as_object_mut()) else { continue };
                node.insert("minimum".into(), lo.into());
                if let Some(h) = hi {
                    node.insert("maximum".into(), h.into());
                }
                let d = node.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let range = format!("[{lo}, {}]", hi.map_or("-".to_string(), |h| h.to_string()));
                node.insert("description".into(), format!("{d}; range {range}: {source}").trim_start_matches("; ").into());
            }
        }
        schema
    }

    fn consistency_checks() -> &'static [&'static str] {
        checks::CONSISTENCY_CHECKS
    }

    fn validate_spec(spec: &Self::Spec) -> anyhow::Result<()> {
        spec.requirements.validate().map_err(anyhow::Error::msg)
    }

    fn design(spec: &Self::Spec) -> anyhow::Result<Self::Design> {
        let mut log = search::SearchLog { lines: vec![], evaluations: 0 };
        search::design(spec, &mut log, &|_| {})
    }

    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        options::paths()
    }

    fn options(design: &Self::Design, path: &str, query: Option<&str>) -> Vec<wright_core::CatalogOption> {
        options::options(design, path, query)
    }

    fn design_under(spec: &Self::Spec, conditions: &[wright_core::Condition<Self::Spec>]) -> anyhow::Result<Self::Design> {
        let screens: Vec<search::Screen> = conditions.iter().map(|c| search::Screen { id: c.id.clone(), check: c.check.clone(), requirements: c.spec.requirements.clone() }).collect();
        let mut log = search::SearchLog { lines: vec![], evaluations: 0 };
        search::design_under(spec, &screens, &mut log, &|_| {})
    }

    fn check(spec: &Self::Spec, design: &Self::Design) -> Finished {
        checks::check_against(&spec.requirements, design)
    }

    fn bom(design: &Self::Design) -> Bom {
        match checks::evaluate(design, powertrain::Factors::NOMINAL) {
            Ok(e) => e.bom,
            Err(_) => Bom::default(),
        }
    }

    fn build(design: &Self::Design, out: &Path) -> anyhow::Result<Artifacts> {
        build::build(design, out)
    }

    fn objective(design: &Self::Design) -> (f64, &'static str) {
        let r = checks::evaluate(design, powertrain::Factors::NOMINAL).map(|e| e.perf.range_km).unwrap_or(0.0);
        (r, "range km (15 % reserve)")
    }

    fn headlines(spec: &Self::Spec, design: &Self::Design) -> Vec<Headline> {
        // judged against the spec, as `check` is
        let mut d = design.clone();
        d.requirements = spec.requirements.clone();
        let Ok(e) = checks::evaluate(&d, powertrain::Factors::NOMINAL) else { return vec![] };
        let req = &spec.requirements;
        let h = |label: &str, value: f64, unit: &str, limit: Option<f64>, bound: Bound, check: Option<&str>| Headline {
            label: label.into(),
            value,
            unit: unit.into(),
            limit,
            bound,
            check: check.map(str::to_string),
        };
        let mut v = vec![
            h("Endurance", e.perf.endurance_min, "min", req.endurance_min, if req.endurance_min.is_some() { Bound::AtLeast } else { Bound::Info }, Some("endurance")),
            h("Range", e.perf.range_km, "km", req.range_km, if req.range_km.is_some() { Bound::AtLeast } else { Bound::Info }, Some("range")),
            h("Cost", e.bom.total().as_usd(), "USD", req.budget_usd, if req.budget_usd.is_some() { Bound::AtMost } else { Bound::Info }, Some("budget")),
            h("Mass", e.layout.mass().as_kg(), "kg", None, Bound::Info, None),
            h("Stall at the field (TAS)", e.perf.stall.as_mps(), "m/s", None, Bound::Info, None),
        ];
        if let Some(lim) = req.max_print_hours {
            if let Ok(p) = print::slice_cached(&e.layout) {
                v.push(h("Print time", p.iter().map(|x| x.seconds).sum::<f64>() / 3600.0, "h", Some(lim), Bound::AtMost, Some("print_time")));
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::CheckId;

    #[test]
    fn check_list_is_closed_and_named_uniquely() {
        let names: std::collections::BTreeSet<&str> = checks::Check::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), checks::Check::ALL.len());
    }

    #[test]
    fn a_missing_part_fails_checks_instead_of_reading_zero() {
        let mut d = reference::design(220.0);
        d.propulsion.lift_esc = "holybro_tekko32_f4_4in1_mini_45a_am32".into(); // no published mass: rejected
        let f = checks::check(&d);
        assert!(!f.passed());
        assert!(f.get("parts").is_some_and(|o| !o.passed()));
        assert_eq!(f.outcomes.len(), checks::Check::ALL.len());
    }

    #[test]
    fn requirements_only_spec_seeds_and_evaluates() {
        let spec: spec::Spec = serde_json::from_str(r#"{"requirements": {"budget_usd": 2000}}"#).unwrap();
        let s = search::score(&search::seed(&spec));
        assert!(s.score.is_finite());
    }

    #[test]
    fn reference_reproduction_is_close_to_its_report() {
        let d = reference::sized(220.0).unwrap();
        let e = checks::evaluate(&d, powertrain::Factors::NOMINAL).unwrap();
        let m = e.layout.mass().as_kg();
        assert!((m - reference::REPORTED.mass_kg).abs() / reference::REPORTED.mass_kg < 0.1, "mass {m}");
        let (_, t, km) = perf::at_speed(&e.layout, &e.perf, wright_core::units::Speed::mps(reference::flown_speed_mps())).unwrap();
        assert!((km / reference::MILE_KM - 130.0).abs() < 25.0, "{km}");
        assert!((t.as_h() - 3.0).abs() < 0.6);
    }

    #[test]
    fn budget_check_reads_the_bom_rows() {
        let mut d = reference::sized(220.0).unwrap();
        d.requirements.budget_usd = Some(2000.0);
        let e = checks::evaluate(&d, powertrain::Factors::NOMINAL).unwrap();
        let f = checks::physics_only(&e);
        let b = f.get("budget").unwrap().margin().unwrap();
        assert!((2000.0 - b - Aircraft::bom(&d).total().as_usd()).abs() < 1e-6);
    }

    #[test]
    fn scoring_is_deterministic() {
        let d = reference::sized(220.0).unwrap();
        let (a, b) = (search::score(&d), search::score(&d));
        assert_eq!(a.score, b.score);
        assert_eq!(a.design, b.design);
    }

    #[test]
    fn pessimistic_model_is_worse() {
        let d = reference::sized(220.0).unwrap();
        let n = checks::evaluate(&d, powertrain::Factors::NOMINAL).unwrap().perf.range_km;
        let p = checks::evaluate(&d, powertrain::Factors::PESSIMISTIC).unwrap().perf.range_km;
        assert!(p < 0.85 * n, "{p} vs {n}");
    }

    #[test]
    fn made_to_order_parts_fail_unless_the_spec_allows_them() {
        // the reference flies a made-to-order pack and a discontinued FC
        let mut d = reference::sized(220.0).unwrap();
        let text = |d: &spec::Design| {
            let e = checks::evaluate(d, powertrain::Factors::NOMINAL).unwrap();
            let o = checks::physics_only(&e).get("parts_orderable").unwrap().clone();
            (o.passed(), o.detail.clone())
        };
        let (ok, t) = text(&d);
        assert!(!ok && t.contains("upgrade_energy_gold_v1_6s2p_22ah_sa08 (made to order)") && t.contains("speedybee_f405_wing_app (discontinued)"), "{t}");
        d.requirements.stock = spec::StockPolicy::LeadTime;
        let (ok, t) = text(&d);
        assert!(!ok && !t.contains("upgrade_energy") && t.contains("discontinued"), "{t}");
        // a requirements-only seed under the default policy orders nothing it may not
        let spec: spec::Spec = serde_json::from_str(r#"{"requirements": {"budget_usd": 2000}}"#).unwrap();
        let seed = sizing::size_and_repair(search::seed(&spec)).unwrap();
        let (ok, t) = text(&seed);
        assert!(ok, "{t}");
    }

    #[test]
    fn stock_policy_orders_the_states() {
        use catalog::Stock as S;
        use spec::StockPolicy as P;
        assert!(S::InStock.accepted(P::InStock) && !S::Unstated.accepted(P::InStock));
        assert!(S::Unstated.accepted(P::Listed) && !S::MadeToOrder.accepted(P::Listed) && !S::OutOfStock.accepted(P::Listed));
        assert!(S::MadeToOrder.accepted(P::LeadTime) && S::PreOrder.accepted(P::LeadTime) && !S::OutOfStock.accepted(P::LeadTime));
        assert!(S::OutOfStock.accepted(P::Restock) && !S::Discontinued.accepted(P::Restock));
    }

    #[test]
    fn checklist_flags_ribless_wings_and_unconfirmed_stock() {
        let d = reference::sized(220.0).unwrap();
        let e = checks::evaluate(&d, powertrain::Factors::NOMINAL).unwrap();
        let c = build::checklist(&e);
        assert!(c.contains("NO internal ribs") && c.contains("Confirm stock before ordering"), "{c}");
    }

    #[test]
    fn lift_prop_mount_needs_published_dimensions_on_both_sides() {
        let c = catalog::catalog();
        let fit = |p: &str, m: &str| catalog::mount_fit(c.props[p].hub.as_ref().unwrap(), c.motors[m].prop_interface.as_ref().unwrap());
        assert!(fit("tmotor_mf1302_folding", "tmotor_mn3110_470kv").is_ok());
        assert!(fit("tmotor_mf1302_folding", "aos_supernova_2810_900kv").is_err(), "a bolt-on prop on a bare threaded shaft");
        assert!(fit("gemfan_flash_7042_2blade", "aos_supernova_2810_900kv").is_ok());
        assert!(fit("apc_10x4_5mr_pair", "aos_supernova_2810_900kv").is_err(), "6.35 mm bore, no rings, on 5 mm");
        assert!(c.motors["aos_supernova_2807_1400kv"].prop_interface.is_none());
    }

    #[test]
    fn reference_hover_reads_the_makers_table() {
        let d = reference::sized(176.0).unwrap();
        let e = checks::evaluate(&d, powertrain::Factors::NOMINAL).unwrap();
        let op = e.perf.hover.std_full.as_ref().unwrap();
        let model = powertrain::operate(e.layout.parts.lift_motor, &e.layout.parts.lift_prop.data, op.thrust, wright_core::units::Speed::mps(0.0), wright_core::units::Density::kg_m3(1.225), wright_core::units::Voltage::v(23.0), powertrain::Factors::NOMINAL).unwrap();
        println!("table: {:.1} W {:.2} duty {:.1} A | model {:.1} W {:.2} duty {:.1} A", op.battery.as_w(), op.throttle, op.motor_a.as_a(), model.battery.as_w(), model.throttle, model.motor_a.as_a());
        assert!(op.battery.as_w() < model.battery.as_w());
    }

    #[test]
    fn the_envelope_is_in_the_schema_and_enforced() {
        let schema = Aircraft::spec_schema();
        let full: spec::Requirements = serde_json::from_str(r#"{"range_km": 100, "endurance_min": 60, "budget_usd": 2000, "max_print_hours": 50}"#).unwrap();
        for &(k, lo, hi, _) in rules::REQUIREMENT_BOUNDS {
            let node = schema.pointer(&format!("/properties/requirements/properties/{k}")).unwrap_or_else(|| panic!("{k} not in the schema"));
            assert_eq!(node["minimum"].as_f64(), Some(lo), "{k}");
            assert_eq!(node["maximum"].as_f64(), hi, "{k}");
            // every bounded field is read by validate (a typo would skip it)
            let mut v = serde_json::to_value(&full).unwrap();
            v[k] = serde_json::json!(lo - 1.0);
            let r: spec::Requirements = serde_json::from_value(v).unwrap_or_else(|_| full.clone());
            if r != full {
                assert!(r.validate().is_err(), "{k} below its minimum passed validation");
            }
        }
        let at = |alt: f64, dk: f64| {
            let r: spec::Requirements = serde_json::from_value(serde_json::json!({"altitude_m": alt, "hot_day_dk": dk})).unwrap();
            Aircraft::validate_spec(&spec::Spec { name: "t".into(), requirements: r, design: Default::default() })
        };
        // the reviewer's confirmed attacks are outside MIL-HDBK-310's 1 % hot day
        assert!(at(3500.0, 45.0).is_err());
        assert!(at(3000.0, 35.0).is_err());
        assert!(at(3000.0, 30.0).unwrap_err().to_string().contains("MIL-HDBK-310"));
        // and the envelope keeps realistic hot days
        assert!(at(3000.0, 27.0).is_ok());
        assert!(at(0.0, 34.0).is_ok() && at(0.0, 34.5).is_err());
        assert!(at(1000.0, 15.0).is_ok() && at(4411.0, 28.0).is_ok() && at(4500.0, 15.0).is_err());
        assert!(at(0.0, -5.0).is_err());
    }

    #[test]
    fn review5_ids_and_counts_are_refused_at_read_time() {
        let d = reference::sized(220.0).unwrap();
        let mut v = serde_json::to_value(&d).unwrap();
        // F3: a pack count of 0 (or 4) is refused, not read as 1
        for bad in [serde_json::json!({"kind": "pack", "id": "upgrade_energy_gold_v1_6s2p_22ah_sa08", "count": 0}), serde_json::json!({"kind": "cells", "id": "molicel_p50b", "s": 6, "p": 9})] {
            let mut w = v.clone();
            w["battery"] = bad;
            assert!(serde_json::from_value::<spec::Design>(w).is_err());
        }
        assert!(battery::resolve(&spec::BatteryChoice::Pack { id: catalog::catalog().packs.keys().next().unwrap().clone(), count: 0 }).is_err());
        // F8: the option id catalog lists for the current limit is accepted
        let opts = Aircraft::options(&d, "propulsion.lift_current_limit_a", None);
        v["propulsion"]["lift_current_limit_a"] = serde_json::json!(opts[11].id);
        let r: spec::Design = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(r.propulsion.lift_current_limit_a, Some(24.0));
        v["propulsion"]["lift_current_limit_a"] = serde_json::json!(25);
        assert!(serde_json::from_value::<spec::Design>(v.clone()).is_err());
        v["propulsion"]["lift_current_limit_a"] = serde_json::Value::Null;
        // F9: made-up hardware roles and ids are refused with the valid ones
        v["hardware"] = serde_json::json!({"made_up_role": "x"});
        let e = serde_json::from_value::<spec::Design>(v.clone()).unwrap_err().to_string();
        assert!(e.contains("valid roles"), "{e}");
        v["hardware"] = serde_json::json!({"ca_glue": "made_up_glue"});
        let e = serde_json::from_value::<spec::Design>(v.clone()).unwrap_err().to_string();
        assert!(e.contains("valid ids") && e.contains("estes_bsi_insta_cure_super_thin_2oz"), "{e}");
    }

    #[test]
    fn consistency_checks_are_the_copy_comparisons() {
        // F1: spec_requirements is the only check comparing the design's copy
        // of the spec with the spec; every listed name is a declared check
        assert_eq!(Aircraft::consistency_checks(), &["spec_requirements"]);
        let d = reference::sized(220.0).unwrap();
        let spec = spec::Spec { name: "t".into(), requirements: d.requirements.clone(), design: Default::default() };
        let mut changed = spec.clone();
        changed.requirements.altitude_m = 1000.0;
        let f = Aircraft::check(&changed, &d);
        for c in Aircraft::consistency_checks() {
            assert!(!f.get(c).unwrap().passed(), "{c} should fail on a changed spec: that is why it is a consistency check");
        }
        // and the physics checks are judged on the changed spec, not the copy
        assert_ne!(f.get("stall_speed").unwrap().evidence, Aircraft::check(&spec, &d).get("stall_speed").unwrap().evidence);
    }

    #[test]
    fn impossible_payloads_are_errors_not_nan() {
        let spec: spec::Spec = serde_json::from_str(r#"{"requirements": {"payload_g": -100}}"#).unwrap();
        assert!(Aircraft::design(&spec).is_err());
        let mut d = reference::sized(220.0).unwrap();
        d.requirements.payload_g = 1500.0;
        if let Ok(l) = layout::solve(&d) {
            assert!(l.mass().as_kg().is_finite());
        }
        d.requirements.payload_g = f64::NAN;
        assert!(layout::solve(&d).is_err());
    }

    #[test]
    fn checks_judge_the_spec_not_the_designs_copy() {
        let d = reference::sized(220.0).unwrap();
        let spec = spec::Spec { name: "t".into(), requirements: d.requirements.clone(), design: Default::default() };
        let base = Aircraft::check(&spec, &d);
        assert!(base.get("spec_requirements").unwrap().passed());
        // an edit that lowers the design's own requirement changes nothing but fails
        let mut lowered = d.clone();
        lowered.requirements.range_km = Some(5.0);
        lowered.requirements.budget_usd = Some(1e6);
        let f = Aircraft::check(&spec, &lowered);
        assert!(!f.get("spec_requirements").unwrap().passed());
        assert_eq!(f.get("budget").unwrap().evidence, base.get("budget").unwrap().evidence);
        // a harder spec changes the outcomes
        let mut hard = spec.clone();
        hard.requirements.range_km = Some(600.0);
        hard.requirements.budget_usd = Some(100.0);
        let h = Aircraft::check(&hard, &d);
        assert!(!h.get("range").unwrap().passed() && !h.get("budget").unwrap().passed());
    }

    #[test]
    fn catalog_options_cover_every_choice_and_swap_the_glue() {
        let d = reference::sized(220.0).unwrap();
        for (path, _) in Aircraft::catalog_paths() {
            assert!(!Aircraft::options(&d, path, None).is_empty(), "{path} has no options");
        }
        let booms = Aircraft::options(&d, "structure.boom_tube", Some("CST"));
        assert!(!booms.is_empty() && booms.iter().all(|o| o.id.contains("cst") || o.label.to_lowercase().contains("cst")));
        assert!(booms[0].specs.contains_key("ei_n_m2") && booms[0].specs.contains_key("wall_mm"));
        let glue = Aircraft::options(&d, "hardware.ca_glue", Some("insta-cure"));
        assert!(glue.len() >= 2, "{glue:?}");
        // swapping the glue by design field reaches the BOM
        let mut d2 = d.clone();
        d2.hardware.insert("ca_glue".into(), "bsi_insta_cure_thin_2oz".into());
        assert!(Aircraft::bom(&d2).lines.iter().any(|l| l.id == "bsi_insta_cure_thin_2oz"));
        assert!(Aircraft::bom(&d).lines.iter().any(|l| l.id == "estes_bsi_insta_cure_super_thin_2oz"));
        assert!(Aircraft::options(&d, "propulsion.lift_motor", Some("MN3110")).len() == 3);
    }

    /// An agent can reach the headline design by edits alone: from the
    /// sized requirements-only seed, patch each catalog path to the
    /// headline's value (every one listed in `catalog_paths`, or a geometry
    /// number) and the result passes every check.
    #[test]
    fn edits_from_the_seed_reach_the_headline_design() {
        let o: serde_json::Value = serde_json::from_str(include_str!("../../../evals/vtol/result.json")).unwrap();
        let target = &o["wright"]["design"];
        let spec = spec::Spec { name: "edits".into(), requirements: serde_json::from_value(target["requirements"].clone()).unwrap(), design: Default::default() };
        let mut d = serde_json::to_value(sizing::size_and_repair(search::seed(&spec)).unwrap()).unwrap();
        let paths: Vec<&str> = Aircraft::catalog_paths().into_iter().map(|p| p.0).collect();
        let geometry = ["wing.span_mm", "wing.root_chord_mm", "wing.tip_chord_mm", "wing.incidence_deg", "wing.washout_deg", "structure.leg_thickness_mm", "propulsion.lift_tilt_deg", "tail.vh", "tail.vv"];
        let mut patched = 0;
        for path in paths.iter().copied().chain(geometry) {
            let mut src = target;
            let mut ok = true;
            for k in path.split('.') {
                match src.get(k) {
                    Some(v) => src = v,
                    None => ok = false,
                }
            }
            if !ok {
                continue; // e.g. a hardware role the headline leaves to default
            }
            // the patch an agent's edit_design sends: {"a": {"b": value}}
            let mut patch = src.clone();
            for k in path.split('.').rev() {
                patch = serde_json::json!({ k: patch });
            }
            fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
                match (a, b) {
                    (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                        for (k, v) in b {
                            merge(a.entry(k.clone()).or_insert(serde_json::Value::Null), v);
                        }
                    }
                    (a, b) => *a = b.clone(),
                }
            }
            merge(&mut d, &patch);
            patched += 1;
        }
        d["name"] = target["name"].clone();
        let d: spec::Design = serde_json::from_value(d).unwrap();
        assert!(patched > 20, "{patched} patches");
        assert_eq!(serde_json::to_value(&d).unwrap(), *target, "every field the headline sets is reachable by these paths");
        let f = Aircraft::check(&spec, &d);
        assert!(f.passed(), "{f}");
        assert_eq!(f.outcomes.len(), 48);
        assert!(Aircraft::options(&d, "wing.airfoil", Some("sd7032")).iter().any(|o| o.specs.contains_key("clmax_corrected")));
        assert!(Aircraft::options(&d, "propulsion.lift_current_limit_a", Some("24")).iter().any(|o| o.id == "24"));
    }

    #[test]
    fn made_up_catalog_ids_are_refused_with_the_valid_ones() {
        let d = reference::sized(220.0).unwrap();
        let mut v = serde_json::to_value(&d).unwrap();
        assert!(serde_json::from_value::<spec::Design>(v.clone()).is_ok());
        v["wing"]["airfoil"] = "__probe__".into();
        let e = serde_json::from_value::<spec::Design>(v.clone()).unwrap_err().to_string();
        assert!(e.contains("unknown wing.airfoil id \"__probe__\"") && e.contains("sd7032") && e.contains("catalog wing.airfoil"), "{e}");
        for (path, bad) in [("/structure/boom_tube", "cst_cs625_1m"), ("/propulsion/lift_motor", "nope"), ("/avionics/airspeed", "nope")] {
            let mut w = serde_json::to_value(&d).unwrap();
            *w.pointer_mut(path).unwrap() = bad.into();
            assert!(serde_json::from_value::<spec::Design>(w).is_err(), "{path} accepted {bad}");
        }
        let mut w = serde_json::to_value(&d).unwrap();
        w["battery"] = serde_json::json!({"kind": "cells", "id": "unobtainium", "s": 6, "p": 4});
        assert!(serde_json::from_value::<spec::Design>(w).is_err());
        // and at check time (a design built in code)
        let mut d2 = d.clone();
        d2.wing.airfoil = "__probe__".into();
        let f = checks::check(&d2);
        let o = f.get("parts").unwrap();
        assert!(!o.passed() && o.to_string().contains("catalog wing.airfoil"), "{o}");
    }

    /// Checking (which slices every printed piece) leaves nothing in the
    /// temp dir but a capped index of times and grams.
    #[test]
    fn slicing_leaves_no_files_in_the_temp_dir() {
        let tmp = std::env::temp_dir();
        let entries = || -> std::collections::BTreeSet<String> {
            std::fs::read_dir(&tmp).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("wright-air")).collect()
        };
        let before = entries();
        let mut d = reference::sized(220.0).unwrap();
        d.wing.root_chord_mm += 0.001 * (std::process::id() % 997) as f64; // a design not sliced before
        let f = checks::check(&d);
        assert!(f.get("print_time").is_some());
        // other tests in this process may be slicing right now (their
        // scratch dirs exist until they finish): wait for them, up to 5 min
        let mut new: Vec<String> = vec![];
        for _ in 0..300 {
            new = entries().difference(&before).filter(|n| *n != print::SLICE_INDEX_DIR).cloned().collect();
            if new.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        assert!(new.is_empty(), "left behind: {new:?}");
        assert!(!tmp.join("wright-air-slices").exists());
        let idx: u64 = std::fs::read_dir(tmp.join(print::SLICE_INDEX_DIR)).map(|rd| rd.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0);
        assert!(idx <= print::SLICE_INDEX_CAP_BYTES);
    }

    /// A confirmed attack (a 1000 m field) on stall: the design the search
    /// returns passes stall at 1000 m, or it is the closest and the log
    /// warns that none does.
    #[test]
    fn design_under_screens_a_1000_m_stall_attack() {
        let mut req = reference::requirements();
        req.stock = spec::StockPolicy::Listed;
        req.range_km = Some(150.0);
        // the parts fixed so the search is only the airframe (fast)
        let design = spec::Choices {
            airfoil: Some("sd7062".into()),
            lift_motor: Some("aos_supernova_2810_900kv".into()),
            lift_prop: Some("hqprop_mr_9x4_5_pair".into()),
            cruise_motor: Some("brotherhobby_avenger_2806_5_1300kv".into()),
            cruise_prop: Some("gemfan_flash_7042_2blade".into()),
            battery: Some(spec::BatteryChoice::Cells { id: "molicel_p50b".into(), s: 6, p: 5 }),
            ..Default::default()
        };
        let spec = spec::Spec { name: "attack".into(), requirements: req.clone(), design };
        let mut attacked = spec.clone();
        attacked.requirements.altitude_m = 1000.0;
        let cond = wright_core::Condition { id: "t-r1-1".into(), check: "stall_speed".into(), claim: "1000 m field".into(), spec: attacked };
        let screen = search::Screen { id: cond.id.clone(), check: cond.check.clone(), requirements: cond.spec.requirements.clone() };
        let mut log = search::SearchLog { lines: vec![], evaluations: 0 };
        // the closest design comes back either way (review 5, F6); one that
        // fails the attack says so in the log
        let d = search::design_under(&spec, std::slice::from_ref(&screen), &mut log, &|_| {}).expect("a design, passing or closest");
        if search::passes_screen(&d, &screen).is_err() || !checks::check(&d).passed() {
            assert!(log.lines.iter().any(|l| l.starts_with("WARNING") && l.contains("stall_speed")), "{:?}", log.lines);
        }
        assert!(Aircraft::design_under(&spec, std::slice::from_ref(&cond)).is_ok());
    }
}
