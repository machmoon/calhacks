//! Meshes: structured quadratic shells (S8R) written here for the
//! parametric thin parts (tubes, fins), and gmsh solids (C3D10 / C3D20R)
//! read from gmsh's own `.inp` writer for plates and STL-derived parts.
//!
//! Element conventions are CalculiX's (ccx 2.23 manual, sections 6.2.x):
//! S8R corners 1-4 counter-clockwise about the normal, midsides 5 = 1-2,
//! 6 = 2-3, 7 = 3-4, 8 = 4-1. Solid face numbering for distributed loads
//! (`*DLOAD elem, Pn`): C3D10 faces 1:1-2-3, 2:1-4-2, 3:2-4-3, 4:3-4-1;
//! C3D20(R) faces 1:1-2-3-4, 2:5-8-7-6, 3:1-5-6-2, 4:2-6-7-3, 5:3-7-8-4,
//! 6:4-8-5-1 (the same tables FreeCAD's `femmesh/meshtools.py`
//! `get_ccx_elements_face_numbers` uses).

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::f64::consts::PI;

pub type V3 = [f64; 3];

#[derive(Debug, Clone)]
pub struct Element {
    /// CalculiX type: "S8R", "C3D10", "C3D20R".
    pub ty: &'static str,
    /// 1-based node ids.
    pub nodes: Vec<usize>,
    /// Element set (one section per set).
    pub set: String,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// Node id = index + 1.
    pub nodes: Vec<V3>,
    pub elements: Vec<Element>,
}

impl Mesh {
    pub fn add_node(&mut self, p: V3) -> usize {
        self.nodes.push(p);
        self.nodes.len()
    }
    pub fn node(&self, id: usize) -> V3 {
        self.nodes[id - 1]
    }
    /// Node ids where `f(position)` holds (only nodes used by an element).
    pub fn select(&self, f: impl Fn(V3) -> bool) -> Vec<usize> {
        let mut used = vec![false; self.nodes.len() + 1];
        for e in &self.elements {
            for &n in &e.nodes {
                used[n] = true;
            }
        }
        (1..=self.nodes.len()).filter(|&i| used[i] && f(self.nodes[i - 1])).collect()
    }
    /// Faces (element index, face number, corner node ids) of solid
    /// elements whose corners all satisfy `f`.
    pub fn faces_where(&self, f: impl Fn(V3) -> bool) -> Vec<(usize, usize, Vec<usize>)> {
        let mut out = vec![];
        for (ei, e) in self.elements.iter().enumerate() {
            let table: &[&[usize]] = match e.ty {
                "C3D10" => &[&[0, 1, 2], &[0, 3, 1], &[1, 3, 2], &[2, 3, 0]],
                "C3D20R" | "C3D20" => &[&[0, 1, 2, 3], &[4, 7, 6, 5], &[0, 4, 5, 1], &[1, 5, 6, 2], &[2, 6, 7, 3], &[3, 7, 4, 0]],
                _ => continue,
            };
            for (k, face) in table.iter().enumerate() {
                let ids: Vec<usize> = face.iter().map(|&i| e.nodes[i]).collect();
                if ids.iter().all(|&n| f(self.node(n))) {
                    out.push((ei, k + 1, ids));
                }
            }
        }
        out
    }
}

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// A structured S8R tube: `stations` are the axial element boundaries
/// (s from the origin along `axis`), `n_circ` elements around, mid-surface
/// radius `radius`; e1, e2 span the section (axis x e1 = e2). Element set
/// of each axial segment from `set_of(s_mid)`.
pub struct Tube {
    pub mesh: Mesh,
    /// Node ids of each axial node row (2 * segments + 1 rows; even rows lie
    /// on stations, odd rows are mid-length rows holding only midside nodes).
    pub rows: Vec<Vec<usize>>,
    /// s of each row.
    pub row_s: Vec<f64>,
    /// Per row node: true for a corner (on an even circumferential index).
    pub corner: Vec<Vec<bool>>,
    pub origin: V3,
    pub axis: V3,
    pub e1: V3,
    pub e2: V3,
}

pub fn tube(origin: V3, axis: V3, e1: V3, e2: V3, radius: f64, stations: &[f64], n_circ: usize, set_of: impl Fn(f64) -> String) -> Tube {
    let mut m = Mesh::default();
    let nc = 2 * n_circ;
    let mut rows = vec![];
    let mut row_s = vec![];
    let mut corner = vec![];
    let pos = |s: f64, k: usize| {
        let th = 2.0 * PI * k as f64 / nc as f64;
        add(add(origin, scale(axis, s)), add(scale(e1, radius * th.cos()), scale(e2, radius * th.sin())))
    };
    for i in 0..stations.len() {
        // station row: every circumferential node
        let s = stations[i];
        rows.push((0..nc).map(|k| m.add_node(pos(s, k))).collect::<Vec<_>>());
        corner.push((0..nc).map(|k| k % 2 == 0).collect());
        row_s.push(s);
        if i + 1 < stations.len() {
            // mid row: midside nodes only on corner columns
            let sm = 0.5 * (stations[i] + stations[i + 1]);
            rows.push((0..nc).map(|k| if k % 2 == 0 { m.add_node(pos(sm, k)) } else { 0 }).collect());
            corner.push((0..nc).map(|_| false).collect());
            row_s.push(sm);
        }
    }
    for i in 0..stations.len() - 1 {
        let (r0, rm, r1) = (&rows[2 * i], &rows[2 * i + 1], &rows[2 * i + 2]);
        let set = set_of(0.5 * (stations[i] + stations[i + 1]));
        for j in 0..n_circ {
            let (c0, cm, c1) = (2 * j, 2 * j + 1, (2 * j + 2) % nc);
            // corners (s0,c0) (s1,c0) (s1,c1) (s0,c1): normal = axis x tangent
            // = radially inward (see the module notes on pressure sign)
            m.elements.push(Element {
                ty: "S8R",
                nodes: vec![r0[c0], r1[c0], r1[c1], r0[c1], rm[c0], r1[cm], rm[c1], r0[cm]],
                set: set.clone(),
            });
        }
    }
    Tube { mesh: m, rows, row_s, corner, origin, axis, e1, e2 }
}

impl Tube {
    /// Station rows (even rows) whose s lies in [a, b].
    pub fn rows_between(&self, a: f64, b: f64) -> Vec<usize> {
        (0..self.rows.len()).filter(|&r| self.row_s[r] >= a - 1e-9 && self.row_s[r] <= b + 1e-9).collect()
    }
    /// Nearest station row (even index) to s.
    pub fn station_row(&self, s: f64) -> usize {
        (0..self.rows.len()).step_by(2).min_by(|&a, &b| (self.row_s[a] - s).abs().total_cmp(&(self.row_s[b] - s).abs())).unwrap()
    }
    pub fn nodes_in_rows(&self, rows: &[usize]) -> Vec<usize> {
        rows.iter().flat_map(|&r| self.rows[r].iter().copied().filter(|&n| n > 0)).collect()
    }
    /// A force `f` spread over a station ring with consistent quadratic-edge
    /// weights (corner 1/3, midside 2/3 of an edge's share): (node, vector).
    pub fn ring_force(&self, row: usize, f: V3) -> Vec<(usize, V3)> {
        let w: Vec<f64> = self.corner[row].iter().map(|&c| if c { 1.0 } else { 2.0 }).collect();
        let tot: f64 = w.iter().sum();
        self.rows[row].iter().zip(&w).map(|(&n, &wi)| (n, scale(f, wi / tot))).collect()
    }
    /// A bending moment `m` about the section axis `about` (unit, normal to
    /// the tube axis) as axial nodal forces linear in the lever arm.
    pub fn ring_moment(&self, row: usize, m: f64, about: V3) -> Vec<(usize, V3)> {
        // lever direction d = about x axis; F_i = m w_i d_i / sum w d^2, along axis
        let lever = [about[1] * self.axis[2] - about[2] * self.axis[1], about[2] * self.axis[0] - about[0] * self.axis[2], about[0] * self.axis[1] - about[1] * self.axis[0]];
        let c = add(self.origin, scale(self.axis, self.row_s[row]));
        let arms: Vec<f64> = self.rows[row].iter().map(|&n| dot(sub(self.mesh.node(n), c), lever)).collect();
        let w: Vec<f64> = self.corner[row].iter().map(|&c| if c { 1.0 } else { 2.0 }).collect();
        let den: f64 = arms.iter().zip(&w).map(|(a, wi)| wi * a * a).sum();
        self.rows[row].iter().zip(arms.iter().zip(&w)).map(|(&n, (a, wi))| (n, scale(self.axis, -m * wi * a / den))).collect()
    }
    /// Mass `kg` spread over a ring like `ring_force`.
    pub fn ring_mass(&self, row: usize, kg: f64) -> Vec<(usize, f64)> {
        self.ring_force(row, [kg, 0.0, 0.0]).into_iter().map(|(n, v)| (n, v[0])).collect()
    }
}

/// Stations from 0 to `len` with spacing at most `h`, passing exactly
/// through every point in `marks` (inside the range).
pub fn stations(start: f64, end: f64, h: f64, marks: &[f64]) -> Vec<f64> {
    let mut pts: Vec<f64> = vec![start, end];
    pts.extend(marks.iter().copied().filter(|&m| m > start + 1e-6 && m < end - 1e-6));
    pts.sort_by(|a, b| a.total_cmp(b));
    pts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let n = ((w[1] - w[0]) / h).ceil().max(1.0) as usize;
        for k in 1..=n {
            out.push(w[0] + (w[1] - w[0]) * k as f64 / n as f64);
        }
    }
    out
}

/// A structured S8R quadrilateral plate in the x-y plane from four corners
/// (counter-clockwise seen from +z): `na` elements along 0-1, `nb` along
/// 1-2. Returns the mesh and the node grid [(2na+1) x (2nb+1)] (0 = unused
/// centre node).
pub fn quad_plate(c: [V3; 4], na: usize, nb: usize, set: &str) -> (Mesh, Vec<Vec<usize>>) {
    let mut m = Mesh::default();
    let (ga, gb) = (2 * na + 1, 2 * nb + 1);
    let mut grid = vec![vec![0usize; gb]; ga];
    for i in 0..ga {
        for j in 0..gb {
            if i % 2 == 1 && j % 2 == 1 {
                continue;
            }
            let (u, v) = (i as f64 / (ga - 1) as f64, j as f64 / (gb - 1) as f64);
            // bilinear: P = (1-u)(1-v) c0 + u (1-v) c1 + u v c2 + (1-u) v c3
            let p = add(add(scale(c[0], (1.0 - u) * (1.0 - v)), scale(c[1], u * (1.0 - v))), add(scale(c[2], u * v), scale(c[3], (1.0 - u) * v)));
            grid[i][j] = m.add_node(p);
        }
    }
    for i in 0..na {
        for j in 0..nb {
            let (a, b) = (2 * i, 2 * j);
            m.elements.push(Element {
                ty: "S8R",
                nodes: vec![grid[a][b], grid[a + 2][b], grid[a + 2][b + 2], grid[a][b + 2], grid[a + 1][b], grid[a + 2][b + 1], grid[a + 1][b + 2], grid[a][b + 1]],
                set: set.into(),
            });
        }
    }
    (m, grid)
}

/// Read nodes and solid elements from a gmsh-written `.inp` (C3D10 or
/// C3D20, the latter returned as C3D20R). Lower-dimensional blocks gmsh
/// writes alongside are skipped.
pub fn read_gmsh_inp(text: &str, set: &str) -> Result<Mesh> {
    let mut ids: BTreeMap<usize, usize> = BTreeMap::new();
    let mut m = Mesh::default();
    let mut mode = 0; // 1 nodes, 2 wanted elements, 0 skip
    let mut ty = "";
    let mut pending: Vec<usize> = vec![];
    let mut raw: Vec<(String, Vec<usize>)> = vec![];
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("**") {
            continue;
        }
        if l.starts_with('*') {
            let u = l.to_ascii_uppercase();
            mode = 0;
            if u == "*NODE" || (u.starts_with("*NODE,") && !u.contains("FILE") && !u.contains("PRINT")) {
                mode = 1;
            } else if u.starts_with("*ELEMENT") {
                if u.contains("TYPE=C3D10") {
                    mode = 2;
                    ty = "C3D10";
                } else if u.contains("TYPE=C3D20") {
                    mode = 2;
                    ty = "C3D20R";
                }
            }
            continue;
        }
        match mode {
            1 => {
                let v: Vec<f64> = l.split(',').map(|x| x.trim().parse::<f64>()).collect::<std::result::Result<_, _>>().with_context(|| format!("node line {l:?}"))?;
                let id = v[0] as usize;
                ids.insert(id, m.add_node([v[1], v[2], v[3]]));
            }
            2 => {
                let mut v: Vec<usize> = l.split(',').filter(|x| !x.trim().is_empty()).map(|x| x.trim().parse::<usize>()).collect::<std::result::Result<_, _>>().with_context(|| format!("element line {l:?}"))?;
                if pending.is_empty() {
                    pending = v;
                } else {
                    pending.append(&mut v);
                }
                let need = if ty == "C3D10" { 11 } else { 21 };
                if pending.len() >= need {
                    raw.push((ty.to_string(), pending[1..need].to_vec()));
                    pending.clear();
                }
            }
            _ => {}
        }
    }
    for (t, ns) in raw {
        let nodes = ns.iter().map(|n| ids.get(n).copied().with_context(|| format!("element uses unknown node {n}"))).collect::<Result<Vec<_>>>()?;
        m.elements.push(Element { ty: if t == "C3D10" { "C3D10" } else { "C3D20R" }, nodes, set: set.into() });
    }
    if m.elements.is_empty() {
        bail!("no C3D10/C3D20 elements in the gmsh mesh");
    }
    Ok(m)
}
