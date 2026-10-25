//! Constructive solid geometry on polygon meshes by BSP trees.
//!
//! A port of Evan Wallace's csg.js (github.com/evanw/csg.js, `csg.js`, MIT),
//! the algorithm csgrs and OpenJSCAD descend from: each solid becomes a BSP
//! tree of its polygons; `a - b` clips a's polygons to outside b and b's to
//! inside a, inverts b and joins. Kept to the original's structure and
//! names (`Plane::split_polygon`, `Node::clip_to`, `Node::invert`,
//! `Node::build`) so it can be checked line by line against csg.js.
//! Deviation: f64 and an explicit epsilon of 1e-6 in the mesh's own unit
//! (mm here), csg.js's `Plane.EPSILON = 1e-5` in whatever unit it gets.

use crate::V3;

const EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, Copy)]
pub struct Plane {
    pub normal: V3,
    pub w: f64,
}

#[derive(Debug, Clone)]
pub struct Polygon {
    pub vertices: Vec<V3>,
    pub plane: Plane,
}

impl Polygon {
    /// A convex planar polygon; `None` when degenerate (no area).
    pub fn new(vertices: Vec<V3>) -> Option<Self> {
        let plane = Plane::from_points(vertices[0], vertices[1], vertices[2])?;
        Some(Self { vertices, plane })
    }
    fn flip(&mut self) {
        self.vertices.reverse();
        self.plane.flip();
    }
}

impl Plane {
    pub fn from_points(a: V3, b: V3, c: V3) -> Option<Self> {
        let n = (b - a).cross(c - a);
        let len = n.norm();
        if len < 1e-12 {
            return None;
        }
        let normal = n / len;
        Some(Self { normal, w: normal.dot(a) })
    }
    fn flip(&mut self) {
        self.normal = -self.normal;
        self.w = -self.w;
    }

    /// Split `p` by this plane into the four lists, as csg.js does.
    fn split_polygon(
        &self,
        p: &Polygon,
        coplanar_front: &mut Vec<Polygon>,
        coplanar_back: &mut Vec<Polygon>,
        front: &mut Vec<Polygon>,
        back: &mut Vec<Polygon>,
    ) {
        const COPLANAR: u8 = 0;
        const FRONT: u8 = 1;
        const BACK: u8 = 2;
        const SPANNING: u8 = 3;
        let mut kind = 0u8;
        let types: Vec<u8> = p
            .vertices
            .iter()
            .map(|v| {
                let t = self.normal.dot(*v) - self.w;
                let ty = if t < -EPSILON {
                    BACK
                } else if t > EPSILON {
                    FRONT
                } else {
                    COPLANAR
                };
                kind |= ty;
                ty
            })
            .collect();
        match kind {
            COPLANAR => {
                if self.normal.dot(p.plane.normal) > 0.0 {
                    coplanar_front.push(p.clone())
                } else {
                    coplanar_back.push(p.clone())
                }
            }
            FRONT => front.push(p.clone()),
            BACK => back.push(p.clone()),
            _ => {
                let (mut f, mut b) = (Vec::new(), Vec::new());
                let n = p.vertices.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (ti, tj) = (types[i], types[j]);
                    let (vi, vj) = (p.vertices[i], p.vertices[j]);
                    if ti != BACK {
                        f.push(vi);
                    }
                    if ti != FRONT {
                        b.push(vi);
                    }
                    if (ti | tj) == SPANNING {
                        let t = (self.w - self.normal.dot(vi)) / self.normal.dot(vj - vi);
                        let v = vi + (vj - vi) * t;
                        f.push(v);
                        b.push(v);
                    }
                }
                if f.len() >= 3 {
                    front.push(Polygon { vertices: f, plane: p.plane });
                }
                if b.len() >= 3 {
                    back.push(Polygon { vertices: b, plane: p.plane });
                }
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    fn new(polygons: Vec<Polygon>) -> Self {
        let mut n = Node::default();
        n.build(polygons);
        n
    }

    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(pl) = &mut self.plane {
            pl.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = self.plane else {
            return polygons;
        };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in &polygons {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            plane.split_polygon(p, &mut cf, &mut cb, &mut front, &mut back);
            front.extend(cf);
            back.extend(cb);
        }
        let front = match &self.front {
            Some(f) => f.clip_polygons(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back),
            None => Vec::new(),
        };
        let mut out = front;
        out.extend(back);
        out
    }

    fn clip_to(&mut self, bsp: &Node) {
        self.polygons = bsp.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(f) = &mut self.front {
            f.clip_to(bsp);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(bsp);
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        let mut out = self.polygons.clone();
        if let Some(f) = &self.front {
            out.extend(f.all_polygons());
        }
        if let Some(b) = &self.back {
            out.extend(b.all_polygons());
        }
        out
    }

    fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        let plane = *self.plane.get_or_insert(polygons[0].plane);
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in &polygons {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            plane.split_polygon(p, &mut cf, &mut cb, &mut front, &mut back);
            self.polygons.extend(cf);
            self.polygons.extend(cb);
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back);
        }
    }
}

/// `a ∪ b` (csg.js `union`).
pub fn union(a: Vec<Polygon>, b: Vec<Polygon>) -> Vec<Polygon> {
    let mut a = Node::new(a);
    let mut b = Node::new(b);
    a.clip_to(&b);
    b.clip_to(&a);
    b.invert();
    b.clip_to(&a);
    b.invert();
    a.build(b.all_polygons());
    a.all_polygons()
}

/// `a - b` (csg.js `subtract`).
pub fn subtract(a: Vec<Polygon>, b: Vec<Polygon>) -> Vec<Polygon> {
    let mut a = Node::new(a);
    let mut b = Node::new(b);
    a.invert();
    a.clip_to(&b);
    b.clip_to(&a);
    b.invert();
    b.clip_to(&a);
    b.invert();
    a.build(b.all_polygons());
    a.invert();
    a.all_polygons()
}

/// `a ∩ b` (csg.js `intersect`).
pub fn intersect(a: Vec<Polygon>, b: Vec<Polygon>) -> Vec<Polygon> {
    let mut a = Node::new(a);
    let mut b = Node::new(b);
    a.invert();
    b.clip_to(&a);
    b.invert();
    a.clip_to(&b);
    b.clip_to(&a);
    a.build(b.all_polygons());
    a.invert();
    a.all_polygons()
}
