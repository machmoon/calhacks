//! Evaluate one hand-picked kit combination (debugging aid):
//!   cargo run -p wright-balloon --example probe -- <camera> <primary> <secondary> [backup] <balloon> <chute> <wall_mm> <fill_mps>
use wright_balloon::catalog::catalog;
use wright_balloon::{checks, design, reference};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n = a.len();
    let kit_ids = &a[..n - 4];
    let ks: Vec<_> = kit_ids.iter().map(|id| design::kits().iter().find(|k| &k.id == id).expect(id)).collect();
    let cat = catalog();
    let mut d = design::assemble("probe", &ks, &["aprs-ham-ht".to_string()]);
    d.balloon = a[n - 4].clone();
    d.parachute = a[n - 3].clone();
    d.gas = "he-uhp-300".into();
    d.fill_target_mps = a[n - 1].parse()?;
    design::size_enclosure(&mut d, cat, a[n - 2].parse()?)?;
    design::size_fill(&mut d, cat)?;
    let spec = reference::mission_spec(33_570.0, 1209.43);
    let (f, an) = checks::evaluate(&spec, &d, cat);
    println!("{f}");
    if let Some(an) = an {
        println!("box {:?} min {:.1} max {:.1}; package {:.0} g", d.enclosure.inner_mm, an.box_min_c, an.box_max_c, an.masses.package().as_g());
    }
    Ok(())
}
