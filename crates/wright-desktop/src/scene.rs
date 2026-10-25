//! The CAD scene: named parts with faces (triangles) and feature edges
//! (line segments), plus what CAD knows about each part (material, mass,
//! parameters), read from either
//!
//! * wright-cad's assembly **GLB** (`<out_dir>/cad/*.glb`), with the `gltf`
//!   crate (gltf-rs/gltf 1.4; loading follows its `examples/tree`: walk the
//!   scene's nodes recursively, `node.transform().matrix()` accumulated to
//!   world, `primitive.reader(..)` for positions/indices). Each node with a
//!   mesh is a part named by the node; TRIANGLES primitives are faces,
//!   LINES/LINE_STRIP primitives are the B-rep's feature edges. A node's
//!   `extras` may carry `{"material", "mass_g" | "mass_kg", "params": {..},
//!   "interferes_with": [..]}` (the glTF place for app data).
//! * or the run's **STL** files (`wright_geom::Mesh::read_stl`), chosen as
//!   `wright_guide::from_artifacts` does (crates/wright-guide/src/
//!   artifacts.rs): the "assembly" STL if there is one, else every STL but
//!   jigs and fixtures. A one-file assembly is split into connected pieces
//!   so a click still selects something. STL has no edges, so they are
//!   derived: three.js `EdgesGeometry` (src/geometries/EdgesGeometry.js,
//!   r170): an edge is drawn when its two faces' normals differ by more than
//!   a threshold angle, or when it has only one face.
//!
//! Units: millimetres. A GLB whose extent is under 20 units is taken to be
//! in metres (the glTF spec's unit) and scaled by 1000.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use glam::{Mat4, Vec3};
use wright_geom::Mesh;

use crate::model::locate;

#[derive(Debug, Clone, Default)]
pub struct ScenePart {
    pub name: String,
    pub file: PathBuf,
    pub first_tri: usize,
    pub tris: usize,
    pub first_edge: usize,
    pub edges: usize,
    pub min: Vec3,
    pub max: Vec3,
    /// Area-weighted centre of the part's surface.
    pub centroid: Vec3,
    pub material: Option<String>,
    pub mass_g: Option<f64>,
    pub params: BTreeMap<String, serde_json::Value>,
    /// Names of parts the CAD says this one overlaps.
    pub interferes_with: Vec<String>,
}

impl ScenePart {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Three vertices per triangle.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Part index per face vertex.
    pub part_of_vertex: Vec<f32>,
    /// Two vertices per edge segment.
    pub edge_positions: Vec<[f32; 3]>,
    pub part_of_edge_vertex: Vec<f32>,
    pub parts: Vec<ScenePart>,
    pub min: Vec3,
    pub max: Vec3,
    /// Files that would not load: (path, why).
    pub errors: Vec<(PathBuf, String)>,
    /// "GLB (wright-cad)" or "STL".
    pub source: String,
    pub geometry_check: Option<serde_json::Value>,
}

/// The files that make the scene: a GLB under `<dir>/cad/` or named by an
/// artifact wins; else STLs as the guide picks them.
pub fn scene_files(artifacts: &[(String, String)], dir: &Path) -> Vec<(String, PathBuf)> {
    let ext = |p: &str| p.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mut glbs: Vec<(String, PathBuf)> = artifacts
        .iter()
        .filter(|(_, p)| ext(p) == "glb")
        .filter_map(|(l, p)| locate(dir, p).map(|f| (l.clone(), f)))
        .collect();
    if glbs.is_empty() {
        if let Ok(rd) = std::fs::read_dir(dir.join("cad")) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb"))).collect();
            v.sort_by_key(|p| (!p.to_string_lossy().contains("assembly"), p.clone()));
            glbs = v.into_iter().map(|p| ("CAD assembly".to_string(), p)).collect();
        }
    }
    if let Some(g) = glbs.into_iter().next() {
        return vec![g];
    }
    let mut stls: Vec<(String, PathBuf)> = vec![];
    for (label, recorded) in artifacts {
        if ext(recorded) != "stl" {
            continue;
        }
        if let Some(p) = locate(dir, recorded) {
            if !stls.iter().any(|(_, q)| q == &p) {
                stls.push((label.clone(), p));
            }
        }
    }
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if let Some(a) = stls.iter().find(|(l, p)| stem(p).contains("assembly") || l.to_ascii_lowercase().contains("assembly")) {
        return vec![a.clone()];
    }
    stls.into_iter()
        .filter(|(l, p)| {
            let s = format!("{} {}", l.to_ascii_lowercase(), stem(p));
            !s.contains("jig") && !s.contains("fixture")
        })
        .collect()
}

fn to_v(p: wright_geom::V3) -> Vec3 {
    Vec3::new(p.x as f32, p.y as f32, p.z as f32)
}

fn key(v: Vec3) -> (i64, i64, i64) {
    ((v.x * 1000.0).round() as i64, (v.y * 1000.0).round() as i64, (v.z * 1000.0).round() as i64)
}

/// Connected pieces of a triangle soup: triangles sharing a vertex
/// position (to 1 micron) are one piece. Union-find with path halving.
fn components(tris: &[[Vec3; 3]]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut first: HashMap<(i64, i64, i64), usize> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for v in t {
            match first.get(&key(*v)) {
                Some(&j) => {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a] = b;
                    }
                }
                None => {
                    first.insert(key(*v), i);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..tris.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

/// Feature edges of a triangle set, after three.js `EdgesGeometry`: each
/// edge keyed by its rounded endpoints; an edge shared by two faces whose
/// normals' dot is <= cos(threshold) is drawn, and so is an edge with one
/// face. (The pairing is undirected here: STL winding is not reliable.)
type EdgeKey = ((i64, i64, i64), (i64, i64, i64));
pub fn feature_edges(tris: &[[Vec3; 3]], threshold_deg: f32) -> Vec<[Vec3; 2]> {
    let threshold_dot = threshold_deg.to_radians().cos();
    let mut open: HashMap<EdgeKey, (Vec3, Vec3, Vec3, bool)> = HashMap::new();
    let mut out = vec![];
    for t in tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.length_squared() == 0.0 {
            continue;
        }
        let n = n.normalize();
        for j in 0..3 {
            let (a, b) = (t[j], t[(j + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            match open.get_mut(&k) {
                Some(e) if !e.3 => {
                    if n.dot(e.2) <= threshold_dot {
                        out.push([a, b]);
                    }
                    e.3 = true;
                }
                Some(_) => {}
                None => {
                    open.insert(k, (a, b, n, false));
                }
            }
        }
    }
    out.extend(open.into_values().filter(|e| !e.3).map(|e| [e.0, e.1]));
    out
}

#[derive(Default)]
struct PartMeta {
    material: Option<String>,
    mass_g: Option<f64>,
    params: BTreeMap<String, serde_json::Value>,
    interferes_with: Vec<String>,
}

fn meta_from_extras(raw: Option<&str>) -> PartMeta {
    let Some(v) = raw.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else { return PartMeta::default() };
    PartMeta {
        material: v.get("material").and_then(|m| m.as_str()).map(str::to_string),
        mass_g: v.get("mass_g").and_then(|m| m.as_f64()).or_else(|| v.get("mass_kg").and_then(|m| m.as_f64()).map(|k| k * 1000.0)),
        params: v.get("params").and_then(|p| p.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
        interferes_with: v
            .get("interferes_with")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    }
}

impl Scene {
    fn empty(source: &str) -> Scene {
        Scene { min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN), source: source.into(), ..Default::default() }
    }

    pub fn load(files: &[(String, PathBuf)]) -> Scene {
        if let Some((_, p)) = files.iter().find(|(_, p)| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))) {
            return Scene::load_gltf(p);
        }
        let mut s = Scene::empty("STL");
        for (label, path) in files {
            let mesh = match Mesh::read_stl(path) {
                Ok(m) => m,
                Err(e) => {
                    s.errors.push((path.clone(), e.to_string()));
                    continue;
                }
            };
            let tris: Vec<[Vec3; 3]> = mesh.triangles().into_iter().map(|t| [to_v(t[0]), to_v(t[1]), to_v(t[2])]).collect();
            let name = path.file_stem().map(|s| s.to_string_lossy().replace('_', " ")).unwrap_or_else(|| label.clone());
            let is_assembly = files.len() == 1;
            let groups = if is_assembly { components(&tris) } else { vec![(0..tris.len()).collect()] };
            // tiny slivers are merged into one "small pieces" part
            let mut small: Vec<usize> = vec![];
            let mut big: Vec<Vec<usize>> = vec![];
            for g in groups {
                if is_assembly && g.len() < 12 {
                    small.extend(g);
                } else {
                    big.push(g);
                }
            }
            if !small.is_empty() {
                big.push(small);
            }
            let n_big = big.len();
            for (k, g) in big.into_iter().enumerate() {
                let pname = if n_big == 1 { name.clone() } else { format!("{name} piece {}", k + 1) };
                let t: Vec<[Vec3; 3]> = g.iter().map(|&i| tris[i]).collect();
                let edges = feature_edges(&t, 30.0);
                s.push_part(pname, path.clone(), &t, &edges, PartMeta::default());
            }
        }
        s.finish()
    }

    pub fn load_gltf(path: &Path) -> Scene {
        let mut s = Scene::empty("GLB (wright-cad)");
        let (doc, buffers, _) = match gltf::import(path) {
            Ok(x) => x,
            Err(e) => {
                s.errors.push((path.to_path_buf(), e.to_string()));
                return s.finish();
            }
        };
        struct Raw {
            name: String,
            tris: Vec<[Vec3; 3]>,
            edges: Vec<[Vec3; 2]>,
            meta: PartMeta,
        }
        let mut raws: Vec<Raw> = vec![];
        fn walk(node: gltf::Node, parent: Mat4, buffers: &[gltf::buffer::Data], out: &mut Vec<Raw>) {
            let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                let name = node.name().or(mesh.name()).map(str::to_string).unwrap_or_else(|| format!("part {}", node.index()));
                let extras = node.extras().as_ref().map(|r| r.get().to_string()).or_else(|| mesh.extras().as_ref().map(|r| r.get().to_string()));
                let mut raw = Raw { name, tris: vec![], edges: vec![], meta: meta_from_extras(extras.as_deref()) };
                for prim in mesh.primitives() {
                    let reader = prim.reader(|b| Some(&buffers[b.index()]));
                    let Some(pos) = reader.read_positions() else { continue };
                    let pos: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from(p))).collect();
                    let idx: Vec<usize> = match reader.read_indices() {
                        Some(i) => i.into_u32().map(|i| i as usize).collect(),
                        None => (0..pos.len()).collect(),
                    };
                    let at = |i: usize| pos.get(i).copied();
                    match prim.mode() {
                        gltf::mesh::Mode::Triangles => {
                            #[allow(clippy::chunks_exact_to_as_chunks)]
                            for c in idx.chunks_exact(3) {
                                if let (Some(a), Some(b), Some(d)) = (at(c[0]), at(c[1]), at(c[2])) {
                                    raw.tris.push([a, b, d]);
                                }
                            }
                        }
                        gltf::mesh::Mode::Lines => {
                            #[allow(clippy::chunks_exact_to_as_chunks)]
                            for c in idx.chunks_exact(2) {
                                if let (Some(a), Some(b)) = (at(c[0]), at(c[1])) {
                                    raw.edges.push([a, b]);
                                }
                            }
                        }
                        gltf::mesh::Mode::LineStrip | gltf::mesh::Mode::LineLoop => {
                            for w in idx.windows(2) {
                                if let (Some(a), Some(b)) = (at(w[0]), at(w[1])) {
                                    raw.edges.push([a, b]);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                out.push(raw);
            }
            for c in node.children() {
                walk(c, world, buffers, out);
            }
        }
        for scene in doc.scenes() {
            for node in scene.nodes() {
                walk(node, Mat4::IDENTITY, &buffers, &mut raws);
            }
        }
        // metres -> mm when the model is small (glTF's unit is the metre)
        let (mut mn, mut mx) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for r in &raws {
            for t in &r.tris {
                for v in t {
                    mn = mn.min(*v);
                    mx = mx.max(*v);
                }
            }
        }
        let coordinates: Option<serde_json::Value> = doc.as_json().asset.extras.as_ref().and_then(|r| serde_json::from_str(r.get()).ok());
        s.geometry_check = coordinates.as_ref().and_then(|v| v.get("geometry_check")).cloned();
        let explicit_metres = coordinates.as_ref().is_some_and(|v| v["wright_units"] == "m");
        let y_up = coordinates.as_ref().is_some_and(|v| v["wright_up_axis"] == "Y");
        let scale = if explicit_metres || (mx.x > mn.x && (mx - mn).max_element() < 20.0) { 1000.0 } else { 1.0 };
        for mut r in raws {
            if y_up {
                let to_z_up = |v: &mut Vec3| { *v = Vec3::new(v.x, -v.z, v.y); };
                r.tris.iter_mut().flatten().for_each(to_z_up);
                r.edges.iter_mut().flatten().for_each(to_z_up);
            }
            if scale != 1.0 {
                r.tris.iter_mut().flatten().for_each(|v| *v *= scale);
                r.edges.iter_mut().flatten().for_each(|v| *v *= scale);
            }
            if r.edges.is_empty() {
                r.edges = feature_edges(&r.tris, 30.0);
            }
            s.push_part(r.name, path.to_path_buf(), &r.tris, &r.edges, r.meta);
        }
        s.finish()
    }

    fn finish(mut self) -> Scene {
        if self.parts.is_empty() {
            self.min = Vec3::ZERO;
            self.max = Vec3::ZERO;
        }
        self
    }

    fn push_part(&mut self, name: String, file: PathBuf, tris: &[[Vec3; 3]], edges: &[[Vec3; 2]], meta: PartMeta) {
        if tris.is_empty() {
            return;
        }
        let idx = self.parts.len() as f32;
        let first = self.positions.len() / 3;
        let (mut mn, mut mx) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let (mut area, mut acc) = (0.0f32, Vec3::ZERO);
        for t in tris {
            let c = (t[1] - t[0]).cross(t[2] - t[0]);
            let n = c.normalize_or_zero();
            let a = c.length() * 0.5;
            area += a;
            acc += (t[0] + t[1] + t[2]) / 3.0 * a;
            for v in t {
                self.positions.push(v.to_array());
                self.normals.push(n.to_array());
                self.part_of_vertex.push(idx);
                mn = mn.min(*v);
                mx = mx.max(*v);
            }
        }
        let first_edge = self.edge_positions.len() / 2;
        for e in edges {
            for v in e {
                self.edge_positions.push(v.to_array());
                self.part_of_edge_vertex.push(idx);
            }
        }
        self.min = self.min.min(mn);
        self.max = self.max.max(mx);
        let centroid = if area > 0.0 { acc / area } else { (mn + mx) * 0.5 };
        self.parts.push(ScenePart {
            name,
            file,
            first_tri: first,
            tris: tris.len(),
            first_edge,
            edges: edges.len(),
            min: mn,
            max: mx,
            centroid,
            material: meta.material,
            mass_g: meta.mass_g,
            params: meta.params,
            interferes_with: meta.interferes_with,
        });
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn radius(&self) -> f32 {
        ((self.max - self.min).length() * 0.5).max(1.0)
    }

    pub fn triangles(&self) -> usize {
        self.positions.len() / 3
    }

    /// Where part `i` is moved by an exploded view of `amount` (0 = as
    /// assembled): away from the assembly centre along the line through its
    /// own centre, scaled by `amount`.
    pub fn explode_offset(&self, i: usize, amount: f32) -> Vec3 {
        if amount == 0.0 {
            return Vec3::ZERO;
        }
        (self.parts[i].center() - self.center()) * amount
    }

    /// Total mass and centre of gravity, from the parts that carry a mass
    /// (each at its surface centroid). `None` if no part has a mass.
    pub fn mass_cg(&self) -> Option<(f64, Vec3, usize)> {
        let with: Vec<&ScenePart> = self.parts.iter().filter(|p| p.mass_g.is_some_and(|m| m > 0.0)).collect();
        if with.is_empty() {
            return None;
        }
        let m: f64 = with.iter().map(|p| p.mass_g.unwrap()).sum();
        let cg = with.iter().fold(Vec3::ZERO, |a, p| a + p.centroid * (p.mass_g.unwrap() / m) as f32);
        Some((m, cg, with.len()))
    }

    /// The first surface point a ray hits, with exploded offsets applied
    /// and the section plane (`clip`: keep points with `n.p <= d`) honoured.
    pub fn pick(&self, origin: Vec3, dir: Vec3, explode: f32, clip: Option<(Vec3, f32)>) -> Option<(usize, Vec3)> {
        let mut best: Option<(usize, f32)> = None;
        for (pi, part) in self.parts.iter().enumerate() {
            let off = self.explode_offset(pi, explode);
            let o = origin - off;
            if !ray_box(o, dir, part.min, part.max) {
                continue;
            }
            for t in part.first_tri..part.first_tri + part.tris {
                let a = Vec3::from(self.positions[3 * t]);
                let b = Vec3::from(self.positions[3 * t + 1]);
                let c = Vec3::from(self.positions[3 * t + 2]);
                if let Some(d) = ray_tri(o, dir, a, b, c) {
                    if let Some((n, dd)) = clip {
                        if n.dot(origin + dir * d) > dd {
                            continue;
                        }
                    }
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((pi, d));
                    }
                }
            }
        }
        best.map(|(i, d)| (i, origin + dir * d))
    }
}

fn ray_box(o: Vec3, d: Vec3, mn: Vec3, mx: Vec3) -> bool {
    let inv = d.recip();
    let t1 = (mn - o) * inv;
    let t2 = (mx - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    tmax >= tmin.max(0.0)
}

fn ray_tri(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_aircraft_assembly_loads_and_splits_into_pieces() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/aircraft");
        let arts = vec![
            ("assembly (STL, mm)".to_string(), "demo/aircraft/cad/assembly.stl".to_string()),
            ("printed pieces".to_string(), "demo/aircraft/print".to_string()),
        ];
        let files = scene_files(&arts, &dir);
        assert_eq!(files.len(), 1);
        let s = Scene::load(&files);
        assert!(s.errors.is_empty(), "{:?}", s.errors);
        assert!(s.triangles() > 1000);
        assert!(s.parts.len() > 1, "one STL should split into pieces");
        assert!(!s.edge_positions.is_empty(), "feature edges derived");
        let c = s.center();
        let hit = s.pick(c + Vec3::new(0.0, 0.0, s.radius() * 3.0), Vec3::new(0.0, 0.0, -1.0), 0.0, None);
        assert!(hit.is_some());
        // a section plane above everything hides nothing; one below hides all
        let down = Vec3::new(0.0, 0.0, -1.0);
        let top = c + Vec3::new(0.0, 0.0, s.radius() * 3.0);
        assert!(s.pick(top, down, 0.0, Some((Vec3::Z, s.max.z + 1.0))).is_some());
        assert!(s.pick(top, down, 0.0, Some((Vec3::Z, s.min.z - 1.0))).is_none());
    }

    #[test]
    fn a_cube_has_twelve_feature_edges() {
        let m = wright_geom::shapes::cuboid(wright_geom::v3(0.0, 0.0, 0.0), wright_geom::v3(10.0, 10.0, 10.0));
        let tris: Vec<[Vec3; 3]> = m.triangles().into_iter().map(|t| [to_v(t[0]), to_v(t[1]), to_v(t[2])]).collect();
        let e = feature_edges(&tris, 30.0);
        assert_eq!(e.len(), 12, "{e:?}");
    }

    #[test]
    fn jigs_stay_out_of_the_vehicle() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/rocket");
        let arts = vec![
            ("STL nose".to_string(), "demo/rocket/nose.stl".to_string()),
            ("STL fin alignment jig (print)".to_string(), "demo/rocket/fin_alignment_jig.stl".to_string()),
        ];
        let files = scene_files(&arts, &dir);
        assert_eq!(files.len(), 1);
        assert!(files[0].1.ends_with("nose.stl"));
    }

    #[test]
    fn mass_and_cg_come_from_parts_that_have_mass() {
        let mut s = Scene::empty("t");
        let tri = |x: f32| [Vec3::new(x, 0.0, 0.0), Vec3::new(x + 1.0, 0.0, 0.0), Vec3::new(x, 1.0, 0.0)];
        s.push_part("a".into(), "a".into(), &[tri(0.0)], &[], PartMeta { mass_g: Some(100.0), ..Default::default() });
        s.push_part("b".into(), "b".into(), &[tri(10.0)], &[], PartMeta { mass_g: Some(300.0), ..Default::default() });
        s.push_part("c".into(), "c".into(), &[tri(50.0)], &[], PartMeta::default());
        let (m, cg, n) = s.mass_cg().unwrap();
        assert_eq!((m, n), (400.0, 2));
        assert!((cg.x - (0.333 * 0.25 + 10.333 * 0.75)).abs() < 0.01, "{cg}");
    }

    /// A glTF with one named part: a triangle (faces), a LINES primitive
    /// (edges), a node translation, and extras with material/mass/params.
    #[test]
    fn a_gltf_part_keeps_its_name_edges_transform_and_extras() {
        let mut bytes: Vec<u8> = vec![];
        for v in [[0.0f32, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0]] {
            for c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in [0u16, 1, 1, 2] {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        let b64 = b64(&bytes);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"boom_L","mesh":0,"translation":[0,0,50],
              "extras":{{"material":"CF tube","mass_g":42.5,"params":{{"od_mm":20}},"interferes_with":["wing_L"]}}}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"mode":4}},{{"attributes":{{"POSITION":0}},"indices":1,"mode":1}}]}}],
            "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":8}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[100,100,0]}},
                         {{"bufferView":1,"componentType":5123,"count":4,"type":"SCALAR"}}]}}"#,
            bytes.len(),
            b64
        );
        let dir = std::env::temp_dir().join(format!("wright-gltf-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cad")).unwrap();
        let p = dir.join("cad/assembly.gltf");
        std::fs::write(&p, json).unwrap();
        let s = Scene::load(&[("cad".into(), p)]);
        assert!(s.errors.is_empty(), "{:?}", s.errors);
        assert_eq!(s.parts.len(), 1);
        let part = &s.parts[0];
        assert_eq!(part.name, "boom_L");
        assert_eq!(part.edges, 2, "edges from the LINES primitive, not derived");
        assert_eq!(part.min.z, 50.0, "node translation applied (mm, no metre scaling at 100 units)");
        assert_eq!(part.material.as_deref(), Some("CF tube"));
        assert_eq!(part.mass_g, Some(42.5));
        assert_eq!(part.params["od_mm"], 20);
        assert_eq!(part.interferes_with, vec!["wing_L".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_cad_metadata_sets_metres_and_y_up_without_extent_guessing() {
        for length in [0.005f32, 25.0] {
            let mut bytes=vec![];
            for vertex in [[0.0f32,0.0,0.0],[length,0.0,0.0],[0.0,0.002,-0.003]] {
                for v in vertex { bytes.extend_from_slice(&v.to_le_bytes()); }
            }
            let doc=serde_json::json!({
                "asset":{"version":"2.0","extras":{"wright_units":"m","wright_up_axis":"Y"}},
                "scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"native","mesh":0}],
                "meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],
                "buffers":[{"byteLength":36,"uri":format!("data:application/octet-stream;base64,{}",b64(&bytes))}],
                "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],
                "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,-0.003],"max":[length,0.002,0]}]
            });
            let path=std::env::temp_dir().join(format!("wright-native-coordinates-{}-{length}.gltf",std::process::id()));
            std::fs::write(&path,doc.to_string()).unwrap();
            let scene=Scene::load_gltf(&path);
            assert!(scene.errors.is_empty(),"{:?}",scene.errors);
            assert_eq!(scene.parts.len(),1);
            let p=&scene.parts[0];
            assert!((p.max.x-length*1000.).abs()<0.001);
            assert!((p.max.y-3.).abs()<0.001);
            assert!((p.max.z-2.).abs()<0.001);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn native_cad_export_loads_in_desktop_at_its_original_dimensions() {
        let app=std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Applications/FreeCAD.app");
        if !app.exists() { eprintln!("SKIP native exporter test: FreeCAD not installed"); return; }
        let launcher=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cad/freecad/run.sh");
        let dir=std::env::temp_dir().join(format!("wright-native-scene-{}",std::process::id()));
        let mut worker=wright_cad::Worker::spawn(&launcher,std::time::Duration::from_secs(60)).unwrap();
        worker.call("document",serde_json::json!({})).unwrap();
        worker.call("make_part",serde_json::json!({"spec":{"name":"Plate","kind":"box","params":{"x":25,"y":10,"z":2}}})).unwrap();
        worker.call("place",serde_json::json!({"object_name":"Plate","position":[3,-4,5]})).unwrap();
        worker.call("check",serde_json::json!({"objects":["Plate"]})).unwrap();
        worker.call("save",serde_json::json!({"directory":dir,"objects":["Plate"]})).unwrap();
        let scene=Scene::load_gltf(&dir.join("assembly.glb"));
        assert!(scene.errors.is_empty(),"{:?}",scene.errors);
        assert_eq!(scene.parts.len(),1);
        let part=&scene.parts[0];
        assert_eq!(part.name,"Plate");
        assert_eq!(part.params["Length"],25.0);
        assert_eq!(scene.geometry_check.as_ref().unwrap()["status"],"checked");
        assert_eq!(scene.geometry_check.as_ref().unwrap()["passed"],true);
        assert!((part.min-Vec3::new(3.,-4.,5.)).length()<0.001,"{}",part.min);
        assert!((part.max-Vec3::new(28.,6.,7.)).length()<0.001,"{}",part.max);
        worker.call("set_parameter",serde_json::json!({"object_name":"Plate","parameter":"Length","value":30})).unwrap();
        worker.call("save",serde_json::json!({"directory":dir,"objects":["Plate"]})).unwrap();
        let edited=Scene::load_gltf(&dir.join("assembly.glb"));
        assert!(edited.errors.is_empty());
        assert_eq!(edited.parts[0].params["Length"],30.0);
        assert_eq!(edited.geometry_check.as_ref().unwrap()["status"],"not_checked");
        drop(worker);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Standard base64 (RFC 4648), test-only, to inline a buffer.
    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                if k <= c.len() {
                    out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
}
