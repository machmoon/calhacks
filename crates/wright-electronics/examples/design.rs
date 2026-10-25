//! `cargo run -p wright-electronics --example design -- SPEC.json OUT_DIR`:
//! design, check and build one spec with no agents (what `wright
//! electronics design` does, without building the CLI).
use wright_core::Domain;
use wright_electronics::{Electronics, Spec};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let spec: Spec = serde_json::from_str(&std::fs::read_to_string(&a[0])?)?;
    let out = std::path::PathBuf::from(a.get(1).cloned().unwrap_or_else(|| "out/electronics".into()));
    std::fs::create_dir_all(&out)?;
    let t = std::time::Instant::now();
    let d = Electronics::design(&spec)?;
    let design_s = t.elapsed().as_secs_f64();
    std::fs::write(out.join("design.json"), serde_json::to_string_pretty(&d)?)?;
    let f = Electronics::check(&spec, &d);
    std::fs::write(out.join("checks.json"), serde_json::to_string_pretty(&f)?)?;
    let arts = Electronics::build(&d, &out)?;
    println!("{f}");
    println!("design {design_s:.2} s, total {:.2} s; place {:?} moves {} ; route {:.2} s", t.elapsed().as_secs_f64(), d.place.strategy, d.place.moves, d.route.seconds);
    for (l, p) in &arts.files {
        println!("  {l}: {}", p.display());
    }
    Ok(())
}
