//! Wright's aero (aero.rs, a port of AeroSandbox's AeroBuildup) on a design,
//! as JSON, for `sim/aero_audit.py` to compare with AeroBuildup itself:
//! `aero_audit DESIGN.json > wright_aero.json`.
use wright_air::aero::{oswald_inviscid, parasite, trim, WingModel};
use wright_core::units::Speed;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d: wright_air::spec::Design = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let e = wright_air::checks::evaluate(&d, wright_air::powertrain::Factors::NOMINAL).unwrap();
    let lay = &e.layout;
    let air = wright_air::atmosphere::isa(wright_core::units::Length::m(d.requirements.altitude_m), 0.0);
    let v = e.perf.best_range.as_ref().map(|c| c.speed).unwrap_or(Speed::mps(18.0));
    let wm = WingModel::new(lay, v, &air);
    let osw = oswald_inviscid(lay.taper, lay.aspect_ratio, lay.pod_width / lay.span);
    let rows: Vec<[f64; 5]> = (-8..=16).map(|k| {
        let al = k as f64;
        let (cl, cdp, cm) = wm.coefficients(al);
        [al, cl, cdp, cl * cl / (std::f64::consts::PI * lay.aspect_ratio * osw), cm]
    }).collect();
    let speeds: Vec<serde_json::Value> = [12.0, 15.0, 18.0, 21.0, 24.0].iter().map(|&s| {
        let t = trim(lay, Speed::mps(s), &air, 1.0, wright_air::powertrain::Factors::NOMINAL);
        serde_json::json!({"v": s, "alpha_deg": t.alpha_deg, "cl_wing": t.cl_wing, "drag_n": t.drag.value(), "areas": t.areas, "weight_n": t.weight.value()})
    }).collect();
    let p = parasite(lay, v, &air);
    println!("{}", serde_json::json!({"v": v.value(), "rho": air.density.value(), "area": lay.area.as_m2(), "ar": lay.aspect_ratio, "oswald": osw, "cl_max": wm.cl_max(),
        "wing": rows, "parasite_at_v": p, "trims": speeds, "sim": wright_air::sim::inputs(&e)}));
}
