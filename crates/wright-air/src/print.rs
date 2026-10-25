//! Printing: every printed part split to the bed, stood on it, and sliced by
//! PrusaSlicer (through `wright_geom::slice`). The slicer's own time and
//! grams are evidence for the `print_time` and `printed_mass` checks
//! (Source::Slicer), and its G-code is a build artifact.
//!
//! Skins (wing, tail, pod: the shell filament) print in vase mode, one
//! foamed wall, as the reference build's were; fittings (mounts, clamps,
//! joints, legs) print with 4 perimeters and 40 % infill.

use crate::geometry::{self, PartGeometry, Shape};
use crate::layout::Layout;
use crate::rules::*;
use std::path::{Path, PathBuf};
use wright_geom::slice::{self, Recipe};
use wright_geom::{v3, Mesh};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Piece {
    pub part: String,
    pub index: usize,
    pub filament: String,
    pub stl: PathBuf,
    pub gcode: PathBuf,
    pub seconds: f64,
    pub grams: f64,
}

/// The part's own long axis vertical (a wing panel stands on its root rib,
/// a pod section on its bulkhead), resting on the bed.
fn stand(m: &Mesh, axis: usize) -> Mesh {
    let r = match axis {
        0 => m.rotate(v3(0.0, 1.0, 0.0), std::f64::consts::FRAC_PI_2),
        1 => m.rotate(v3(1.0, 0.0, 0.0), std::f64::consts::FRAC_PI_2),
        _ => m.clone(),
    };
    r.on_bed()
}

fn material_for(filament: &str) -> &'static str {
    if filament.contains("aero") || filament.contains("lw_pla") {
        "pla_aero"
    } else if filament.contains("petg") {
        "petg"
    } else {
        "pla"
    }
}

/// The printed pieces as STLs in `dir`, split to the printer's bed.
/// (part, index, filament, STL path, vase mode).
pub type PieceFile = (String, usize, String, PathBuf, bool);

pub fn pieces(lay: &Layout, geo: &[PartGeometry], dir: &Path) -> anyhow::Result<Vec<PieceFile>> {
    std::fs::create_dir_all(dir)?;
    let printer = slice::printer(&lay.design.requirements.printer)?;
    let limit = printer.bed.iter().cloned().fold(f64::INFINITY, f64::min) - 2.0 * BED_MARGIN_MM;
    let mut out = vec![];
    for g in geo.iter().filter(|g| g.filament.is_some()) {
        let mesh = g.shape.mesh();
        let s = mesh.bounds().size();
        let axis = if s.x >= s.y && s.x >= s.z { 0 } else if s.y >= s.z { 1 } else { 2 };
        let fil = g.filament.clone().unwrap();
        let vase = fil == lay.design.structure.shell_filament;
        let n = ((match axis { 0 => s.x, 1 => s.y, _ => s.z }) / limit).ceil().max(1.0) as usize;
        let cut: Vec<Mesh> = match &g.shape {
            // lofts are cut by re-lofting between interpolated rings: clean,
            // closed caps (a BSP split leaves open edges slicers reject)
            Shape::Loft { rings } if n > 1 => sub_lofts(rings, axis, n).iter().map(|r| Shape::Loft { rings: r.clone() }.mesh()).collect(),
            Shape::Shell { outer, inner } if n > 1 => sub_lofts(outer, axis, n)
                .into_iter()
                .zip(sub_lofts(inner, axis, n))
                .map(|(o, i)| Shape::Shell { outer: o, inner: i }.mesh())
                .collect(),
            _ => wright_geom::split_to_fit(&mesh, axis, limit),
        };
        // each printed piece is rebuilt in FreeCAD (watertight STL for the
        // slicer); csg.rs pieces only when FreeCAD is missing
        let mut cut = cut;
        let names: Vec<String> = (0..cut.len()).map(|k| format!("{}_{k}", g.name)).collect();
        let opts = wright_geom::kernel::Options { equivalence: false, interference: false, exchange: false, stem: g.name.clone(), ..Default::default() };
        wright_geom::kernel::realize(names.iter().map(String::as_str).zip(cut.iter_mut()).collect(), dir, &opts);
        for (k, piece) in cut.into_iter().enumerate() {
            let path = dir.join(format!("{}_{k}.stl", g.name));
            stand(&piece, axis).write_stl(&path)?;
            out.push((g.name.clone(), k, fil.clone(), path, vase));
        }
    }
    Ok(out)
}

/// Slice every printed piece; G-code next to each STL.
pub fn slice_all(lay: &Layout, dir: &Path) -> anyhow::Result<Vec<Piece>> {
    let geo = geometry::parts(lay);
    let printer = slice::printer(&lay.design.requirements.printer)?;
    let list = pieces(lay, &geo, dir)?;
    let mut out = vec![];
    for (part, index, fil, stl, vase) in list {
        let gcode = stl.with_extension("gcode");
        let recipe = if vase { Recipe::Vase } else { Recipe::Structural { perimeters: 4, infill_pct: 40 } };
        let s = slice::slice(&stl, &gcode, printer, slice::material(material_for(&fil))?, recipe)?;
        out.push(Piece { part, index, filament: fil, stl, gcode, seconds: s.seconds, grams: s.grams });
    }
    Ok(out)
}

/// Slicing for the checks, cached by the design's JSON so `check` does not
/// re-slice an unchanged aircraft.
pub fn slice_cached(lay: &Layout) -> anyhow::Result<Vec<Piece>> {
    use std::hash::{Hash, Hasher};
    let json = serde_json::to_string(&lay.design)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut h);
    let tmp = std::env::temp_dir();
    // the old cache kept every candidate's STL and G-code (8 GB seen)
    let _ = std::fs::remove_dir_all(tmp.join("wright-air-slices"));
    let index_dir = tmp.join(SLICE_INDEX_DIR);
    let index = index_dir.join(format!("{:016x}.json", h.finish()));
    let unkept = |(part, index, filament, seconds, grams): (String, usize, String, f64, f64)| Piece { stl: PathBuf::new(), gcode: PathBuf::new(), part, index, filament, seconds, grams };
    if let Ok(text) = std::fs::read_to_string(&index) {
        if let Ok(v) = serde_json::from_str::<Vec<(String, usize, String, f64, f64)>>(&text) {
            return Ok(v.into_iter().map(unkept).collect());
        }
    }
    // slice in a scratch dir removed when this returns; only the times and
    // grams are kept (`build` slices the chosen design into its own dir)
    let scratch = Scratch::new()?;
    let pieces = slice_all(lay, &scratch.0)?;
    drop(scratch);
    let v: Vec<(String, usize, String, f64, f64)> = pieces.iter().map(|p| (p.part.clone(), p.index, p.filament.clone(), p.seconds, p.grams)).collect();
    // the cache is best-effort: a full disk must not turn a good slice into
    // Missing evidence. A failed or timed-out slice (WRIGHT_SLICE_TIMEOUT_S)
    // returned Err above and is never cached, so `checks::slicer_checks`
    // records PrintTime and PrintedMass as Missing and the other checks run.
    if std::fs::create_dir_all(&index_dir).is_ok() {
        if let Ok(j) = serde_json::to_string(&v) {
            let _ = std::fs::write(&index, j);
        }
        cap_dir(&index_dir, SLICE_INDEX_CAP_BYTES);
    }
    Ok(v.into_iter().map(unkept).collect())
}

/// The slice-result cache: one small JSON per sliced design, under the temp
/// dir, oldest evicted past the cap.
pub const SLICE_INDEX_DIR: &str = "wright-air-slice-index";
pub const SLICE_INDEX_CAP_BYTES: u64 = 20 * 1024 * 1024;

/// A temp dir removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> std::io::Result<Scratch> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("wright-air-scratch-{}-{k}", std::process::id()));
        std::fs::create_dir_all(&p)?;
        Ok(Scratch(p))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Delete the oldest files in `dir` until it holds at most `cap` bytes.
fn cap_dir(dir: &Path, cap: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// A loft's rings re-cut into `n` equal spans along `axis` (rings carry
/// corresponding points, so linear interpolation is exact for a ruled loft).
fn sub_lofts(rings: &[Vec<[f64; 3]>], axis: usize, n: usize) -> Vec<Vec<Vec<[f64; 3]>>> {
    let key = |r: &Vec<[f64; 3]>| r.iter().map(|p| p[axis]).sum::<f64>() / r.len() as f64;
    let (a0, a1) = (key(&rings[0]), key(&rings[rings.len() - 1]));
    let at = |t: f64| -> Vec<[f64; 3]> {
        let target = a0 + (a1 - a0) * t;
        for w in rings.windows(2) {
            let (k0, k1) = (key(&w[0]), key(&w[1]));
            let lo = k0.min(k1);
            let hi = k0.max(k1);
            if target >= lo - 1e-12 && target <= hi + 1e-12 {
                let u = if (k1 - k0).abs() > 1e-15 { (target - k0) / (k1 - k0) } else { 0.0 };
                return w[0].iter().zip(&w[1]).map(|(p, q)| [p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u, p[2] + (q[2] - p[2]) * u]).collect();
            }
        }
        rings[rings.len() - 1].clone()
    };
    (0..n)
        .map(|k| {
            let (t0, t1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
            let mut v = vec![at(t0)];
            for r in rings {
                let t = (key(r) - a0) / (a1 - a0);
                if t > t0 + 1e-9 && t < t1 - 1e-9 {
                    v.push(r.clone());
                }
            }
            v.push(at(t1));
            v
        })
        .collect()
}
