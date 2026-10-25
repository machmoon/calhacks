//! `Domain::design_under`: a reviewer-confirmed attack must not be undone
//! by the next design. And `build` must leave nothing behind outside its
//! output directory (wright-air once leaked 8 GB of slices into $TMPDIR).

use wright_core::{Condition, Domain};
use wright_rocket::domain::{Rocket, RocketSpec};

fn spec() -> RocketSpec {
    serde_json::from_value(serde_json::json!({
        "name": "N2O/RP-1, FAR-OUT 2027 Group B",
        "competition": "FarOut2027",
        "contract_apogee_ft": 20000.0,
        "oxidizer": "n2o",
        "fuel": "rp1",
        "budget_usd": 15000.0
    }))
    .unwrap()
}

/// A confirmed attack: the team's launcher at FAR is a 10 ft rail (3 m
/// shorter than the 20 ft the spec assumed), and the design leaves it too
/// slowly. The design returned under that condition leaves the 10 ft rail
/// at the rule's speed and still passes every check of the spec; or the
/// error says plainly which attack no design survives.
#[test]
fn design_under_a_shorter_rail_attack() {
    let base = spec();
    let mut attacked = base.clone();
    attacked.rail_length_m = Some(10.0 * 0.3048);
    let cond = Condition { id: "t-r1-1".into(), check: "rail_exit_speed".into(), claim: "10 ft rail at the FAR pad".into(), spec: attacked };
    // The plain design does fail the attack (otherwise the test proves nothing).
    let plain = Rocket::design(&base).unwrap();
    assert!(!Rocket::check(&cond.spec, &plain).get("rail_exit_speed").unwrap().passed(), "the attack should break the unconditioned design");
    match Rocket::design_under(&base, std::slice::from_ref(&cond)) {
        Ok(d) => {
            let under = Rocket::check(&cond.spec, &d);
            assert!(under.get("rail_exit_speed").unwrap().passed(), "{under}");
            let fin = Rocket::check(&base, &d);
            assert!(fin.passed(), "{fin}");
            println!("design_under: {} | BOM ${:.0} | {}", under.get("rail_exit_speed").unwrap(), Rocket::bom(&d).total().as_usd(), d.search_log.last().unwrap());
        }
        Err(e) => {
            let m = e.to_string();
            assert!(m.contains("confirmed attacks") && m.contains("rail_exit_speed") && m.contains("closest"), "{m}");
            println!("design_under refused: {m}");
        }
    }
}

/// A milder attack the search can answer: a 15 ft rail. The design sized
/// for it is returned, re-stamped to the spec's 20 ft rail and contract.
#[test]
fn design_under_answers_a_15_ft_rail() {
    let base = spec();
    let mut attacked = base.clone();
    attacked.rail_length_m = Some(15.0 * 0.3048);
    let cond = Condition { id: "t-r1-2".into(), check: "rail_exit_speed".into(), claim: "15 ft rail".into(), spec: attacked };
    let d = Rocket::design_under(&base, std::slice::from_ref(&cond)).unwrap_or_else(|e| panic!("{e}"));
    assert!(Rocket::check(&cond.spec, &d).get("rail_exit_speed").unwrap().passed());
    assert!(Rocket::check(&base, &d).passed(), "{}", Rocket::check(&base, &d));
}

#[test]
fn design_under_without_conditions_is_design() {
    let a = serde_json::to_string(&Rocket::design_under(&spec(), &[]).unwrap().params).unwrap();
    let b = serde_json::to_string(&Rocket::design(&spec()).unwrap().params).unwrap();
    assert_eq!(a, b);
}

#[test]
fn unknown_condition_check_is_refused() {
    let cond = Condition { id: "x".into(), check: "no_such_check".into(), claim: String::new(), spec: spec() };
    assert!(Rocket::design_under(&spec(), &[cond]).unwrap_err().to_string().contains("unknown check"));
}

/// `build` (STLs, the sliced fin jig, drawings, BOM) writes only into its
/// output directory: nothing slicer- or rocket-shaped appears in $TMPDIR.
#[test]
fn build_leaves_no_temp_files() {
    let tmp = std::env::temp_dir();
    let suspicious = |n: &str| {
        let n = n.to_ascii_lowercase();
        ["rocket", "jig", "slice", "prusa", ".stl", ".gcode", ".3mf", ".ini"].iter().any(|k| n.contains(k))
    };
    let list = || -> std::collections::BTreeSet<String> {
        std::fs::read_dir(&tmp).map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| suspicious(n)).collect()).unwrap_or_default()
    };
    let before = list();
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("rocket-build-leak-test");
    let _ = std::fs::remove_dir_all(&out);
    let s = RocketSpec { oxidizer: "lox".into(), fuel: "propane".into(), ..spec() };
    let d = Rocket::design(&s).unwrap();
    Rocket::build(&d, &out).unwrap();
    let after = list();
    let new: Vec<_> = after.difference(&before).collect();
    std::fs::remove_dir_all(&out).unwrap();
    assert!(new.is_empty(), "build left files in {}: {new:?}", tmp.display());
}
