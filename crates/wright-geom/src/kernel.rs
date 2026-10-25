//! FreeCAD as the solid kernel behind the same vocabulary the domains
//! already use (Pat, 10/24 16:50: "I don't want to reinvent FreeCAD").
//!
//! Every [`crate::shapes`] primitive and every [`Mesh`] boolean or rigid
//! move records a [`Solid`] program node next to its in-house mesh. A
//! domain's `build()` hands its finished meshes to [`realize`], which sends
//! the programs to one persistent FreeCAD worker (`wright_cad::Worker`,
//! `cad/freecad/kernel_program.py`). FreeCAD rebuilds each part as an OCCT
//! B-rep (Part.makeBox / makeCylinder / makeLoft(ruled) / revolve, then
//! Shape.fuse / cut / common), writes a watertight STL from the B-rep
//! (MeshPart.meshFromShape at a stated deflection), measures volume, centre
//! of mass and inertia on the B-rep, runs the all-pairs interference with
//! Shape.common, and exports one AP242 STEP and one GLB. The mesh the domain
//! holds is then replaced by FreeCAD's tessellation, so slicing (slice.rs)
//! and the domain's own checks read the FreeCAD part.
//!
//! When FreeCAD is not installed (or `WRIGHT_GEOM_KERNEL=csg`), the csg.rs
//! meshes stay and the report says so; a part whose mesh was built by an
//! arbitrary vertex map or read from an STL has no program and also stays
//! in-house, named in the report.

use crate::{Mesh, V3};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// One node of a build program, in mm and radians.
#[derive(Debug, Clone)]
pub enum Solid {
    Box { min: V3, max: V3 },
    /// `n` is the in-house facet count; FreeCAD builds the exact circle.
    Cylinder { p0: V3, p1: V3, r: f64, n: usize },
    /// Ruled loft through closed planar rings (BRepOffsetAPI_ThruSections, ruled).
    Loft { rings: Vec<Vec<V3>> },
    /// Profile of (radius, z) revolved 360 degrees about z.
    Revolve { profile: Vec<(f64, f64)>, n: usize },
    Union(Arc<Solid>, Arc<Solid>),
    Subtract(Arc<Solid>, Arc<Solid>),
    Intersect(Arc<Solid>, Arc<Solid>),
    Translate(Arc<Solid>, V3),
    /// Rotation about an axis through the origin.
    Rotate(Arc<Solid>, V3, f64),
}

fn p(v: V3) -> Value {
    json!({"x": v.x, "y": v.y, "z": v.z})
}

impl Solid {
    /// The JSON program `kernel_program.build` executes.
    pub fn to_json(&self) -> Value {
        match self {
            Solid::Box { min, max } => json!({"op": "box", "min": p(*min), "max": p(*max)}),
            Solid::Cylinder { p0, p1, r, n } => json!({"op": "cylinder", "p0": p(*p0), "p1": p(*p1), "r": r, "n": n}),
            Solid::Loft { rings } => json!({"op": "loft", "rings": rings.iter().map(|r| r.iter().map(|v| p(*v)).collect::<Vec<_>>()).collect::<Vec<_>>()}),
            Solid::Revolve { profile, n } => json!({"op": "revolve", "profile": profile, "n": n}),
            Solid::Union(a, b) => json!({"op": "union", "a": a.to_json(), "b": b.to_json()}),
            Solid::Subtract(a, b) => json!({"op": "subtract", "a": a.to_json(), "b": b.to_json()}),
            Solid::Intersect(a, b) => json!({"op": "intersect", "a": a.to_json(), "b": b.to_json()}),
            Solid::Translate(s, d) => json!({"op": "translate", "s": s.to_json(), "d": p(*d)}),
            Solid::Rotate(s, axis, angle) => json!({"op": "rotate", "s": s.to_json(), "axis": p(*axis), "angle": angle}),
        }
    }
}

/// Combine two meshes' programs (None when either has none).
pub(crate) fn both(a: &Mesh, b: &Mesh, f: fn(Arc<Solid>, Arc<Solid>) -> Solid) -> Option<Arc<Solid>> {
    Some(Arc::new(f(a.solid.clone()?, b.solid.clone()?)))
}

/// Tessellation and which extra evidence to compute.
#[derive(Debug, Clone)]
pub struct Options {
    /// MeshPart.meshFromShape LinearDeflection, mm (absolute).
    pub deflection_mm: f64,
    /// AngularDeflection, radians.
    pub angular_deflection_rad: f64,
    /// Also rebuild cylinders and revolves as the in-house n-gons, so the
    /// translation is checked against the old volumes like for like.
    pub equivalence: bool,
    /// All-pairs OCCT interference (Shape.common).
    pub interference: bool,
    /// Write `<stem>.step` (AP242, re-import verified) and `<stem>.glb`.
    pub exchange: bool,
    pub stem: String,
    /// Keep FreeCAD's transport STLs in `dir` (false: the caller writes the
    /// replaced meshes under its own names and the transport files go).
    pub keep_stl: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { deflection_mm: 0.05, angular_deflection_rad: 0.25, equivalence: true, interference: true, exchange: true, stem: "assembly".into(), keep_stl: false }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PartReport {
    pub name: String,
    /// "freecad" or "csg" (in-house fallback).
    pub backend: String,
    pub stl: Option<PathBuf>,
    /// The in-house csg.rs mesh's volume before replacement, mm^3.
    pub csg_volume_mm3: f64,
    /// OCCT B-rep volume (exact circles), mm^3.
    pub volume_mm3: Option<f64>,
    /// OCCT volume of the same program with in-house n-gon facets, mm^3.
    pub faceted_volume_mm3: Option<f64>,
    /// Volume of the written STL, mm^3.
    pub stl_volume_mm3: f64,
    pub center_of_mass_mm: Option<[f64; 3]>,
    /// Volume inertia about the centre of mass, mm^5 (multiply by density).
    pub inertia_mm5: Option<[[f64; 3]; 3]>,
    /// Edges of the written mesh used by one triangle / by more than two
    /// (Rust count on the STL, vertices welded at f32).
    pub boundary_edges: usize,
    pub non_manifold_edges: usize,
    /// FreeCAD's own mesh verdicts (Mesh.isSolid, hasNonManifolds).
    pub freecad_mesh_solid: Option<bool>,
    pub brep_valid: Option<bool>,
    pub note: Option<String>,
}

impl PartReport {
    pub fn watertight(&self) -> bool {
        self.boundary_edges == 0 && self.non_manifold_edges == 0
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub backend: String,
    pub version: Option<String>,
    pub fallback_reason: Option<String>,
    pub parts: Vec<PartReport>,
    /// OCCT overlaps (a, b, mm^3), all pairs with overlapping boxes.
    pub interference: Vec<(String, String, f64)>,
    pub step: Option<Value>,
    pub glb: Option<Value>,
}

impl Report {
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }
    /// The exchange files FreeCAD wrote: ("STEP AP242" | "GLB", path).
    pub fn files(&self) -> Vec<(&'static str, PathBuf)> {
        [("STEP AP242 (FreeCAD)", &self.step), ("GLB (FreeCAD)", &self.glb)]
            .into_iter()
            .filter_map(|(k, v)| Some((k, PathBuf::from(v.as_ref()?.get("path")?.as_str()?))))
            .collect()
    }
    /// One line per domain build: backend, watertightness, volume match.
    pub fn summary(&self) -> String {
        let fc: Vec<&PartReport> = self.parts.iter().filter(|p| p.backend == "freecad").collect();
        let tight = fc.iter().filter(|p| p.watertight()).count();
        let csg: f64 = fc.iter().map(|p| p.csg_volume_mm3).sum();
        let fac: f64 = fc.iter().filter_map(|p| p.faceted_volume_mm3).sum();
        let exact: f64 = fc.iter().filter_map(|p| p.volume_mm3).sum();
        format!(
            "{}: {} of {} parts from FreeCAD, {tight} watertight; volume csg {csg:.0} / FreeCAD faceted {fac:.0} ({:+.3} %) / exact {exact:.0} mm^3 ({:+.3} %)",
            self.backend,
            fc.len(),
            self.parts.len(),
            100.0 * (fac - csg) / csg.max(1e-9),
            100.0 * (exact - csg) / csg.max(1e-9)
        )
    }
}

/// Boundary and non-manifold edge counts of a triangle mesh, vertices
/// welded at f32 (the STL's precision).
pub fn edge_defects(m: &Mesh) -> (usize, usize) {
    let key = |v: V3| [(v.x as f32).to_bits(), (v.y as f32).to_bits(), (v.z as f32).to_bits()];
    let mut ids: HashMap<[u32; 3], usize> = HashMap::new();
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    for t in m.triangles() {
        let n = ids.len();
        let a = *ids.entry(key(t[0])).or_insert(n);
        let n = ids.len();
        let b = *ids.entry(key(t[1])).or_insert(n);
        let n = ids.len();
        let c = *ids.entry(key(t[2])).or_insert(n);
        if a == b || b == c || a == c {
            continue;
        }
        for (i, j) in [(a, b), (b, c), (c, a)] {
            *edges.entry((i.min(j), i.max(j))).or_default() += 1;
        }
    }
    let boundary = edges.values().filter(|&&k| k == 1).count();
    let nonmanifold = edges.values().filter(|&&k| k > 2).count();
    (boundary, nonmanifold)
}

/// The FreeCAD launcher: `WRIGHT_CAD_LAUNCHER`, else the repo's
/// `cad/freecad/run.sh` (as `wright_cad::Session::new`).
fn launcher() -> PathBuf {
    std::env::var_os("WRIGHT_CAD_LAUNCHER").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cad/freecad/run.sh"))
}

fn freecad_installed() -> Result<(), String> {
    if std::env::var("WRIGHT_GEOM_KERNEL").is_ok_and(|v| v == "csg") {
        return Err("WRIGHT_GEOM_KERNEL=csg".into());
    }
    let app = std::env::var_os("WRIGHT_FREECAD_APP")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Applications/FreeCAD.app")))
        .ok_or("no HOME")?;
    if !app.join("Contents/Resources/bin/python").exists() {
        return Err(format!("FreeCAD not installed at {}", app.display()));
    }
    if !launcher().exists() {
        return Err(format!("launcher missing: {}", launcher().display()));
    }
    Ok(())
}

/// One persistent worker per process (FreeCAD's import costs seconds).
static WORKER: OnceLock<Mutex<Option<wright_cad::Worker>>> = OnceLock::new();

fn call(params: Value) -> Result<Value, String> {
    freecad_installed()?;
    let lock = WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(wright_cad::Worker::spawn(&launcher(), Duration::from_secs(900)).map_err(|e| format!("FreeCAD worker did not start: {e:#}"))?);
    }
    let result = guard.as_mut().unwrap().call("kernel_build", params).map_err(|e| format!("{e:#}"));
    if result.is_err() {
        // a stopped worker refuses every later call; start a new one next time
        *guard = None;
    }
    result
}

fn f3(v: &Value) -> Option<[f64; 3]> {
    Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?])
}

fn safe_file(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// Rebuild every mesh that carries a program in FreeCAD and replace it with
/// FreeCAD's watertight tessellation (its program kept). STLs go to `dir`
/// as `<name>.stl`. Never fails: without FreeCAD the meshes stay as they
/// were and the report says why (also printed once to stderr).
pub fn realize(items: Vec<(&str, &mut Mesh)>, dir: &Path, opts: &Options) -> Report {
    let mut report = Report { backend: "freecad".into(), ..Default::default() };
    let mut sent = vec![];
    let mut programs = vec![];
    let mut seen = std::collections::HashSet::new();
    for (k, (name, mesh)) in items.iter().enumerate() {
        if let Some(s) = &mesh.solid {
            let mut file = safe_file(name);
            if !seen.insert(file.clone()) {
                file = format!("{file}_{k}");
            }
            programs.push(json!({"name": name, "file": file, "program": s.to_json()}));
            sent.push(k);
        }
    }
    let reply = if programs.is_empty() {
        Err("no part carries a build program".to_string())
    } else {
        let _ = std::fs::create_dir_all(dir);
        let dir_abs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        call(json!({
            "items": programs, "directory": dir_abs, "deflection_mm": opts.deflection_mm,
            "angular_deflection_rad": opts.angular_deflection_rad, "faceted_check": opts.equivalence,
            "interference": opts.interference, "exchange": opts.exchange, "stem": opts.stem,
        }))
    };
    let reply = match reply {
        Ok(v) => v,
        Err(why) => {
            static WARNED: OnceLock<()> = OnceLock::new();
            WARNED.get_or_init(|| eprintln!("wright-geom: FreeCAD kernel unavailable ({why}); using the in-house csg.rs meshes, which are not watertight"));
            report.backend = "csg".into();
            report.fallback_reason = Some(why.clone());
            for (name, mesh) in &items {
                let (b, n) = edge_defects(mesh);
                let v = mesh.volume();
                report.parts.push(PartReport { name: name.to_string(), backend: "csg".into(), stl: None, csg_volume_mm3: v, volume_mm3: None, faceted_volume_mm3: None, stl_volume_mm3: v, center_of_mass_mm: None, inertia_mm5: None, boundary_edges: b, non_manifold_edges: n, freecad_mesh_solid: None, brep_valid: None, note: Some(why.clone()) });
            }
            return report;
        }
    };
    report.version = reply["version"].as_str().map(|v| format!("FreeCAD {v} / OCCT {}", reply["occt"].as_str().unwrap_or("?")));
    let results = reply["parts"].as_array().cloned().unwrap_or_default();
    let mut items = items;
    for (k, (name, mesh)) in items.iter_mut().enumerate() {
        let csg_volume = mesh.volume();
        let r = sent.iter().position(|&i| i == k).and_then(|j| results.get(j));
        let mut part = PartReport { name: name.to_string(), backend: "csg".into(), stl: None, csg_volume_mm3: csg_volume, volume_mm3: None, faceted_volume_mm3: None, stl_volume_mm3: csg_volume, center_of_mass_mm: None, inertia_mm5: None, boundary_edges: 0, non_manifold_edges: 0, freecad_mesh_solid: None, brep_valid: None, note: None };
        let loaded = match r {
            None => Err("no build program (arbitrary vertex map or STL import): in-house mesh kept".to_string()),
            Some(r) if r.get("error").is_some() => Err(format!("FreeCAD failed: {}; in-house mesh kept", r["error"])),
            Some(r) => {
                let stl = PathBuf::from(r["stl"].as_str().unwrap_or_default());
                Mesh::read_stl(&stl).map(|m| (r, stl, m)).map_err(|e| format!("reading FreeCAD STL: {e}"))
            }
        };
        match loaded {
            Ok((r, stl, m)) => {
                let solid = mesh.solid.clone();
                **mesh = Mesh { solid, ..m };
                part.backend = "freecad".into();
                if opts.keep_stl {
                    part.stl = Some(stl);
                } else {
                    let _ = std::fs::remove_file(&stl);
                }
                part.volume_mm3 = r["volume_mm3"].as_f64();
                part.faceted_volume_mm3 = r["faceted_volume_mm3"].as_f64();
                part.center_of_mass_mm = f3(&r["center_of_mass_mm"]);
                part.inertia_mm5 = r["inertia_mm5"].as_array().and_then(|rows| Some([f3(rows.first()?)?, f3(rows.get(1)?)?, f3(rows.get(2)?)?]));
                part.freecad_mesh_solid = r["mesh"]["is_solid"].as_bool().map(|s| s && r["mesh"]["non_manifold"] == false);
                part.brep_valid = r["valid"].as_bool();
                part.note = r.get("faceted_error").map(|e| format!("faceted rebuild failed: {e}"));
            }
            Err(why) => part.note = Some(why),
        }
        let (b, n) = edge_defects(mesh);
        part.boundary_edges = b;
        part.non_manifold_edges = n;
        part.stl_volume_mm3 = mesh.volume();
        report.parts.push(part);
    }
    report.interference = reply["interference"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some((p["a"].as_str()?.to_string(), p["b"].as_str()?.to_string(), p["overlap_mm3"].as_f64()?))).collect())
        .unwrap_or_default();
    report.step = reply.get("step").cloned();
    report.glb = reply.get("glb").cloned();
    report
}

/// [`realize`] over domain parts, writing `<dir>/kernel-report.json`.
pub fn realize_parts(parts: &mut [crate::Part], dir: &Path, opts: &Options) -> Report {
    let report = realize(parts.iter_mut().map(|p| (p.name.as_str(), &mut p.mesh)).collect(), dir, opts);
    let _ = std::fs::create_dir_all(dir);
    let _ = report.write(&dir.join(format!("{}-kernel-report.json", opts.stem)));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shapes, v3};

    #[test]
    fn primitives_and_booleans_record_programs() {
        let a = shapes::cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let b = shapes::cylinder(v3(5.0, 5.0, -1.0), v3(5.0, 5.0, 11.0), 2.0, 16);
        let m = a.subtract(&b).rotate(v3(0.0, 0.0, 1.0), 0.3).translate(v3(1.0, 0.0, 0.0));
        let j = m.solid.as_ref().unwrap().to_json();
        assert_eq!(j["op"], "translate");
        assert_eq!(j["s"]["op"], "rotate");
        assert_eq!(j["s"]["s"]["op"], "subtract");
        assert_eq!(j["s"]["s"]["b"]["op"], "cylinder");
        // an arbitrary vertex map cannot be replayed in FreeCAD
        assert!(m.map(|v| v * 2.0).solid.is_none());
        assert!(shapes::tube(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 5.0), 3.0, 2.0, 12).solid.is_some());
    }

    #[test]
    fn the_csg_jig_is_not_watertight_and_the_counter_sees_it() {
        let mut jig = shapes::tube(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 25.0), 80.0, 51.0, 32);
        for k in 0..4 {
            let slot = shapes::cuboid(v3(49.0, -2.6, -1.0), v3(85.0, 2.6, 26.0)).rotate(v3(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2 * k as f64);
            jig = jig.subtract(&slot);
        }
        let (b, n) = edge_defects(&jig);
        assert!(b + n > 0, "csg.rs T-junctions expected");
        let (b, n) = edge_defects(&shapes::cuboid(v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0)));
        assert_eq!((b, n), (0, 0));
    }

    /// Runs only where FreeCAD is installed (skips with a message otherwise).
    #[test]
    fn freecad_rebuilds_a_jig_watertight_with_matching_volume() {
        if let Err(why) = freecad_installed() {
            eprintln!("skipped: {why}");
            return;
        }
        let mut jig = shapes::tube(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 25.0), 80.0, 51.0, 64);
        for k in 0..4 {
            let slot = shapes::cuboid(v3(49.0, -2.6, -1.0), v3(85.0, 2.6, 26.0)).rotate(v3(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2 * k as f64);
            jig = jig.subtract(&slot);
        }
        let old = jig.volume();
        let dir = std::env::temp_dir().join(format!("wright-geom-kernel-test-{}", std::process::id()));
        let opts = Options { exchange: false, ..Default::default() };
        let r = realize(vec![("jig", &mut jig)], &dir, &opts);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(r.backend, "freecad", "{:?}", r.fallback_reason);
        let p = &r.parts[0];
        assert!(p.watertight(), "{p:?}");
        assert_eq!(p.freecad_mesh_solid, Some(true));
        let fac = p.faceted_volume_mm3.unwrap();
        assert!((fac - old).abs() / old < 0.01, "faceted {fac} vs csg {old}");
        assert!(jig.solid.is_some(), "program kept after replacement");
    }
}
