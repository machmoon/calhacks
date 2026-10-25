//! Write sim inputs for a design JSON and run the sim: `sim_probe DESIGN.json OUT_DIR`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d: wright_air::spec::Design = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let e = wright_air::checks::evaluate(&d, wright_air::powertrain::Factors::NOMINAL).unwrap();
    let m = wright_air::sim::run(&e, std::path::Path::new(&a[2])).unwrap();
    println!("{}", m.brief());
}
