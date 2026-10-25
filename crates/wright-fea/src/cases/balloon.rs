//! High-altitude balloon payload: the printed rack's most loaded shelf at
//! landing. wright-balloon has no structural check on the rack today (its
//! strength checks are the suspension line); this is new evidence.
//!
//! Geometry from the build's `layout.json` (inner size, shelf heights, every
//! placed part) and wright-balloon's rack constants (geometry.rs:
//! `SHELF_MM` 3, `POST_R_MM` 5, `POST_MARGIN_MM` 12, the wiring hole r 4 mm
//! at (w/2, 6 mm)); masses from the balloon catalog's `mass_g`; PETG as
//! the build prints it (`PRINT_MATERIAL = "petg"`), with wright-air's PETG
//! row (Bambu PETG HF TDS: E 1.9 GPa, 20 MPa allowable for layer lines).
//!
//! Load: touchdown at wright-balloon's landing-speed limit
//! (`LANDING_MAX_MPS`, 5 m/s) stopped over the foam wall's thickness:
//! a = v^2 / 2 d, plus 1 g. ASSUMPTION: the XPS wall crushes over its full
//! thickness at constant force (a lower bound on the peak; hard ground and
//! a corner-first landing are worse).

use super::Run;
use crate::deck::{Analysis, Material, Model, Section};
use crate::mesh::{norm, sub};
use crate::results::{solve, von_mises};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const SHELF_M: f64 = 0.003;
pub const POST_R_M: f64 = 0.005;
pub const POST_MARGIN_M: f64 = 0.012;
pub const HOLE_R_M: f64 = 0.004;
/// PETG modulus and allowable: wright-air's `rules::PETG` row (Bambu PETG HF
/// TDS: E 1.9 GPa; 20 MPa allowable for layer-line weakness), restated so
/// the balloon case builds without wright-air (a test pins them equal).
pub const PETG_E_PA: f64 = 1.9e9;
pub const PETG_ALLOWABLE_PA: f64 = 20e6;
/// PETG Poisson's ratio. ASSUMPTION: 0.38 (typical amorphous copolyester).
pub const PETG_NU: f64 = 0.38;
/// Solid PETG, kg/m^3 (wright-geom slice.rs `petg` 1.27 g/cm^3).
pub const PETG_DENSITY: f64 = 1270.0;

fn catalog_masses() -> Result<std::collections::HashMap<String, f64>> {
    let dir = std::env::var("WRIGHT_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("balloon/catalog.json")).context("balloon catalog")?)?;
    let mut m = std::collections::HashMap::new();
    for p in v["parts"].as_array().into_iter().flatten() {
        if let (Some(id), Some(g)) = (p["id"].as_str(), p["mass_g"].as_f64()) {
            m.insert(id.to_string(), g / 1e3);
        }
    }
    Ok(m)
}

pub fn run(design: &Value, build_dir: &Path, dir: &Path, media: bool) -> Result<wright_core::Manifest> {
    let mut run = Run::new(dir, "FEA: balloon payload rack at landing (CalculiX)");
    run.media = media;
    std::fs::create_dir_all(dir)?;
    let layout: Value = serde_json::from_str(&std::fs::read_to_string(build_dir.join("layout.json")).context("layout.json in the build")?)?;
    let inner: Vec<f64> = layout["inner_mm"].as_array().context("inner_mm")?.iter().filter_map(|x| x.as_f64()).collect();
    let shelves: Vec<f64> = layout["shelves"].as_array().context("shelves (printed rack only)")?.iter().filter_map(|x| x.as_f64()).collect();
    if design["enclosure"]["printed_tray"].as_bool() == Some(false) || shelves.len() < 2 {
        bail!("no printed rack above the floor: nothing for FEA to load");
    }
    let (w, dd) = (inner[0] / 1e3, inner[1] / 1e3);
    let masses = catalog_masses()?;
    // items per shelf (resting on shelf k: bottom at shelf z + SHELF)
    let placed = layout["placed"].as_array().context("placed")?;
    let mut per: Vec<Vec<(String, f64, [f64; 4])>> = vec![vec![]; shelves.len()];
    for p in placed {
        let part = p["part"].as_str().unwrap_or("");
        let z = p["min"][2].as_f64().unwrap_or(-1.0);
        let k = shelves.iter().position(|s| (s + SHELF_M * 1e3 - z).abs() < 0.6);
        let kg = masses.get(part).copied().with_context(|| format!("{part}: no mass_g in the balloon catalog"))?;
        if let Some(k) = k {
            let (x, y) = (p["min"][0].as_f64().unwrap_or(0.0) / 1e3, p["min"][1].as_f64().unwrap_or(0.0) / 1e3);
            let (sx, sy) = (p["size"][0].as_f64().unwrap_or(0.0) / 1e3, p["size"][1].as_f64().unwrap_or(0.0) / 1e3);
            per[k].push((p["name"].as_str().unwrap_or(part).to_string(), kg, [x, y, x + sx, y + sy]));
        }
    }
    // the heaviest shelf carried by posts (shelf 0 rests on the floor)
    let k = (1..shelves.len()).max_by(|&a, &b| per[a].iter().map(|i| i.1).sum::<f64>().total_cmp(&per[b].iter().map(|i| i.1).sum::<f64>())).unwrap();
    let load_kg: f64 = per[k].iter().map(|i| i.1).sum();
    let v = wright_balloon::rules::LANDING_MAX_MPS;
    let stop = design["enclosure"]["wall_mm"].as_f64().unwrap_or(25.4) / 1e3;
    let a = v * v / (2.0 * stop) + 9.80665;
    let g_load = a / 9.80665;

    let mesh = crate::gmsh::plate(&run.work("shelf"), w, dd, SHELF_M, 0.003, &[(w / 2.0, POST_MARGIN_M / 2.0, HOLE_R_M)], "SHELF")?;
    let mat = Material::iso("PETG", PETG_E_PA, PETG_NU, PETG_DENSITY, PETG_ALLOWABLE_PA, "Bambu PETG HF TDS (wright-air rules PETG)");
    let mut m = Model::new(mesh.clone(), Analysis::Static);
    m.materials.push(mat.clone());
    m.sections.push(Section::Solid { set: "SHELF".into(), material: "PETG".into() });
    let c = POST_MARGIN_M / 2.0;
    let posts = [[c, c], [w - c, c], [c, dd - c], [w - c, dd - c]];
    let on_post = |q: [f64; 3]| q[2].abs() < 1e-9 && posts.iter().any(|p| (q[0] - p[0]).hypot(q[1] - p[1]) <= POST_R_M + 1e-9);
    let fixed = mesh.select(on_post);
    if fixed.is_empty() {
        bail!("no shelf nodes over the posts");
    }
    m.fix(&fixed, 1, 3);
    // each item's weight x landing g as a pressure on the top faces under its footprint
    let top = mesh.faces_where(|q| (q[2] - SHELF_M).abs() < 1e-9);
    for (_, kg, fp) in &per[k] {
        let faces: Vec<&(usize, usize, Vec<usize>)> = top
            .iter()
            .filter(|(_, _, ns)| {
                let cx = ns.iter().map(|&n| mesh.node(n)[0]).sum::<f64>() / ns.len() as f64;
                let cy = ns.iter().map(|&n| mesh.node(n)[1]).sum::<f64>() / ns.len() as f64;
                cx >= fp[0] && cx <= fp[2] && cy >= fp[1] && cy <= fp[3]
            })
            .collect();
        let area: f64 = faces.iter().map(|(_, _, ns)| {
            let (p0, p1, p2) = (mesh.node(ns[0]), mesh.node(ns[1]), mesh.node(ns[2]));
            let (u, v) = (sub(p1, p0), sub(p2, p0));
            0.5 * norm([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]])
        }).sum();
        if area > 0.0 {
            let p = kg * a / area;
            m.face_pressure.extend(faces.iter().map(|(e, f, _)| (*e, *f, p)));
        }
    }
    let s = solve(&m, &run.work("shelf"), "landing")?;
    let b = s.block("STRESS").context("STRESS")?;
    // skip the post edges' singular corners: nodes within 1.5 mm of a post edge
    let (mut away, mut all) = (0.0f64, 0.0f64);
    for (n, st) in &b.data {
        let vm = von_mises(st);
        all = all.max(vm);
        let q = s.frd.nodes[n];
        if posts.iter().all(|p| ((q[0] - p[0]).hypot(q[1] - p[1]) - POST_R_M).abs() > 0.0015) {
            away = away.max(vm);
        }
    }
    let margin = mat.allowable_pa / away - 1.0;
    let dmax = s.max_displacement();
    let names: Vec<String> = per[k].iter().map(|i| i.0.clone()).collect();
    let summary = format!("shelf {} ({} items, {:.0} g: {}) at {g_load:.0} g ({v} m/s stopped over {:.0} mm of foam); PETG 3 mm on four 10 mm posts; {} C3D10 (gmsh)", k + 1, names.len(), load_kg * 1e3, names.join(", "), stop * 1e3, mesh.elements.len());
    run.picture(&s, "von_mises", "rack_shelf_landing_von_mises.png", &format!("Rack shelf {} at landing ({g_load:.0} g), von Mises", k + 1), &summary, [35.0, -60.0], &[("max_von_mises_mpa", away / 1e6), ("peak_at_post_edge_mpa", all / 1e6), ("allowable_mpa", mat.allowable_pa / 1e6), ("margin", margin), ("landing_g", g_load)])?;
    run.picture(&s, "disp_mm", "rack_shelf_landing_displacement.png", &format!("Rack shelf {} at landing, displacement", k + 1), "displacement magnitude (shape exaggerated)", [35.0, -60.0], &[("max_displacement_mm", dmax * 1e3)])?;
    run.cmp("rack shelf", &format!("landing {g_load:.0} g"), "max von Mises off the post edges, MPa", f64::NAN, away / 1e6, "no closed-form rack check exists in wright-balloon");
    run.manifest.check("rack_shelf_landing", margin > 0.0, &format!("shelf {} von Mises {:.1} MPa at {g_load:.0} g vs {:.0} MPa PETG allowable; sag {:.2} mm", k + 1, away / 1e6, mat.allowable_pa / 1e6, dmax * 1e3));
    run.finish()
}
