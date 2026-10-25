//! Can Wright design a VTOL that beats the 2025 130-mile 3D-printed build?
//!
//!     cargo run --release -p wright-air --example vtol_eval            # scoreboard
//!     cargo run --release -p wright-air --example vtol_eval -- --build # also the builder's files
//!
//! 1. The reference reproduced from its published parts and graded against
//!    its author's numbers (mass, hover throttle, range and endurance at the
//!    speed they flew), its unpublished chord swept +/- 20 %.
//! 2. Wright's design from a requirements-only spec under the same
//!    constraints (1.2 m span, Bambu A1, 6S, $2000 with the battery),
//!    judged by EVERY check (meshes and PrusaSlicer included).
//! 3. Both in one model at their best-range speeds with the same 15 %
//!    reserve, and again under a pessimistic model (parasite drag +20 %,
//!    powertrain 10 % worse).
//! 4. evals/vtol/result.md and result.json. Exit 0 only when Wright's design
//!    passes every check AND out-ranges the reference in both models.

use serde_json::json;
use std::fmt::Write as _;
use wright_air::powertrain::Factors;
use wright_air::reference::{self, MILE_KM, REPORTED};
use wright_air::{checks, perf, search, spec};
use wright_core::units::Speed;
use wright_core::Domain;

fn pct(model: f64, reported: f64) -> f64 {
    100.0 * (model - reported) / reported
}

fn main() {
    let build = std::env::args().any(|a| a == "--build");
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/vtol");
    let t0 = std::time::Instant::now();

    // 1. the reference reproduced, chord swept
    let v_flown = Speed::mps(reference::flown_speed_mps());
    let mut repro = vec![];
    for chord in [176.0, 220.0, 264.0] {
        let d = reference::sized(chord).expect("reference sizes");
        let e = checks::evaluate(&d, Factors::NOMINAL).expect("reference evaluates");
        let hover = e.perf.hover.std_full.as_ref().map(|o| o.throttle).unwrap_or(f64::NAN);
        let (p, t, km) = perf::at_speed(&e.layout, &e.perf, v_flown).expect("flies at the flown speed");
        repro.push(json!({
            "root_chord_mm": chord,
            "mass_kg": e.layout.mass().as_kg(),
            "hover_throttle": hover,
            "power_at_flown_speed_w": p.as_w(),
            "endurance_at_flown_speed_h": t.as_h(),
            "range_at_flown_speed_mi": km / MILE_KM,
            "best_range_km_reserve": e.perf.range_km,
            "errors_pct": {"mass": pct(e.layout.mass().as_kg(), REPORTED.mass_kg), "hover_throttle": pct(hover, REPORTED.hover_throttle),
                           "endurance": pct(t.as_h(), REPORTED.endurance_h), "range": pct(km / MILE_KM, REPORTED.range_miles)},
        }));
    }
    eprintln!("[{:?}] reference reproduced", t0.elapsed());

    // 2. Wright from requirements only
    // The headline is a like-for-like contest: BOTH aircraft may use parts
    // with lead time (the reference flies a made-to-order pack), stock policy
    // `lead_time`. `--stock listed` (or in_stock, restock) re-runs both under
    // another policy and writes result_<policy>.md/json.
    const HEADLINE: spec::StockPolicy = spec::StockPolicy::LeadTime;
    let args: Vec<String> = std::env::args().collect();
    let policy: spec::StockPolicy = args.iter().position(|a| a == "--stock").and_then(|i| args.get(i + 1)).map(|p| serde_json::from_value(json!(p)).expect("stock policy: in_stock, listed, lead_time or restock")).unwrap_or(HEADLINE);
    let mut requirements = reference::requirements();
    requirements.stock = policy;
    // `--span <mm>` lifts the 1.2 m span the reference was held to (what
    // span would a passing design need?); writes result_span<mm>[...].md
    let span: Option<f64> = args.iter().position(|a| a == "--span").and_then(|i| args.get(i + 1)).map(|s| s.parse().expect("--span <mm>"));
    if let Some(sp) = span {
        requirements.max_span_mm = sp;
    }
    // `--range <km>` states a range requirement; `--attack-altitude <m>`
    // adds a confirmed attack: stall_speed judged at that field altitude
    let num = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(|s| s.parse::<f64>().expect("a number"));
    if let Some(r) = num("--range") {
        requirements.range_km = Some(r);
    }
    let attack_alt = num("--attack-altitude");
    let screens: Vec<search::Screen> = attack_alt
        .map(|alt| {
            let mut r = requirements.clone();
            r.altitude_m = alt;
            vec![search::Screen { id: format!("altitude-{alt:.0}"), check: "stall_speed".into(), requirements: r }]
        })
        .unwrap_or_default();
    let spec = spec::Spec { name: "Wright VTOL (requirements-only search)".into(), requirements, design: Default::default() };
    let mut log = search::SearchLog { lines: vec![], evaluations: 0 };
    let wright = search::design_under(&spec, &screens, &mut log, &|m| eprintln!("[{:?}] {m}", t0.elapsed())).expect("a design");
    let w_checks = wright_air::Aircraft::check(&spec, &wright);
    eprintln!("[{:?}] Wright designed and checked", t0.elapsed());

    // 3. one model, best-range speeds, same reserve; then pessimistic. The
    // reference is taken at whichever swept chord flies furthest in EACH
    // model, so its unknown chord can only help it.
    let best_chord = |f: Factors| {
        [176.0, 220.0, 264.0]
            .into_iter()
            .map(|c| {
                let mut d = reference::sized(c).unwrap();
                d.requirements.stock = policy;
                (checks::evaluate(&d, f).unwrap().perf.range_km, c, d)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap()
    };
    let (_, chord_nom, ref_d) = best_chord(Factors::NOMINAL);
    let (_, chord_pes, ref_dp) = best_chord(Factors::PESSIMISTIC);
    let ref_checks = checks::check(&ref_d);
    let ev = |d: &spec::Design, f| checks::evaluate(d, f).unwrap();
    let (rn, wn) = (ev(&ref_d, Factors::NOMINAL), ev(&wright, Factors::NOMINAL));
    let (rp, wp) = (ev(&ref_dp, Factors::PESSIMISTIC), ev(&wright, Factors::PESSIMISTIC));
    let summary = |e: &checks::Evaluation| {
        let br = e.perf.best_range.as_ref();
        json!({
            "range_km_reserve": e.perf.range_km,
            "range_to_cutoff_mi": e.perf.range_to_cutoff_km / MILE_KM,
            "endurance_min_reserve": e.perf.endurance_min,
            "best_range_speed_ms": br.map(|b| b.speed.as_mps()),
            "cruise_power_w": br.map(|b| b.power.as_w()),
            "lift_to_drag": br.map(|b| b.trim.lift_to_drag()),
            "prop_efficiency": br.map(|b| b.point.prop_efficiency),
            "motor_efficiency": br.map(|b| b.point.motor_efficiency),
            "mass_kg": e.layout.mass().as_kg(),
            "battery": e.layout.battery.label,
            "battery_wh": e.layout.battery.pack.energy().as_wh(),
            "cost_usd": e.bom.total().as_usd(),
            "hover_throttle": e.perf.hover.std_full.as_ref().map(|o| o.throttle).ok(),
            "hover_current_per_motor_a": e.perf.hover.std_full.as_ref().map(|o| o.motor_a.as_a()).ok(),
            "stall_ms": e.perf.stall.as_mps(),
            "hover_tw": e.perf.hover.thrust_to_weight,
            "allowance_tw": search::Allowance::of(e).tw,
            "allowance_tw_limit": search::Allowance::of(e).tw_limit,
            "allowance_stall_ms": search::Allowance::of(e).stall_ms,
            "drag_areas_m2": br.map(|b| b.trim.areas),
        })
    };
    let better_nom = wn.perf.range_km > rn.perf.range_km;
    let better_pes = wp.perf.range_km > rp.perf.range_km;
    let passed = w_checks.passed() && better_nom && better_pes;
    let out = json!({
        "date": "2026-10-24",
        "stock_policy": serde_json::to_value(spec.requirements.stock).unwrap(),
        "reported": {"mass_kg": REPORTED.mass_kg, "hover_throttle": REPORTED.hover_throttle, "endurance_h": REPORTED.endurance_h,
                     "range_miles": REPORTED.range_miles, "flown_speed_ms": v_flown.as_mps(), "cost_usd": REPORTED.cost_usd},
        "reference_reproduction": repro,
        "reference": {"chord_nominal_mm": chord_nom, "chord_pessimistic_mm": chord_pes, "design": ref_d, "nominal": summary(&rn), "pessimistic": summary(&rp), "checks": ref_checks},
        "wright": {"design": wright, "nominal": summary(&wn), "pessimistic": summary(&wp), "checks": w_checks,
                   "search": {"evaluations": log.evaluations, "log": log.lines, "seconds": t0.elapsed().as_secs_f64()}},
        "verdict": {"wright_passes_every_check": w_checks.passed(), "beats_reference_nominal": better_nom,
                    "beats_reference_pessimistic": better_pes, "passed": passed,
                    "ratio_nominal": wn.perf.range_km / rn.perf.range_km, "ratio_pessimistic": wp.perf.range_km / rp.perf.range_km},
    });
    let mut stem = if policy == HEADLINE { "result".to_string() } else { format!("result_{}", serde_json::to_value(policy).unwrap().as_str().unwrap()) };
    if let Some(sp) = span {
        stem = format!("{stem}_span{sp:.0}");
    }
    if let Some(alt) = attack_alt {
        stem = format!("{stem}_alt{alt:.0}");
    }
    std::fs::write(here.join(format!("{stem}.json")), serde_json::to_string_pretty(&out).unwrap()).unwrap();
    let md = markdown(&out, &w_checks, &ref_checks);
    std::fs::write(here.join(format!("{stem}.md")), &md).unwrap();
    println!("{md}");
    if build {
        let dir = here.join("out/wright");
        match wright_air::Aircraft::build(&out_design(&out), &dir) {
            Ok(a) => a.files.iter().for_each(|(l, p)| eprintln!("  {l}: {}", p.display())),
            Err(e) => eprintln!("build failed: {e}"),
        }
    }
    std::process::exit(if passed { 0 } else { 1 });
}

/// Reference failures that come from what Wright had to reconstruct for
/// it (items its author never published), not from the build: graded in
/// the reference's favour.
fn excuse(check: &str) -> Option<&'static str> {
    Some(match check {
        "servo_fit" => "the wing chord is unpublished; the servo fails only at the chord Wright swept",
        "interference" => "the pod and joiner shapes are Wright's reconstruction, not the author's parts",
        "lift_prop_mount" => "the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop",
        "pusher_prop_mount" => "the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop",
        _ => return None,
    })
}

fn out_design(o: &serde_json::Value) -> spec::Design {
    serde_json::from_value(o["wright"]["design"].clone()).unwrap()
}

fn markdown(o: &serde_json::Value, w: &wright_core::Finished, r: &wright_core::Finished) -> String {
    let f = |v: &serde_json::Value, d: usize| v.as_f64().map(|x| format!("{x:.d$}")).unwrap_or("-".into());
    let v = &o["verdict"];
    let mut s = String::new();
    let _ = writeln!(s, "# VTOL eval: Wright vs the 2025 130-mile 3D-printed VTOL\n");
    let _ = writeln!(
        s,
        "**Verdict: {}.** Wright's design {} ({} checks, {} failing); range with 15 % reserve x{:.3} nominal, x{:.3} pessimistic.\n",
        if v["passed"].as_bool() == Some(true) { "PASS" } else { "FAIL" },
        if w.passed() { "passes every check" } else { "does NOT pass every check" },
        w.outcomes.len(),
        w.failures().count(),
        v["ratio_nominal"].as_f64().unwrap_or(0.0),
        v["ratio_pessimistic"].as_f64().unwrap_or(0.0)
    );
    let _ = writeln!(s, "Stock policy: `{}` (`listed`: bought parts in stock, or listed for sale with stock unstated; made to order, pre-order, backorder and out of stock refused. `lead_time` also accepts made to order / pre-order / backorder). Both aircraft are judged under it.\n", o["stock_policy"].as_str().unwrap_or("?"));
    let _ = writeln!(s, "Run: `cargo run --release -p wright-air --example vtol_eval` (add `-- --build` for CAD, G-code, BOM, params, checklist).\n");
    let _ = writeln!(s, "## 1. Reference reproduced from its published parts\n");
    let _ = writeln!(s, "Graded against the author's report; the wing chord was never published, so it is swept. Range and endurance are to cutoff at the {:.1} m/s the pilot averaged (130 mi in 3 h).\n", o["reported"]["flown_speed_ms"].as_f64().unwrap_or(0.0));
    let _ = writeln!(s, "| root chord | mass kg | hover throttle | endurance at flown speed | range at flown speed |\n|---|---|---|---|---|");
    for row in o["reference_reproduction"].as_array().unwrap() {
        let e = &row["errors_pct"];
        let _ = writeln!(
            s,
            "| {} mm | {} ({:+.0} %) | {} ({:+.0} %) | {} h ({:+.0} %) | {} mi ({:+.0} %) |",
            f(&row["root_chord_mm"], 0),
            f(&row["mass_kg"], 3),
            e["mass"].as_f64().unwrap_or(0.0),
            f(&row["hover_throttle"], 2),
            e["hover_throttle"].as_f64().unwrap_or(0.0),
            f(&row["endurance_at_flown_speed_h"], 2),
            e["endurance"].as_f64().unwrap_or(0.0),
            f(&row["range_at_flown_speed_mi"], 1),
            e["range"].as_f64().unwrap_or(0.0)
        );
    }
    let rep = &o["reported"];
    let _ = writeln!(s, "| **reported** | {} | {} | {} h | {} mi |\n", f(&rep["mass_kg"], 2), f(&rep["hover_throttle"], 2), f(&rep["endurance_h"], 1), f(&rep["range_miles"], 0));
    let _ = writeln!(s, "## 2-3. Same model, same constraints, best-range speeds, 15 % reserve\n");
    let (rn, wn, rp, wp) = (&o["reference"]["nominal"], &o["wright"]["nominal"], &o["reference"]["pessimistic"], &o["wright"]["pessimistic"]);
    let _ = writeln!(s, "The reference is taken at the swept chord that flies furthest in each model ({} mm nominal, {} mm pessimistic), so its unpublished chord can only help it.\n", o["reference"]["chord_nominal_mm"], o["reference"]["chord_pessimistic_mm"]);
    let _ = writeln!(s, "| | reference | Wright |\n|---|---|---|");
    for (label, key, d) in [
        ("range with reserve, km", "range_km_reserve", 1),
        ("range to cutoff, mi", "range_to_cutoff_mi", 1),
        ("endurance with reserve, min", "endurance_min_reserve", 0),
        ("best-range speed, m/s", "best_range_speed_ms", 1),
        ("cruise power from the pack, W", "cruise_power_w", 1),
        ("L/D", "lift_to_drag", 2),
        ("pusher prop efficiency", "prop_efficiency", 3),
        ("pusher motor efficiency", "motor_efficiency", 3),
        ("take-off mass, kg", "mass_kg", 3),
        ("battery energy, Wh", "battery_wh", 0),
        ("cost with battery (BOM), USD (information: no budget on either side)", "cost_usd", 0),
        ("hover throttle", "hover_throttle", 2),
        ("predicted hover current per motor, A", "hover_current_per_motor_a", 1),
        ("stall, m/s", "stall_ms", 1),
        ("hover T/W (check: at least 1.8)", "hover_tw", 2),
        ("search allowance: T/W, pessimistic powertrain, less thrust-model error", "allowance_tw", 2),
        ("search allowance: stall with the airfoil's measured CLmax error, m/s", "allowance_stall_ms", 1),
    ] {
        let _ = writeln!(s, "| {label} | {} | {} |", f(&rn[key], d), f(&wn[key], d));
    }
    let _ = writeln!(s, "| **pessimistic** range with reserve, km | {} | {} |", f(&rp["range_km_reserve"], 1), f(&wp["range_km_reserve"], 1));
    let excused: Vec<(&str, &str)> = r.failures().filter_map(|o| excuse(&o.name).map(|why| (o.name.as_str(), why))).collect();
    let _ = writeln!(s, "| checks failing | {} (+{} excused, below) | {} |\n", r.failures().count() - excused.len(), excused.len(), w.failures().count());
    if !excused.is_empty() {
        let _ = writeln!(s, "Reference failures graded in its favour (not counted):\n");
        for (n, why) in excused {
            let _ = writeln!(s, "- `{n}`: {why}");
        }
        let _ = writeln!(s);
    }
    let wd = &o["wright"]["design"];
    let _ = writeln!(s, "## Wright's design\n");
    let _ = writeln!(
        s,
        "- wing: {} mm span, {}/{} mm chords, {}\n- lift: 4 x {} with {}, ESC {}\n- pusher: {} with {}, ESC {}\n- battery: {}\n- avionics: {}, {}, {}, airspeed {}, servos {}\n- structure: spar {} + joiner {}, booms {}, legs {} mm\n",
        wd["wing"]["span_mm"], wd["wing"]["root_chord_mm"], wd["wing"]["tip_chord_mm"], wd["wing"]["airfoil"],
        wd["propulsion"]["lift_motor"], wd["propulsion"]["lift_prop"], wd["propulsion"]["lift_esc"],
        wd["propulsion"]["cruise_motor"], wd["propulsion"]["cruise_prop"], wd["propulsion"]["cruise_esc"],
        wn["battery"], wd["avionics"]["fc"], wd["avionics"]["gps"], wd["avionics"]["receiver"], wd["avionics"]["airspeed"], wd["avionics"]["servo"],
        wd["structure"]["main_spar"], wd["structure"]["joiner"], wd["structure"]["boom_tube"], wd["structure"]["leg_thickness_mm"]
    );
    let srch = &o["wright"]["search"];
    let _ = writeln!(s, "Search: {} whole-model evaluations in {:.0} s.\n", srch["evaluations"], srch["seconds"].as_f64().unwrap_or(0.0));
    for l in srch["log"].as_array().unwrap() {
        let _ = writeln!(s, "- {}", l.as_str().unwrap_or(""));
    }
    let _ = writeln!(s, "\n## Wright's checks\n\n```\n{w}\n```\n");
    let _ = writeln!(s, "## The reference's checks (informational)\n\n```\n{r}\n```");
    s
}
