//! cargo run --release -p wright-rocket --example rocket_eval
//! Writes evals/rocket/result.json and prints the summary.
fn main() -> anyhow::Result<()> {
    let r = wright_rocket::eval::run()?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/rocket");
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("result.json"), serde_json::to_string_pretty(&r)?)?;
    for g in r.reference_engine.iter().chain(&r.reference_flight) {
        println!("{:<55} wright {:>10.3} published {:>10.3} {:<5} err {:+6.1}% {}", g.quantity, g.wright, g.published, g.unit, g.error_pct, if g.pass { "ok" } else { "MISS" });
    }
    println!("implied eta_c* {:.3}", r.reference_eta_cstar_implied);
    for d in &r.reference_diagnostics {
        println!("if {}: {:.1} {} -> max Mach {:.2} (published {:.2}); {}", d.hypothesis, d.fitted_value, d.unit, d.predicted_max_mach, d.published_max_mach, d.note);
    }
    for (n, s) in [("REFERENCE", &r.reference_summary), ("DESIGN", &r.design_summary), ("DESIGN @ a-priori eta 0.90", &r.design_at_seb_eta), ("DESIGN uncalibrated", &r.design_uncalibrated), ("DESIGN LOX/propane (Eureka-2 pair)", &r.design_same_propellants)] {
        println!("\n== {n}: {}\n{}", s.name, s.checks);
        println!("m0 {:.1} kg dry {:.1} prop {:.1} L {:.2} m D {:.3} F {:.0} N Pc {:.1} O/F {:.2} Isp {:.1} s burn {:.1} s I {:.0} Ns apogee {:.0} ft band {:.0}-{:.0} Mach {:.2} rail {:.0} ft/s TWR {:.2} BOM ${:.0} score {:.0}",
            s.liftoff_mass_kg, s.dry_mass_kg, s.propellant_kg, s.length_m, s.diameter_m, s.thrust_n, s.pc_bar, s.of, s.isp_site_s, s.burn_s, s.total_impulse_ns, s.apogee_nominal_ft, s.apogee_band_ft.0, s.apogee_band_ft.1, s.max_mach, s.rail_exit_ftps, s.twr, s.bom_usd, s.far_score_mean);
    }
    for l in &r.design_search_log { println!("  {l}"); }
    for p in &r.pair_search { println!("pair {}/{}: {}/{} checks, score {:.0}, BOM ${:.0} {:?}", p.oxidizer, p.fuel, p.checks_passed, p.checks_total, p.far_score, p.bom_usd, p.failures.iter().map(|f| f.chars().take(90).collect::<String>()).collect::<Vec<_>>()); }
    println!("\nfitted dry-mass calibration x{:.3}", r.mass_calibration_fitted);
    println!("sim vs sim (uncalibrated): Wright {:.0}, Eureka-2 reconstruction {:.0}", r.sim_scores.0, r.sim_scores.1);
    for c in &r.corrected {
        println!("corrected ({}): Wright nominal {:.0} ft, score {:.0} vs Eureka-2 {:.0}", c.hypothesis, c.wright_apogee_ft, c.wright_score, c.reference_score);
    }
    for (k, why) in &r.reference_excluded { println!("not applied to the 2024 reference: {k} ({why})"); }
    println!("reference failures that remain: {:?}", r.reference_applicable_failures);
    println!("\n{}", r.verdict);
    // Builder artifacts for the requirements-only design.
    use wright_core::Domain;
    // Builder artifacts for the headline design (the pair the search chose).
    let spec = wright_rocket::domain::RocketSpec { oxidizer: r.headline_pair.0.clone(), fuel: r.headline_pair.1.clone(), eta_cstar: Some(r.reference_eta_cstar_implied), ..wright_rocket::eval::requirements_spec() };
    let d = wright_rocket::Rocket::design(&spec)?;
    let art = wright_rocket::Rocket::build(&d, &root.join("build"))?;
    for (label, path) in &art.files {
        println!("artifact: {label} -> {}", path.display());
    }
    Ok(())
}
