//! Every oxidizer/fuel pair the spec schema offers, at both competitions,
//! designed from a requirements-only spec. Live agent runs showed the
//! domain had only ever been exercised on LOX/propane at FAR-OUT; this
//! sweeps the whole advertised space so a pair cannot silently regress.
//!
//! Each pair must either pass every check or fail with a stated reason:
//! no NaN or infinite margin, no failure without its detail, and an
//! unpriced BOM line only with the reason on the line.

use wright_core::{Domain, Evidence};
use wright_rocket::domain::{Competition, Rocket, RocketSpec};

fn spec(comp: Competition, ox: &str, fuel: &str) -> RocketSpec {
    // IREC's lower contract category (10k ft); FAR-OUT 2027 Group B (20k ft).
    let apogee = if comp == Competition::Irec2025 { 10_000.0 } else { 20_000.0 };
    let json = serde_json::json!({
        "name": format!("{comp:?} {ox}/{fuel}"),
        "competition": format!("{comp:?}"),
        "contract_apogee_ft": apogee,
        "oxidizer": ox,
        "fuel": fuel,
        // The matrix tests engineering feasibility with a stated budget (the eval states none)
        // budget path is exercised with its numbers by the domain tests.
        "budget_usd": 15_000.0
    });
    serde_json::from_value(json).expect("requirements-only spec parses")
}

fn schema_enum(field: &str) -> Vec<String> {
    Rocket::spec_schema()["properties"][field]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("schema has no {field} enum"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// Design + check one pair; returns (passed, failure lines).
fn run(comp: Competition, ox: &str, fuel: &str) -> (bool, Vec<String>) {
    let s = spec(comp, ox, fuel);
    let d = Rocket::design(&s).unwrap_or_else(|e| panic!("{comp:?} {ox}/{fuel}: design refused: {e:#}"));
    let fin = Rocket::check(&s, &d);
    for o in &fin.outcomes {
        match &o.evidence {
            Evidence::Margin { value, .. } => {
                assert!(value.is_finite(), "{comp:?} {ox}/{fuel}: {} has a non-finite margin: {o}", o.name);
                if *value < 0.0 {
                    assert!(!o.detail.trim().is_empty(), "{comp:?} {ox}/{fuel}: {} fails without a detail", o.name);
                }
            }
            Evidence::Missing { why } => {
                assert!(!why.trim().is_empty(), "{comp:?} {ox}/{fuel}: {} missing without a reason", o.name);
            }
        }
    }
    for l in Rocket::bom(&d).unpriced() {
        assert!(!l.note.trim().is_empty(), "{comp:?} {ox}/{fuel}: unpriced line {} gives no reason", l.id);
    }
    // Headlines are numbers a person reads: never NaN.
    for h in Rocket::headlines(&s, &d) {
        assert!(h.value.is_finite(), "{comp:?} {ox}/{fuel}: headline {} is {}", h.label, h.value);
    }
    (fin.passed(), fin.failures().map(|o| o.to_string()).collect())
}

/// The whole schema space at both competitions, designed with the Eureka-2
/// mass calibration. Every pair passes every engineering check except two
/// failures that name their cause:
/// * budget, when a line has no public price (nitrogen at 4500 psi for the
///   LOX designs) -- the check gives the priced subtotal and the reason;
/// * parts_fit, when the engine needs a phenolic rod larger than the
///   largest priced one (4.5 in) -- the check gives both diameters.
///
/// Designs run in parallel (each is independent and deterministic).
#[test]
fn every_schema_pair_passes_or_names_its_cause() {
    let oxs = schema_enum("oxidizer");
    let fuels = schema_enum("fuel");
    assert!(oxs.contains(&"lox".to_string()) && oxs.contains(&"n2o".to_string()));
    let mut cases = Vec::new();
    for comp in [Competition::FarOut2027, Competition::Irec2025] {
        for ox in &oxs {
            for fuel in &fuels {
                cases.push((comp, ox.clone(), fuel.clone()));
            }
        }
    }
    let results: Vec<_> = std::thread::scope(|sc| {
        let hs: Vec<_> = cases.iter().map(|(c, o, f)| sc.spawn(move || ((*c, o.clone(), f.clone()), run(*c, o, f)))).collect();
        hs.into_iter().map(|h| h.join().expect("design thread panicked")).collect()
    });
    let mut bad = Vec::new();
    for ((c, o, f), (_, fails)) in &results {
        for line in fails {
            let ok = (line.contains(" budget ") && (line.contains("no public price") || line.contains("bulk contract")))
                || (line.contains(" parts_fit ") && line.contains("phenolic rod"));
            if !ok {
                bad.push(format!("{c:?} {o}/{f}: {line}"));
            }
        }
    }
    assert!(bad.is_empty(), "failures without a named cause:\n{}", bad.join("\n"));
    // The N2O designs need no unpriced gas: at IREC every one passes.
    for ((c, o, f), (ok, fails)) in &results {
        if *c == Competition::Irec2025 && o == "n2o" {
            assert!(ok, "{c:?} {o}/{f}: {fails:?}");
        }
    }
}

/// Liquid methane is modelled but has no retail price, so it is kept out
/// of the schema; asked for anyway, it designs and fails only the budget,
/// with the reason on the line.
#[test]
fn lox_methane_fails_only_budget_and_says_why() {
    assert!(!schema_enum("fuel").contains(&"methane".to_string()));
    let (ok, fails) = run(Competition::FarOut2027, "lox", "methane");
    assert!(!ok);
    assert_eq!(fails.len(), 1, "{fails:?}");
    assert!(fails[0].contains("budget") && fails[0].contains("bulk contract or quote"), "{}", fails[0]);
}

/// N2O beside a cryogenic fuel is refused at design time with the reason.
#[test]
fn n2o_with_cryogenic_fuel_is_refused_with_a_reason() {
    let e = Rocket::design(&spec(Competition::FarOut2027, "n2o", "methane")).unwrap_err();
    assert!(format!("{e:#}").contains("not modelled"), "{e:#}");
}

/// "n2o_gas" (the gas-phase CEA record's id, which the schema used to
/// offer) and "nitrous" mean the liquid, self-pressurised oxidiser.
#[test]
fn nitrous_aliases_map_to_the_liquid_run_tank() {
    for alias in ["n2o_gas", "N2O", "nitrous"] {
        assert_eq!(spec(Competition::FarOut2027, alias, "ethanol").oxidizer, "n2o");
    }
}
