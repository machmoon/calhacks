//! Reads the same JSON cases as `wright-oracle` (haskell/) on stdin and
//! answers them with the Rust crates' own functions. Cases a domain does not
//! cover (or whose feature is off) come back with "error".

use serde_json::{json, Map, Value};
use std::io::Read;

#[cfg(feature = "electronics")]
mod pcb;

fn num(c: &Map<String, Value>, k: &str) -> Option<f64> {
    c.get(k).and_then(|v| v.as_f64().or_else(|| v.as_bool().map(|b| b as u8 as f64)))
}

#[allow(unused_variables)]
fn eval(fn_: &str, c: &Map<String, Value>) -> Result<Value, String> {
    match fn_ {
        #[cfg(feature = "rocket")]
        "us76" => {
            let a = wright_rocket::atmosphere::at(num(c, "z_m").ok_or("z_m")?);
            Ok(json!({"t_k": a.t, "p_pa": a.p, "rho_kg_m3": a.rho, "mu_pa_s": a.mu, "a_m_s": a.a}))
        }
        #[cfg(feature = "balloon")]
        "us76_balloon" => {
            use wright_core::units::Length;
            let a = wright_balloon::atmosphere::at(Length::m(num(c, "z_m").ok_or("z_m")?));
            Ok(json!({"t_k": a.temperature.as_k(), "p_pa": a.pressure.as_pa(),
                      "rho_kg_m3": a.density.as_kg_m3(), "mu_pa_s": a.viscosity}))
        }
        #[cfg(feature = "air")]
        "us76_air" => {
            use wright_core::units::Length;
            let a = wright_air::atmosphere::isa(Length::m(num(c, "z_m").ok_or("z_m")?), 0.0);
            Ok(json!({"t_k": a.temperature.as_k(), "p_pa": a.pressure.as_pa(),
                      "rho_kg_m3": a.density.as_kg_m3(), "mu_pa_s": a.viscosity_pa_s}))
        }
        #[cfg(feature = "rocket")]
        "nozzle_mach" => {
            use wright_rocket::ideal::*;
            let (g, m) = (num(c, "gamma").ok_or("gamma")?, num(c, "mach").ok_or("mach")?);
            Ok(json!({"area_ratio": area_ratio_from_mach(m, g), "p_ratio": pressure_ratio_from_mach(m, g)}))
        }
        #[cfg(feature = "rocket")]
        "nozzle_area" => {
            use wright_rocket::ideal::*;
            let (g, e) = (num(c, "gamma").ok_or("gamma")?, num(c, "area_ratio").ok_or("area_ratio")?);
            Ok(json!({"mach": supersonic_mach(e, g), "p_ratio": exit_pressure_ratio(e, g)}))
        }
        #[cfg(feature = "rocket")]
        "nozzle_cf" => {
            use wright_rocket::ideal::*;
            let g = num(c, "gamma").ok_or("gamma")?;
            let pe = num(c, "pe_pc").ok_or("pe_pc")?;
            let pa = num(c, "pa_pc").unwrap_or(0.0);
            let e = num(c, "area_ratio").ok_or("area_ratio")?;
            Ok(json!({"cf": thrust_coefficient(g, pe, pa, e), "cf_opt": thrust_coefficient(g, pe, pe, e)}))
        }
        #[cfg(feature = "rocket")]
        "nozzle_cstar" => {
            let g = num(c, "gamma").ok_or("gamma")?;
            let t = num(c, "tc_k").ok_or("tc_k")?;
            let m = num(c, "molar_mass_kg_mol").ok_or("molar_mass_kg_mol")?;
            Ok(json!({"cstar_m_s": wright_rocket::ideal::c_star(t, m, g)}))
        }
        #[cfg(feature = "balloon")]
        "balloon" => {
            use wright_balloon::physics::*;
            use wright_core::units::*;
            let rho_a = num(c, "rho_air").unwrap_or(1.205);
            let rho_g = num(c, "rho_gas").unwrap_or(0.1786);
            let cd = num(c, "cd").unwrap_or(0.3);
            let r = num(c, "radius_m").ok_or("radius_m")?;
            let v = 4.0 / 3.0 * std::f64::consts::PI * r.powi(3);
            let carried = Mass::kg(num(c, "balloon_kg").ok_or("balloon_kg")? + num(c, "payload_kg").ok_or("payload_kg")?);
            let gross = Mass::kg(v * (rho_a - rho_g));
            let free = free_lift(gross, carried);
            let mut air = wright_balloon::atmosphere::at(Length::ZERO);
            air.density = Density::kg_m3(rho_a);
            let mut out = json!({"free_lift_n": free.as_n(),
                                 "ascent_m_s": ascent_rate(free, Volume::m3(v), cd, &air).as_mps()});
            if let Some(d) = num(c, "burst_diameter_m") {
                out["burst_altitude_m"] = json!(cusf_burst_altitude(Volume::m3(v), Length::m(d)).as_m());
            }
            Ok(out)
        }
        #[cfg(feature = "balloon")]
        "balloon_fill" => {
            use wright_balloon::physics::*;
            use wright_core::units::*;
            let (v, neck) = cusf_fill_for_ascent(
                Mass::kg(num(c, "balloon_kg").ok_or("balloon_kg")?),
                Mass::kg(num(c, "payload_kg").ok_or("payload_kg")?),
                num(c, "cd").unwrap_or(0.3),
                num(c, "rho_gas").unwrap_or(0.1786),
                Speed::mps(num(c, "ascent_m_s").ok_or("ascent_m_s")?),
            );
            Ok(json!({"volume_m3": v.as_m3(), "neck_lift_kg": neck.as_kg()}))
        }
        // structures: the boom's checks in wright-air perf.rs, on a catalog
        // tube built from the case's diameters
        #[cfg(feature = "air")]
        "tube_bending" => {
            let t = probe_tube(num(c, "od_mm").ok_or("od_mm")?, num(c, "id_mm").ok_or("id_mm")?);
            Ok(json!({"stress_pa": wright_air::perf::bending_stress(&t, num(c, "moment_nm").ok_or("moment_nm")?)}))
        }
        #[cfg(feature = "air")]
        "tube_buckling" => {
            let t = probe_tube(num(c, "od_mm").ok_or("od_mm")?, num(c, "id_mm").ok_or("id_mm")?);
            let (s, g, rt) = wright_air::perf::shell_buckling(&t, num(c, "modulus_pa").ok_or("modulus_pa")?);
            Ok(json!({"stress_pa": s, "knockdown": g, "r_over_t": rt}))
        }
        #[cfg(feature = "rocket")]
        "lame" => {
            let (p, ri, ro) = (num(c, "p_pa").ok_or("p_pa")?, num(c, "ri_m").ok_or("ri_m")?, num(c, "ro_m").ok_or("ro_m")?);
            let (vm, hoop, axial, radial) = wright_rocket::domain::lame_von_mises(p, ri, ro);
            Ok(json!({"hoop_pa": hoop, "axial_pa": axial, "radial_pa": radial, "von_mises_pa": vm}))
        }
        #[cfg(feature = "rocket")]
        "flutter" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            Ok(json!({"vf_m_s": wright_rocket::domain::flutter_speed(g("a_m_s")?, g("p_pa")?, g("g_pa")?, g("span_m")?, g("root_m")?, g("tip_m")?, g("t_m")?)}))
        }
        #[cfg(feature = "rocket")]
        "bulkhead" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            Ok(json!({"t_m": wright_rocket::vehicle::bulkhead_thickness(g("radius_m")?, g("p_pa")?, g("sf")?, g("allow_pa")?)}))
        }
        // radio: the balloon's link budget, wright-balloon radio.rs
        #[cfg(feature = "balloon")]
        "fspl" => Ok(json!({"loss_db": wright_balloon::radio::fspl_db(num(c, "d_m").ok_or("d_m")?, num(c, "f_mhz").ok_or("f_mhz")?)})),
        #[cfg(feature = "balloon")]
        "radio_geometry" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            let (slant, el, within) = wright_balloon::radio::geometry(g("ground_m")?, g("station_alt_m")?, g("payload_alt_m")?);
            Ok(json!({"slant_m": slant, "elevation_rad": el, "within_horizon": within as u8}))
        }
        #[cfg(feature = "balloon")]
        "dipole_gain" => Ok(json!({"gain_dbi": wright_balloon::radio::dipole_gain_dbi(num(c, "peak_dbi").ok_or("peak_dbi")?, num(c, "el_rad").ok_or("el_rad")?)})),
        // GNSS coexistence: radio.rs, composed as analysis.rs builds its
        // `Coexist` rows (coupled = tx + gain - fspl(sep); fundamental through
        // the patch rejection; harmonics 60 dB under the carrier)
        #[cfg(feature = "balloon")]
        "patch_rejection" => Ok(json!({"rejection_db": wright_balloon::radio::patch_rejection_db(num(c, "f_mhz").ok_or("f_mhz")?)})),
        #[cfg(feature = "balloon")]
        "gnss_limits" => Ok(json!({"blocking_dbm": wright_balloon::radio::gnss_blocking_limit_dbm(), "inband_dbm": wright_balloon::radio::gnss_inband_limit_dbm()})),
        #[cfg(feature = "balloon")]
        "gnss_harmonics" => {
            let hs = wright_balloon::radio::harmonics_in_l1(num(c, "f_mhz").ok_or("f_mhz")?);
            Ok(json!({"count": hs.len(), "lowest": hs.iter().min().copied().unwrap_or(0)}))
        }
        #[cfg(feature = "balloon")]
        "gnss_coexist" => {
            use wright_balloon::radio::*;
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            let f = g("f_mhz")?;
            let coupled = g("tx_dbm")? + g("gain_dbi")? - fspl_db(g("sep_m")?, f);
            Ok(json!({"coupled_dbm": coupled, "at_gnss_dbm": coupled - patch_rejection_db(f), "harmonic_dbm": coupled - 60.0}))
        }
        // thermal: rules.rs warmer power, analysis.rs capacity table
        #[cfg(feature = "balloon")]
        "warmer_power" => Ok(json!({"watts": wright_balloon::rules::warmer_power_sl_w(num(c, "rated_h").ok_or("rated_h")?)})),
        // the lumped box: `steps` forward-Euler steps of analysis.rs box_step
        #[cfg(feature = "balloon")]
        "box_thermal" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            let (cap, rth, p, amb, dt) = (g("heat_cap_j_k")?, g("r_th_k_w")?, g("p_w")?, g("amb_c")?, g("dt_s")?);
            let mut t = g("t0_c")?;
            for _ in 0..g("steps")? as usize {
                t = wright_balloon::analysis::box_step(cap, rth, p, amb, dt, t);
            }
            Ok(json!({"t_c": t}))
        }
        #[cfg(feature = "balloon")]
        "capacity_fraction" => {
            use wright_balloon::catalog::Chemistry;
            let chem = match c.get("chemistry").and_then(Value::as_str).unwrap_or("") {
                "lipo" => Chemistry::Lipo,
                "li_ion" => Chemistry::LiIon,
                "lithium_primary" => Chemistry::LithiumPrimary,
                other => return Err(format!("chemistry {other:?}")),
            };
            Ok(json!({"fraction": wright_balloon::analysis::capacity_fraction(chem, num(c, "t_c").ok_or("t_c")?)}))
        }
        // rocket aero (aero.rs), chute (domain.rs), closures (vehicle.rs)
        #[cfg(feature = "rocket")]
        "von_karman" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            Ok(json!({"radius_m": wright_rocket::aero::von_karman_radius(g("x_m")?, g("l_m")?, g("r_m")?)}))
        }
        #[cfg(feature = "rocket")]
        "stagnation_cd" => Ok(json!({"cd": wright_rocket::aero::stagnation_cd(num(c, "mach").ok_or("mach")?)})),
        #[cfg(feature = "rocket")]
        "nose_cd" => Ok(json!({"cd": wright_rocket::aero::von_karman_pressure_cd(num(c, "mach").ok_or("mach")?, num(c, "fineness").ok_or("fineness")?)})),
        #[cfg(feature = "rocket")]
        "descent_rate" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            Ok(json!({"v_m_s": wright_rocket::domain::descent_rate(g("mass_kg")?, g("diameter_in")?, g("cd")?, g("rho")?)}))
        }
        #[cfg(feature = "rocket")]
        "closure_capacity" => {
            let (s, b, t) = wright_rocket::vehicle::closure_capacity(num(c, "wall_m").ok_or("wall_m")?);
            Ok(json!({"shear_n": s, "bearing_n": b, "tearout_n": t}))
        }
        #[cfg(feature = "rocket")]
        "closure_screws" => {
            let g = |k: &str| num(c, k).ok_or(k.to_string());
            Ok(json!({"n": wright_rocket::vehicle::closure_screws(g("inner_d_m")?, g("wall_m")?, g("meop_pa")?)}))
        }
        #[cfg(feature = "electronics")]
        f if f.starts_with("pcb_") => pcb::eval(f, c),
        _ => Err(format!("{fn_}: not exposed by the enabled crates")),
    }
}

/// A catalog tube with only the geometry filled in: the structural
/// functions read `od`, `id` and nothing else from it.
#[cfg(feature = "air")]
fn probe_tube(od_mm: f64, id_mm: f64) -> wright_air::catalog::Tube {
    use wright_air::catalog::{Row, Stock, Tube, TubeKind};
    use wright_core::units::Length;
    Tube {
        row: Row {
            category: "tube",
            id: "probe".into(),
            name: "probe".into(),
            provenance: wright_core::Provenance { url: String::new(), retrieved: String::new() },
            price: None,
            available: true,
            stock: Stock::Unstated,
            stock_evidence: None,
        },
        kind: TubeKind::Pultruded,
        od: Length::m(od_mm * 1e-3),
        id: Some(Length::m(id_mm * 1e-3)),
        stock_length: Length::m(1.0),
        kg_per_m: 0.1,
    }
}

fn main() {
    // `wright-crosscheck gen-pcb FIXTURES OUT`: write the synthetic boards.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("gen-pcb") {
        #[cfg(feature = "electronics")]
        {
            let names = pcb::gen(std::path::Path::new(&args[2]), std::path::Path::new(&args[3])).expect("gen-pcb");
            eprintln!("wrote {} synthetic boards: {}", names.len(), names.join(", "));
            return;
        }
        #[cfg(not(feature = "electronics"))]
        panic!("gen-pcb needs the electronics feature");
    }
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).expect("stdin");
    let cases: Vec<Value> = serde_json::from_str(&s).expect("a JSON array of cases");
    let out: Vec<Value> = cases
        .into_iter()
        .map(|mut c| {
            let obj = c.as_object_mut().expect("case object");
            let f = obj.get("rust_fn").or(obj.get("fn")).and_then(Value::as_str).unwrap_or("").to_string();
            match eval(&f, obj) {
                Ok(Value::Object(m)) => obj.extend(m),
                Ok(_) => {}
                Err(e) => {
                    obj.insert("error".into(), json!(e));
                }
            }
            c
        })
        .collect();
    println!("{}", serde_json::to_string(&out).unwrap());
}
