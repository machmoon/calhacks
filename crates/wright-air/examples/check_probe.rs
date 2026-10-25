//! Print one check's outcome for design JSON files: `check_probe CHECK DESIGN.json...`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for p in &a[2..] {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let v = v.get("wright").and_then(|w| w.get("design")).cloned().unwrap_or(v);
        let d: wright_air::spec::Design = serde_json::from_value(v).unwrap();
        let e = wright_air::checks::evaluate(&d, wright_air::powertrain::Factors::NOMINAL).unwrap();
        let f = wright_air::checks::physics_only(&e);
        println!("{p}: boom {} | {}", d.structure.boom_tube, f.get(&a[1]).map(|o| o.to_string()).unwrap_or("none".into()));
    }
}
