//! A footprint as the placer sees it: its courtyard and pad centres for each
//! of the four quarter turns, relative to the footprint origin, in float
//! millimetres. The annealer works in float mm as upstream does
//! (KiCadRoutingTools `simple_anneal.py`; the Python baseline's `simple/objective.py`
//! "Units"); integer nanometres hold at the boundary, where
//! [`crate::legal`] snaps every pose and checks it exactly.
//!
//! Rotation is `wright_eda::Pt::rot90`, the board's one convention (KiCad
//! CCW in Y-down: (x, y) -> (y, -x) per quarter turn), so a pose read here
//! is the pose `Placed::to_board` draws. Every part is on the top side.

use wright_eda::{Footprint, Pt, Rect};

pub const NM: f64 = 1e6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl R {
    pub fn from_rect(r: Rect) -> R {
        R { x0: r.min.x.0 as f64 / NM, y0: r.min.y.0 as f64 / NM, x1: r.max.x.0 as f64 / NM, y1: r.max.y.0 as f64 / NM }
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn at(&self, x: f64, y: f64) -> R {
        R { x0: self.x0 + x, y0: self.y0 + y, x1: self.x1 + x, y1: self.y1 + y }
    }
    pub fn grow(&self, m: f64) -> R {
        R { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m }
    }
    pub fn union(&self, o: &R) -> R {
        R { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    /// Manhattan gap between two rects; 0 when they touch or overlap
    /// (upstream `rect_gap`).
    pub fn gap(&self, o: &R) -> f64 {
        (self.x0.max(o.x0) - self.x1.min(o.x1)).max(0.0) + (self.y0.max(o.y0) - self.y1.min(o.y1)).max(0.0)
    }
    /// Intersection area, 0 when they only touch.
    pub fn overlap(&self, o: &R) -> f64 {
        let ox = self.x1.min(o.x1) - self.x0.max(o.x0);
        if ox <= 0.0 {
            return 0.0;
        }
        let oy = self.y1.min(o.y1) - self.y0.max(o.y0);
        if oy <= 0.0 {
            0.0
        } else {
            ox * oy
        }
    }
}

/// One part's shape under each quarter turn.
#[derive(Debug, Clone)]
pub struct PartGeom {
    /// Courtyard relative to the origin, per quarter turn.
    pub crt: [R; 4],
    /// Pad centres relative to the origin: `pads[q][k]` for footprint pad `k`.
    pub pads: [Vec<(f64, f64)>; 4],
    /// Pad copper boxes relative to the origin, per quarter turn.
    pub pad_rects: [Vec<R>; 4],
    /// Drilled (copper on both layers).
    pub drilled: Vec<bool>,
    /// Pad numbers, footprint order.
    pub numbers: Vec<String>,
    /// May the placer turn it (false for a part pinned to an edge by name
    /// that should keep its library orientation; true otherwise).
    pub can_turn: bool,
}

impl PartGeom {
    pub fn new(fp: &Footprint) -> PartGeom {
        let crt = [0, 1, 2, 3].map(|q| R::from_rect(fp.courtyard.rot90(q)));
        let pads = [0, 1, 2, 3].map(|q| {
            fp.pads
                .iter()
                .map(|p| {
                    let r: Pt = p.at.rot90(q);
                    (r.x.0 as f64 / NM, r.y.0 as f64 / NM)
                })
                .collect()
        });
        let pad_rects = [0, 1, 2, 3].map(|q| fp.pads.iter().map(|p| R::from_rect(p.local_bbox().rot90(q))).collect());
        let drilled = fp.pads.iter().map(|p| p.drill.is_some()).collect();
        let numbers = fp.pads.iter().map(|p| p.number.clone()).collect();
        PartGeom { crt, pads, pad_rects, drilled, numbers, can_turn: true }
    }
    /// Indices of the footprint pads carrying pad number `n`.
    pub fn pads_numbered(&self, n: &str) -> Vec<usize> {
        self.numbers.iter().enumerate().filter(|(_, m)| m.as_str() == n).map(|(i, _)| i).collect()
    }
}

/// Rotate an offset by quarter turns, `Pt::rot90`'s convention.
pub fn rot(x: f64, y: f64, q: usize) -> (f64, f64) {
    match q % 4 {
        0 => (x, y),
        1 => (y, -x),
        2 => (-x, -y),
        _ => (-y, x),
    }
}
