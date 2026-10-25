//! Builder outputs: the enclosure's meshes (printed rack STLs sliced with
//! PrusaSlicer, foam panels as cut sheets), the packed layout, the BOM.

use crate::catalog::Catalog;
use crate::geometry;
use crate::model::{Design, Printed};
use anyhow::{Context, Result};
use std::path::Path;
use wright_core::{ArtifactKind, Artifacts};
use wright_geom::slice::{self, Recipe};

/// A per-run temp dir removed on drop (as wright-air's `print::Scratch`).
pub struct Scratch(pub std::path::PathBuf);

/// Prefix of every temp dir this crate makes (the cleanup test looks for it).
pub const SCRATCH_PREFIX: &str = "wright-balloon-scratch";

impl Scratch {
    pub fn new() -> std::io::Result<Scratch> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("{SCRATCH_PREFIX}-{}-{k}", std::process::id()));
        std::fs::create_dir_all(&p)?;
        Ok(Scratch(p))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Shelves: 3 perimeters, 20 % infill -- stiff plates at low mass.
const RECIPE: Recipe = Recipe::Structural { perimeters: 3, infill_pct: 20 };

/// Slice every printed rack piece; the sum is the rack's real mass.
pub fn slice_rack(d: &Design, cat: &Catalog, dir: &Path) -> Result<Printed> {
    std::fs::create_dir_all(dir)?;
    let l = geometry::layout(d, cat)?;
    let printer = slice::printer(geometry::PRINTER)?;
    let mat = slice::material(geometry::PRINT_MATERIAL)?;
    let (mut grams, mut seconds) = (0.0, 0.0);
    // the rack's solids come from FreeCAD (wright_geom::kernel); csg.rs
    // meshes only when FreeCAD is missing
    let mut rack = geometry::rack_meshes(&l);
    let opts = wright_geom::kernel::Options { equivalence: false, interference: false, exchange: false, stem: "rack".into(), ..Default::default() };
    wright_geom::kernel::realize(rack.iter_mut().map(|(n, m)| (n.as_str(), m)).collect(), dir, &opts);
    for (name, mesh) in rack {
        let stl = dir.join(format!("{name}.stl"));
        mesh.on_bed().write_stl(&stl)?;
        let s = slice::slice(&stl, &dir.join(format!("{name}.gcode")), printer, mat, RECIPE).with_context(|| format!("slicing {name}"))?;
        grams += s.grams;
        seconds += s.seconds;
    }
    Ok(Printed { grams, seconds, printer: printer.name.to_string() })
}

pub fn build(d: &Design, cat: &Catalog, out: &Path) -> Result<Artifacts> {
    std::fs::create_dir_all(out)?;
    let mut arts = Artifacts::default();
    let l = geometry::layout(d, cat)?;
    // every solid of the assembly, for viewing
    let mut solids = geometry::solids(d, cat, &l);
    // FreeCAD builds every solid (B-rep, watertight STL, AP242 STEP, GLB,
    // OCCT volumes and interference); csg.rs only as the fallback
    let cad = out.join("cad");
    let k = wright_geom::kernel::realize_parts(&mut solids, &cad, &wright_geom::kernel::Options { stem: "payload".into(), ..Default::default() });
    arts.add(format!("CAD kernel report: {}", k.summary()), cad.join("payload-kernel-report.json"));
    for (what, path) in k.files() {
        arts.add(format!("payload {what}"), path);
    }
    let mut all = wright_geom::Mesh::default();
    for p in &solids {
        let path = out.join(format!("{}.stl", p.name.replace('#', "_")));
        p.mesh.write_stl(&path)?;
        all = if all.is_empty() { p.mesh.clone() } else { wright_geom::Mesh { polygons: [all.polygons, p.mesh.polygons.clone()].concat(), solid: None } };
    }
    let asm = out.join("payload_assembly.stl");
    all.write_stl(&asm)?;
    arts.add_kind("payload assembly (STL, mm)", asm, ArtifactKind::Assembly);
    // the whole train (balloon, line, chute, box) at true scale, so the
    // picture is the vehicle and not only its payload box
    let train_dir = out.join("train");
    std::fs::create_dir_all(&train_dir)?;
    let mut train = wright_geom::Mesh::default();
    let mut train_parts = geometry::flight_train(d, cat)?;
    // true-scale balloon: a coarser chord tolerance keeps the STL small
    let opts = wright_geom::kernel::Options { deflection_mm: 0.5, interference: false, stem: "flight_train".into(), ..Default::default() };
    wright_geom::kernel::realize_parts(&mut train_parts, &cad, &opts);
    for p in train_parts {
        p.mesh.write_stl(&train_dir.join(format!("{}.stl", p.name)))?;
        train = if train.is_empty() { p.mesh } else { wright_geom::Mesh { polygons: [train.polygons, p.mesh.polygons].concat(), solid: None } };
    }
    let train_path = out.join("flight_train.stl");
    train.write_stl(&train_path)?;
    arts.add("flight train at true scale: balloon, line, parachute, box (STL, mm)", train_path);
    if d.enclosure.printed_tray {
        let p = slice_rack(d, cat, &out.join("print"))?;
        let path = out.join("print/summary.json");
        std::fs::write(&path, serde_json::to_string_pretty(&p)?)?;
        arts.add("printed rack G-code + slicer summary", path);
    }
    let lay = out.join("layout.json");
    std::fs::write(&lay, serde_json::to_string_pretty(&l)?)?;
    arts.add("packed layout", lay);
    let bom = out.join("bom.csv");
    std::fs::write(&bom, crate::bom(d, cat).csv())?;
    arts.add("bill of materials", bom);
    let dj = out.join("design.json");
    std::fs::write(&dj, serde_json::to_string_pretty(d)?)?;
    arts.add("design", dj);
    Ok(arts)
}
