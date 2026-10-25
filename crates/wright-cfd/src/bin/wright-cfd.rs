//! wright-cfd vtol <result.json | design.json> <out_dir>
//! wright-cfd rocket <design.json> <out_dir> [--quick]
//! wright-cfd validate <out_dir> [--avl-only]
//!
//! Everything lands in <out_dir>/cfd with a manifest.json.

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use wright_cfd::{rocket, validate, vtol, Manifest};

fn read(p: &Path) -> anyhow::Result<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(p).with_context(|| format!("reading {}", p.display()))?).with_context(|| format!("parsing {}", p.display()))
}

fn threads() -> usize {
    std::env::var("WRIGHT_CFD_THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    match pos.first().map(|s| s.as_str()) {
        Some("vtol") if pos.len() >= 3 => {
            let v = read(Path::new(pos[1]))?;
            // an eval result (wright.design + wright.nominal) or a bare design
            let (design, nominal) = match v.get("wright") {
                Some(w) => (w["design"].clone(), w.get("nominal").cloned()),
                None => (v.get("design").cloned().unwrap_or(v.clone()), None),
            };
            let design: wright_air::spec::Design = serde_json::from_value(design).context("reading the VTOL design")?;
            let speed = nominal.as_ref().and_then(|n| n["best_range_speed_ms"].as_f64()).unwrap_or(17.0);
            let range = nominal.as_ref().and_then(|n| n["range_km_reserve"].as_f64()).unwrap_or(f64::NAN);
            let dir = PathBuf::from(pos[2]).join("cfd");
            let mut man = Manifest::new("cfd", format!("CFD: {}", design.name));
            let c = vtol::run(&design, range, speed, &dir, &mut man)?;
            let p = man.write(&dir)?;
            println!("{}", serde_json::to_string_pretty(&c)?);
            println!("manifest: {}", p.display());
        }
        Some("rocket") if pos.len() >= 3 => {
            let v = read(Path::new(pos[1]))?;
            let veh = v.get("vehicle").unwrap_or(&v);
            let aero: wright_rocket::aero::AeroBody = serde_json::from_value(veh["aero"].clone()).context("vehicle.aero")?;
            let engine: wright_rocket::engine::Engine = serde_json::from_value(veh["engine"].clone()).context("vehicle.engine")?;
            let machs: Vec<f64> = if flag("--quick") { vec![0.9, 1.1, 1.4] } else { vec![0.8, 0.9, 0.95, 1.0, 1.05, 1.1, 1.2, 1.4, 1.6, 2.0] };
            let dir = PathBuf::from(pos[2]).join("cfd");
            let mut man = Manifest::new("cfd", "CFD: rocket");
            let (rows, nz) = rocket::run(&aero, &engine, &machs, &dir, &mut man, threads())?;
            let p = man.write(&dir)?;
            for r in &rows {
                println!("M {:.2}: CD {:.4} -> {:.4} (nose wright-rocket {:.4}, OpenRocket {:.4}, SU2 {:.4})", r.mach, r.cd_wright, r.cd_with_su2_nose, r.nose_wright, r.nose_openrocket, r.nose_su2);
            }
            println!("{}", serde_json::to_string_pretty(&nz)?);
            if let Ok(d) = serde_json::from_value::<wright_rocket::RocketDesign>(v.clone()) {
                println!("{}", serde_json::to_string_pretty(&rocket::apogee_effect(&d, &rows))?);
            }
            println!("manifest: {}", p.display());
        }
        Some("nozzle") if pos.len() >= 3 => {
            let v = read(Path::new(pos[1]))?;
            let veh = v.get("vehicle").unwrap_or(&v);
            let engine: wright_rocket::engine::Engine = serde_json::from_value(veh["engine"].clone()).context("vehicle.engine")?;
            let dir = PathBuf::from(pos[2]).join("cfd");
            let (nz, c1, c2) = rocket::nozzle(&engine, &dir.join("rocket_su2"), threads())?;
            c1.save_png(&dir.join("nozzle_mach.png"))?;
            c2.save_png(&dir.join("nozzle_pressure.png"))?;
            println!("{}", serde_json::to_string_pretty(&nz)?);
        }
        Some("nose") if pos.len() >= 6 => {
            // wright-cfd nose vk|cone LENGTH RADIUS M1,M2,... OUT  (mesh studies: WRIGHT_CFD_FINE)
            let shape = if pos[1].as_str() == "cone" { rocket::NoseShape::Cone } else { rocket::NoseShape::VonKarman };
            let nose = rocket::Nose { shape, length: pos[2].parse()?, radius: pos[3].parse()? };
            let machs: Vec<f64> = pos[4].split(',').map(|s| s.parse()).collect::<Result<_, _>>()?;
            let dir = PathBuf::from(pos[5]).join("cfd");
            let (pts, frames) = rocket::nose_sweep(&nose, &machs, &dir, "nose", threads())?;
            for (p, f) in pts.iter().zip(&frames) {
                f.save_png(&dir.join(format!("nose_M{:.2}.png", p.mach)))?;
                println!("M {:.2}: CD_pressure {:.5} (SU2 body CD {:.5}; {} it, rms {:.1}, {:.0} s)", p.mach, p.cd_pressure, p.cd_su2_body, p.iterations, p.log10_rms_density, p.seconds);
            }
        }
        Some("validate") if pos.len() >= 2 => {
            let dir = PathBuf::from(pos[1]).join("cfd");
            let mut man = Manifest::new("cfd", "CFD validation");
            let mut lines = validate::run_avl(&dir.join("validation_avl"))?;
            if !flag("--avl-only") {
                lines.extend(validate::run_su2(&dir, &mut man, threads())?.lines);
            }
            validate::write(&dir, &lines, &mut man)?;
            let p = man.write(&dir)?;
            for l in &lines {
                println!("{:<34} {:<30} ref {:>9.4}  got {:>9.4}  {:+6.2} %", l.case, l.quantity, l.reference, l.computed, l.error_pct);
            }
            println!("manifest: {}", p.display());
        }
        _ => bail!("usage: wright-cfd vtol <result.json> <out> | rocket <design.json> <out> [--quick] | validate <out> [--avl-only]"),
    }
    Ok(())
}
