//! The agents' `catalog` lookups (`Domain::catalog_paths` / `options`):
//! every path offers something, and every id offered is one the design
//! accepts at that path.

use serde_json::Value;
use wright_balloon::catalog::{catalog, LinkKind};
use wright_balloon::{design, reference, Balloon, Design};
use wright_core::{Domain, Evidence, Finished};

fn pat() -> Design {
    let cat = catalog();
    let mut d = reference::pat_design();
    let wall = d.enclosure.wall_mm;
    design::size_enclosure(&mut d, cat, wall).unwrap();
    design::size_fill(&mut d, cat).unwrap();
    d
}

/// Put `id` at `path` of the design as JSON (the shape an agent's
/// edit_design patch produces); list paths take element 0.
fn set(d: &mut Value, path: &str, id: &str) {
    let id = Value::String(id.into());
    match path {
        "enclosure.insulation" => d["enclosure"]["insulation"] = id,
        "items[].part" => d["items"][0]["part"] = id,
        "buses[].battery" => d["buses"][0]["battery"] = id,
        // element 0 of a non-empty list keeps every item's rail index valid
        "buses[].rails" => d["buses"][0]["rails"][0] = id,
        "trackers[].radio" => d["trackers"][0]["radio"] = id,
        "trackers[].gps" => d["trackers"][0]["gps"] = id,
        "trackers[].ground" => d["trackers"][0]["ground"] = id,
        "buy_ground[]" => d["buy_ground"].as_array_mut().unwrap().push(id),
        top => {
            assert!(!top.contains(['.', '[']), "test does not know how to set {top}");
            d[top] = id;
        }
    }
}

/// What the domain says when `id` is unknown or of the wrong kind
/// (`Catalog::balloon` .. `Catalog::station`, and the analysis's and
/// checks' role tests), as it appears in a check's text.
fn rejections(id: &str) -> Vec<String> {
    let mut v: Vec<String> = ["balloon", "parachute", "gas", "insulation", "line", "part", "ground station"].iter().map(|t| format!("no {t} {id}")).collect();
    for tail in ["is not a battery", "has no battery spec", "has no radio spec", "is not a GPS"] {
        v.push(format!("{id} {tail}"));
    }
    v.push(format!("rail {id} is not a known regulator"));
    v.push(format!("unknown gps {id}"));
    v.push(format!("unknown battery {id}"));
    v
}

/// The domain's lookup for the field at `path`, with the role test the
/// analysis applies to it (analysis.rs: a bus battery needs a battery spec,
/// a rail a regulator spec, a tracker radio a radio spec, a tracker GPS a
/// GPS or a phone's cellular modem).
fn accepts(path: &str, id: &str) -> bool {
    let c = catalog();
    let part = c.part(id).ok();
    match path {
        "balloon" => c.balloon(id).is_ok(),
        "gas" => c.gas(id).is_ok(),
        "parachute" => c.chute(id).is_ok(),
        "line" => c.line(id).is_ok(),
        "enclosure.insulation" => c.insulation(id).is_ok(),
        "items[].part" => part.is_some(),
        "buses[].battery" => part.is_some_and(|p| p.battery.is_some()),
        "buses[].rails" => part.is_some_and(|p| p.regulator.is_some()),
        "trackers[].radio" => part.is_some_and(|p| p.radio.is_some()),
        "trackers[].gps" => part.is_some_and(|p| p.gps.is_some() || p.radio.as_ref().is_some_and(|r| r.link == LinkKind::Cellular)),
        "trackers[].ground" | "buy_ground[]" => c.station(id).is_ok(),
        other => panic!("no lookup for {other}"),
    }
}

/// The first check text that rejects `id`, if any.
fn rejected(f: &Finished, id: &str) -> Option<String> {
    let r = rejections(id);
    messages(f).into_iter().find(|m| r.iter().any(|r| m.contains(r.as_str())))
}

/// Every check's text: its detail and, for missing evidence, why.
fn messages(f: &Finished) -> Vec<String> {
    f.outcomes
        .iter()
        .map(|o| match &o.evidence {
            Evidence::Missing { why } => format!("{}: {} {why}", o.name, o.detail),
            Evidence::Margin { .. } => format!("{}: {}", o.name, o.detail),
        })
        .collect()
}

#[test]
fn every_catalog_path_offers_options() {
    let d = pat();
    let paths = Balloon::catalog_paths();
    let names: std::collections::BTreeSet<_> = paths.iter().map(|(p, _)| *p).collect();
    assert_eq!(names.len(), paths.len(), "duplicate path");
    for (path, what) in &paths {
        assert!(!what.is_empty());
        let opts = Balloon::options(&d, path, None);
        assert!(!opts.is_empty(), "{path} offers nothing");
        for o in &opts {
            assert!(!o.id.is_empty() && !o.label.is_empty() && !o.url.is_empty(), "{path}: {o:?}");
        }
    }
    assert!(Balloon::options(&d, "no.such.path", None).is_empty());
}

#[test]
fn every_offered_id_round_trips_into_a_design() {
    let base = serde_json::to_value(pat()).unwrap();
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    for (path, _) in Balloon::catalog_paths() {
        for o in Balloon::options(&pat(), path, None) {
            let mut v = base.clone();
            set(&mut v, path, &o.id);
            let d: Design = serde_json::from_value(v).unwrap_or_else(|e| panic!("{path} = {}: does not deserialize: {e}", o.id));
            let f = Balloon::check(&spec, &d);
            assert!(!messages(&f).iter().any(|m| m.contains("analysis failed")), "{path} = {}: {:?}", o.id, messages(&f));
            assert!(accepts(path, &o.id), "{path} = {}: the catalog lookup rejects it", o.id);
            if let Some(m) = rejected(&f, &o.id) {
                panic!("{path} = {}: rejected: {m}", o.id);
            }
        }
    }
}

#[test]
fn the_round_trip_test_sees_a_bad_id() {
    // the rejection list above matches what the domain says
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let mut v = serde_json::to_value(pat()).unwrap();
    set(&mut v, "buses[].battery", "heater-pad");
    let d: Design = serde_json::from_value(v).unwrap();
    let f = Balloon::check(&spec, &d);
    assert!(rejected(&f, "heater-pad").is_some(), "{:?}", messages(&f));
    for (path, wrong) in [("trackers[].radio", "gopro-hero13"), ("trackers[].gps", "dra818v")] {
        let mut v = serde_json::to_value(pat()).unwrap();
        set(&mut v, path, wrong);
        let f = Balloon::check(&spec, &serde_json::from_value(v).unwrap());
        assert!(rejected(&f, wrong).is_some(), "{path} = {wrong}: {:?}", messages(&f));
    }
    assert!(!accepts("buses[].rails", "l91-aa") && !accepts("trackers[].gps", "dra818v"));
    // and an id the catalog does not have at all
    let mut v = serde_json::to_value(pat()).unwrap();
    set(&mut v, "parachute", "spherachute-99");
    let f = Balloon::check(&spec, &serde_json::from_value(v).unwrap());
    assert!(rejected(&f, "spherachute-99").is_some(), "{:?}", messages(&f));
}

#[test]
fn query_matches_id_or_label_ignoring_case() {
    let d = pat();
    let ids = |path: &str, q: &str| Balloon::options(&d, path, Some(q)).into_iter().map(|o| o.id).collect::<Vec<_>>();
    assert_eq!(ids("balloon", "TOTEX-1600"), ["totex-1600"]);
    assert!(ids("balloon", "hwoyee").contains(&"hwoyee-1600".to_string()));
    assert!(ids("parachute", "36\"").contains(&"spherachute-36".to_string()));
    assert!(ids("items[].part", "Camera").contains(&"gopro-hero13".to_string()));
    assert!(ids("items[].part", "heater").contains(&"heater-pad".to_string()));
    assert!(ids("items[].part", "Hand Warmer").contains(&"hothands-hw".to_string()));
    assert!(ids("buses[].rails", "buck").contains(&"lm2596-buck-4v0".to_string()));
    assert!(ids("trackers[].radio", "aprs").contains(&"lightaprs-2".to_string()));
    assert!(ids("trackers[].gps", "phone").contains(&"phone-backup".to_string()));
    assert!(ids("balloon", "no such balloon").is_empty());
    // a blank query lists everything
    assert_eq!(ids("balloon", "  ").len(), catalog().balloons.len());
}

#[test]
fn options_carry_the_specs_that_matter() {
    let d = pat();
    let has = |path: &str, keys: &[&str]| {
        for o in Balloon::options(&d, path, None) {
            for k in keys {
                assert!(o.specs.contains_key(*k), "{path} {}: no {k} in {:?}", o.id, o.specs);
            }
        }
    };
    has("balloon", &["burst_diameter_m", "mass_g"]);
    has("parachute", &["diameter_m", "cd"]);
    has("line", &["breaking_n"]);
    has("enclosure.insulation", &["k_w_mk"]);
    has("items[].part", &["temp_range_c", "mass_g"]);
    has("buses[].battery", &["voltage_v", "capacity_ah", "current_a_max"]);
    has("buses[].rails", &["vin_v", "vout_v", "current_a_max"]);
    has("trackers[].radio", &["link", "freq_mhz", "tx_dbm", "temp_range_c"]);
    has("trackers[].ground", &["link"]);
    // a bus-powered load says what it takes
    let esp = Balloon::options(&d, "items[].part", Some("esp32-devkit")).remove(0);
    assert!(esp.specs.contains_key("supply_v") && esp.specs.contains_key("current_a_max"), "{:?}", esp.specs);
}
