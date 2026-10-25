//! The board's copper as clearance shapes: pads (with the net the netlist
//! gives each), existing and new tracks and vias, NPTH holes, and a uniform
//! spatial hash over them for the router's distance queries.
//!
//! The spatial hash plays the part of KiCadRoutingTools'
//! `rust_router/src/obstacle_map.rs` (a per-layer obstacle grid the A*
//! asks "is this cell free for this net"), but stores exact shapes
//! instead of rasterised discs, so a query answers with a true distance and
//! the clearance never depends on how a pad lines up with the lattice
//! (the Python baseline's `routing.py` docstring: a keep-out rounded out to whole cells
//! "seals pads that are not actually crowded").

use crate::geom::{Shape, P};
use wright_eda::{Board, Layer, Netlist, PadShape, Placed, Side, Track, Via};

pub const F: u8 = 1;
pub const B: u8 = 2;

/// KiCad's `min_hole_to_hole` (lane A's `.kicad_pro` writes 0.25 mm):
/// drill edge to drill edge, any nets, same net included.
pub const HOLE_TO_HOLE: f64 = 250_000.0;

/// Is a drill of radius `r` at `c` at least [`HOLE_TO_HOLE`] from every
/// drill in `drills` (centre, radius)?
pub fn drill_clear(c: P, r: f64, drills: &[(P, f64)]) -> bool {
    drills.iter().all(|&(d, dr)| c.dist(d) - r - dr >= HOLE_TO_HOLE)
}

pub fn layer_bit(l: Layer) -> u8 {
    match l {
        Layer::FCu => F,
        Layer::BCu => B,
    }
}

/// One pad on the board, in the board frame.
#[derive(Debug, Clone)]
pub struct PadItem {
    pub part: usize,
    pub number: String,
    pub net: Option<usize>,
    pub layers: u8,
    pub shape: Shape,
    /// `shape` is the pad's exact copper (axis-aligned rect, circle or
    /// oval). False for a pad rotated off 90 degrees, whose `shape` is its
    /// bounding box: then only its centre counts as inside.
    pub exact: bool,
    pub centre: P,
    pub drilled: bool,
    /// Drill radius (0 for SMD), for hole-to-hole spacing.
    pub drill_r: f64,
    /// Corner radius of a roundrect pad (KiCad: `roundrect_rratio` times
    /// the short side); 0 otherwise. `shape` stays the full rectangle for
    /// clearance (safe side); "inside the copper" must honour the corners:
    /// a stub ending 0.01 mm inside a SOT-223 pad's bounding-box corner was
    /// outside its 0.375 mm rounding, and KiCad left it unconnected.
    pub corner: f64,
}

impl PadItem {
    /// A point strictly inside the pad copper, as close to `n` as the pad
    /// allows, `inset` in from the edge: where a track from `n` ends so
    /// its end lies inside the pad (KiCad joins a track to a pad when the
    /// track's end point is inside the pad shape).
    pub fn anchor_towards(&self, n: P, inset: f64) -> P {
        if !self.exact {
            return self.centre;
        }
        match self.shape {
            Shape::Rect { lo, hi } => {
                // Inset by the corner radius too, so the point is in the
                // straight-edged cross of a roundrect, never a corner.
                let ix = (inset + self.corner).min((hi.x - lo.x) / 2.0);
                let iy = (inset + self.corner).min((hi.y - lo.y) / 2.0);
                P::new(n.x.clamp(lo.x + ix, hi.x - ix), n.y.clamp(lo.y + iy, hi.y - iy))
            }
            Shape::Seg { a, b, r } => {
                // nearest skeleton point, then towards n by up to r - inset
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let l2 = dx * dx + dy * dy;
                let t = if l2 == 0.0 { 0.0 } else { (((n.x - a.x) * dx + (n.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
                let s = P::new(a.x + t * dx, a.y + t * dy);
                let d = s.dist(n);
                let reach = (r - inset).max(0.0);
                if d <= reach {
                    n
                } else {
                    P::new(s.x + (n.x - s.x) * reach / d, s.y + (n.y - s.y) * reach / d)
                }
            }
        }
    }
}

impl PadItem {
    /// Candidate track end points inside the pad for a stub from `n`, best
    /// first: the nearest inside point, the nearest point on the pad's long
    /// centreline (a stub that slants onto the centreline clears fine-pitch
    /// neighbours a straight one misses), and the pad centre.
    /// Is `q` on this pad's real copper (rounded corners included)? The
    /// test `verify::connected` uses for a track or via end.
    pub fn contains_copper(&self, q: P) -> bool {
        if !self.exact {
            return self.centre.dist(q) <= 1.0;
        }
        match self.shape {
            Shape::Rect { lo, hi } if self.corner > 0.0 => {
                let r = self.corner.min((hi.x - lo.x) / 2.0).min((hi.y - lo.y) / 2.0);
                crate::geom::point_rect(q, P::new(lo.x + r, lo.y + r), P::new(hi.x - r, hi.y - r)) <= r
            }
            s => s.contains(q),
        }
    }

    pub fn anchors(&self, n: P, inset: f64) -> Vec<P> {
        // The pad centre first (the Python baseline's escape starts at the exact centre, and
        // KiCad joins a track that covers the pad's centre as well as one
        // that ends in it), then the nearer points when a centre stub
        // would not clear the neighbours.
        let mut v = vec![self.centre, self.anchor_towards(n, inset)];
        if let (true, Shape::Rect { lo, hi }) = (self.exact, self.shape) {
            let c = self.centre;
            let k = inset + self.corner;
            let line = if hi.x - lo.x >= hi.y - lo.y { P::new(n.x.clamp(lo.x + k, (hi.x - k).max(lo.x + k)), c.y) } else { P::new(c.x, n.y.clamp(lo.y + k, (hi.y - k).max(lo.y + k))) };
            v.insert(1, line);
        }
        v.dedup();
        v
    }
}

fn pad_net(netlist: &Netlist, pl: &Placed, number: &str) -> Option<usize> {
    let part = netlist.parts.get(pl.part)?;
    part.pins.iter().find(|p| p.pad == number).and_then(|p| if p.no_connect { None } else { p.net })
}

/// Every copper pad on the board, plus NPTH holes as `(centre, radius)`.
pub fn pads(netlist: &Netlist, board: &Board) -> (Vec<PadItem>, Vec<(P, f64)>) {
    let mut out = Vec::new();
    let mut holes = Vec::new();
    for pl in &board.parts {
        for pad in &pl.footprint.pads {
            let c = pl.pad_abs(pad);
            let centre = P::new(c.x.0 as f64, c.y.0 as f64);
            if let (Some(drill), false) = (pad.drill, pad.plated) {
                let d = drill.0.max(pad.size.0 .0.max(pad.size.1 .0)) as f64;
                holes.push((centre, d / 2.0));
                continue;
            }
            let bb = pl.pad_bbox_abs(pad);
            let (lo, hi) = (P::new(bb.min.x.0 as f64, bb.min.y.0 as f64), P::new(bb.max.x.0 as f64, bb.max.y.0 as f64));
            let mut total = pad.rot_deg + 90.0 * pl.rot as f64;
            if pl.side == Side::Bottom {
                total = -total;
            }
            let ortho = (total / 90.0 - (total / 90.0).round()).abs() < 1e-6;
            let (w, h) = (hi.x - lo.x, hi.y - lo.y);
            let corner = match pad.shape {
                PadShape::RoundRect(rr) => rr.clamp(0.0, 0.5) * w.min(h),
                _ => 0.0,
            };
            let (shape, exact) = match pad.shape {
                PadShape::Circle if ortho || (w - h).abs() < 1.0 => (Shape::circle(centre, w.min(h) / 2.0), true),
                PadShape::Oval if ortho => {
                    let r = w.min(h) / 2.0;
                    if w >= h {
                        (Shape::Seg { a: P::new(lo.x + r, centre.y), b: P::new(hi.x - r, centre.y), r }, true)
                    } else {
                        (Shape::Seg { a: P::new(centre.x, lo.y + r), b: P::new(centre.x, hi.y - r), r }, true)
                    }
                }
                _ => (Shape::Rect { lo, hi }, ortho),
            };
            // Every pad is copper on each of its own layers (lane A's
            // `Pad::layers`, mapped through the part's side; empty = the old
            // side / through-hole rule). The TI PowerPAD's B.Cu pad 9 was
            // invisible before this and a +6V B.Cu track shorted it to GND.
            let layers = pl.pad_layers(pad).into_iter().fold(0u8, |m, l| m | layer_bit(l));
            if layers == 0 {
                continue;
            }
            out.push(PadItem { part: pl.part, number: pad.number.clone(), net: pad_net(netlist, pl, &pad.number), layers, shape, exact, centre, drilled: pad.drill.is_some(), drill_r: pad.drill.map(|d| d.0 as f64 / 2.0).unwrap_or(0.0), corner });
        }
    }
    (out, holes)
}

pub fn track_shape(t: &Track) -> Shape {
    Shape::Seg { a: P::new(t.a.x.0 as f64, t.a.y.0 as f64), b: P::new(t.b.x.0 as f64, t.b.y.0 as f64), r: t.width.0 as f64 / 2.0 }
}

pub fn via_shape(v: &Via) -> Shape {
    Shape::circle(P::new(v.at.x.0 as f64, v.at.y.0 as f64), v.diameter.0 as f64 / 2.0)
}

/// An obstacle for nets other than `net` (`None`: an obstacle for all).
#[derive(Debug, Clone)]
pub struct Obst {
    pub shape: Shape,
    pub layers: u8,
    pub net: Option<usize>,
    /// Clearance this item demands (its net class's).
    pub clr: f64,
    /// Router copper that rip-up may lift (tracks and vias it laid).
    pub soft: bool,
}

/// Obstacles bucketed into square cells.
pub struct Store {
    pub obs: Vec<Obst>,
    cell: f64,
    ox: f64,
    oy: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    pub max_clr: f64,
}

/// Distance slack, split into what rip-up cannot move (`hard`: pads, holes,
/// locked copper) and what it can (`soft`).
#[derive(Debug, Clone, Copy)]
pub struct Slack {
    pub hard: f64,
    pub soft: f64,
}

impl Store {
    pub fn new(lo: P, hi: P, cell: f64) -> Store {
        let nx = (((hi.x - lo.x) / cell).ceil() as usize).max(1) + 2;
        let ny = (((hi.y - lo.y) / cell).ceil() as usize).max(1) + 2;
        Store { obs: Vec::new(), cell, ox: lo.x - cell, oy: lo.y - cell, nx, ny, cells: vec![Vec::new(); nx * ny], max_clr: 0.0 }
    }
    fn range(&self, lo: P, hi: P) -> (usize, usize, usize, usize) {
        let f = |v: f64, o: f64, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        (f(lo.x, self.ox, self.nx), f(hi.x, self.ox, self.nx), f(lo.y, self.oy, self.ny), f(hi.y, self.oy, self.ny))
    }
    pub fn push(&mut self, o: Obst) {
        let (lo, hi) = o.shape.bbox();
        let (i0, i1, j0, j1) = self.range(lo, hi);
        let id = self.obs.len() as u32;
        for j in j0..=j1 {
            for i in i0..=i1 {
                // Only cells the shape touches: a long diagonal track's
                // bounding box covers far more cells than its copper.
                let clo = P::new(self.ox + i as f64 * self.cell, self.oy + j as f64 * self.cell);
                let cell = Shape::Rect { lo: clo, hi: P::new(clo.x + self.cell, clo.y + self.cell) };
                if o.shape.dist_shape(&cell) <= 0.0 {
                    self.cells[j * self.nx + i].push(id);
                }
            }
        }
        self.max_clr = self.max_clr.max(o.clr);
        self.obs.push(o);
    }
    /// Smallest `distance - max(clearance)` from segment `p`-`q` to copper
    /// of other nets on `layers`, each part capped at `reach`.
    pub fn slack(&self, p: P, q: P, layers: u8, net: usize, my_clr: f64, reach: f64) -> Slack {
        let m = reach + self.max_clr.max(my_clr);
        let (i0, i1, j0, j1) = self.range(P::new(p.x.min(q.x) - m, p.y.min(q.y) - m), P::new(p.x.max(q.x) + m, p.y.max(q.y) + m));
        let mut s = Slack { hard: reach, soft: reach };
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &id in &self.cells[j * self.nx + i] {
                    let o = &self.obs[id as usize];
                    if o.layers & layers == 0 || o.net == Some(net) {
                        continue;
                    }
                    let d = o.shape.dist_seg(p, q) - o.clr.max(my_clr);
                    if o.soft {
                        s.soft = s.soft.min(d);
                    } else {
                        s.hard = s.hard.min(d);
                    }
                }
            }
        }
        s
    }
    /// Nets of soft copper the segment (radius `hw`) would violate.
    #[allow(clippy::too_many_arguments)]
    pub fn soft_conflicts(&self, p: P, q: P, layers: u8, net: usize, my_clr: f64, hw: f64, out: &mut Vec<usize>) {
        let m = hw + self.max_clr.max(my_clr);
        let (i0, i1, j0, j1) = self.range(P::new(p.x.min(q.x) - m, p.y.min(q.y) - m), P::new(p.x.max(q.x) + m, p.y.max(q.y) + m));
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &id in &self.cells[j * self.nx + i] {
                    let o = &self.obs[id as usize];
                    if !o.soft || o.layers & layers == 0 || o.net == Some(net) {
                        continue;
                    }
                    if o.shape.dist_seg(p, q) - o.clr.max(my_clr) < hw {
                        if let Some(n) = o.net {
                            if !out.contains(&n) {
                                out.push(n);
                            }
                        }
                    }
                }
            }
        }
    }
}
