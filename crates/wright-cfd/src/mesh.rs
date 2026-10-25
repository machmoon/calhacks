//! Structured 2D quad meshes written in SU2's native format.
//!
//! Format from SU2's documentation and reader
//! (`Common/src/geometry/meshreader/CSU2ASCIIMeshReaderBase.cpp`, and the VTK type codes in `Common/include/option_structure.hpp` `GEO_TYPE`, v8.5.0):
//! `NDIME= 2`, `NELEM=` then one line per element `9 n0 n1 n2 n3 id`
//! (VTK_QUAD = 9, counter-clockwise), `NPOIN=` then `x y id`, `NMARK=`, and
//! for each marker `MARKER_TAG=`, `MARKER_ELEMS=` and lines `3 a b`
//! (VTK_LINE = 3).
//!
//! The meshes here are algebraic: every grid line of constant i runs from a
//! point on the lower boundary (axis + body, or axis) straight to a point on
//! the upper boundary, with a geometric stretching towards the wall. That is
//! enough for inviscid (Euler) flow, which is all this crate asks of SU2.

use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Grid {
    pub ni: usize,
    pub nj: usize,
    /// Node (i, j) at `pts[j * ni + i]`, (x, r).
    pub pts: Vec<[f64; 2]>,
    /// Boundary markers: name and the edges along it as node pairs.
    pub markers: Vec<(String, Vec<(usize, usize)>)>,
}

impl Grid {
    pub fn id(&self, i: usize, j: usize) -> usize {
        j * self.ni + i
    }
    pub fn at(&self, i: usize, j: usize) -> [f64; 2] {
        self.pts[self.id(i, j)]
    }
    /// Edges along j = `j` for i in [i0, i1).
    pub fn row_edges(&self, j: usize, i0: usize, i1: usize) -> Vec<(usize, usize)> {
        (i0..i1).map(|i| (self.id(i, j), self.id(i + 1, j))).collect()
    }
    pub fn col_edges(&self, i: usize, j0: usize, j1: usize) -> Vec<(usize, usize)> {
        (j0..j1).map(|j| (self.id(i, j), self.id(i, j + 1))).collect()
    }
    /// Quads, counter-clockwise in (x, r) with r up.
    pub fn quads(&self) -> Vec<[usize; 4]> {
        let mut v = Vec::with_capacity((self.ni - 1) * (self.nj - 1));
        for j in 0..self.nj - 1 {
            for i in 0..self.ni - 1 {
                v.push([self.id(i, j), self.id(i + 1, j), self.id(i + 1, j + 1), self.id(i, j + 1)]);
            }
        }
        v
    }
    /// Smallest signed quad area (negative = folded mesh).
    pub fn min_area(&self) -> f64 {
        self.quads()
            .iter()
            .map(|q| {
                let p: Vec<[f64; 2]> = q.iter().map(|&k| self.pts[k]).collect();
                0.5 * ((p[2][0] - p[0][0]) * (p[3][1] - p[1][1]) - (p[3][0] - p[1][0]) * (p[2][1] - p[0][1]))
            })
            .fold(f64::INFINITY, f64::min)
    }
    pub fn write_su2(&self, path: &Path) -> anyhow::Result<()> {
        let mut s = String::with_capacity(self.pts.len() * 60);
        let q = self.quads();
        writeln!(s, "NDIME= 2\nNELEM= {}", q.len())?;
        for (k, e) in q.iter().enumerate() {
            writeln!(s, "9 {} {} {} {} {k}", e[0], e[1], e[2], e[3])?;
        }
        writeln!(s, "NPOIN= {}", self.pts.len())?;
        for (k, p) in self.pts.iter().enumerate() {
            writeln!(s, "{:.10e} {:.10e} {k}", p[0], p[1])?;
        }
        writeln!(s, "NMARK= {}", self.markers.len())?;
        for (name, edges) in &self.markers {
            writeln!(s, "MARKER_TAG= {name}\nMARKER_ELEMS= {}", edges.len())?;
            for (a, b) in edges {
                writeln!(s, "3 {a} {b}")?;
            }
        }
        std::fs::write(path, s)?;
        Ok(())
    }
}

/// `n` points on [0, 1], first spacing `d0` (as a fraction), geometric growth
/// (solved by bisection on the ratio). Falls back to uniform if `d0` >= 1/(n-1).
pub fn stretched(n: usize, d0: f64) -> Vec<f64> {
    let m = (n - 1) as f64;
    if d0 >= 1.0 / m {
        return (0..n).map(|k| k as f64 / m).collect();
    }
    // sum_{k<m} d0 r^k = 1
    let f = |r: f64| d0 * (r.powf(m) - 1.0) / (r - 1.0) - 1.0;
    let (mut lo, mut hi) = (1.0 + 1e-9, 2.0);
    while f(hi) < 0.0 {
        hi *= 1.5;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let r = 0.5 * (lo + hi);
    let mut v = vec![0.0];
    let mut x = 0.0;
    for k in 0..n - 1 {
        x += d0 * r.powi(k as i32);
        v.push(x);
    }
    let last = *v.last().unwrap();
    v.iter().map(|x| x / last).collect()
}

/// Points along a polyline at the given arc-length fractions.
pub fn along(poly: &[[f64; 2]], fracs: &[f64]) -> Vec<[f64; 2]> {
    let mut cum = vec![0.0];
    for w in poly.windows(2) {
        let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        cum.push(cum.last().unwrap() + d);
    }
    let total = *cum.last().unwrap();
    fracs
        .iter()
        .map(|&f| {
            let s = f * total;
            let k = cum.partition_point(|&c| c <= s).clamp(1, poly.len() - 1);
            let t = ((s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-300)).clamp(0.0, 1.0);
            [poly[k - 1][0] + t * (poly[k][0] - poly[k - 1][0]), poly[k - 1][1] + t * (poly[k][1] - poly[k - 1][1])]
        })
        .collect()
}

/// A grid between `lower` and `upper` (same length ni), straight lines of
/// constant i, `dist` the j fractions (0 at lower).
pub fn ruled(lower: &[[f64; 2]], upper: &[[f64; 2]], dist: &[f64]) -> Grid {
    let ni = lower.len();
    let nj = dist.len();
    let mut pts = vec![[0.0; 2]; ni * nj];
    for (j, &e) in dist.iter().enumerate() {
        for i in 0..ni {
            pts[j * ni + i] = [lower[i][0] + e * (upper[i][0] - lower[i][0]), lower[i][1] + e * (upper[i][1] - lower[i][1])];
        }
    }
    Grid { ni, nj, pts, markers: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stretching_hits_ends_and_first_cell() {
        let s = stretched(41, 0.002);
        assert!((s[0]).abs() < 1e-15 && (s[40] - 1.0).abs() < 1e-12);
        assert!((s[1] - 0.002).abs() < 1e-6, "{}", s[1]);
        assert!(s.windows(2).all(|w| w[1] > w[0]));
    }
    #[test]
    fn a_ruled_box_is_valid_and_written() {
        let lo: Vec<[f64; 2]> = (0..5).map(|i| [i as f64, 0.0]).collect();
        let up: Vec<[f64; 2]> = (0..5).map(|i| [i as f64, 1.0]).collect();
        let mut g = ruled(&lo, &up, &stretched(4, 0.2));
        assert!(g.min_area() > 0.0);
        g.markers.push(("wall".into(), g.row_edges(0, 0, 4)));
        let p = std::env::temp_dir().join("wright_cfd_box.su2");
        g.write_su2(&p).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("NELEM= 12") && t.contains("NPOIN= 20") && t.contains("MARKER_ELEMS= 4"));
        let _ = std::fs::remove_file(p);
    }
}
