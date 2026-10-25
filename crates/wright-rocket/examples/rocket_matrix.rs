//! cargo run --release -p wright-rocket --example rocket_matrix
//! Designs every oxidizer/fuel pair in the schema at both competitions from
//! a requirements-only spec and prints the outcome of each.
//! `-- --shard i/n --json OUT.json` runs one slice of the (filtered) cases
//! and writes `{"kind": "rocket_matrix", "rows": [...]}` for `wright merge`
//! (wright_core::shard, docs/aci.md).
use wright_core::Domain;
use wright_rocket::domain::{Competition, Rocket, RocketSpec};
fn main() {
    let mut filter: Vec<String> = vec![];
    let (mut shard, mut json_out) = (wright_core::shard::Shard::ALL, None::<std::path::PathBuf>);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--shard" => shard = args.next().expect("--shard wants i/n").parse().unwrap_or_else(|e| panic!("{e}")),
            "--json" => json_out = args.next().map(Into::into),
            _ => filter.push(a),
        }
    }
    let mut cases = vec![];
    for comp in [Competition::FarOut2027, Competition::Irec2025] {
        for ox in ["lox", "n2o"] {
            for fuel in ["propane", "ethanol", "rp1", "methane", "ipa"] {
                let tag = format!("{comp:?}/{ox}/{fuel}");
                if filter.is_empty() || filter.iter().any(|f| tag.contains(f.as_str())) { cases.push((comp, ox, fuel)); }
            }
        }
    }
    let mut rows = vec![];
    for (comp, ox, fuel) in shard.select(cases) {
                let tag = format!("{comp:?}/{ox}/{fuel}");
                let apogee = if comp == Competition::Irec2025 { 10_000.0 } else { 20_000.0 };
                let spec = RocketSpec { name: tag.clone(), competition: comp, contract_apogee_ft: apogee, oxidizer: ox.into(), fuel: fuel.into(), budget_usd: None, payload_kg: 0.0, rail_length_m: None, measured_dry: None, eta_cstar: None, nose_cd_table: None, pinned: None, dry_mass_factor: None };
                let t = std::time::Instant::now();
                match Rocket::design(&spec) {
                    Ok(d) => {
                        let f = Rocket::check(&spec, &d);
                        let n = f.outcomes.iter().filter(|o| o.passed()).count();
                        rows.push(serde_json::json!({"id": tag, "checks_passed": n, "checks_total": f.outcomes.len(), "passed": f.passed(), "seconds": t.elapsed().as_secs_f64(), "pc_bar": d.params.engine.pc/1e5, "of": d.params.engine.of, "bom_usd": Rocket::bom(&d).total().as_usd(), "failures": f.failures().map(|o| o.to_string()).collect::<Vec<_>>()}));
                        println!("{tag}: {n}/{} in {:.0}s  Pc {:.0} O/F {:.2} BOM ${:.0}", f.outcomes.len(), t.elapsed().as_secs_f64(), d.params.engine.pc/1e5, d.params.engine.of, Rocket::bom(&d).total().as_usd());
                        for o in f.failures() { println!("    FAIL {o}"); }
                        if std::env::var("VERBOSE").is_ok() {
                            for k in [278.15, 293.15, 303.15] {
                                let mut p = d.params.clone();
                                p.ox_fill_k = Some(k);
                                let cat = wright_rocket::catalog::Catalog::load().unwrap();
                                if let Ok(v) = wright_rocket::vehicle::build(&p, &cat) {
                                    if let Some(pr) = &v.profile {
                                        let n = pr.pc.len() - 1;
                                        println!("    {k} K: burn {:.1} s Pc {:.1}->{:.1} bar ptank {:.1}->{:.1} O/F {:.2}->{:.2} pf {:.1} bar, ox {:.1} kg fuel {:.1} kg", pr.t_end, pr.pc[0]/1e5, pr.pc[n]/1e5, pr.p_tank[0]/1e5, pr.p_tank[n]/1e5, pr.mdot_ox[0]/pr.mdot_f[0], pr.mdot_ox[n]/pr.mdot_f[n], v.fuel_tank.p_op/1e5, v.ox_tank.propellant_kg, v.fuel_tank.propellant_kg);
                                    }
                                }
                            }
                            if let Some(t) = d.vehicle.engine.triplet() { println!("    {t:?}"); }
                        }
                    }
                    Err(e) => {
                        println!("{tag}: ERROR {e:#}");
                        rows.push(serde_json::json!({"id": tag, "passed": false, "error": format!("{e:#}")}));
                    }
                }
    }
    if let Some(p) = json_out {
        let v = serde_json::json!({"kind": "rocket_matrix", "shard": shard.to_string(), "rows": rows});
        std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap()).unwrap_or_else(|e| panic!("writing {}: {e}", p.display()));
        println!("rows: {}", p.display());
    }
}
