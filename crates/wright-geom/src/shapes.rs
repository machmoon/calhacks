//! Solids built from sections, mm: boxes, cylinders between two points,
//! ruled lofts through closed sections (wings, pods, fins), and revolved
//! profiles (rocket bodies, nozzles, nose cones).
//!
//! Lofts are ruled -- straight between stations -- on purpose: in the
//! prototype a smooth loft through unevenly spaced pod sections overshot
//! between them, the inner loft bulged through the outer, and the cavity cut
//! failed silently. Callers place more stations where curvature matters.

use crate::kernel::Solid;
use crate::{v3, Mesh, V3};
use std::sync::Arc;

pub fn cuboid(min: V3, max: V3) -> Mesh {
    let c = |i: u8| {
        v3(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    };
    // faces wound outward
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut m = Mesh::from_triangles(quads.iter().flat_map(|q| {
        [[c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]]
    }));
    m.solid = Some(Arc::new(Solid::Box { min, max }));
    m
}

/// Two unit vectors perpendicular to `axis` and to each other.
fn frame(axis: V3) -> (V3, V3) {
    let a = axis.unit();
    let helper = if a.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 1.0, 0.0) };
    let u = a.cross(helper).unit();
    (u, a.cross(u))
}

/// A cylinder of `radius` from `p0` to `p1`, `n` sides.
pub fn cylinder(p0: V3, p1: V3, radius: f64, n: usize) -> Mesh {
    let (u, w) = frame(p1 - p0);
    let ring = |c: V3| -> Vec<V3> {
        (0..n)
            .map(|i| {
                let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                c + u * (radius * t.cos()) + w * (radius * t.sin())
            })
            .collect()
    };
    let mut m = loft(&[ring(p0), ring(p1)]);
    m.solid = Some(Arc::new(Solid::Cylinder { p0, p1, r: radius, n }));
    m
}

/// A tube (hollow cylinder).
pub fn tube(p0: V3, p1: V3, outer: f64, inner: f64, n: usize) -> Mesh {
    let d = (p1 - p0).unit();
    cylinder(p0, p1, outer, n).subtract(&cylinder(p0 - d, p1 + d, inner, n))
}

/// A ruled loft through closed rings with equal vertex counts, capped at
/// both ends. Rings must be planar and simple; each may be concave (an
/// airfoil), since caps are ear-clipped. Winding is fixed so the mesh is
/// outward-facing whichever way the rings run.
pub fn loft(rings: &[Vec<V3>]) -> Mesh {
    assert!(rings.len() >= 2 && rings.iter().all(|r| r.len() == rings[0].len() && r.len() >= 3));
    let n = rings[0].len();
    let mut tris = Vec::new();
    for k in 0..rings.len() - 1 {
        let (a, b) = (&rings[k], &rings[k + 1]);
        for i in 0..n {
            let j = (i + 1) % n;
            tris.push([a[i], a[j], b[j]]);
            tris.push([a[i], b[j], b[i]]);
        }
    }
    let first = &rings[0];
    let last = &rings[rings.len() - 1];
    for t in ear_clip(first) {
        tris.push([t[0], t[2], t[1]]);
    }
    tris.extend(ear_clip(last));
    let mut mesh = Mesh::from_triangles(tris);
    if signed_volume(&mesh) < 0.0 {
        mesh = mesh.map(|v| v);
        for p in &mut mesh.polygons {
            p.vertices.reverse();
            if let Some(q) = crate::csg::Polygon::new(p.vertices.clone()) {
                *p = q;
            }
        }
    }
    mesh.solid = Some(Arc::new(Solid::Loft { rings: rings.to_vec() }));
    mesh
}

fn signed_volume(m: &Mesh) -> f64 {
    m.triangles().iter().map(|[a, b, c]| a.dot(b.cross(*c)) / 6.0).sum()
}

/// Revolve a profile of (radius, z) points about the z axis, `n` sides. The
/// profile runs from one end to the other; zero radii close the ends.
pub fn revolve(profile: &[(f64, f64)], n: usize) -> Mesh {
    let mut tris = Vec::new();
    let pt = |r: f64, z: f64, i: usize| {
        let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        v3(r * t.cos(), r * t.sin(), z)
    };
    for w in profile.windows(2) {
        let ((r0, z0), (r1, z1)) = (w[0], w[1]);
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b, c, d) = (pt(r0, z0, i), pt(r0, z0, j), pt(r1, z1, j), pt(r1, z1, i));
            if r0 > 1e-12 {
                tris.push([a, b, c]);
            }
            if r1 > 1e-12 {
                tris.push([a, c, d]);
            }
        }
    }
    // close open ends with a disc
    for &(r, z, flip) in &[(profile[0].0, profile[0].1, true), (profile[profile.len() - 1].0, profile[profile.len() - 1].1, false)] {
        if r > 1e-12 {
            let c = v3(0.0, 0.0, z);
            for i in 0..n {
                let j = (i + 1) % n;
                let t = [c, pt(r, z, i), pt(r, z, j)];
                tris.push(if flip { [t[0], t[2], t[1]] } else { t });
            }
        }
    }
    let mut mesh = Mesh::from_triangles(tris);
    if signed_volume(&mesh) < 0.0 {
        for p in &mut mesh.polygons {
            p.vertices.reverse();
            if let Some(q) = crate::csg::Polygon::new(p.vertices.clone()) {
                *p = q;
            }
        }
    }
    mesh.solid = Some(Arc::new(Solid::Revolve { profile: profile.to_vec(), n }));
    mesh
}

/// Ear-clipping triangulation of a planar simple polygon, wound like the
/// input (O(n^2); fine for section outlines of ~100 points).
pub fn ear_clip(ring: &[V3]) -> Vec<[V3; 3]> {
    // the polygon's normal by Newell's method
    let mut normal = v3(0.0, 0.0, 0.0);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        normal = normal + v3((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    let mut idx: Vec<usize> = (0..ring.len()).collect();
    let mut out = Vec::new();
    let convex = |a: V3, b: V3, c: V3| (b - a).cross(c - b).dot(normal) > 0.0;
    let inside = |p: V3, a: V3, b: V3, c: V3| {
        let s1 = (b - a).cross(p - a).dot(normal);
        let s2 = (c - b).cross(p - b).dot(normal);
        let s3 = (a - c).cross(p - c).dot(normal);
        s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0
    };
    let mut guard = 0;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (ring[ia], ring[ib], ring[ic]);
            if !convex(a, b, c) {
                continue;
            }
            if idx.iter().any(|&q| q != ia && q != ib && q != ic && inside(ring[q], a, b, c)) {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break; // degenerate input: fall back to a fan for the rest
        }
    }
    for k in 1..idx.len().saturating_sub(1) {
        out.push([ring[idx[0]], ring[idx[k]], ring[idx[k + 1]]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_revolved_cylinder_profile_has_pi_r2_h() {
        let m = revolve(&[(0.0, 0.0), (10.0, 0.0), (10.0, 50.0), (0.0, 50.0)], 256);
        let exact = std::f64::consts::PI * 100.0 * 50.0;
        assert!((m.volume() - exact).abs() / exact < 2e-3, "{}", m.volume());
    }

    #[test]
    fn a_loft_of_two_squares_is_a_box() {
        let sq = |z: f64| vec![v3(0.0, 0.0, z), v3(10.0, 0.0, z), v3(10.0, 10.0, z), v3(0.0, 10.0, z)];
        let m = loft(&[sq(0.0), sq(20.0)]);
        assert!((m.volume() - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn a_concave_section_lofts_with_its_true_area() {
        // an L-shaped section, area 75
        let l = |z: f64| {
            vec![v3(0.0, 0.0, z), v3(10.0, 0.0, z), v3(10.0, 5.0, z), v3(5.0, 5.0, z), v3(5.0, 10.0, z), v3(0.0, 10.0, z)]
        };
        let m = loft(&[l(0.0), l(4.0)]);
        assert!((m.volume() - 300.0).abs() < 1e-6, "{}", m.volume());
    }
}
