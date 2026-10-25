//! Float geometry for clearance: capsules (track segments, round pads,
//! vias) and axis-aligned rectangles (rect / roundrect pads, taken as their
//! full rectangle, which over-approximates a rounded corner and so errs on
//! the safe side). All values are nanometres as `f64`.
//!
//! The distance cases follow Horizon EDA's copper clearance check
//! (`src/board/board_rules_check_clearance_copper.cpp`, design read only):
//! two shapes are compared by the distance between their skeletons minus
//! their radii; for disjoint convex shapes the minimum is reached between a
//! vertex of one and an edge of the other, so segment–rectangle distance is
//! the minimum over endpoint–rectangle and corner–segment distances.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

impl P {
    pub const fn new(x: f64, y: f64) -> P {
        P { x, y }
    }
    pub fn dist(self, o: P) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }
}

/// Copper outline used for clearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Capsule: every point within `r` of the segment `a`-`b` (a circle when
    /// `a == b`).
    Seg { a: P, b: P, r: f64 },
    /// Axis-aligned rectangle.
    Rect { lo: P, hi: P },
}

impl Shape {
    pub fn circle(c: P, r: f64) -> Shape {
        Shape::Seg { a: c, b: c, r }
    }
    pub fn bbox(&self) -> (P, P) {
        match *self {
            Shape::Seg { a, b, r } => (P::new(a.x.min(b.x) - r, a.y.min(b.y) - r), P::new(a.x.max(b.x) + r, a.y.max(b.y) + r)),
            Shape::Rect { lo, hi } => (lo, hi),
        }
    }
    /// Distance from this shape's copper to the segment `p`-`q` (a point
    /// when `p == q`); 0 or negative when they touch.
    pub fn dist_seg(&self, p: P, q: P) -> f64 {
        match *self {
            Shape::Seg { a, b, r } => seg_seg(a, b, p, q) - r,
            Shape::Rect { lo, hi } => seg_rect(p, q, lo, hi),
        }
    }
    pub fn dist_shape(&self, o: &Shape) -> f64 {
        match (*self, *o) {
            (Shape::Seg { a, b, r }, _) => o.dist_seg(a, b) - r,
            (_, Shape::Seg { a, b, r }) => self.dist_seg(a, b) - r,
            (Shape::Rect { lo: a0, hi: a1 }, Shape::Rect { lo: b0, hi: b1 }) => {
                let dx = (b0.x - a1.x).max(a0.x - b1.x).max(0.0);
                let dy = (b0.y - a1.y).max(a0.y - b1.y).max(0.0);
                dx.hypot(dy)
            }
        }
    }
    /// Is `p` inside the copper (closed)?
    pub fn contains(&self, p: P) -> bool {
        self.dist_seg(p, p) <= 0.0
    }
}

pub fn point_seg(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    p.dist(P::new(a.x + t * dx, a.y + t * dy))
}

fn orient(a: P, b: P, c: P) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

pub fn seg_seg(a: P, b: P, c: P, d: P) -> f64 {
    if segments_cross(a, b, c, d) {
        return 0.0;
    }
    point_seg(a, c, d).min(point_seg(b, c, d)).min(point_seg(c, a, b)).min(point_seg(d, a, b))
}

pub fn point_rect(p: P, lo: P, hi: P) -> f64 {
    let dx = (lo.x - p.x).max(p.x - hi.x).max(0.0);
    let dy = (lo.y - p.y).max(p.y - hi.y).max(0.0);
    dx.hypot(dy)
}

/// Liang–Barsky: does segment `p`-`q` meet the closed rectangle?
fn seg_hits_rect(p: P, q: P, lo: P, hi: P) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = P::new(q.x - p.x, q.y - p.y);
    for (pi, qi) in [(-d.x, p.x - lo.x), (d.x, hi.x - p.x), (-d.y, p.y - lo.y), (d.y, hi.y - p.y)] {
        if pi == 0.0 {
            if qi < 0.0 {
                return false;
            }
        } else {
            let t = qi / pi;
            if pi < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

pub fn seg_rect(p: P, q: P, lo: P, hi: P) -> f64 {
    if seg_hits_rect(p, q, lo, hi) {
        return 0.0;
    }
    let corners = [lo, P::new(hi.x, lo.y), hi, P::new(lo.x, hi.y)];
    let mut m = point_rect(p, lo, hi).min(point_rect(q, lo, hi));
    for c in corners {
        m = m.min(point_seg(c, p, q));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        let r = Shape::Rect { lo: P::new(0.0, 0.0), hi: P::new(10.0, 10.0) };
        assert_eq!(r.dist_seg(P::new(13.0, 14.0), P::new(13.0, 14.0)), 5.0);
        assert_eq!(r.dist_seg(P::new(-5.0, 5.0), P::new(15.0, 5.0)), 0.0);
        // passes a corner diagonally
        let d = r.dist_seg(P::new(12.0, 10.0), P::new(10.0, 12.0));
        assert!((d - 2f64.sqrt()).abs() < 1e-9);
        let c = Shape::circle(P::new(0.0, 0.0), 1.0);
        assert!((c.dist_seg(P::new(-5.0, 3.0), P::new(5.0, 3.0)) - 2.0).abs() < 1e-12);
        assert!(c.contains(P::new(0.5, 0.5)));
        let a = Shape::Seg { a: P::new(0.0, 0.0), b: P::new(10.0, 0.0), r: 1.0 };
        let b = Shape::Seg { a: P::new(5.0, -5.0), b: P::new(5.0, 5.0), r: 1.0 };
        assert!(a.dist_shape(&b) < 0.0);
        let r2 = Shape::Rect { lo: P::new(13.0, 14.0), hi: P::new(20.0, 20.0) };
        assert_eq!(r.dist_shape(&r2), 5.0);
    }
}
