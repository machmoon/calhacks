//! Export the domain's existing geometry for independent B-rep reconstruction.
//! Usage: cargo run -p wright-air --example cad_geometry -- design.json geometry.json
use serde_json::json;
use wright_air::{geometry::{self, Shape}, layout, spec::Design};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 { return Err("usage: cad_geometry DESIGN.json OUTPUT.json".into()); }
    let design: Design = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let layout = layout::solve(&design)?;
    let parts: Vec<_> = geometry::parts(&layout).into_iter().map(|p| {
        let shape = match p.shape {
            Shape::Box{min,max} => json!({"type":"box","min":min,"max":max}),
            Shape::Cyl{p0,p1,r} => json!({"type":"tube","p0":p0,"p1":p1,"r_out":r,"r_in":0.0}),
            Shape::Tube{p0,p1,r_out,r_in} => json!({"type":"tube","p0":p0,"p1":p1,"r_out":r_out,"r_in":r_in}),
            Shape::Loft{rings} => json!({"type":"loft","rings":rings}),
            Shape::Shell{outer,inner} => json!({"type":"shell","rings":outer,"inner":inner}),
        };
        json!({"name":p.name,"shape":shape,"filament":p.filament,"touches":p.touches,"envelope":p.disc})
    }).collect();
    std::fs::write(&args[2],serde_json::to_vec_pretty(&json!({"name":design.name,"unit":"m","parts":parts}))?)?;
    Ok(())
}
