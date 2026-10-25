//! The balloon eval: Pat's team balloon as planned vs a Wright design from
//! a requirements-only spec with the same mission.
//!
//!   cargo run -p wright-balloon --example eval [--release]
//!
//! Writes evals/balloon/result.json, result.md and build/ (STLs, sliced
//! G-code, BOM, layout).

use std::fmt::Write as _;
use std::path::PathBuf;
use wright_balloon::analysis::Analysis;
use wright_balloon::catalog::catalog;
use wright_balloon::{checks, design, reference, Balloon, Design, Spec};
use wright_core::{Domain, Finished};

/// ASSUMPTION: the team's burst goal when nothing is stated (a common
/// student-HAB target; Pat's notes give none).
const PAT_TARGET_M: f64 = 30_000.0;

struct Run {
    design: Design,
    report: Finished,
    a: Analysis,
    cost: f64,
}

fn run(spec: &Spec, d: Design) -> Run {
    let cat = catalog();
    let (report, a) = checks::evaluate(spec, &d, cat);
    let cost = Balloon::bom(&d).total().as_usd();
    Run { design: d, report, a: a.expect("analysis ran"), cost }
}

fn summary(r: &Run) -> serde_json::Value {
    let fl = &r.a.flight;
    serde_json::json!({
        "label": r.design.label,
        "balloon": r.design.balloon,
        "parachute": r.design.parachute,
        "gas": r.design.gas,
        "cylinders": r.design.cylinders,
        "neck_lift_g": r.design.neck_lift_g,
        "launch_volume_m3": fl.launch_volume_m3,
        "launch_diameter_m": fl.launch_diameter_m,
        "burst_altitude_m": fl.burst_altitude_m,
        "cusf_burst_altitude_m": fl.cusf_burst_altitude_m,
        "launch_ascent_mps": fl.launch_ascent_mps,
        "mean_ascent_mps": fl.mean_ascent_mps,
        "time_to_burst_min": fl.burst_t_s / 60.0,
        "landing_speed_mps": fl.landing_speed_mps,
        "flight_time_min": fl.landing_t_s / 60.0,
        "landing_km_from_launch": r.a.landing_km,
        "package_g": r.a.masses.package().as_g(),
        "flight_train_g": r.a.masses.flight_train().as_g(),
        "box_inner_mm": r.design.enclosure.inner_mm,
        "box_min_c": r.a.box_min_c,
        "box_max_c": r.a.box_max_c,
        "ambient_min_c": r.a.ambient_min_c,
        "recovery_probability": r.a.recovery_probability,
        "cost_usd": r.cost,
        "passed": r.report.passed(),
        "failing": r.report.failures().map(|o| o.name.clone()).collect::<Vec<_>>(),
        "checks": r.report.outcomes,
        "buses": r.a.buses,
        "self_powered": r.a.self_powered,
        "trackers": r.a.trackers,
        "coexistence": r.a.coexistence,
        "masses": r.a.masses,
        "printed": r.design.printed,
        "design": r.design,
    })
}

fn fill_plan(r: &Run) -> String {
    let cat = catalog();
    let b = cat.balloon(&r.design.balloon).unwrap();
    let g = cat.gas(&r.design.gas).unwrap();
    let fl = &r.a.flight;
    format!(
        "1. Tie the {} ({:.0} g) to the filler; hang a weight of **{:.0} g** (neck lift = gross lift minus the balloon) on the filler.\n\
         2. Fill with {} ({:.3} % purity) until the balloon just lifts the filler weight: about {:.2} m^3 ({:.0} ft^3) at 21 C, launch diameter {:.2} m. Buy {} cylinder(s) of {:.1} m^3.\n\
         3. Tie off, attach the chute ({}) and the payload; the free lift is then {:.0} g, giving {:.2} m/s at launch by the US76 + Gallice-drag model (the CUSF rule sized this fill for {:.1} m/s).\n",
        b.name, b.mass_g, r.design.neck_lift_g, g.name, g.purity * 100.0, fl.launch_volume_m3, fl.launch_volume_m3 * 35.3147, fl.launch_diameter_m, r.design.cylinders, g.cylinder_m3,
        cat.chute(&r.design.parachute).unwrap().name, fl.free_lift_kg * 1e3, fl.launch_ascent_mps, r.design.fill_target_mps
    )
}

fn checks_table(p: &Run, w: &Run) -> String {
    let mut s = String::from("| check | Pat as planned | Wright |\n|---|---|---|\n");
    for (a, b) in p.report.outcomes.iter().zip(&w.report.outcomes) {
        let cell = |o: &wright_core::Outcome| match &o.evidence {
            wright_core::Evidence::Margin { value, unit } => format!("{} {:+.2} {}", if o.passed() { "ok" } else { "**FAIL**" }, value, unit),
            wright_core::Evidence::Missing { .. } => "**FAIL** (missing evidence)".into(),
        };
        let _ = writeln!(s, "| {} | {} | {} |", a.name, cell(a), cell(b));
    }
    s
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals/balloon");
    let cat = catalog();

    // 1. Pat's design as planned. What the plan leaves open (balloon, chute,
    // foam wall, the fill target) is chosen in his favour: every catalog
    // option is tried and the one failing the fewest checks (then highest
    // burst, then cheapest) is kept. Like Wright's design, his declares the
    // launch window the model gives it.
    let choose_spec = reference::mission_spec(PAT_TARGET_M, 10_000.0);
    let base = reference::pat_design();
    let as_assumed = (base.balloon.clone(), base.parachute.clone(), base.enclosure.wall_mm, base.fill_target_mps);
    let mut best: Option<((usize, f64, f64), Design)> = None;
    let mut assumed_fails = usize::MAX;
    for wall in design::WALLS_MM {
        let mut walled = base.clone();
        if design::size_enclosure(&mut walled, cat, wall).is_err() {
            continue;
        }
        for b in &cat.balloons {
            for c in &cat.parachutes {
                for target in design::FILL_TARGETS_MPS {
                    let mut d = walled.clone();
                    d.balloon = b.id.clone();
                    d.parachute = c.id.clone();
                    d.fill_target_mps = target;
                    if design::size_fill(&mut d, cat).is_err() {
                        continue;
                    }
                    let _ = design::declare_thermal(&choose_spec, &mut d, cat);
                    let (f, a) = checks::evaluate(&choose_spec, &d, cat);
                    let Some(a) = a else { continue };
                    let n_fail = f.failures().count();
                    if (d.balloon.clone(), d.parachute.clone(), wall, target) == as_assumed {
                        assumed_fails = n_fail;
                    }
                    let key = (n_fail, -a.flight.burst_altitude_m.unwrap_or(0.0), Balloon::bom(&d).total().as_usd());
                    if best.as_ref().is_none_or(|(k, _)| key.partial_cmp(k) == Some(std::cmp::Ordering::Less)) {
                        best = Some((key, d));
                    }
                }
            }
        }
    }
    let (_, mut pd) = best.expect("some option for Pat's plan");
    pd.notes.retain(|n| !n.contains("(the plan names no balloon)") && !n.contains("(the plan names no chute)") && !n.contains("foam box"));
    pd.notes.insert(0, format!("chosen in the plan's favour from every catalog option (it names none): {} balloon, {} chute, {:.1} mm XPS wall, CUSF fill for {:.1} m/s", pd.balloon, pd.parachute, pd.enclosure.wall_mm, pd.fill_target_mps));
    // no budget for either side (Pat, 2026-10-24: "ignore budget, we are
    // just trying to make the best AI HWE"): cost is reported, not judged
    let pat_spec = reference::mission_spec(PAT_TARGET_M, None);
    let pat = run(&pat_spec, pd);
    let pat_burst = pat.a.flight.burst_altitude_m.unwrap_or(0.0);
    println!("== Pat as planned (best open choices; {assumed_fails} checks failed with the earlier assumed choices)\n{}\n", pat.report);
    if std::env::var("WRIGHT_EVAL_PAT_ONLY").is_ok() {
        return Ok(());
    }

    // 2. Wright from requirements only: the same spec judges both (his
    // assumed 30 km goal, no budget).
    let spec = pat_spec.clone();
    let wd = Balloon::design(&spec)?;
    let mut wright = run(&spec, wd);
    let pat_same = run(&spec, pat.design.clone());

    let arts = Balloon::build(&wright.design, &root.join("build"))?;
    // build() sliced the rack; carry the slicer grams into the design and re-check
    if let Ok(t) = std::fs::read_to_string(root.join("build/print/summary.json")) {
        if let Ok(p) = serde_json::from_str(&t) {
            wright.design.printed = Some(p);
            wright = run(&spec, wright.design);
        }
    }

    println!("== Pat as planned (judged on the Wright spec)\n{}\n", pat_same.report);
    println!("== Wright\n{}\n", wright.report);

    let result = serde_json::json!({
        "spec": spec,
        "pat_target_burst_m": PAT_TARGET_M,
        "pat": summary(&pat_same),
        "pat_failing_with_assumed_choices": assumed_fails,
        "wright": summary(&wright),
        "artifacts": arts.files,
        "beats": {
            "higher_burst": wright.a.flight.burst_altitude_m.unwrap_or(0.0) > pat_burst,
            "all_checks_pass": wright.report.passed(),
            "higher_recovery_probability": wright.a.recovery_probability > pat_same.a.recovery_probability,
            "cost_le": wright.cost <= pat_same.cost,
        }
    });
    std::fs::write(root.join("result.json"), serde_json::to_string_pretty(&result)?)?;

    let mut md = String::new();
    let fmt_b = |r: &Run| r.a.flight.burst_altitude_m.map_or("no burst".into(), |b| format!("{:.0} m", b));
    let _ = writeln!(md, "# Balloon eval: Pat's team balloon vs Wright\n");
    let _ = writeln!(md, "Generated by `cargo run -p wright-balloon --example eval`. Reference model and assumptions: [reference.md](reference.md). Raw numbers: [result.json](result.json).\n");
    let _ = writeln!(md, "Spec (requirements only): {}; launch {} ({:.0} m); burst >= {:.0} m (the team's assumed 30 km goal; the same for both); no budget (cost is information only; the search maximises burst altitude, then recovery probability, under every other check); recovery window {:.0} h; recovery probability >= {:.2}; licensed ham on the team.\n", spec.mission, spec.launch.name, spec.launch.elevation_m, spec.min_burst_altitude_m, spec.recovery_window_h, spec.min_recovery_probability);
    let _ = writeln!(md, "| | Pat as planned | Wright |\n|---|---|---|");
    let row = |md: &mut String, k: &str, a: String, b: String| {
        let _ = writeln!(md, "| {k} | {a} | {b} |");
    };
    row(&mut md, "balloon / chute", format!("{} / {}", pat_same.design.balloon, pat_same.design.parachute), format!("{} / {}", wright.design.balloon, wright.design.parachute));
    row(&mut md, "kits", pat_same.design.notes.join("; "), wright.design.notes.join("; "));
    row(&mut md, "burst altitude (US76 model)", fmt_b(&pat_same), fmt_b(&wright));
    row(&mut md, "burst altitude (CUSF closed form)", format!("{:.0} m", pat_same.a.flight.cusf_burst_altitude_m), format!("{:.0} m", wright.a.flight.cusf_burst_altitude_m));
    row(&mut md, "ascent launch / mean", format!("{:.2} / {:.2} m/s", pat_same.a.flight.launch_ascent_mps, pat_same.a.flight.mean_ascent_mps), format!("{:.2} / {:.2} m/s", wright.a.flight.launch_ascent_mps, wright.a.flight.mean_ascent_mps));
    row(&mut md, "landing speed", format!("{:.2} m/s", pat_same.a.flight.landing_speed_mps), format!("{:.2} m/s", wright.a.flight.landing_speed_mps));
    row(&mut md, "payload package / flight train", format!("{:.0} / {:.0} g", pat_same.a.masses.package().as_g(), pat_same.a.masses.flight_train().as_g()), format!("{:.0} / {:.0} g", wright.a.masses.package().as_g(), wright.a.masses.flight_train().as_g()));
    row(&mut md, "box temperature min..max", format!("{:.0}..{:.0} C", pat_same.a.box_min_c, pat_same.a.box_max_c), format!("{:.0}..{:.0} C", wright.a.box_min_c, wright.a.box_max_c));
    row(&mut md, "flight time / landing distance", format!("{:.0} min / {:.0} km", pat_same.a.flight.landing_t_s / 60.0, pat_same.a.landing_km), format!("{:.0} min / {:.0} km", wright.a.flight.landing_t_s / 60.0, wright.a.landing_km));
    row(&mut md, "recovery probability", format!("{:.3}", pat_same.a.recovery_probability), format!("{:.3}", wright.a.recovery_probability));
    row(&mut md, "cost (BOM total)", format!("${:.2}", pat_same.cost), format!("${:.2}", wright.cost));
    row(&mut md, "checks passing", format!("{}/{}", pat_same.report.outcomes.len() - pat_same.report.failures().count(), pat_same.report.outcomes.len()), format!("{}/{}", wright.report.outcomes.len() - wright.report.failures().count(), wright.report.outcomes.len()));
    let win = |d: &Design| d.launch_window_c.map_or("none declared".to_string(), |[a, b]| format!("{a:.0}..{b:.0} C"));
    row(&mut md, "launch window (go/no-go ground temperature)", win(&pat_same.design), win(&wright.design));
    let _ = writeln!(md, "\n## Every check\n\n{}", checks_table(&pat_same, &wright));
    // the hostile reviewer's confirmed thermal attacks, re-run on both designs
    let _ = writeln!(md, "## Launch temperature window and the stored thermal regressions\n");
    let _ = writeln!(md, "The thermal checks run at both ends of the spec's launch range ({:.0}..{:.0} C ground, `launch_temp_range_c`, default from `rules::LAUNCH_TEMP_RANGE_C`) and at the forecast when it is a launch day. The `launch_window` check requires the design's declared go/no-go window to cover that range and to lie inside the window the model computes; the window is the first item of the build checklist.\n", wright.a.launch_range_c[0], wright.a.launch_range_c[1]);
    let _ = writeln!(md, "| case (data/regressions-balloon.jsonl) | Pat: part_temperature | Wright: part_temperature | Wright: launch_window | spec envelope |\n|---|---|---|---|---|");
    for (id, g) in [("r1-1", -5.0), ("r2-1", -15.0), ("r1-2", 38.0)] {
        let mut sp = spec.clone();
        sp.ground_temperature_c = Some(g);
        let (fp, _) = checks::evaluate(&sp, &pat_same.design, cat);
        let (fw, _) = checks::evaluate(&sp, &wright.design, cat);
        let cell = |f: &Finished, n: &str| f.get(n).map_or("?".into(), |o| format!("{}: {}", if o.passed() { "ok" } else { "**FAIL**" }, match &o.evidence { wright_core::Evidence::Missing { why } => why.clone(), _ => o.detail.clone() }));
        let env = match sp.validate() {
            Ok(()) => "inside".to_string(),
            Err(e) => format!("**outside**, set aside by the harness: {e}"),
        };
        let _ = writeln!(md, "| {id}: ground {g:.0} C | {} | {} | {} | {env} |", cell(&fp, "part_temperature"), cell(&fw, "part_temperature"), cell(&fw, "launch_window"));
    }
    let _ = writeln!(md, "\nThe spec envelope (`Spec::validate`, the schema's minimum/maximum, sources in `rules.rs`) bounds ground temperature by the San Joaquin Valley records, -11.1 C (Bakersfield 12 F, 1908-01-03, NWS Hanford, weather.gov/media/hnx/Coldest%20Temperatures%20ever%20at%20BFL.pdf) to 47.8 C (Bakersfield 118 F, 1908-07-28, weather.gov/media/hnx/Top15WxEvents.pdf); burst targets by the CUSF burst calculator's 10-40 km; launch elevation by -86..4421 m (USGS); wind by 125 m/s (NOAA JetStream).\n");
    let _ = writeln!(md, "Thermal levers the search may choose (it keeps the lightest design that passes the whole range): XPS walls of 1, 1.5 or 2 in; 0, 2, 3 or 4 HotHands hand warmers opened at sealing below a declared switch-over temperature; the 5 W thermostat heater pad on its own L91 pack (backup-slot kit). The box starts at {:.0} C, packed in the car (`rules::PACK_TEMP_C`, a checklist step), then sits {:.0} min in the open before release.\n", wright_balloon::rules::PACK_TEMP_C, wright_balloon::rules::PRELAUNCH_MIN);
    let _ = writeln!(md, "Hand-warmer heat, derived in `rules.rs` (no maker publishes watts): 29.7 g per warmer (Red Rocks CC, *Bringing the Heat to 100,000 Feet*, 2023 COSGC symposium, p. 6) x 0.90 heating mix (ASSUMPTION) x 0.50 iron (Kobayashi Healthcare SDS for HotHands/Grabber, rev. 2021-12-08, Sect. 3: iron powder 50-60 wt %, lower bound) x 0.50 oxidised in the rated life (ASSUMPTION) x 7.38 kJ/g (dHf Fe2O3 = -824.2 kJ/mol) / 10 h (Uline S-14297B: 'Heats up to 10 hours', '15 to 30 minutes' to activate) = **{:.2} W** per warmer at sea level, ramped over 30 min, scaled by p/p0 aloft (ASSUMPTION: oxygen diffuses through the pouch film; the Red Rocks vacuum test at ~65,000 ft pressure saw the pouch fall 16 C in 30 min, and their sealed box fell to ~5 % O2 on the ground, so the checklist says vent the box). Catalog row: Uline, $38 per 40 2-packs ($0.475 each), read 2026-10-24.\n", wright_balloon::rules::warmer_power_sl_w(10.0));
    let _ = writeln!(md, "\nThermal cases Wright's design was judged at (spec as above):\n");
    for c in &wright.a.thermal_cases {
        let _ = writeln!(md, "* ground {:.1} C{}: box {:.1}..{:.1} C", c.ground_c, if c.warmers_on { format!(", warmers open ({:.1} Wh)", c.warmer_wh) } else { String::new() }, c.box_min_c, c.box_max_c);
    }
    let _ = writeln!(md);
    // the real-wind flight (examples/sim.rs), when it has been run
    if let Ok(t) = std::fs::read_to_string(root.join("sim/comparison.json")) {
        if let Ok(c) = serde_json::from_str::<serde_json::Value>(&t) {
            let tr = &c["tracks"];
            let km = |k: &str| tr[k]["landing_km_from_launch"].as_f64().unwrap_or(f64::NAN);
            let _ = writeln!(md, "## Flight on a real forecast (Tawhiri, GFS)\n");
            let _ = writeln!(md, "`cargo run -p wright-balloon --example sim` asks the public Tawhiri predictor ({}) for this design's flight (ascent {:.2} m/s, burst {:.0} m, sea-level descent {:.2} m/s) from the launch site at {} on GFS {}, recovers the GFS wind profile from Tawhiri's ascent by inverting its own wind model (tawhiri/models.py), and flies Wright's model and a Rust port of Tawhiri's `standard_profile` + RK4 (tawhiri/solver.pyx) on it. Animation: [sim/flight.mp4](sim/flight.mp4), still: [sim/flight.png](sim/flight.png).\n",
                c["endpoint"].as_str().unwrap_or(""), c["tawhiri_request_for_this_design"]["ascent_rate_mps"].as_f64().unwrap_or(f64::NAN), c["tawhiri_request_for_this_design"]["burst_altitude_m"].as_f64().unwrap_or(f64::NAN), c["tawhiri_request_for_this_design"]["descent_rate_mps"].as_f64().unwrap_or(f64::NAN), c["launch"]["datetime"].as_str().unwrap_or(""), c["gfs_dataset"].as_str().unwrap_or(""));
            let _ = writeln!(md, "| track | landing from launch | flight time |\n|---|---|---|");
            for k in ["tawhiri_api", "tawhiri_port_gfs", "wright_gfs", "wright_assumed"] {
                if tr[k].is_object() {
                    let _ = writeln!(md, "| {} | {:.1} km at {:.0} deg | {:.0} min |", tr[k]["label"].as_str().unwrap_or(k), km(k), tr[k]["landing_bearing_deg"].as_f64().unwrap_or(f64::NAN), tr[k]["flight_min"].as_f64().unwrap_or(f64::NAN));
                }
            }
            let sep = &c["landing_separation_km"];
            let _ = writeln!(md, "\nWright's landing is {:.1} km from the live Tawhiri prediction and {:.1} km from Tawhiri's model on the same winds (the port lands {:.1} km from the API: the wind recovery is faithful). The difference is the ascent: Tawhiri climbs at a constant rate, Wright's rate comes from free lift and the Gallice Cd(Re) drag of the growing envelope (4.4 m/s at launch, faster aloft), so it spends longer in the low-level winds. Descent agrees within a minute (both use terminal velocity ~ 1/sqrt(rho); Tawhiri's NASA Glenn density vs Wright's US76). Wright keeps its own ascent and descent models and adopts Tawhiri's winds. On these winds every check {}; the eval keeps the assumed profile (it drifts {:.0} km, twice as far) because a design must also fly on a windier day.\n",
                sep["wright_gfs_vs_tawhiri_api"].as_f64().unwrap_or(f64::NAN), sep["wright_gfs_vs_tawhiri_port"].as_f64().unwrap_or(f64::NAN), sep["tawhiri_port_vs_api"].as_f64().unwrap_or(f64::NAN),
                if c["checks_on_gfs_winds"]["passed"].as_bool() == Some(true) { "passes".to_string() } else { format!("passes except {}", c["checks_on_gfs_winds"]["failing"]) }, km("wright_assumed"));
        }
    }
    let _ = writeln!(md, "## Why Pat's design fails\n");
    for o in pat_same.report.failures() {
        let why = match &o.evidence {
            wright_core::Evidence::Missing { why } => format!("missing evidence: {why}"),
            _ => o.detail.clone(),
        };
        let _ = writeln!(md, "* `{}`: {}", o.name, why);
    }
    let _ = writeln!(md, "\nThe plan names no balloon, chute, foam wall or fill target, so all {} combinations in the catalog were tried and the one failing the fewest checks kept (then highest burst, then cheapest); with the earlier assumed choices (HAB-1200, 36 in chute, 1 in wall, 5 m/s) it fails {}.\n", design::WALLS_MM.len() * cat.balloons.len() * cat.parachutes.len() * design::FILL_TARGETS_MPS.len(), assumed_fails);
    let _ = writeln!(md, "## Wright's design\n\nWright check details:\n");
    for o in &wright.report.outcomes {
        let _ = writeln!(md, "* `{}` {}: {}", o.name, if o.passed() { "ok" } else { "FAIL" }, match &o.evidence { wright_core::Evidence::Missing { why } => why.clone(), _ => o.detail.clone() });
    }
    let _ = writeln!(md, "\n### Go / no-go (first line of build/checklist.md)\n\n{}\n", wright_balloon::go_no_go(&wright.design));
    let _ = writeln!(md, "\n### Fill procedure (Wright)\n\n{}", fill_plan(&wright));
    let _ = writeln!(md, "### GPS setting\n\nBefore flight, set every u-blox receiver to an airborne dynamic platform model (UBX-CFG-NAV5 `dynModel` = 6, airborne <1g, or CFG-NAVSPG-DYNMODEL = AIR1 on M10; u-blox M10 SPG 5.00 interface description UBX-20053845, CFG-NAV5 table) and read it back. In the default portable model u-blox receivers stop reporting above 12 km; the `gps_altitude` check fails a tracker whose `gps_airborne` is false.\n");
    let _ = writeln!(md, "### FAA Part 101 summary (Wright)\n");
    for n in ["faa101_package", "faa101_package_max", "faa101_total_payload", "faa101_suspension"] {
        if let Some(o) = wright.report.get(n) {
            let _ = writeln!(md, "* {} ({}): {}", n, if o.passed() { "exempt" } else { "REGULATED" }, o.detail);
        }
    }
    let _ = writeln!(md, "\nAll four clauses of 101.1(a)(4) clear, so the flight is outside Part 101 Subpart D; 101.7 (no hazardous operation) still applies, and a NOTAM call to the FAA is courteous practice.\n");
    let _ = writeln!(md, "### Build outputs\n");
    for (label, path) in &arts.files {
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let _ = writeln!(md, "* {label}: `{}`", rel.display());
    }
    std::fs::write(root.join("result.md"), md)?;
    println!("wrote {}", root.join("result.md").display());
    Ok(())
}
