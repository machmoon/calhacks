//! The Berkeley eval (evals/rocket/): (1) thermochemistry vs NASA CEA,
//! (2) reproduce SEB Eureka-2 from its published inputs and grade against
//! its published flight, (3) design from a requirements-only spec under
//! the same constraints and beat it on the competition's objective while
//! passing every check.

use crate::domain::{mean_far_score, Competition, Rocket, RocketDesign, RocketSpec};
use crate::engine;
use crate::reference::{self, Published};
use crate::rules::{self, FT};
use serde::Serialize;
use wright_core::{Domain, Finished};

#[derive(Debug, Serialize)]
pub struct Graded {
    pub quantity: String,
    pub wright: f64,
    pub published: f64,
    pub unit: String,
    pub error_pct: f64,
    pub tolerance_pct: f64,
    pub pass: bool,
    pub note: String,
}

fn grade(q: &str, w: f64, p: f64, unit: &str, tol: f64, note: &str) -> Graded {
    let e = (w - p) / p * 100.0;
    Graded { quantity: q.into(), wright: w, published: p, unit: unit.into(), error_pct: e, tolerance_pct: tol, pass: e.abs() <= tol, note: note.into() }
}

#[derive(Debug, Serialize)]
pub struct DesignSummary {
    pub name: String,
    pub checks_passed: usize,
    pub checks_total: usize,
    pub failures: Vec<String>,
    pub liftoff_mass_kg: f64,
    pub dry_mass_kg: f64,
    pub propellant_kg: f64,
    pub length_m: f64,
    pub diameter_m: f64,
    pub thrust_n: f64,
    pub pc_bar: f64,
    pub of: f64,
    pub isp_site_s: f64,
    pub burn_s: f64,
    pub total_impulse_ns: f64,
    pub apogee_nominal_ft: f64,
    pub apogee_band_ft: (f64, f64),
    pub max_mach: f64,
    pub rail_exit_ftps: f64,
    pub twr: f64,
    pub margin_min_cal: f64,
    pub bom_usd: f64,
    pub far_score_mean: f64,
    pub checks: Finished,
}

pub fn summarize(spec: &RocketSpec, d: &RocketDesign) -> DesignSummary {
    let fin = Rocket::check(spec, d);
    // Re-flown from the parameters (stored derived fields are not trusted).
    let r = crate::domain::reflown(spec, d).expect("eval designs build");
    let v = &r.vehicle;
    let lo = r.dispersion.iter().map(|x| x.1).fold(f64::MAX, f64::min);
    let hi = r.dispersion.iter().map(|x| x.1).fold(f64::MIN, f64::max);
    let pa = crate::atmosphere::at(spec.competition.limits().elevation_m).p;
    DesignSummary {
        name: spec.name.clone(),
        checks_passed: fin.outcomes.iter().filter(|o| o.passed()).count(),
        checks_total: fin.outcomes.len(),
        failures: fin.failures().map(|o| o.to_string()).collect(),
        liftoff_mass_kg: v.mass_at(0.0).mass,
        dry_mass_kg: v.dry_mass(),
        propellant_kg: v.propellant_mass(),
        length_m: v.length,
        diameter_m: v.diameter,
        thrust_n: v.thrust_at(0.0, pa),
        pc_bar: v.engine.spec.pc / 1e5,
        of: v.engine.spec.of,
        isp_site_s: crate::domain::total_impulse(v, pa) / v.propellant_burned() / 9.80665,
        burn_s: v.burn_time,
        total_impulse_ns: crate::domain::total_impulse(v, pa),
        apogee_nominal_ft: r.nominal.apogee_agl / FT,
        apogee_band_ft: (lo / FT, hi / FT),
        max_mach: r.nominal.max_mach,
        rail_exit_ftps: r.nominal.rail_exit_speed / FT,
        twr: r.nominal.liftoff_twr,
        margin_min_cal: r.nominal.margin_min,
        bom_usd: Rocket::bom(d).total().as_usd(),
        far_score_mean: mean_far_score(d, spec),
        checks: fin,
    }
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub hypothesis: String,
    pub fitted_value: f64,
    pub unit: String,
    pub predicted_max_mach: f64,
    pub published_max_mach: f64,
    pub note: String,
}

/// One explanation of the reconstruction's miss, applied to both rockets:
/// the factor that makes Wright's model fly Eureka-2 to its recorded
/// apogee, applied unchanged to Wright's own design.
#[derive(Debug, Serialize)]
pub struct Corrected {
    pub hypothesis: String,
    pub factor: f64,
    pub wright_apogee_ft: f64,
    pub wright_score: f64,
    /// Eureka-2 under the same correction = its recorded flight, by construction.
    pub reference_score: f64,
}

/// One propellant pair of the eval's pair search.
#[derive(Debug, Serialize)]
pub struct PairRow {
    pub oxidizer: String,
    pub fuel: String,
    pub checks_passed: usize,
    pub checks_total: usize,
    pub far_score: f64,
    pub bom_usd: f64,
    pub failures: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EvalResult {
    /// Every schema propellant pair, designed to the same requirements.
    pub pair_search: Vec<PairRow>,
    /// The pair the search chose (the headline design's propellants).
    pub headline_pair: (String, String),
    /// Wright's design on Eureka-2's own propellants (LOX/propane).
    pub design_same_propellants: DesignSummary,
    pub cea_validation: Vec<serde_json::Value>,
    pub reference_diagnostics: Vec<Diagnostic>,
    pub reference_engine: Vec<Graded>,
    pub reference_flight: Vec<Graded>,
    pub reference_eta_cstar_implied: f64,
    pub reference_summary: DesignSummary,
    pub reference_far_score_achieved: f64,
    pub design_summary: DesignSummary,
    pub design_search_log: Vec<String>,
    /// The headline pair designed at Wright's a-priori c* efficiency (0.90).
    pub design_at_seb_eta: DesignSummary,
    /// The requirements designed with the uncalibrated build-up (x1.0).
    pub design_uncalibrated: DesignSummary,
    /// Dry-mass factor fitted to Eureka-2's flight in this run (the
    /// constant rules::EUREKA2_DRY_MASS_FACTOR must match it).
    pub mass_calibration_fitted: f64,
    /// Simulation vs simulation: both rockets flown by the same model,
    /// uncorrected (mean FAR-OUT score over the dispersion set).
    pub sim_scores: (f64, f64),
    /// Both rockets under each correction that explains the reconstruction's miss.
    pub corrected: Vec<Corrected>,
    /// Checks not applied to the 2024 reference, with the reason.
    pub reference_excluded: Vec<(String, String)>,
    /// Reference failures that remain after those exclusions.
    pub reference_applicable_failures: Vec<String>,
    pub beats_reference: bool,
    pub passes_every_check: bool,
    pub verdict: String,
}

pub fn reference_spec(rail_m: f64) -> RocketSpec {
    RocketSpec {
        name: "SEB Eureka-2 (reconstructed from published inputs)".into(),
        competition: Competition::FarOut2027,
        contract_apogee_ft: Published::TARGET_M / FT,
        oxidizer: "lox".into(),
        fuel: "propane".into(),
        budget_usd: None,
        payload_kg: 0.0,
        rail_length_m: Some(rail_m),
        measured_dry: None,
        eta_cstar: None,
        nose_cd_table: None,
        pinned: None,
        // The reconstruction is what the calibration is fitted on: flown
        // with the uncalibrated build-up.
        dry_mass_factor: Some(1.0),
    }
}

pub fn requirements_spec() -> RocketSpec {
    RocketSpec {
        name: "Wright LOX/propane, FAR-OUT 2027 Group B, 20,000 ft".into(),
        competition: Competition::FarOut2027,
        contract_apogee_ft: 20_000.0,
        oxidizer: "lox".into(),
        fuel: "propane".into(),
        budget_usd: None,
        payload_kg: 0.0,
        rail_length_m: None,
        measured_dry: None,
        eta_cstar: None,
        nose_cd_table: None,
        pinned: None,
        dry_mass_factor: None,
    }
}

/// Eureka-2 as reconstructed (published engine at the c* efficiency its
/// published thrust and flows imply, published length, fins sized to
/// 1.8 cal), plus the a-priori engine and that efficiency.
pub fn reference_params(cat: &crate::catalog::Catalog) -> anyhow::Result<(crate::vehicle::VehicleParams, engine::Engine, f64)> {
    let mut p = reference::eureka2();
    let a_priori = engine::design(&p.engine)?;
    let pub_mdot = Published::MDOT_OX + Published::MDOT_FUEL;
    // As published: thrust 3 kN at mdot 1.63 kg/s => eta_c* such that Isp = 188 s.
    let eta = reference::published_isp() / (a_priori.isp(101_325.0) / a_priori.eta_cstar);
    let _reference_engine: Vec<Graded> = vec![
        grade("propellant mass flow at 3 kN (a priori eta_c* 0.90)", a_priori.mdot, pub_mdot, "kg/s", 10.0, "Wright's Isp from CEA-method equilibrium x eta_c* 0.90 x conical lambda; published flows are 1.14 + 0.49 kg/s (arXiv 2401.07444)"),
        grade("delivered sea-level Isp (a priori)", a_priori.isp(101_325.0) / 9.80665, reference::published_isp() / 9.80665, "s", 10.0, "published value derived: 3000 N / 1.63 kg/s"),
    ];
    p.engine.eta_cstar = Some(eta);
    // Match the published 19.3 ft length (Eureka-1) with the unexplained remainder as extra bay length.
    let v0 = crate::vehicle::build(&p, cat)?;
    p.extra_length = (Published::LENGTH_M - v0.length).max(0.0);
    // Fins: published geometry is not public; use Wright's family sized for
    // >= 1.8 cal like any design (stated).
    let rspec = RocketSpec { pinned: Some(p.clone()), ..reference_spec(50.0 * FT) };
    // Size the reference fins for a 1.8 cal minimum (not public).
    {
        let d = p.body_diameter.unwrap();
        let (mut lo, mut hi) = (0.2, 3.0);
        for _ in 0..20 {
            let mid = 0.5 * (lo + hi);
            let mut q = p.clone();
            q.fin_root = 2.0 * d;
            q.fin_tip = 0.8 * d;
            q.fin_span = mid * d;
            q.fin_sweep = mid * d;
            let v = crate::vehicle::build(&q, cat)?;
            let m = crate::trajectory::fly(&v, &rspec.launch(rules::FAR_WIND_MAX_MPS / 2.0)).margin_min;
            if m < 1.8 { lo = mid } else { hi = mid }
        }
        p.fin_root = 2.0 * d;
        p.fin_tip = 0.8 * d;
        p.fin_span = hi * d;
        p.fin_sweep = hi * d;
    }
    Ok((p, a_priori, eta))
}

/// The dry mass (kg) at which Wright's trajectory model flies the
/// reconstruction to Eureka-2's recorded apogee (mid wind), Wright's own
/// build-up of the same vehicle, and the max Mach at the fitted mass. The
/// ratio is the mass calibration (`rules::EUREKA2_DRY_MASS_FACTOR`).
pub fn fit_dry_mass(p: &crate::vehicle::VehicleParams, cat: &crate::catalog::Catalog) -> anyhow::Result<(f64, f64, f64)> {
    let rspec = RocketSpec { pinned: Some(p.clone()), ..reference_spec(50.0 * FT) };
    let mid = rspec.launch(rules::FAR_WIND_MAX_MPS / 2.0);
    let v_ref = crate::vehicle::build(p, cat)?;
    let dry_cg = {
        let m: f64 = v_ref.dry.iter().map(|i| i.mass).sum();
        v_ref.dry.iter().map(|i| i.mass * i.x).sum::<f64>() / m
    };
    let (mut lo, mut hi) = (v_ref.dry_mass(), v_ref.dry_mass() * 4.0);
    let mut mach = 0.0;
    for _ in 0..30 {
        let midm = 0.5 * (lo + hi);
        let mut q = p.clone();
        q.measured_dry = Some((midm, dry_cg));
        let v = crate::vehicle::build(&q, cat)?;
        let r = crate::trajectory::fly(&v, &mid);
        mach = r.max_mach;
        if r.apogee_agl > Published::APOGEE_M { lo = midm } else { hi = midm }
    }
    Ok((0.5 * (lo + hi), v_ref.dry_mass(), mach))
}

pub fn run() -> anyhow::Result<EvalResult> {
    let cea = crate::performance::validation_table()?;

    // --- Reference: engine from published Pc, O/F and thrust. ---
    let cat = crate::catalog::Catalog::load()?;
    let (p, a_priori, eta) = reference_params(&cat)?;
    let pub_mdot = Published::MDOT_OX + Published::MDOT_FUEL;
    let reference_engine = vec![
        grade("propellant mass flow at 3 kN (a priori eta_c* 0.90)", a_priori.mdot, pub_mdot, "kg/s", 10.0, "Wright's Isp from CEA-method equilibrium x eta_c* 0.90 x conical lambda; published flows are 1.14 + 0.49 kg/s (arXiv 2401.07444)"),
        grade("delivered sea-level Isp (a priori)", a_priori.isp(101_325.0) / 9.80665, reference::published_isp() / 9.80665, "s", 10.0, "published value derived: 3000 N / 1.63 kg/s"),
    ];
    let rspec = RocketSpec { pinned: Some(p.clone()), ..reference_spec(50.0 * FT) };
    let rdesign = Rocket::design(&rspec)?;
    let rsum = summarize(&rspec, &rdesign);
    let reference_flight = vec![
        grade("apogee AGL", rdesign.nominal.apogee_agl / FT, Published::APOGEE_M / FT, "ft", 20.0, "15,879 ft (YouTube flight video description); AGL vs MSL not stated; Wright flies from 2000 ft MSL, mid-range wind"),
        grade("max Mach", rdesign.nominal.max_mach, Published::MAX_MACH, "", 15.0, "Mach 0.88 (same source)"),
        grade("liftoff mass", rsum.liftoff_mass_kg, Published::E1_LIFTOFF_KG, "kg", 25.0, "published 120 lb is Eureka-1; Eureka-2 had larger tanks and lighter structure"),
    ];
    let achieved = rules::far_altitude_score(Published::APOGEE_M, Published::TARGET_M);

    // Which single unknown would explain the miss? Fit it to the published
    // apogee, then see whether the *other* published number (max Mach)
    // follows -- a test of the explanation, not a calibration of the model.
    let mut diags = Vec::new();
    let mid = rspec.launch(rules::FAR_WIND_MAX_MPS / 2.0);
    let v_ref = crate::vehicle::build(&p, &cat)?;
    {
        let (fitted, buildup, mach) = fit_dry_mass(&p, &cat)?;
        diags.push(Diagnostic { hypothesis: "dry mass is higher than Wright's build-up".into(), fitted_value: fitted, unit: "kg dry".into(), predicted_max_mach: mach, published_max_mach: Published::MAX_MACH, note: format!("Wright's build-up gives {:.1} kg dry", buildup) });
    }
    {
        let (mut lo, mut hi) = (0.2, 1.0);
        let mut mach = 0.0;
        for _ in 0..30 {
            let s = 0.5 * (lo + hi);
            let mut l = mid;
            l.thrust_scale = s;
            let r = crate::trajectory::fly(&v_ref, &l);
            mach = r.max_mach;
            if r.apogee_agl > Published::APOGEE_M { hi = s } else { lo = s }
        }
        let s = 0.5 * (lo + hi);
        diags.push(Diagnostic { hypothesis: "flight thrust below the 3 kN stand figure at the published flows (lower delivered Isp)".into(), fitted_value: s * Published::THRUST_N, unit: "N".into(), predicted_max_mach: mach, published_max_mach: Published::MAX_MACH, note: format!("= {:.0} s delivered Isp at 1.63 kg/s; Eureka-1's published average thrust was 2.7 kN", s * reference::published_isp() / 9.80665) });
    }
    {
        let (mut lo, mut hi) = (1.0, 4.0);
        let mut mach = 0.0;
        for _ in 0..30 {
            let s = 0.5 * (lo + hi);
            let mut l = mid;
            l.drag_scale = s;
            let r = crate::trajectory::fly(&v_ref, &l);
            mach = r.max_mach;
            if r.apogee_agl > Published::APOGEE_M { lo = s } else { hi = s }
        }
        diags.push(Diagnostic { hypothesis: "drag higher than the OpenRocket-style buildup".into(), fitted_value: 0.5 * (lo + hi), unit: "x Cd".into(), predicted_max_mach: mach, published_max_mach: Published::MAX_MACH, note: "external plumbing, rail guides, camera shrouds and fin can are not in the buildup".into() });
    }

    // --- Requirements-only design: propellant pair searched too. ---
    // The eval leaves the propellants open the way Wright does for a person:
    // every oxidizer x fuel the spec schema offers is designed at the same
    // competition and contract, with no budget (Pat 2026-10-24: "ignore
    // budget"; cost is reported, not held to a limit, on either side). The
    // least check violation wins, then the higher FAR-OUT score, then the
    // lower BOM (`domain::ranks_before`). LOX/propane (Eureka-2's own pair)
    // is reported alongside.
    // At SEB's demonstrated c* efficiency (implied by Eureka-2's published
    // thrust and flows, the same flight the mass calibration comes from),
    // not Wright's a-priori 0.90: both sides then share SEB's engine
    // efficiency and SEB's mass.
    let base = RocketSpec { eta_cstar: Some(eta), name: requirements_spec().name, ..requirements_spec() };
    let schema = Rocket::spec_schema();
    let list = |k: &str| -> Vec<String> { schema["properties"][k]["enum"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default() };
    let mut pair_search = Vec::new();
    let mut best: Option<((f64, f64, f64), RocketSpec, RocketDesign)> = None;
    for ox in list("oxidizer") {
        for fuel in list("fuel") {
            let s = RocketSpec { name: format!("Wright {ox}/{fuel}, FAR-OUT 2027 Group B, 20,000 ft, eta_c* {eta:.2}"), oxidizer: ox.clone(), fuel: fuel.clone(), ..base.clone() };
            match Rocket::design(&s) {
                Ok(dd) => {
                    let fin = Rocket::check(&s, &dd);
                    let viol = fin.violation(|_| 1.0);
                    let cost = Rocket::bom(&dd).total().as_usd();
                    let score = mean_far_score(&dd, &s);
                    pair_search.push(PairRow { oxidizer: ox.clone(), fuel: fuel.clone(), checks_passed: fin.outcomes.iter().filter(|o| o.passed()).count(), checks_total: fin.outcomes.len(), far_score: score, bom_usd: cost, failures: fin.failures().map(|o| o.to_string()).collect() });
                    let key = (viol, score, cost);
                    if best.as_ref().is_none_or(|b| crate::domain::ranks_before(key, b.0)) {
                        best = Some((key, s, dd));
                    }
                }
                Err(e) => pair_search.push(PairRow { oxidizer: ox.clone(), fuel: fuel.clone(), checks_passed: 0, checks_total: 0, far_score: 0.0, bom_usd: f64::NAN, failures: vec![format!("design refused: {e:#}")] }),
            }
        }
    }
    let (_, spec, d) = best.ok_or_else(|| anyhow::anyhow!("no propellant pair produced a design"))?;
    let dsum = summarize(&spec, &d);
    let d_lox = Rocket::design(&base)?;
    let dsum_lox = summarize(&base, &d_lox);
    // The same pair at Wright's a-priori c* efficiency (rules::ETA_CSTAR, 0.90).
    let spec_eta = RocketSpec { eta_cstar: None, name: format!("{} (at Wright's a-priori eta_c* {:.2})", spec.name, rules::ETA_CSTAR), ..spec.clone() };
    let d_eta = Rocket::design(&spec_eta)?;
    let dsum_eta = summarize(&spec_eta, &d_eta);
    let passes = dsum.checks_passed == dsum.checks_total;
    // The same requirements designed with the uncalibrated build-up (the
    // eval's design before 2026-10-24), kept to show what calibration costs.
    let spec_unc = RocketSpec { dry_mass_factor: Some(1.0), name: format!("{} (uncalibrated dry-mass build-up)", spec.name), ..spec.clone() };
    let d_unc = Rocket::design(&spec_unc)?;
    let dsum_unc = summarize(&spec_unc, &d_unc);

    // Fair comparison (hostile review 2026-10-24): Wright's design is flown
    // by the model that sized it, so its uncorrected score is the model
    // agreeing with itself. Apply each correction that makes the model fly
    // Eureka-2 to its recorded apogee to Wright's design as well.
    let mut corrected = Vec::new();
    let v_d = crate::vehicle::build(&d_unc.params, &cat)?;
    let d_cg = {
        let m: f64 = v_d.dry.iter().map(|i| i.mass).sum();
        v_d.dry.iter().map(|i| i.mass * i.x).sum::<f64>() / m
    };
    let k_mass = diags[0].fitted_value / v_ref.dry_mass();
    let k_thrust = diags[1].fitted_value / Published::THRUST_N;
    let k_drag = diags[2].fitted_value;
    for (label, k) in [("dry mass x", k_mass), ("thrust x", k_thrust), ("drag x", k_drag)] {
        let mut q = d_unc.params.clone();
        if label.starts_with("dry") {
            q.measured_dry = Some((v_d.dry_mass() * k, d_cg));
        }
        let v = crate::vehicle::build(&q, &cat)?;
        let mut apos = Vec::new();
        for w in [0.0, rules::FAR_WIND_MAX_MPS / 2.0, rules::FAR_WIND_MAX_MPS] {
            for (ts, ds) in [(1.0, 1.0), (0.95, 1.10), (1.05, 0.90)] {
                let mut l = spec.launch(w);
                l.thrust_scale = ts * if label.starts_with("thrust") { k } else { 1.0 };
                l.drag_scale = ds * if label.starts_with("drag") { k } else { 1.0 };
                apos.push(crate::trajectory::fly(&v, &l).apogee_agl);
            }
        }
        let score = apos.iter().map(|a| rules::far_altitude_score(*a, spec.contract_m())).sum::<f64>() / apos.len() as f64;
        corrected.push(Corrected { hypothesis: format!("{label} {k:.2}"), factor: k, wright_apogee_ft: apos[4] / FT, wright_score: score, reference_score: achieved });
    }
    let reference_excluded = vec![
        ("thrust_to_weight".to_string(), "the T/W > 8 rule is FAR-OUT 2027's; Eureka-2 flew in 2024".to_string()),
        ("apogee_accuracy".to_string(), "the reconstruction's miss is the simulation's own error (section 2), not the rocket's".to_string()),
        ("pressurant_margin".to_string(), "Eureka-2's GN2 bottle is not published; the airventuri 88 cu ft bottle is Wright's placeholder".to_string()),
        ("parts_fit (GN2 bottle)".to_string(), "the placeholder bottle lists no size, so its fit cannot be measured; it is not the team's".to_string()),
        ("parts_orderable".to_string(), "Eureka-2 was bought in 2023-24; whether its parts are in stock today says nothing about the rocket".to_string()),
    ];
    // The reference's remaining failures, after the exclusions above.
    let reference_applicable_failures: Vec<String> = rsum
        .checks
        .failures()
        .filter(|o| !matches!(o.name.as_str(), "thrust_to_weight" | "apogee_accuracy" | "pressurant_margin" | "parts_orderable"))
        .filter(|o| !(o.name == "parts_fit" && o.to_string().contains("bottle")))
        .map(|o| o.to_string())
        .collect();
    // The design is made and checked with the Eureka-2 calibration (the
    // default, rules::EUREKA2_DRY_MASS_FACTOR, from this fit), so its score
    // and Eureka-2's 588 come from the same model.
    let beats = dsum.far_score_mean > achieved;
    let verdict = format!(
        "calibrated design ({}/{} chosen by the pair search; dry mass x{:.2}, the Eureka-2 calibration, fitted here x{:.3}): {}/{} checks, FAR-OUT score {:.0} vs Eureka-2's {:.0} under the same calibration, BOM ${:.0} (information: no budget on either side). Uncalibrated design flown under that calibration: {:.0}{}",
        spec.oxidizer, spec.fuel, rules::EUREKA2_DRY_MASS_FACTOR, k_mass, dsum.checks_passed, dsum.checks_total, dsum.far_score_mean, achieved, dsum.bom_usd, corrected[0].wright_score,
        if passes && beats { ": PASS" } else { ": NOT YET" }
    );
    let sim_scores = (dsum_unc.far_score_mean, rsum.far_score_mean);
    Ok(EvalResult {
        pair_search,
        headline_pair: (spec.oxidizer.clone(), spec.fuel.clone()),
        design_same_propellants: dsum_lox,
        sim_scores,
        cea_validation: cea,
        reference_diagnostics: diags,
        reference_engine,
        reference_flight,
        reference_eta_cstar_implied: eta,
        reference_summary: rsum,
        reference_far_score_achieved: achieved,
        design_search_log: d.search_log.clone(),
        design_summary: dsum,
        design_at_seb_eta: dsum_eta,
        corrected,
        reference_excluded,
        reference_applicable_failures,
        design_uncalibrated: dsum_unc,
        mass_calibration_fitted: k_mass,
        beats_reference: beats,
        passes_every_check: passes,
        verdict,
    })
}

#[cfg(test)]
mod calibration_tests {
    use super::*;

    /// The default dry-mass calibration is the eval's own fit to Eureka-2's
    /// flight; if the models change, the constant must be refitted.
    #[test]
    fn eureka2_calibration_constant_matches_the_fit() {
        let cat = crate::catalog::Catalog::load().unwrap();
        let (p, _, _) = reference_params(&cat).unwrap();
        let (fitted, buildup, mach) = fit_dry_mass(&p, &cat).unwrap();
        let k = fitted / buildup;
        assert!((k / rules::EUREKA2_DRY_MASS_FACTOR - 1.0).abs() < 0.01, "fitted x{k:.3} vs constant x{}", rules::EUREKA2_DRY_MASS_FACTOR);
        assert!((mach - Published::MAX_MACH).abs() < 0.1, "{mach}");
    }
}

