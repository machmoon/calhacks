//! The one geometric model: closed polygon meshes in millimetres (the unit
//! of STL and of every slicer), booleans, measurement, all-pairs checks,
//! export and slicing.
//!
//! The prototype defined some geometry twice -- once for the physics, once
//! for the CAD -- and the two drifted (prop discs, the servo pocket, the
//! avionics bay). Here a domain builds [`Part`]s once; its physics reads
//! their volumes and positions, and its checks run [`interference`] and
//! [`clearance`] over **all pairs** of a part set rather than over pairs
//! someone remembered to list.

pub mod csg;
pub mod kernel;
pub mod shapes;
pub mod slice;

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::ops::{Add, Div, Mul, Neg, Sub};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn unit(self) -> V3 {
        self / self.norm()
    }
    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}
impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, k: f64) -> V3 {
        v3(self.x / k, self.y / k, self.z / k)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// Axis-aligned bounds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Aabb {
    pub fn size(&self) -> V3 {
        self.max - self.min
    }
    /// Distance between two boxes (0 when they overlap): a lower bound on
    /// the distance between anything inside them.
    pub fn gap(&self, o: &Aabb) -> f64 {
        let d = |a0: f64, a1: f64, b0: f64, b1: f64| (a0.max(b0) - a1.min(b1)).max(0.0);
        let dx = d(self.min.x, self.max.x, o.min.x, o.max.x);
        let dy = d(self.min.y, self.max.y, o.min.y, o.max.y);
        let dz = d(self.min.z, self.max.z, o.min.z, o.max.z);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

/// A closed mesh of convex planar polygons, mm.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub polygons: Vec<csg::Polygon>,
    /// How FreeCAD rebuilds this solid ([`kernel`]); None for arbitrary
    /// vertex maps and imported STLs.
    pub solid: Option<std::sync::Arc<kernel::Solid>>,
}

impl Mesh {
    pub fn from_triangles(tris: impl IntoIterator<Item = [V3; 3]>) -> Self {
        Self {
            polygons: tris.into_iter().filter_map(|t| csg::Polygon::new(t.to_vec())).collect(),
            solid: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }
    /// Every vertex finite (a NaN dimension upstream makes NaN vertices).
    pub fn is_finite(&self) -> bool {
        self.polygons.iter().all(|p| p.vertices.iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()))
    }
    /// A solid that geometry checks can measure: non-empty, every vertex
    /// finite, and a finite volume above round-off.
    pub fn is_solid(&self) -> bool {
        if self.is_empty() || !self.is_finite() {
            return false;
        }
        let v = self.volume();
        v.is_finite() && v > 1e-9
    }
    pub fn union(&self, o: &Mesh) -> Mesh {
        Mesh { polygons: csg::union(self.polygons.clone(), o.polygons.clone()), solid: kernel::both(self, o, kernel::Solid::Union) }
    }
    pub fn subtract(&self, o: &Mesh) -> Mesh {
        Mesh { polygons: csg::subtract(self.polygons.clone(), o.polygons.clone()), solid: kernel::both(self, o, kernel::Solid::Subtract) }
    }
    pub fn intersect(&self, o: &Mesh) -> Mesh {
        Mesh { polygons: csg::intersect(self.polygons.clone(), o.polygons.clone()), solid: kernel::both(self, o, kernel::Solid::Intersect) }
    }
    /// Fan-triangulate the (convex) polygons.
    pub fn triangles(&self) -> Vec<[V3; 3]> {
        let mut out = Vec::new();
        for p in &self.polygons {
            for i in 1..p.vertices.len() - 1 {
                out.push([p.vertices[0], p.vertices[i], p.vertices[i + 1]]);
            }
        }
        out
    }
    /// Enclosed volume, mm^3 (divergence theorem over the triangles).
    pub fn volume(&self) -> f64 {
        self.triangles().iter().map(|[a, b, c]| a.dot(b.cross(*c)) / 6.0).sum::<f64>().abs()
    }
    /// Surface area, mm^2.
    pub fn area(&self) -> f64 {
        self.triangles().iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).norm() / 2.0).sum()
    }
    pub fn bounds(&self) -> Aabb {
        let mut min = v3(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut max = -min;
        for p in &self.polygons {
            for v in &p.vertices {
                min = min.min(*v);
                max = max.max(*v);
            }
        }
        Aabb { min, max }
    }
    pub fn map(&self, f: impl Fn(V3) -> V3) -> Mesh {
        Mesh {
            polygons: self
                .polygons
                .iter()
                .filter_map(|p| csg::Polygon::new(p.vertices.iter().map(|v| f(*v)).collect()))
                .collect(),
            solid: None,
        }
    }
    pub fn translate(&self, d: V3) -> Mesh {
        let solid = self.solid.clone().map(|s| std::sync::Arc::new(kernel::Solid::Translate(s, d)));
        Mesh { solid, ..self.map(|v| v + d) }
    }
    /// Rotate about an axis through the origin by `angle` radians
    /// (Rodrigues' formula).
    pub fn rotate(&self, axis: V3, angle: f64) -> Mesh {
        let k = axis.unit();
        let (s, c) = angle.sin_cos();
        let solid = self.solid.clone().map(|m| std::sync::Arc::new(kernel::Solid::Rotate(m, k, angle)));
        Mesh { solid, ..self.map(|v| v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))) }
    }
    /// Resting on z = 0 with its bounds' minimum corner at the origin: how a
    /// part sits on a print bed.
    pub fn on_bed(&self) -> Mesh {
        let b = self.bounds();
        self.translate(-b.min)
    }

    /// Read a binary STL (what [`Mesh::write_stl`] writes, and what most
    /// CAD exports). ASCII STL is refused with a clear error.
    pub fn read_stl(path: &Path) -> std::io::Result<Mesh> {
        let b = std::fs::read(path)?;
        let bad = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {why}", path.display()));
        if b.len() < 84 {
            return Err(bad("shorter than an STL header"));
        }
        let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
        if b.len() < 84 + n * 50 {
            return Err(bad(if b.starts_with(b"solid") { "ASCII STL is not supported" } else { "truncated binary STL" }));
        }
        let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64;
        let tris = (0..n).map(|i| {
            let o = 84 + i * 50 + 12;
            let p = |k: usize| v3(f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8));
            [p(0), p(1), p(2)]
        });
        Ok(Mesh::from_triangles(tris))
    }

    /// Binary STL (the de facto exchange format every slicer reads).
    pub fn write_stl(&self, path: &Path) -> std::io::Result<()> {
        let tris = self.triangles();
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut header = [0u8; 80];
        let tag = b"wright binary STL, mm";
        header[..tag.len()].copy_from_slice(tag);
        f.write_all(&header)?;
        f.write_all(&(tris.len() as u32).to_le_bytes())?;
        for [a, b, c] in tris {
            let n = (b - a).cross(c - a);
            let n = if n.norm() > 0.0 { n.unit() } else { n };
            for v in [n, a, b, c] {
                for x in [v.x, v.y, v.z] {
                    f.write_all(&(x as f32).to_le_bytes())?;
                }
            }
            f.write_all(&[0, 0])?;
        }
        Ok(())
    }
}

/// What a part is for the BOM and the slicer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartKind {
    /// 3D printed, from `material` (a filament id).
    Printed,
    /// Bought (motor, tube, battery...).
    Bought,
}

/// One part, placed in the vehicle's frame.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub kind: PartKind,
    /// Filament id for printed parts, catalog id for bought ones.
    pub material: String,
    pub mesh: Mesh,
    /// Names of parts this one is allowed to touch (its mount, the tube it
    /// is bonded to). Everything else must not.
    pub touches: Vec<String>,
}

/// Every pair of parts that overlaps by more than `tolerance_mm3`, with the
/// shared volume: (a, b, mm^3). All pairs, minus the declared contacts.
pub fn interference(parts: &[Part], tolerance_mm3: f64) -> Vec<(String, String, f64)> {
    let bounds: Vec<Aabb> = parts.iter().map(|p| p.mesh.bounds()).collect();
    let mut out = Vec::new();
    // a part with no solid (a failed boolean or loft) cannot be shown clear
    // of anything: report it rather than skip it (its infinite bounds made
    // every box test say "far apart")
    // (review 4: a NaN vertex or volume is "no solid" too; `volume() <=
    // 1e-9` is false for NaN, so NaN parts were silently clear)
    let solid: Vec<bool> = parts.iter().map(|p| p.mesh.is_solid()).collect();
    for (p, ok) in parts.iter().zip(&solid) {
        if !ok {
            let why = if p.mesh.is_empty() {
                "(no solid: empty mesh)"
            } else if !p.mesh.is_finite() {
                "(no solid: non-finite vertices)"
            } else {
                "(no solid: zero or non-finite volume)"
            };
            out.push((p.name.clone(), why.to_string(), f64::INFINITY));
        }
    }
    for i in 0..parts.len() {
        for j in i + 1..parts.len() {
            if !solid[i] || !solid[j] {
                continue; // already reported above
            }
            let (a, b) = (&parts[i], &parts[j]);
            if a.touches.contains(&b.name) || b.touches.contains(&a.name) {
                continue;
            }
            if bounds[i].gap(&bounds[j]) > 0.0 {
                continue;
            }
            let v = a.mesh.intersect(&b.mesh).volume();
            if v > tolerance_mm3 {
                out.push((a.name.clone(), b.name.clone(), v));
            }
        }
    }
    out
}

/// The closest approach between two meshes, mm: every vertex of each
/// against every triangle of the other (exact point-triangle distance), with
/// box pruning. 0 when they intersect. Edge-to-edge closest points between
/// coarse faces can be missed by up to the faces' size, so meshes checked
/// for clearance should be tessellated finer than the clearance demanded.
pub fn clearance(a: &Mesh, b: &Mesh) -> f64 {
    if a.is_empty() || b.is_empty() || !a.is_finite() || !b.is_finite() {
        // nothing to measure (or a NaN vertex, whose comparisons would
        // quietly drop out of the min): a check turns this into Missing
        return f64::NAN;
    }
    if a.bounds().gap(&b.bounds()) == 0.0 {
        let i = a.intersect(b);
        if !i.is_empty() && i.volume() > 1e-6 {
            return 0.0;
        }
    }
    let ta = a.triangles();
    let tb = b.triangles();
    let mut best = f64::INFINITY;
    // vertex against face, both ways
    for (pts, tris) in [(&ta, &tb), (&tb, &ta)] {
        for t in pts.iter() {
            for p in t {
                for tri in tris.iter() {
                    best = best.min(point_triangle(*p, tri));
                }
            }
        }
    }
    // edge against edge: two faces that cross without touching are closest
    // edge to edge, where no vertex is near (review 2026-10-24: two crossed
    // bars 1 mm apart measured 99.5 mm without this)
    for x in &ta {
        for (p0, p1) in [(x[0], x[1]), (x[1], x[2]), (x[2], x[0])] {
            for y in &tb {
                for (q0, q1) in [(y[0], y[1]), (y[1], y[2]), (y[2], y[0])] {
                    best = best.min(segment_segment(p0, p1, q0, q1));
                }
            }
        }
    }
    best
}

/// Distance between segments p0-p1 and q0-q1 (Ericson, Real-Time Collision
/// Detection, 5.1.9, ClosestPtSegmentSegment).
pub fn segment_segment(p0: V3, p1: V3, q0: V3, q1: V3) -> f64 {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    const EPS: f64 = 1e-12;
    let (s, t);
    if a <= EPS && e <= EPS {
        return (p0 - q0).norm();
    }
    if a <= EPS {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= EPS {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let s0 = if denom > EPS { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    ((p0 + d1 * s) - (q0 + d2 * t)).norm()
}

/// Distance from `p` to triangle `t` (Ericson, Real-Time Collision
/// Detection, 5.1.5, closest point on triangle).
pub fn point_triangle(p: V3, t: &[V3; 3]) -> f64 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (p - a).norm();
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (p - b).norm();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (p - (a + ab * v)).norm();
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (p - c).norm();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (p - (a + ac * w)).norm();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).norm();
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (p - (a + ab * v + ac * w)).norm()
}

/// Cut `mesh` into pieces along `axis` (0 x, 1 y, 2 z) so that each fits
/// `max_len_mm` in that axis, for a print bed.
pub fn split_to_fit(mesh: &Mesh, axis: usize, max_len_mm: f64) -> Vec<Mesh> {
    let b = mesh.bounds();
    let (lo, hi) = match axis {
        0 => (b.min.x, b.max.x),
        1 => (b.min.y, b.max.y),
        _ => (b.min.z, b.max.z),
    };
    let n = ((hi - lo) / max_len_mm).ceil().max(1.0) as usize;
    let big = 1e5;
    (0..n)
        .map(|i| {
            let a = lo + (hi - lo) * i as f64 / n as f64 - if i == 0 { 1.0 } else { 0.0 };
            let c = lo + (hi - lo) * (i + 1) as f64 / n as f64 + if i == n - 1 { 1.0 } else { 0.0 };
            let (mut min, mut max) = (v3(-big, -big, -big), v3(big, big, big));
            match axis {
                0 => {
                    min.x = a;
                    max.x = c;
                }
                1 => {
                    min.y = a;
                    max.y = c;
                }
                _ => {
                    min.z = a;
                    max.z = c;
                }
            }
            mesh.intersect(&shapes::cuboid(min, max))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapes::*;

    /// Review 4, finding 10: a part with NaN vertices (a NaN dimension
    /// upstream) was neither "no solid" nor intersected, so interference
    /// said clear, and clearance returned a number.
    #[test]
    fn a_nan_part_is_no_solid_and_has_no_clearance() {
        let part = |name: &str, mesh: Mesh| Part { name: name.into(), kind: PartKind::Bought, material: String::new(), mesh, touches: vec![] };
        let nan = f64::NAN;
        let pod = cuboid(v3(0.0, 0.0, 0.0), v3(100.0, 100.0, 100.0));
        let all_nan = cuboid(v3(nan, nan, nan), v3(nan, nan, nan));
        let half_nan = cuboid(v3(10.0, 10.0, 10.0), v3(50.0, 50.0, nan));
        let hits = interference(&[part("pod", pod.clone()), part("battery", all_nan.clone()), part("fc", half_nan.clone())], 1e-6);
        let named: Vec<&str> = hits.iter().map(|h| h.0.as_str()).collect();
        assert!(named.contains(&"battery") && named.contains(&"fc"), "{hits:?}");
        assert!(hits.iter().all(|h| h.1.starts_with("(no solid")), "{hits:?}");
        assert!(clearance(&pod, &all_nan).is_nan());
        assert!(clearance(&pod, &half_nan).is_nan());
        assert!(clearance(&half_nan, &pod).is_nan());
        // a finite pair still measures
        let far = cuboid(v3(110.0, 0.0, 0.0), v3(120.0, 10.0, 10.0));
        assert!((clearance(&pod, &far) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_cube_has_its_volume_and_a_bore_removes_a_cylinder() {
        let c = cuboid(v3(0.0, 0.0, 0.0), v3(20.0, 20.0, 20.0));
        assert!((c.volume() - 8000.0).abs() < 1e-6);
        let bore = cylinder(v3(10.0, 10.0, -5.0), v3(10.0, 10.0, 25.0), 4.0, 64);
        let holed = c.subtract(&bore);
        // a 64-gon of radius 4 has area 64/2 r^2 sin(2pi/64)
        let disc = 32.0 * 16.0 * (2.0 * std::f64::consts::PI / 64.0).sin();
        assert!((holed.volume() - (8000.0 - 20.0 * disc)).abs() < 1e-3, "{}", holed.volume());
    }

    #[test]
    fn interference_finds_overlaps_and_respects_declared_contacts() {
        let a = Part {
            name: "a".into(),
            kind: PartKind::Bought,
            material: "x".into(),
            mesh: cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0)),
            touches: vec![],
        };
        let mut b = a.clone();
        b.name = "b".into();
        b.mesh = cuboid(v3(5.0, 5.0, 5.0), v3(15.0, 15.0, 15.0));
        let mut c = a.clone();
        c.name = "c".into();
        c.mesh = cuboid(v3(30.0, 0.0, 0.0), v3(40.0, 10.0, 10.0));
        let hits = interference(&[a.clone(), b.clone(), c.clone()], 1e-6);
        assert_eq!(hits.len(), 1);
        assert!((hits[0].2 - 125.0).abs() < 1e-6);
        b.touches = vec!["a".into()];
        assert!(interference(&[a, b, c], 1e-6).is_empty());
    }

    #[test]
    fn clearance_between_separated_boxes() {
        let a = cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let b = cuboid(v3(13.0, 0.0, 0.0), v3(20.0, 10.0, 10.0));
        assert!((clearance(&a, &b) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn crossed_bars_face_to_face_measure_their_true_gap() {
        // review 2026-10-24: 200 mm bars crossing 1 mm apart read 99.5 mm
        let a = cuboid(v3(-100.0, -5.0, 0.0), v3(100.0, 5.0, 10.0));
        let b = cuboid(v3(-5.0, -100.0, 11.0), v3(5.0, 100.0, 21.0));
        assert!((clearance(&a, &b) - 1.0).abs() < 1e-9, "{}", clearance(&a, &b));
    }

    #[test]
    fn an_empty_part_is_reported_not_skipped() {
        let solid = Part {
            name: "box".into(),
            kind: PartKind::Bought,
            material: "x".into(),
            mesh: cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0)),
            touches: vec![],
        };
        let mut empty = solid.clone();
        empty.name = "failed_loft".into();
        empty.mesh = Mesh::default();
        let hits = interference(&[solid, empty.clone()], 1e-6);
        assert!(hits.iter().any(|h| h.0 == "failed_loft"));
        assert!(clearance(&empty.mesh, &cuboid(v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0))).is_nan());
    }

    #[test]
    fn stl_round_trips() {
        let c = cuboid(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 5.0));
        let path = std::env::temp_dir().join(format!("wright-rt-{}.stl", std::process::id()));
        c.write_stl(&path).unwrap();
        let back = Mesh::read_stl(&path).unwrap();
        assert!((back.volume() - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn splitting_keeps_the_volume() {
        let c = cuboid(v3(0.0, 0.0, 0.0), v3(100.0, 10.0, 10.0));
        let parts = split_to_fit(&c, 0, 30.0);
        assert_eq!(parts.len(), 4);
        let total: f64 = parts.iter().map(Mesh::volume).sum();
        assert!((total - 10_000.0).abs() < 1e-6);
    }
}
