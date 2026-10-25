//! WRIGHT_PYTHON=<venv>/bin/python cargo run --release -p wright-rocket --example rocket_sim -- <ox> <fuel> <out_dir> [reference]
//! Designs the eval's requirements spec on that pair at SEB's c* efficiency
//! (or, with `reference`, flies the Eureka-2 reconstruction) and flies it in RocketPy.
use wright_core::Domain;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&a[3]);
    let cat = wright_rocket::catalog::Catalog::load()?;
    let (p, _, eta) = wright_rocket::eval::reference_params(&cat)?;
    let spec = if a.get(4).map(String::as_str) == Some("reference") {
        wright_rocket::RocketSpec { pinned: Some(p), ..wright_rocket::eval::reference_spec(50.0 * wright_rocket::rules::FT) }
    } else {
        wright_rocket::RocketSpec { oxidizer: a[1].clone(), fuel: a[2].clone(), eta_cstar: Some(eta), ..wright_rocket::eval::requirements_spec() }
    };
    let d = wright_rocket::Rocket::design(&spec)?;
    let m = wright_rocket::sim::run(&spec, &d, &out)?;
    println!("{}", m.brief());
    Ok(())
}
