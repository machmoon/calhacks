//! Two-layer octilinear A* over a uniform lattice, the search of the Python baseline's
//! `routing.py` `_astar` ported to Rust:
//!
//! * eight moves priced 12 (orthogonal) and 17 (diagonal), the integer pair
//!   closest to 1 : sqrt(2) so ties resolve the same way every run (the baseline's
//!   `_ORTHO_COST`/`_DIAG_COST`; HOG2 `environments/Map2DEnvironment.cpp`
//!   octile heuristic `min * DIAGONAL_COST + (max - min)`);
//! * no acute turns: a step may turn at most 90 degrees from the step that
//!   reached the node (FreeRouting `AngleRestriction.FORTYFIVE_DEGREE`);
//! * a via is a move to the other layer at the same node with its own cost
//!   and its own clearance disc;
//! * budgets are counted in node expansions, never seconds.
//!
//! What differs from the baseline: clearance is not a rasterised occupancy grid but
//! an exact distance query against [`crate::copper::Store`], cached per node
//! for the net being routed and computed **with that net's class width**,
//! so a 1.85 mm rail and a 0.2 mm signal see different free space on the
//! same lattice. HOG2's no-corner-cutting rule is not needed: every diagonal
//! edge is checked as a real segment.

use crate::copper::{Store, B, F};
use crate::geom::{Shape, P};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub const ORTHO: u32 = 12;
pub const DIAG: u32 = 17;
const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const NO_DIR: u8 = 8;
const UNSET: f32 = f32::NAN;

/// The routing lattice over the board outline.
#[derive(Debug, Clone)]
pub struct Grid {
    pub ox: i64,
    pub oy: i64,
    pub g: i64,
    pub nx: usize,
    pub ny: usize,
    /// Outline, nm.
    pub lo: P,
    pub hi: P,
    pub edge_clr: f64,
}

impl Grid {
    pub fn layer_nodes(&self) -> usize {
        self.nx * self.ny
    }
    pub fn idx(&self, i: usize, j: usize, l: usize) -> u32 {
        (l * self.layer_nodes() + j * self.nx + i) as u32
    }
    pub fn ijl(&self, n: u32) -> (usize, usize, usize) {
        let n = n as usize;
        let ln = self.layer_nodes();
        let l = n / ln;
        let r = n % ln;
        (r % self.nx, r / self.nx, l)
    }
    pub fn xy(&self, n: u32) -> (i64, i64) {
        let (i, j, _) = self.ijl(n);
        (self.ox + i as i64 * self.g, self.oy + j as i64 * self.g)
    }
    pub fn pt(&self, n: u32) -> P {
        let (x, y) = self.xy(n);
        P::new(x as f64, y as f64)
    }
    pub fn layer_bit(&self, n: u32) -> u8 {
        if self.ijl(n).2 == 0 {
            F
        } else {
            B
        }
    }
    /// Node index range (inclusive) covering `[lo, hi]`, clipped.
    pub fn span(&self, lo: P, hi: P) -> Option<(usize, usize, usize, usize)> {
        let g = self.g as f64;
        let i0 = ((lo.x - self.ox as f64) / g).ceil().max(0.0) as usize;
        let j0 = ((lo.y - self.oy as f64) / g).ceil().max(0.0) as usize;
        let i1 = ((hi.x - self.ox as f64) / g).floor();
        let j1 = ((hi.y - self.oy as f64) / g).floor();
        if i1 < 0.0 || j1 < 0.0 {
            return None;
        }
        let (i1, j1) = ((i1 as usize).min(self.nx - 1), (j1 as usize).min(self.ny - 1));
        (i0 <= i1 && j0 <= j1).then_some((i0, i1, j0, j1))
    }
    /// Distance from `p` to Edge.Cuts minus `Rules.edge_clearance` (the
    /// copper-to-edge rule lane A's `.kicad_pro` writes as
    /// `min_copper_edge_clearance`).
    pub fn edge_slack(&self, p: P) -> f64 {
        (p.x - self.lo.x).min(self.hi.x - p.x).min(p.y - self.lo.y).min(self.hi.y - p.y) - self.edge_clr
    }
}

/// What one net asks of the search.
#[derive(Debug, Clone)]
pub struct Spec {
    pub net: usize,
    /// Half track width plus a 1 µm safety margin.
    pub hw: f64,
    pub clr: f64,
    pub via_r: f64,
    /// Own-net SMD pads: a via may not sit in one (no via-in-pad).
    pub own_smd: Vec<Shape>,
    /// Step cost multiplier per layer, in halves (2 = 1x, 3 = 1.5x).
    pub layer_mult: [u32; 2],
    pub via_cost: u32,
    pub allow_vias: bool,
    /// Rip-up probe: foreign router copper is passable at this cost per node.
    pub probe_cost: Option<u32>,
    /// Proximity cost: a step whose node is within `prox_nm` of fixed
    /// copper (pads, holes, the edge) beyond what clearance needs pays up to
    /// `prox_cost`, falling off linearly. KiCadRoutingTools
    /// `rust_router/src/router.rs` charges the same kind of term
    /// (`get_stub_proximity_cost`, `get_layer_proximity_cost`) so early
    /// nets leave pad approaches free for the nets that must reach them.
    pub prox_nm: f64,
    pub prox_cost: u32,
    /// Weighted A*: f = g + w·h. KiCadRoutingTools' rust_router
    /// (`rust_router/src/router.rs` `heuristic_to_targets`, "multiplies by
    /// h_weight") with `heuristic_weight: float = 2.3` in
    /// `py_router/routing_config.py`. 1.0 is plain admissible A*.
    pub h_weight: f64,
    pub via_drill_r: f64,
    /// Drills a new via of this net must keep [`crate::copper::HOLE_TO_HOLE`]
    /// from: its own vias and drilled pads (foreign ones are kept away by
    /// copper clearance already). Without this two same-net vias could
    /// land on one spot (kicad-cli `hole_to_hole` 0.000 mm).
    pub own_drills: Vec<(P, f64)>,
}

/// Reusable search buffers (sized once per board) plus a per-net cache of
/// clearance slack at each node.
pub struct Search {
    gcost: Vec<u32>,
    par: Vec<u32>,
    dir: Vec<u8>,
    seen: Vec<u32>,
    closed: Vec<u32>,
    gen: u32,
    sl_h: Vec<f32>,
    sl_s: Vec<f32>,
    via_ok: Vec<u8>,
    goal: Vec<u32>,
    pub expansions: u64,
}

pub enum Fail {
    Blocked,
    Budget,
}

impl Search {
    pub fn new(grid: &Grid) -> Search {
        let n = grid.layer_nodes() * 2;
        Search {
            gcost: vec![0; n],
            par: vec![u32::MAX; n],
            dir: vec![NO_DIR; n],
            seen: vec![0; n],
            closed: vec![0; n],
            gen: 0,
            sl_h: vec![UNSET; n],
            sl_s: vec![UNSET; n],
            via_ok: vec![2; n],
            goal: vec![0; n],
            expansions: 0,
        }
    }

    /// Forget cached slack (call when the net or the copper changes).
    /// Forget cached via legality only (after the net's own drills grow).
    pub fn reset_via_cache(&mut self) {
        self.via_ok.fill(2);
    }

    pub fn reset_cache(&mut self) {
        self.sl_h.fill(UNSET);
        self.sl_s.fill(UNSET);
        self.via_ok.fill(2);
    }

    fn reach(grid: &Grid, s: &Spec) -> f64 {
        s.hw.max(s.via_r) + 1.5 * grid.g as f64 + 1_000.0
    }

    pub fn slack(&mut self, grid: &Grid, store: &Store, s: &Spec, n: u32) -> (f64, f64) {
        let k = n as usize;
        if self.sl_h[k].is_nan() {
            let p = grid.pt(n);
            let r = Self::reach(grid, s);
            let sl = store.slack(p, p, grid.layer_bit(n), s.net, s.clr, r);
            self.sl_h[k] = sl.hard.min(grid.edge_slack(p)) as f32;
            self.sl_s[k] = sl.soft as f32;
        }
        (self.sl_h[k] as f64, self.sl_s[k] as f64)
    }

    pub fn node_ok(&mut self, grid: &Grid, store: &Store, s: &Spec, n: u32) -> bool {
        let (h, so) = self.slack(grid, store, s, n);
        h >= s.hw && (s.probe_cost.is_some() || so >= s.hw)
    }

    /// Exact check of a straight piece of copper of half-width `s.hw` on
    /// `layer`, used for edges near copper and for pad stubs.
    pub fn seg_ok(grid: &Grid, store: &Store, s: &Spec, p: P, q: P, layer: u8) -> bool {
        let sl = store.slack(p, q, layer, s.net, s.clr, s.hw + 1.0);
        let edge = grid.edge_slack(p).min(grid.edge_slack(q));
        sl.hard >= s.hw && edge >= s.hw && (s.probe_cost.is_some() || sl.soft >= s.hw)
    }

    fn edge_ok(&mut self, grid: &Grid, store: &Store, s: &Spec, u: u32, v: u32, len: f64) -> bool {
        let (hu, su) = self.slack(grid, store, s, u);
        let (hv, sv) = self.slack(grid, store, s, v);
        let half = len / 2.0;
        if hu.min(hv) - half >= s.hw && (s.probe_cost.is_some() || su.min(sv) - half >= s.hw) {
            return true;
        }
        Self::seg_ok(grid, store, s, grid.pt(u), grid.pt(v), grid.layer_bit(u))
    }

    fn via_here(&mut self, grid: &Grid, store: &Store, s: &Spec, u: u32) -> bool {
        let k = u as usize;
        if self.via_ok[k] == 2 {
            let (i, j, _) = grid.ijl(u);
            let (a, b) = (grid.idx(i, j, 0), grid.idx(i, j, 1));
            let (ha, sa) = self.slack(grid, store, s, a);
            let (hb, sb) = self.slack(grid, store, s, b);
            let p = grid.pt(u);
            let mut ok = ha.min(hb) >= s.via_r && (s.probe_cost.is_some() || sa.min(sb) >= s.via_r);
            if ok {
                ok = s.own_smd.iter().all(|sh| sh.dist_seg(p, p) - s.clr >= s.via_r);
            }
            if ok {
                ok = crate::copper::drill_clear(p, s.via_drill_r, &s.own_drills);
            }
            let v = ok as u8;
            self.via_ok[a as usize] = v;
            self.via_ok[b as usize] = v;
        }
        self.via_ok[k] == 1
    }

    pub fn mark_goals(&mut self, goals: &[u32]) {
        for &g in goals {
            self.goal[g as usize] = self.gen + 1;
        }
    }

    /// Cheapest path from any source to any marked goal (call
    /// [`Search::mark_goals`] first). `target` is the goal region's bounding
    /// box for the heuristic. Returns source-to-goal node indices.
    pub fn run(&mut self, grid: &Grid, store: &Store, s: &Spec, sources: &[u32], target: (P, P), budget: u64) -> Result<Vec<u32>, Fail> {
        self.gen += 1;
        let gen = self.gen;
        let g = grid.g as f64;
        let h = |n: u32| -> u32 {
            let p = grid.pt(n);
            let dx = ((target.0.x - p.x).max(p.x - target.1.x).max(0.0) / g).floor();
            let dy = ((target.0.y - p.y).max(p.y - target.1.y).max(0.0) / g).floor();
            let (mn, mx) = (dx.min(dy), dx.max(dy));
            ((ORTHO as f64 * (mx - mn) + DIAG as f64 * mn) * s.h_weight) as u32
        };
        let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
        for &src in sources {
            let k = src as usize;
            if self.seen[k] == gen || !self.node_ok(grid, store, s, src) {
                continue;
            }
            self.seen[k] = gen;
            self.gcost[k] = 0;
            self.par[k] = u32::MAX;
            self.dir[k] = NO_DIR;
            let hh = h(src);
            heap.push(Reverse((hh, hh, src)));
        }
        let mut spent = 0u64;
        while let Some(Reverse((_, _, u))) = heap.pop() {
            let ku = u as usize;
            if self.closed[ku] == gen {
                continue;
            }
            self.closed[ku] = gen;
            if self.goal[ku] == gen {
                let mut path = vec![u];
                let mut c = u;
                while self.par[c as usize] != u32::MAX {
                    c = self.par[c as usize];
                    path.push(c);
                }
                path.reverse();
                return Ok(path);
            }
            spent += 1;
            self.expansions += 1;
            if spent > budget {
                return Err(Fail::Budget);
            }
            let (i, j, l) = grid.ijl(u);
            let gu = self.gcost[ku];
            let du = self.dir[ku];
            for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                if du != NO_DIR {
                    let (ax, ay) = DIRS[du as usize];
                    if ax * dx + ay * dy < 0 {
                        continue; // acute turn
                    }
                }
                let (ni, nj) = (i as i64 + dx as i64, j as i64 + dy as i64);
                if ni < 0 || nj < 0 || ni >= grid.nx as i64 || nj >= grid.ny as i64 {
                    continue;
                }
                let v = grid.idx(ni as usize, nj as usize, l);
                let kv = v as usize;
                if self.closed[kv] == gen || !self.node_ok(grid, store, s, v) {
                    continue;
                }
                let diag = dx != 0 && dy != 0;
                let len = if diag { g * std::f64::consts::SQRT_2 } else { g };
                let mut step = if diag { DIAG } else { ORTHO } * s.layer_mult[l] / 2;
                if du != NO_DIR && du as usize != d {
                    step += 2; // a bend costs a little: fewer, longer segments
                }
                if s.prox_cost > 0 {
                    let free = self.slack(grid, store, s, v).0 - s.hw;
                    if free < s.prox_nm {
                        step += (s.prox_cost as f64 * (1.0 - free.max(0.0) / s.prox_nm)) as u32;
                    }
                }
                if let Some(pc) = s.probe_cost {
                    if self.slack(grid, store, s, v).1 < s.hw {
                        step += pc;
                    }
                }
                let ng = gu + step;
                if self.seen[kv] == gen && self.gcost[kv] <= ng {
                    continue;
                }
                if !self.edge_ok(grid, store, s, u, v, len) {
                    continue;
                }
                self.seen[kv] = gen;
                self.gcost[kv] = ng;
                self.par[kv] = u;
                self.dir[kv] = d as u8;
                let hv = h(v);
                heap.push(Reverse((ng + hv, hv, v)));
            }
            if s.allow_vias {
                let v = grid.idx(i, j, 1 - l);
                let kv = v as usize;
                if self.closed[kv] != gen && self.node_ok(grid, store, s, v) && self.via_here(grid, store, s, u) {
                    let ng = gu + s.via_cost;
                    if !(self.seen[kv] == gen && self.gcost[kv] <= ng) {
                        self.seen[kv] = gen;
                        self.gcost[kv] = ng;
                        self.par[kv] = u;
                        self.dir[kv] = NO_DIR;
                        let hv = h(v);
                        heap.push(Reverse((ng + hv, hv, v)));
                    }
                }
            }
        }
        Err(Fail::Blocked)
    }
}
