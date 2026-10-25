use wright_balloon::catalog::catalog;
use wright_balloon::{checks, design, reference, Balloon};
use wright_core::{Domain, Evidence};

fn pat() -> wright_balloon::Design {
    let cat = catalog();
    let mut d = reference::pat_design();
    let wall = d.enclosure.wall_mm;
    design::size_enclosure(&mut d, cat, wall).unwrap();
    design::size_fill(&mut d, cat).unwrap();
    d
}

#[test]
fn pats_plan_fails_for_stated_reasons() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let (f, a) = checks::evaluate(&spec, &pat(), catalog());
    assert!(a.is_some());
    let failing: Vec<_> = f.failures().map(|o| o.name.as_str()).collect();
    for must in ["landing_speed", "part_temperature", "launch_window", "link_secondary"] {
        assert!(failing.contains(&must), "{must} should fail; failing = {failing:?}");
    }
    // the DRA818V is fed in range by the plan's own buck converter (in its favour)
    assert!(f.get("supply_voltage").unwrap().passed());
    // the ELRS link has no ground transmitter: missing evidence, not a margin
    assert!(matches!(f.get("link_secondary").unwrap().evidence, Evidence::Missing { .. }));
    // Part 101: under every clause
    for ok in ["faa101_package", "faa101_package_max", "faa101_total_payload", "faa101_suspension"] {
        assert!(f.get(ok).unwrap().passed(), "{ok}");
    }
}

#[test]
fn removing_every_tracker_cannot_pass_the_link_checks() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let mut d = pat();
    d.trackers.clear();
    let (f, _) = checks::evaluate(&spec, &d, catalog());
    for c in ["link_primary", "link_secondary", "gps_altitude", "gps_coexistence"] {
        assert!(!f.get(c).unwrap().passed(), "{c} passed with no trackers");
    }
}

#[test]
fn an_unknown_part_fails_cost_and_analysis_instead_of_reading_zero() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let mut d = pat();
    d.items[0].part = "no-such-part".into();
    let f = Balloon::check(&spec, &d);
    assert!(!f.passed());
    assert!(f.outcomes.iter().all(|o| !o.passed()), "an unknown part must fail every check");
}

/// Searches run one at a time: they are the slow tests, and the temp-file
/// test must see only its own run.
static SEARCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn balloon_temp_entries() -> std::collections::BTreeSet<String> {
    std::fs::read_dir(std::env::temp_dir())
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("wright-balloon")).collect())
        .unwrap_or_default()
}

#[test]
fn the_same_spec_designs_the_same_balloon_and_it_passes() {
    let _g = SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    // Requirements-only: the eval's spec, with Pat's budget scale.
    let spec = reference::mission_spec(33_070.0, 1_209.43);
    let before = balloon_temp_entries();
    let a = Balloon::design(&spec).unwrap();
    // the search sliced in a scratch dir and removed it
    let after = balloon_temp_entries();
    assert!(after.is_subset(&before), "a design left temp files: {:?}", after.difference(&before).collect::<Vec<_>>());
    assert!(!after.contains("wright-balloon-slice"), "the old shared slice dir is still there");
    let b = Balloon::design(&spec).unwrap();
    assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    let f = Balloon::check(&spec, &a);
    // the rack is sliced only when PrusaSlicer is installed
    let fails: Vec<_> = f.failures().map(|o| o.name.clone()).filter(|n| n != "printed_parts").collect();
    assert!(fails.is_empty(), "{f}");
}

/// Wright's eval kit set, sized by hand (no search), with its thermal
/// decisions declared.
fn wright_like(warmers: u32) -> wright_balloon::Design {
    let cat = catalog();
    let ids = ["cam-runcam5", "aprs-lightaprs2", "lora-heltec"];
    let ks: Vec<_> = ids.iter().map(|id| design::kits().iter().find(|k| &k.id == id).unwrap()).collect();
    let mut d = design::with_warmers(&design::assemble("test", &ks, &["aprs-ham-ht".to_string()]), warmers);
    d.balloon = "totex-2000".into();
    d.parachute = "spherachute-60".into();
    d.gas = "he-uhp-300".into();
    d.fill_target_mps = 5.5;
    design::size_enclosure(&mut d, cat, 38.1).unwrap();
    design::size_fill(&mut d, cat).unwrap();
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    design::declare_thermal(&spec, &mut d, cat).unwrap();
    d
}

#[test]
fn trackers_switched_off_at_burst_get_no_recovery_credit() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let mut d = wright_like(0);
    let (f0, a0) = checks::evaluate(&spec, &d, catalog());
    assert!(a0.unwrap().recovery_probability > 0.85, "{f0}");
    for it in d.items.iter_mut() {
        if it.part == "lightaprs-2" || it.part == "heltec-lora32-v3" {
            it.until_burst = true;
        }
    }
    let (f, a) = checks::evaluate(&spec, &d, catalog());
    let a = a.unwrap();
    assert!(a.recovery_probability < 1e-9, "P = {}", a.recovery_probability);
    for c in ["link_primary", "link_secondary", "recovery_probability"] {
        assert!(!f.get(c).unwrap().passed(), "{c} passed with trackers off on the descent");
    }
}

#[test]
fn a_tracker_without_its_gps_gets_no_credit_and_fails_gps_altitude() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let mut d = wright_like(0);
    d.items.retain(|it| it.part != "matek-m10q");
    let (f, a) = checks::evaluate(&spec, &d, catalog());
    let lora = a.unwrap().trackers.into_iter().find(|t| t.name.contains("LoRa")).unwrap();
    assert_eq!(lora.p_recover, 0.0, "{}", lora.why);
    assert!(!f.get("gps_altitude").unwrap().passed());
    assert!(!f.get("link_secondary").unwrap().passed());
}

#[test]
fn launch_window_is_declared_checked_and_bounds_the_forecast() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let d = wright_like(0);
    let [lo, hi] = d.launch_window_c.expect("declared");
    assert!(lo <= -10.0 && hi >= 35.0, "window {lo}..{hi} misses the default range");
    let (f, _) = checks::evaluate(&spec, &d, catalog());
    assert!(f.get("launch_window").unwrap().passed(), "{f}");
    assert!(f.get("part_temperature").unwrap().passed(), "{f}");
    // undeclared: missing evidence, not a pass
    let mut u = d.clone();
    u.launch_window_c = None;
    let (fu, _) = checks::evaluate(&spec, &u, catalog());
    assert!(matches!(fu.get("launch_window").unwrap().evidence, Evidence::Missing { .. }));
    // declaring wider than the model supports fails
    let mut w = d.clone();
    w.launch_window_c = Some([lo - 15.0, hi]);
    let (fw, _) = checks::evaluate(&spec, &w, catalog());
    assert!(!fw.get("launch_window").unwrap().passed());
    // a forecast inside the window is judged; one outside it is a no-go day
    let mut s_in = spec.clone();
    s_in.ground_temperature_c = Some(lo + 1.0);
    let (fi, ai) = checks::evaluate(&s_in, &d, catalog());
    assert!(ai.unwrap().thermal_cases.iter().any(|c| (c.ground_c - (lo + 1.0)).abs() < 1e-9));
    assert!(fi.get("part_temperature").unwrap().passed());
    let mut s_out = spec.clone();
    s_out.ground_temperature_c = Some(hi + 3.0);
    let (fo, ao) = checks::evaluate(&s_out, &d, catalog());
    assert!(ao.unwrap().thermal_cases.iter().all(|c| c.ground_c <= hi));
    assert!(fo.get("part_temperature").unwrap().detail.contains("no-go"));
}

#[test]
fn a_narrow_spec_range_that_the_design_cannot_meet_fails() {
    // a 45 C day at the hot end: the box cannot be cooler than the air
    let mut spec = reference::mission_spec(30_000.0, 5_000.0);
    spec.launch_temp_range_c = Some([0.0, 45.0]);
    let (f, _) = checks::evaluate(&spec, &wright_like(0), catalog());
    assert!(!f.get("part_temperature").unwrap().passed());
    assert!(!f.get("launch_window").unwrap().passed());
}

#[test]
fn hand_warmer_heat_is_derived_and_falls_with_pressure() {
    let w = wright_balloon::rules::warmer_power_sl_w(10.0);
    assert!((w - 1.37).abs() < 0.01, "{w} W");
    // opened warmers warm a cold day's box
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let d = wright_like(3);
    let cat = catalog();
    let m = wright_balloon::mass::masses(&d, cat).unwrap();
    let fl = wright_balloon::flight::simulate(&spec, &d, cat, &m).unwrap();
    let on = wright_balloon::Design { warmers_below_c: None, ..d.clone() };
    let off = wright_balloon::Design { warmers_below_c: Some(f64::NEG_INFINITY), ..d.clone() };
    let c_on = wright_balloon::analysis::ThermalModel::new(&on, cat, &m, &fl).unwrap().case(-20.0).unwrap();
    let c_off = wright_balloon::analysis::ThermalModel::new(&off, cat, &m, &fl).unwrap().case(-20.0).unwrap();
    assert!(c_on.warmers_on && !c_off.warmers_on);
    assert!(c_on.box_min_c > c_off.box_min_c + 1.0, "{} vs {}", c_on.box_min_c, c_off.box_min_c);
    // less than full sea-level output over the flight: pressure cuts it
    let hours = (wright_balloon::rules::PRELAUNCH_MIN * 60.0 + fl.samples.last().unwrap().t_s) / 3600.0;
    assert!(c_on.warmer_wh < 3.0 * w * hours);
}

#[test]
fn the_camera_sees_out_of_its_port_and_a_blocked_port_fails_imaging() {
    let spec = reference::mission_spec(30_000.0, 5_000.0);
    let d = wright_like(0);
    let cat = catalog();
    let (f, a) = checks::evaluate(&spec, &d, cat);
    let a = a.unwrap();
    let v = wright_balloon::geometry::camera_view(&d, &a.layout, "runcam5-orange#1").unwrap();
    assert!(v.gap_mm < 1e-6 && v.blocked_by.is_empty(), "{v:?}");
    assert!(f.get("imaging").unwrap().passed(), "{f}");
    // move the camera 97 mm back from the wall: the port crops it
    let mut l = a.layout.clone();
    let cam = l.placed.iter_mut().find(|p| p.name == "runcam5-orange#1").unwrap();
    cam.min[0] -= 97.0;
    let v2 = wright_balloon::geometry::camera_view(&d, &l, "runcam5-orange#1").unwrap();
    assert!(v2.clear_half_deg < wright_balloon::rules::VIEW_HALF_DEG, "{v2:?}");
}

#[test]
fn every_flight_buys_fill_gear_rental_harness_and_cards() {
    let b = Balloon::bom(&wright_like(0));
    for id in ["rocketman-inflator", "cylinder-rental", "rocketman-harness-swivel", "microsd-64gb"] {
        assert!(b.lines.iter().any(|l| l.id == id), "{id} missing from the BOM");
    }
}

/// The stored thermal attacks (data/regressions-balloon.jsonl) as conditions.
/// A live run may have set a case aside into data/regressions-rejected.jsonl
/// (the harness moves cases the domain refuses, e.g. r2-1 at -15 C); those
/// are read back here so the test sees the same three cases either way.
fn stored_conditions(spec: &wright_balloon::Spec) -> Vec<wright_core::Condition<wright_balloon::Spec>> {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut text = std::fs::read_to_string(data.join("regressions-balloon.jsonl")).unwrap();
    if let Ok(rejected) = std::fs::read_to_string(data.join("regressions-rejected.jsonl")) {
        text.push('\n');
        text.push_str(&rejected);
    }
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|r| r["domain"] == "balloon")
        .map(|r| {
            let mut v = serde_json::to_value(spec).unwrap();
            for (k, x) in r["spec_patch"].as_object().unwrap() {
                v[k] = x.clone();
            }
            wright_core::Condition { id: r["id"].as_str().unwrap().into(), check: r["check"].as_str().unwrap().into(), claim: r["claim"].as_str().unwrap().into(), spec: serde_json::from_value(v).unwrap() }
        })
        .collect()
}

#[test]
fn design_under_the_stored_thermal_attacks_passes_them_or_says_why_not() {
    let _g = SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    let spec = reference::mission_spec(30_000.0, 1_500.0);
    let all = stored_conditions(&spec);
    assert_eq!(all.len(), 3);
    // the harness sets aside stored cases the domain refuses: r2-1 (-15 C)
    // is colder than the San Joaquin Valley has ever been (-11.1 C)
    let (conds, refused): (Vec<_>, Vec<_>) = all.into_iter().partition(|c| Balloon::validate_spec(&c.spec).is_ok());
    assert_eq!(refused.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["r2-1"]);
    let e = Balloon::design_under(&spec, &refused).unwrap_err().to_string();
    assert!(e.contains("r2-1") && e.contains("ground_temperature_c"), "{e}");
    let d = Balloon::design_under(&spec, &conds).unwrap();
    for c in &conds {
        let f = Balloon::check(&c.spec, &d);
        let o = f.get(&c.check).unwrap();
        assert!(o.passed(), "{} under {}: {}", c.check, c.id, o.detail);
    }
    let f = Balloon::check(&spec, &d);
    assert!(f.failures().all(|o| o.name == "printed_parts"), "{f}");
    // an attack no balloon can meet: a 45 C launch range. The search says so
    // and names it, instead of returning a design that fails it
    let mut hot = spec.clone();
    hot.launch_temp_range_c = Some([0.0, 45.0]);
    assert!(Balloon::validate_spec(&hot).is_ok(), "45 C is inside the 47.8 C record");
    let impossible = vec![wright_core::Condition { id: "t-hot".into(), check: "part_temperature".into(), claim: "a 45 C day".into(), spec: hot }];
    let e = Balloon::design_under(&spec, &impossible).unwrap_err().to_string();
    assert!(e.contains("t-hot") && e.contains("part_temperature"), "{e}");
}

#[test]
fn spec_envelope_has_bounds_and_refuses_implausible_specs() {
    let schema = Balloon::spec_schema();
    let p = &schema["properties"];
    let bounded = |v: &serde_json::Value| (v.get("minimum").is_some() || v.get("exclusiveMinimum").is_some()) && v.get("maximum").is_some();
    for k in ["min_burst_altitude_m", "recovery_window_h", "min_recovery_probability", "ground_temperature_c"] {
        assert!(bounded(&p[k]), "{k} has no bounds");
    }
    for k in ["lat_deg", "lon_deg", "elevation_m"] {
        assert!(bounded(&p["launch"]["properties"][k]), "launch.{k} has no bounds");
    }
    assert!(bounded(&p["launch_temp_range_c"]["items"]));
    for k in ["alt_m", "speed_mps", "toward_deg"] {
        assert!(bounded(&p["wind"]["items"]["properties"][k]), "wind.{k} has no bounds");
    }
    let ok = reference::mission_spec(30_000.0, 1_500.0);
    Balloon::validate_spec(&ok).unwrap();
    let bad = |f: &dyn Fn(&mut wright_balloon::Spec), word: &str| {
        let mut s = ok.clone();
        f(&mut s);
        let e = Balloon::validate_spec(&s).unwrap_err().to_string();
        assert!(e.contains(word), "{word}: {e}");
        assert!(Balloon::design(&s).is_err());
    };
    bad(&|s| s.ground_temperature_c = Some(55.0), "ground_temperature_c");
    bad(&|s| s.launch_temp_range_c = Some([30.0, 0.0]), "low end above");
    bad(&|s| s.launch.elevation_m = 3_500.0 + 2_000.0, "elevation_m");
    bad(&|s| s.min_burst_altitude_m = 45_000.0, "min_burst_altitude_m");
    bad(&|s| {
        s.launch.elevation_m = 4_000.0;
        s.min_burst_altitude_m = 4_500.0;
    }, "min_burst_altitude_m");
    bad(&|s| {
        s.launch.elevation_m = 4_000.0;
        s.min_burst_altitude_m = 10_000.0;
        s.launch.elevation_m = 9_500.0;
    }, "1 km above");
    bad(&|s| s.budget_usd = Some(f64::NAN), "budget_usd");
    // no budget, a null budget, and a large one all load
    for b in [serde_json::json!(null), serde_json::json!(100_000)] {
        let mut v = serde_json::to_value(&ok).unwrap();
        v["budget_usd"] = b;
        let s: wright_balloon::Spec = serde_json::from_value(v).unwrap();
        Balloon::validate_spec(&s).unwrap();
    }
    let mut v = serde_json::to_value(&ok).unwrap();
    v.as_object_mut().unwrap().remove("budget_usd");
    let s: wright_balloon::Spec = serde_json::from_value(v).unwrap();
    assert!(s.budget_usd.is_none());
    bad(&|s| s.min_recovery_probability = 0.99, "P_SITE");
    bad(&|s| s.wind = vec![wright_balloon::model::WindPoint { alt_m: 0.0, speed_mps: 200.0, toward_deg: 90.0 }], "speed_mps");
}

#[test]
fn rack_shelf_landing_is_closed_form_with_an_explicit_stop() {
    let cat = catalog();
    let d = wright_like(0);
    assert!(d.enclosure.printed_tray);
    let spec = reference::mission_spec(30_000.0, None);
    let a = wright_balloon::analysis::analyse(&spec, &d, cat).unwrap();
    let s = checks::rack_shelf_landing(&d, cat, &a).expect("a post-carried shelf");
    // soft ground: stopped over the whole wall, a = v^2 / 2 d, plus 1 g
    let g0 = wright_core::units::G0;
    let v = a.flight.landing_speed_mps;
    assert!((s.field_stop_mm - d.enclosure.wall_mm).abs() < 1e-9);
    assert!((s.field_g - (v * v / (2.0 * d.enclosure.wall_mm / 1e3) / g0 + 1.0)).abs() < 1e-9);
    // stress is linear in the load factor, and hard ground is far worse
    assert!((s.field_mpa / s.field_g - s.hard_mpa / s.hard_g).abs() < 1e-9);
    assert!(s.hard_stop_mm < s.field_stop_mm && s.hard_g > s.field_g);
    // statics by hand: W L / 8 over b t^2, times the peak factor
    let w_n = s.load_g / 1e3 * g0 * s.field_g;
    let hand = wright_balloon::rules::SHELF_PEAK_FACTOR * 6.0 * (w_n * s.span_mm / 8.0) / (s.width_mm * 9.0);
    assert!((s.field_mpa - hand).abs() < 1e-9);
    let f = Balloon::check(&spec, &d);
    let o = f.get("rack_shelf_landing").unwrap();
    assert!(o.passed(), "{}", o.detail);
    assert!(o.detail.contains("Hard ground"), "{}", o.detail);
}
