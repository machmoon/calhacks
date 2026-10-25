//! Simulated annealing placement after KiCadRoutingTools v0.23.7 (MIT,
//! github.com/drandyhaas/KiCadRoutingTools, read 2026-10-24):
//! `py_placer/placement/simple_anneal.py` (schedule, moves, carry, best
//! state, legality) and `py_placer/placement/simple.py` (the objective),
//! with the weights `py_placer/place_simple.py` passes. The Python baseline's earlier port
//! (`engine/silkscreen/simple/{anneal,objective}.py`) was read alongside.
//!
//! Cost terms (upstream name, weight):
//! * wire: each net's Manhattan spanning tree (`mst_l1`, Prim from pin 0),
//!   signal 1, rail 0.3, ground 0.02;
//! * crossings among signal-tree edges, 2 each;
//! * claims (`claim_cost` 1 per mm, `claim_over` 10 per mm past `max_mm`):
//!   crystals and load caps, and the intent's proximity rows;
//! * decaps (2 per mm): caps matched to their cluster's supply-pin sites
//!   (`cluster_match`: pins of one IC within `SITE_MM` 1.5 mm are one site;
//!   every site takes one cap before any takes two, by assignment);
//! * group contiguity (5, `gap_tree` over member courtyards) and the
//!   groups' `near` 2 / `apart` 5 (under `APART_MM` 3) / `edge` 2 hints;
//! * front halo (10 x short^2): room for fine-pitch fan-out, 0.25 mm per
//!   net leaving each side of a part with >= 8 pads at <= 1.0 mm pitch;
//! * congestion (5, [`crate::congest`]);
//! * template drift (100 per mm of mean pad deviation, `templates.py`).
//!
//! Overlap is soft early and hard late: courtyards grown by half the
//! spacing, weight `LAM_START` 1 -> `LAM_END` 1e4 geometrically; T from the
//! median-move temperature at `ACCEPT` 0.1 down to `T_END` 0.02; reach from
//! a sixth of the span as (1 - progress)^2, floor 0.1 mm. Moves and their
//! draw weights are upstream's `MOVES` (shift .40, turn .15, swap .10, jump
//! .10, dock .10, gshift .12, gturn .08, gswap .05, escape .05), `gturn` and
//! `gswap` only in the first `BIG_UNTIL` quarter; `shift`, `turn`, `jump`
//! and `dock` carry the moved part's decaps, claimed parts and template
//! members (`CARRY`, `_carry`, `follow`). Only an overlap-free state can be
//! the best; the exact integer legality gate is [`crate::legal`].
//!
//! Wright's changes, each for a stated reason:
//! * **No fixed board** unless one is given: the outline is the result, so
//!   an AREA term (courtyard bounding box, mm^2) joins the cost, as the baseline's
//!   CP-SAT objective had (`packing.py`: HPWL + area); edge hints read the
//!   bounding box's sides.
//! * The decap term adds the **ground leg** (cap ground pad to its IC's
//!   ground pin): the loop is what decouples, and it is the number the baseline's eval
//!   reports (`_loop_mm`).
//! * Connectors with no stated edge get a soft any-edge hint (weight 2):
//!   upstream seats declared edge parts on a known outline; here the outline
//!   does not exist until the end.
//! * Not ported: `flip` and opposite faces (single-sided slice), seated-edge
//!   projection and `edge_seat.pack`, foreign-pad hits, intent decap limits,
//!   `repair`/shove (a seed that never comes clean falls back, and eleven
//!   other seeds remain).

use crate::congest::{Grid, TRACK_PITCH_MM};
use crate::decap::assign;
use crate::geom::{rot, PartGeom, R};
use crate::rng::Rng;

pub const LAM_START: f64 = 1.0;
pub const LAM_END: f64 = 1e4;
pub const ACCEPT: f64 = 0.1;
pub const T_END: f64 = 0.02;
pub const ESCAPE_TRIES: usize = 48;
pub const BIG_UNTIL: f64 = 0.25;
/// How far (mm, past the chip's size) a decap may sit and still be carried.
pub const TETHER_MM: f64 = 5.0;
/// mm^2 of overlap that counts as none for the running sum (upstream
/// `CLEAN`); a candidate best is re-checked from scratch at exactly 0.
pub const CLEAN: f64 = 1e-6;
pub const APART_MM: f64 = 3.0;
pub const SITE_MM: f64 = 1.5;
pub const HALO_MIN_PADS: usize = 8;
pub const HALO_PITCH_MM: f64 = 1.0;
pub const FAN_MM: f64 = 0.25;
pub const HALO_SMALL_PADS: usize = 4;

/// Cost weights: upstream `place_simple.py` defaults, plus Wright's area.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Weights {
    pub signal: f64,
    pub rail: f64,
    pub ground: f64,
    pub crossing: f64,
    pub claim: f64,
    pub claim_limit: f64,
    pub decap: f64,
    pub group: f64,
    pub near: f64,
    pub apart: f64,
    pub edge: f64,
    pub halo: f64,
    pub halo_coef: f64,
    pub congestion: f64,
    pub template: f64,
    /// Per mm a decap's loop runs past its limit (upstream `decap_limit_weight`).
    pub decap_limit: f64,
    /// Per mm^2 of courtyard bounding box (0 with a fixed outline).
    pub area: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            signal: 1.0,
            rail: 0.3,
            ground: 0.02,
            crossing: 2.0,
            claim: 1.0,
            claim_limit: 10.0,
            decap: 2.0,
            group: 5.0,
            near: 2.0,
            apart: 5.0,
            edge: 2.0,
            halo: 10.0,
            halo_coef: 0.25,
            congestion: 5.0,
            template: 100.0,
            decap_limit: 10.0,
            area: 0.25,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Signal,
    Rail,
    Ground,
}

/// A net as the cost sees it: one footprint pad per pin, (part, pad index).
#[derive(Debug, Clone)]
pub struct CNet {
    pub kind: Kind,
    pub pins: Vec<(usize, usize)>,
    /// Counts toward congestion (signals and unpoured rails).
    pub congest: bool,
}

#[derive(Debug, Clone)]
pub struct CapLeg {
    pub part: usize,
    pub rail: Vec<usize>,
    pub ground: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Site {
    pub ic: usize,
    pub pads: Vec<usize>,
    /// The IC's pads on the caps' ground net.
    pub ground: Vec<usize>,
}

/// Decaps that compete for one set of supply-pin sites.
#[derive(Debug, Clone)]
pub struct Cluster {
    pub caps: Vec<CapLeg>,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone)]
pub struct PClaim {
    pub subject: usize,
    pub subject_pads: Vec<usize>,
    pub partner: usize,
    pub partner_pads: Vec<usize>,
    pub max_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeSide {
    Any,
    Left,
    Right,
    Top,
    Bottom,
}

/// A part held to the board's edge (sliding along it) or to a corner
/// (mounting holes): upstream's seated edge parts (`simple_anneal.py`
/// seats, projected onto their edge line at every move).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Edge(EdgeSide),
    /// (Left|Right, Top|Bottom)
    Corner(EdgeSide, EdgeSide),
}

/// One decap's loop limit, measured as the eval does (the netlist's own
/// cap -> IC pairing, rail leg + ground leg).
#[derive(Debug, Clone)]
pub struct DLimit {
    pub cap: usize,
    pub ic: usize,
    pub rail: (Vec<usize>, Vec<usize>),
    pub ground: Option<(Vec<usize>, Vec<usize>)>,
    pub max_mm: f64,
}

#[derive(Debug, Clone)]
pub struct PGroup {
    pub members: Vec<usize>,
    pub near: Vec<usize>,
    pub apart: Vec<usize>,
    pub edge: Option<EdgeSide>,
}

/// A fitted template: members' poses relative to the anchor, unturned.
#[derive(Debug, Clone)]
pub struct PTemplate {
    pub anchor: usize,
    /// (part, dx, dy, quarter turns)
    pub members: Vec<(usize, f64, f64, usize)>,
}

/// A fine-pitch part's fan-out fronts for one quarter turn, sides W E N S
/// (N = min y): depth needed (mm) and the span the front covers along its
/// side, relative to the origin.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fronts {
    pub depth: [f64; 4],
    pub span: [(f64, f64); 4],
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub parts: Vec<PartGeom>,
    pub locked: Vec<bool>,
    pub nets: Vec<CNet>,
    pub clusters: Vec<Cluster>,
    pub claims: Vec<PClaim>,
    pub groups: Vec<PGroup>,
    pub templates: Vec<PTemplate>,
    /// Per part, its fronts per quarter turn when it is fine-pitch.
    pub fine: Vec<Option<[Fronts; 4]>>,
    /// Pairs exempt from the halo (a small part wired to the fine part).
    pub halo_exempt: Vec<(usize, usize)>,
    pub edges: Vec<(usize, EdgeSide, f64)>,
    /// Seated parts (hard edge / corner), per part.
    pub seat: Vec<Option<Seat>>,
    /// Allowed quarter turns per part; empty = any.
    pub turns: Vec<Vec<usize>>,
    pub decap_limits: Vec<DLimit>,
    /// Half the spacing plus the snap tolerance, mm.
    pub half: f64,
    /// Canvas every courtyard stays inside, mm.
    pub canvas: R,
    /// Where seeds scatter, mm.
    pub board: R,
    /// The canvas is the board: edge hints read its sides, not the bbox.
    pub fixed_outline: bool,
    pub copper_layers: u8,
    /// Half the clearance, mm (pad copper grown by it for congestion).
    pub half_clearance: f64,
    pub weights: Weights,
}

type Seg = (f64, f64, f64, f64);

/// Manhattan MST, Prim from point 0, lowest index among equal keys, a key
/// lowered only by a strictly shorter edge: upstream's `mst_l1`.
pub fn mst_l1(pts: &[(f64, f64)], edges: Option<&mut Vec<Seg>>) -> f64 {
    let n = pts.len();
    if n < 2 {
        if let Some(e) = edges {
            e.clear();
        }
        return 0.0;
    }
    let mut key = vec![f64::INFINITY; n];
    let mut parent = vec![usize::MAX; n];
    let mut used = vec![false; n];
    key[0] = 0.0;
    let mut total = 0.0;
    let mut out = Vec::new();
    for _ in 0..n {
        let mut u = usize::MAX;
        let mut best = f64::INFINITY;
        for v in 0..n {
            if !used[v] && key[v] < best {
                u = v;
                best = key[v];
            }
        }
        used[u] = true;
        if parent[u] != usize::MAX {
            total += best;
            let p = pts[parent[u]];
            out.push((p.0, p.1, pts[u].0, pts[u].1));
        }
        let (ux, uy) = pts[u];
        for v in 0..n {
            if !used[v] {
                let d = (pts[v].0 - ux).abs() + (pts[v].1 - uy).abs();
                if d < key[v] {
                    key[v] = d;
                    parent[v] = u;
                }
            }
        }
    }
    if let Some(e) = edges {
        *e = out;
    }
    total
}

/// Prim spanning tree over rects weighted by `rect_gap` (upstream `gap_tree`).
fn gap_tree(rects: &[R]) -> f64 {
    let n = rects.len();
    if n < 2 {
        return 0.0;
    }
    let mut key = vec![f64::INFINITY; n];
    let mut used = vec![false; n];
    key[0] = 0.0;
    let mut total = 0.0;
    for _ in 0..n {
        let mut u = usize::MAX;
        let mut best = f64::INFINITY;
        for v in 0..n {
            if !used[v] && key[v] < best {
                u = v;
                best = key[v];
            }
        }
        used[u] = true;
        total += best;
        for v in 0..n {
            if !used[v] {
                key[v] = key[v].min(rects[u].gap(&rects[v]));
            }
        }
    }
    total
}

fn orient(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> i32 {
    let v = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
    i32::from(v > 1e-12) - i32::from(v < -1e-12)
}

/// A proper crossing (upstream `_crosses`): segments sharing an end never cross.
fn crosses(s: &Seg, t: &Seg) -> bool {
    let a = orient(s.0, s.1, s.2, s.3, t.0, t.1);
    let b = orient(s.0, s.1, s.2, s.3, t.2, t.3);
    if a == 0 || b == 0 || a == b {
        return false;
    }
    let c = orient(t.0, t.1, t.2, t.3, s.0, s.1);
    let d = orient(t.0, t.1, t.2, t.3, s.2, s.3);
    c != 0 && d != 0 && c != d
}

fn crossings(a: &[Seg], b: &[Seg]) -> u32 {
    let mut n = 0;
    for s in a {
        let (sx0, sx1) = (s.0.min(s.2), s.0.max(s.2));
        let (sy0, sy1) = (s.1.min(s.3), s.1.max(s.3));
        for t in b {
            if t.0.max(t.2) < sx0 || t.0.min(t.2) > sx1 || t.1.max(t.3) < sy0 || t.1.min(t.3) > sy1 {
                continue;
            }
            if crosses(s, t) {
                n += 1;
            }
        }
    }
    n
}

/// Fan-out fronts of a part (upstream `pad_fronts`), for pads `connected`
/// (index, distinct non-ground net id) in one quarter turn's pad offsets.
pub fn fronts(pads: &[(f64, f64)], connected: &[(usize, usize)], coef: f64) -> Fronts {
    let mut f = Fronts::default();
    if connected.is_empty() {
        return f;
    }
    let pts: Vec<(f64, f64)> = connected.iter().map(|&(k, _)| pads[k]).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(x, y) in &pts {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (cx, cy, hx, hy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, ((x1 - x0) / 2.0).max(1e-9), ((y1 - y0) / 2.0).max(1e-9));
    let mut nets: [Vec<usize>; 4] = Default::default();
    let mut span = [(f64::INFINITY, f64::NEG_INFINITY); 4];
    for (i, &(x, y)) in pts.iter().enumerate() {
        let (u, v) = ((x - cx) / hx, (y - cy) / hy);
        if u.abs().max(v.abs()) < 0.5 {
            continue;
        }
        // W E N S; ties to the longer line (x when the pad row is wider).
        let side = if u.abs() > v.abs() || (u.abs() == v.abs() && hy > hx) {
            if u < 0.0 {
                0
            } else {
                1
            }
        } else if v < 0.0 {
            2
        } else {
            3
        };
        let along = if side < 2 { y } else { x };
        span[side].0 = span[side].0.min(along - FAN_MM);
        span[side].1 = span[side].1.max(along + FAN_MM);
        let net = connected[i].1;
        if net != usize::MAX && !nets[side].contains(&net) {
            nets[side].push(net);
        }
    }
    for s in 0..4 {
        f.depth[s] = coef * nets[s].len() as f64;
        f.span[s] = span[s];
    }
    f
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub q: usize,
}

pub struct Model<'a> {
    pub p: &'a Problem,
    pub pose: Vec<Pose>,
    nets_of: Vec<Vec<usize>>,
    clusters_of: Vec<Vec<usize>>,
    claims_of: Vec<Vec<usize>>,
    templates_of: Vec<Vec<usize>>,
    dlim_of: Vec<Vec<usize>>,
    group_of: Vec<Option<usize>>,
    /// Hints (kind 0 near / 1 apart / 2 edge, group a, group b or side).
    hints: Vec<(u8, usize, usize)>,
    hints_of_group: Vec<Vec<usize>>,
    sig_slot: Vec<usize>,
    signals: Vec<usize>,
    net_cost: Vec<f64>,
    net_len: Vec<f64>,
    edges: Vec<Vec<Seg>>,
    cross: Vec<u32>,
    glob: f64,
    grid: Option<Grid>,
    /// Committed congestion demand per net: (box, per-area density).
    net_dem: Vec<(R, f64)>,
    pub cong: f64,
    local_total: f64,
}

const SIDES: [EdgeSide; 4] = [EdgeSide::Left, EdgeSide::Right, EdgeSide::Top, EdgeSide::Bottom];

impl<'a> Model<'a> {
    pub fn new(p: &'a Problem, pose: Vec<Pose>) -> Model<'a> {
        let n = p.parts.len();
        let mut nets_of = vec![Vec::new(); n];
        for (k, net) in p.nets.iter().enumerate() {
            for &(i, _) in &net.pins {
                if !nets_of[i].contains(&k) {
                    nets_of[i].push(k);
                }
            }
        }
        let mut clusters_of = vec![Vec::new(); n];
        for (k, c) in p.clusters.iter().enumerate() {
            for i in c.caps.iter().map(|c| c.part).chain(c.sites.iter().map(|s| s.ic)) {
                if !clusters_of[i].contains(&k) {
                    clusters_of[i].push(k);
                }
            }
        }
        let mut claims_of = vec![Vec::new(); n];
        for (k, c) in p.claims.iter().enumerate() {
            claims_of[c.subject].push(k);
            if c.partner != c.subject {
                claims_of[c.partner].push(k);
            }
        }
        let mut templates_of = vec![Vec::new(); n];
        for (k, t) in p.templates.iter().enumerate() {
            templates_of[t.anchor].push(k);
            for m in &t.members {
                if !templates_of[m.0].contains(&k) {
                    templates_of[m.0].push(k);
                }
            }
        }
        let mut dlim_of = vec![Vec::new(); n];
        for (k, d) in p.decap_limits.iter().enumerate() {
            dlim_of[d.cap].push(k);
            if d.ic != d.cap {
                dlim_of[d.ic].push(k);
            }
        }
        let mut group_of = vec![None; n];
        for (g, grp) in p.groups.iter().enumerate() {
            for &i in &grp.members {
                group_of[i] = Some(g);
            }
        }
        let mut hints = Vec::new();
        for (g, grp) in p.groups.iter().enumerate() {
            for &o in &grp.near {
                let h = (0u8, g.min(o), g.max(o));
                if o != g && !hints.contains(&h) {
                    hints.push(h);
                }
            }
            for &o in &grp.apart {
                let h = (1u8, g.min(o), g.max(o));
                if o != g && !hints.contains(&h) {
                    hints.push(h);
                }
            }
            if let Some(e) = grp.edge {
                hints.push((2, g, SIDES.iter().position(|s| *s == e).unwrap_or(0)));
            }
        }
        let mut hints_of_group = vec![Vec::new(); p.groups.len()];
        for (h, &(kind, a, b)) in hints.iter().enumerate() {
            hints_of_group[a].push(h);
            if kind < 2 && b != a {
                hints_of_group[b].push(h);
            }
        }
        let signals: Vec<usize> = (0..p.nets.len()).filter(|&k| p.nets[k].kind == Kind::Signal).collect();
        let mut sig_slot = vec![usize::MAX; p.nets.len()];
        for (s, &k) in signals.iter().enumerate() {
            sig_slot[k] = s;
        }
        let ns = signals.len();
        let grid = (p.weights.congestion > 0.0).then(|| Grid::new(p.canvas, p.copper_layers));
        let mut m = Model {
            p,
            pose,
            nets_of,
            clusters_of,
            claims_of,
            templates_of,
            dlim_of,
            group_of,
            hints,
            hints_of_group,
            sig_slot,
            signals,
            net_cost: vec![0.0; p.nets.len()],
            net_len: vec![0.0; p.nets.len()],
            edges: vec![Vec::new(); p.nets.len()],
            cross: vec![0; ns * ns],
            glob: 0.0,
            grid,
            net_dem: vec![(R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 }, 0.0); p.nets.len()],
            cong: 0.0,
            local_total: 0.0,
        };
        m.reprice_all();
        m
    }

    pub fn movable(&self, i: usize) -> bool {
        !self.p.locked[i]
    }
    pub fn rect(&self, i: usize) -> R {
        let ps = self.pose[i];
        self.p.parts[i].crt[ps.q].at(ps.x, ps.y)
    }
    fn pin(&self, i: usize, k: usize) -> (f64, f64) {
        let ps = self.pose[i];
        let (ox, oy) = self.p.parts[i].pads[ps.q][k];
        (ps.x + ox, ps.y + oy)
    }
    fn pad_rect(&self, i: usize, k: usize) -> R {
        let ps = self.pose[i];
        self.p.parts[i].pad_rects[ps.q][k].at(ps.x, ps.y)
    }
    fn weight(&self, k: Kind) -> f64 {
        let w = &self.p.weights;
        match k {
            Kind::Signal => w.signal,
            Kind::Rail => w.rail,
            Kind::Ground => w.ground,
        }
    }
    /// (weighted cost, raw length).
    fn net(&self, k: usize, edges: Option<&mut Vec<Seg>>) -> (f64, f64) {
        let net = &self.p.nets[k];
        let pts: Vec<(f64, f64)> = net.pins.iter().map(|&(i, pad)| self.pin(i, pad)).collect();
        let l = mst_l1(&pts, edges);
        (l * self.weight(net.kind), l)
    }
    /// A net's congestion demand: its pins' box grown by half a pitch, and
    /// its tree length per unit area of that box.
    fn demand(&self, k: usize, len: f64) -> (R, f64) {
        let net = &self.p.nets[k];
        let mut b: Option<R> = None;
        for &(i, pad) in &net.pins {
            let (x, y) = self.pin(i, pad);
            let r = R { x0: x, y0: y, x1: x, y1: y };
            b = Some(b.map_or(r, |a| a.union(&r)));
        }
        let b = b.unwrap_or(R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 }).grow(TRACK_PITCH_MM / 2.0);
        let area = (b.w() * b.h()).max(1e-9);
        (b, len / area)
    }
    fn nearest(&self, a: usize, pa: &[usize], b: usize, pb: &[usize]) -> f64 {
        let mut best = f64::INFINITY;
        for &x in pa {
            let (ax, ay) = self.pin(a, x);
            for &y in pb {
                let (bx, by) = self.pin(b, y);
                best = best.min((bx - ax).hypot(by - ay));
            }
        }
        best
    }
    /// Distance from cap `c`'s legs to site `s`: rail leg + ground leg.
    fn cap_site(&self, c: &CapLeg, s: &Site) -> f64 {
        let mut d = self.nearest(c.part, &c.rail, s.ic, &s.pads);
        if !c.ground.is_empty() && !s.ground.is_empty() {
            d += self.nearest(c.part, &c.ground, s.ic, &s.ground);
        }
        d
    }
    /// Upstream `cluster_match`: (cost mm, site per cap).
    pub fn cluster_match(&self, k: usize) -> (f64, Vec<usize>) {
        let c = &self.p.clusters[k];
        let ns = c.sites.len();
        if ns == 0 || c.caps.is_empty() {
            return (0.0, Vec::new());
        }
        let d: Vec<Vec<f64>> = c.caps.iter().map(|cap| c.sites.iter().map(|s| self.cap_site(cap, s)).collect()).collect();
        if c.caps.len() == 1 {
            let (j, v) = d[0].iter().enumerate().fold((0, f64::INFINITY), |acc, (j, &v)| if v < acc.1 { (j, v) } else { acc });
            return (v, vec![j]);
        }
        let layers = c.caps.len().div_ceil(ns);
        let big: Vec<Vec<f64>> = d.iter().map(|row| (0..layers).flat_map(|t| row.iter().map(move |v| v + 1e6 * t as f64)).collect()).collect();
        let a = assign(&big);
        let sites: Vec<usize> = a.iter().map(|j| j % ns).collect();
        (sites.iter().enumerate().map(|(i, &j)| d[i][j]).sum(), sites)
    }
    fn claim_cost(&self, k: usize) -> f64 {
        let c = &self.p.claims[k];
        let w = &self.p.weights;
        let mut cost = 0.0;
        for &s in &c.subject_pads {
            let (sx, sy) = self.pin(c.subject, s);
            let d = c.partner_pads.iter().map(|&q| {
                let (px, py) = self.pin(c.partner, q);
                (px - sx).hypot(py - sy)
            });
            cost += w.claim * d.fold(f64::INFINITY, f64::min);
            if let Some(max) = c.max_mm {
                let sr = self.pad_rect(c.subject, s);
                let g = c.partner_pads.iter().map(|&q| self.pad_rect(c.partner, q).gap(&sr)).fold(f64::INFINITY, f64::min);
                cost += w.claim_limit * (g - max).max(0.0);
            }
        }
        cost
    }
    fn dlimit_cost(&self, k: usize) -> f64 {
        let d = &self.p.decap_limits[k];
        let mut l = self.nearest(d.cap, &d.rail.0, d.ic, &d.rail.1);
        if let Some((a, b)) = &d.ground {
            l += self.nearest(d.cap, a, d.ic, b);
        }
        self.p.weights.decap_limit * (l - d.max_mm).max(0.0)
    }
    /// The frame seated parts sit on: the board when it is given; else the
    /// bounding box of the unseated parts grown, side by side, by the
    /// deepest part seated on that side (and a spacing), so every seated
    /// part sits flush with the outer edge of the board it makes. Upstream
    /// (`py_placer/placement/place_simple.py`, edge parts) has a fixed board;
    /// with a free outline this is the board it converges to.
    pub fn core(&self) -> R {
        if self.p.fixed_outline {
            return self.p.canvas;
        }
        let mut b: Option<R> = None;
        for i in 0..self.pose.len() {
            if self.p.seat[i].is_none() {
                let r = self.rect(i);
                b = Some(b.map_or(r, |a| a.union(&r)));
            }
        }
        let Some(mut f) = b else { return self.bbox() };
        let gap = 2.0 * self.p.half;
        let (mut l, mut rt, mut t, mut bt) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for i in 0..self.pose.len() {
            let Some(seat) = self.p.seat[i] else { continue };
            let r = self.p.parts[i].crt[self.pose[i].q];
            let mut deepen = |side: EdgeSide| match side {
                EdgeSide::Left => l = l.max(r.w() + gap),
                EdgeSide::Right => rt = rt.max(r.w() + gap),
                EdgeSide::Top => t = t.max(r.h() + gap),
                EdgeSide::Bottom => bt = bt.max(r.h() + gap),
                EdgeSide::Any => {}
            };
            match seat {
                Seat::Edge(e) => deepen(e),
                Seat::Corner(a, c) => {
                    deepen(a);
                    deepen(c);
                }
            }
        }
        f.x0 -= l;
        f.x1 += rt;
        f.y0 -= t;
        f.y1 += bt;
        f
    }
    /// Pose `ps` of seated part `i` projected onto its line on `frame`:
    /// flush with that side, and kept within the frame along it (the
    /// sliding line).
    pub fn project(&self, i: usize, ps: Pose, frame: &R) -> Pose {
        let Some(seat) = self.p.seat[i] else { return ps };
        let r = self.p.parts[i].crt[ps.q];
        let mut out = ps;
        out.x = out.x.max(frame.x0 - r.x0).min((frame.x1 - r.x1).max(frame.x0 - r.x0));
        out.y = out.y.max(frame.y0 - r.y0).min((frame.y1 - r.y1).max(frame.y0 - r.y0));
        let mut put = |side: EdgeSide| match side {
            EdgeSide::Left => out.x = frame.x0 - r.x0,
            EdgeSide::Right => out.x = frame.x1 - r.x1,
            EdgeSide::Top => out.y = frame.y0 - r.y0,
            EdgeSide::Bottom => out.y = frame.y1 - r.y1,
            EdgeSide::Any => {}
        };
        match seat {
            Seat::Edge(e) => put(e),
            Seat::Corner(a, b) => {
                put(a);
                put(b);
            }
        }
        self.clamp(i, out)
    }
    fn group_cost(&self, g: usize) -> f64 {
        let rects: Vec<R> = self.p.groups[g].members.iter().map(|&i| self.rect(i)).collect();
        self.p.weights.group * gap_tree(&rects)
    }
    fn min_gap(&self, a: usize, b: usize) -> f64 {
        let mut best = f64::INFINITY;
        for &i in &self.p.groups[a].members {
            let ri = self.rect(i);
            for &j in &self.p.groups[b].members {
                best = best.min(ri.gap(&self.rect(j)));
            }
        }
        best
    }
    fn frame(&self) -> R {
        if self.p.fixed_outline {
            self.p.canvas
        } else {
            self.bbox()
        }
    }
    fn side_gap(r: &R, b: &R, side: EdgeSide) -> f64 {
        let (l, rt, t, bt) = (r.x0 - b.x0, b.x1 - r.x1, r.y0 - b.y0, b.y1 - r.y1);
        match side {
            EdgeSide::Any => l.min(rt).min(t).min(bt),
            EdgeSide::Left => l,
            EdgeSide::Right => rt,
            EdgeSide::Top => t,
            EdgeSide::Bottom => bt,
        }
        .max(0.0)
    }
    fn hint_cost(&self, h: usize, frame: &R) -> f64 {
        let (kind, a, b) = self.hints[h];
        let w = &self.p.weights;
        match kind {
            0 => w.near * self.min_gap(a, b),
            1 => w.apart * (APART_MM - self.min_gap(a, b)).max(0.0),
            _ => {
                let side = SIDES[b];
                w.edge * self.p.groups[a].members.iter().map(|&i| Self::side_gap(&self.rect(i), frame, side)).fold(f64::INFINITY, f64::min)
            }
        }
    }
    /// Template member drift: mean distance of its pads from the template.
    fn template_cost(&self, k: usize) -> f64 {
        let t = &self.p.templates[k];
        let a = self.pose[t.anchor];
        let mut cost = 0.0;
        for &(m, dx, dy, mq) in &t.members {
            let (ox, oy) = rot(dx, dy, a.q);
            let tq = (mq + a.q) % 4;
            let (tx, ty) = (a.x + ox, a.y + oy);
            let ps = self.pose[m];
            let g = &self.p.parts[m];
            let n = g.pads[0].len();
            let d = if n == 0 {
                (ps.x - tx).hypot(ps.y - ty)
            } else {
                (0..n)
                    .map(|k| {
                        let (cx, cy) = g.pads[ps.q][k];
                        let (gx, gy) = g.pads[tq][k];
                        (ps.x + cx - tx - gx).hypot(ps.y + cy - ty - gy)
                    })
                    .sum::<f64>()
                    / n as f64
            };
            cost += self.p.weights.template * d;
        }
        cost
    }
    fn halo_pair(&self, a: usize, b: usize) -> f64 {
        let (fa, fb) = (self.p.fine[a].as_ref(), self.p.fine[b].as_ref());
        if fa.is_none() && fb.is_none() {
            return 0.0;
        }
        if self.p.halo_exempt.contains(&(a.min(b), a.max(b))) || (self.p.locked[a] && self.p.locked[b]) {
            return 0.0;
        }
        let (ra, rb) = (self.rect(a), self.rect(b));
        let (pa, pb) = (self.pose[a], self.pose[b]);
        let mut short = f64::INFINITY;
        for axis in 0..2 {
            // Which of a's sides faces b on this axis (W/E or N/S).
            let (ca, cb) = if axis == 0 { (ra.cx(), rb.cx()) } else { (ra.cy(), rb.cy()) };
            let (sa, sb) = if axis == 0 { if cb >= ca { (1, 0) } else { (0, 1) } } else if cb >= ca { (3, 2) } else { (2, 3) };
            let mut need = 0.0;
            let reach = |f: &Fronts, s: usize, p: Pose, other: &R| -> f64 {
                let (lo, hi) = f.span[s];
                if f.depth[s] <= 0.0 {
                    return 0.0;
                }
                let (olo, ohi) = if s < 2 { (other.y0, other.y1) } else { (other.x0, other.x1) };
                let off = if s < 2 { p.y } else { p.x };
                if lo + off < ohi && olo < hi + off {
                    f.depth[s]
                } else {
                    0.0
                }
            };
            if let Some(f) = fa {
                need += reach(&f[pa.q], sa, pa, &rb);
            }
            if let Some(f) = fb {
                need += reach(&f[pb.q], sb, pb, &ra);
            }
            // Signed gap on this axis: negative when the extents overlap.
            let gap = if axis == 0 { ra.x0.max(rb.x0) - ra.x1.min(rb.x1) } else { ra.y0.max(rb.y0) - ra.y1.min(rb.y1) };
            short = short.min(need - gap);
        }
        if short <= 0.0 {
            0.0
        } else {
            self.p.weights.halo * short * short
        }
    }
    pub fn bbox(&self) -> R {
        if self.pose.is_empty() {
            return R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
        }
        (1..self.pose.len()).fold(self.rect(0), |acc, i| acc.union(&self.rect(i)))
    }
    /// Area, connector edge hints and group edge hints: global, since they
    /// read the bounding box.
    fn global(&self) -> f64 {
        if self.pose.is_empty() {
            return 0.0;
        }
        let b = self.frame();
        let mut c = if self.p.fixed_outline { 0.0 } else { self.p.weights.area * b.w() * b.h() };
        for &(i, side, w) in &self.p.edges {
            c += w * Self::side_gap(&self.rect(i), &b, side);
        }
        for h in 0..self.hints.len() {
            if self.hints[h].0 == 2 {
                c += self.hint_cost(h, &b);
            }
        }
        c
    }

    /// The local terms touching `parts`: decap clusters, claims, groups and
    /// their near/apart hints, halo pairs and templates.
    fn local(&self, parts: &[usize]) -> f64 {
        let mut ks: Vec<usize> = parts.iter().flat_map(|&i| self.clusters_of[i].iter().copied()).collect();
        ks.sort_unstable();
        ks.dedup();
        let w = &self.p.weights;
        let mut c: f64 = ks.iter().map(|&k| w.decap * self.cluster_match(k).0).sum();
        let mut dl: Vec<usize> = parts.iter().flat_map(|&i| self.dlim_of[i].iter().copied()).collect();
        dl.sort_unstable();
        dl.dedup();
        c += dl.iter().map(|&k| self.dlimit_cost(k)).sum::<f64>();
        let mut cl: Vec<usize> = parts.iter().flat_map(|&i| self.claims_of[i].iter().copied()).collect();
        cl.sort_unstable();
        cl.dedup();
        c += cl.iter().map(|&k| self.claim_cost(k)).sum::<f64>();
        let mut gs: Vec<usize> = parts.iter().filter_map(|&i| self.group_of[i]).collect();
        gs.sort_unstable();
        gs.dedup();
        c += gs.iter().map(|&g| self.group_cost(g)).sum::<f64>();
        let mut hs: Vec<usize> = gs.iter().flat_map(|&g| self.hints_of_group[g].iter().copied()).filter(|&h| self.hints[h].0 < 2).collect();
        hs.sort_unstable();
        hs.dedup();
        let dummy = R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
        c += hs.iter().map(|&h| self.hint_cost(h, &dummy)).sum::<f64>();
        let mut ts: Vec<usize> = parts.iter().flat_map(|&i| self.templates_of[i].iter().copied()).collect();
        ts.sort_unstable();
        ts.dedup();
        c += ts.iter().map(|&k| self.template_cost(k)).sum::<f64>();
        if self.p.fine.iter().any(Option::is_some) {
            let n = self.pose.len();
            for (ai, &a) in parts.iter().enumerate() {
                for b in 0..n {
                    if b == a || parts[..ai].contains(&b) {
                        continue;
                    }
                    c += self.halo_pair(a, b);
                }
            }
        }
        c
    }
    fn local_all(&self) -> f64 {
        let w = &self.p.weights;
        let mut c: f64 = (0..self.p.clusters.len()).map(|k| w.decap * self.cluster_match(k).0).sum();
        c += (0..self.p.claims.len()).map(|k| self.claim_cost(k)).sum::<f64>();
        c += (0..self.p.decap_limits.len()).map(|k| self.dlimit_cost(k)).sum::<f64>();
        c += (0..self.p.groups.len()).map(|g| self.group_cost(g)).sum::<f64>();
        let dummy = R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
        c += (0..self.hints.len()).filter(|&h| self.hints[h].0 < 2).map(|h| self.hint_cost(h, &dummy)).sum::<f64>();
        c += (0..self.p.templates.len()).map(|k| self.template_cost(k)).sum::<f64>();
        let n = self.pose.len();
        if self.p.fine.iter().any(Option::is_some) {
            for a in 0..n {
                for b in a + 1..n {
                    c += self.halo_pair(a, b);
                }
            }
        }
        c
    }

    fn pads_into_grid(&mut self, i: usize, sign: f64) -> f64 {
        let Some(grid) = self.grid.as_mut() else { return 0.0 };
        let ps = self.pose[i];
        let g = &self.p.parts[i];
        let mut d = 0.0;
        for (k, r) in g.pad_rects[ps.q].iter().enumerate() {
            let layers = if g.drilled[k] { f64::from(self.p.copper_layers.max(1)) } else { 1.0 };
            d += grid.add(&r.at(ps.x, ps.y).grow(self.p.half_clearance), sign * layers, false);
        }
        d
    }

    pub fn reprice_all(&mut self) {
        for k in 0..self.p.nets.len() {
            let mut e = Vec::new();
            let (c, l) = self.net(k, Some(&mut e));
            self.net_cost[k] = c;
            self.net_len[k] = l;
            self.edges[k] = e;
        }
        let ns = self.signals.len();
        for a in 0..ns {
            for b in a + 1..ns {
                self.cross[a * ns + b] = crossings(&self.edges[self.signals[a]], &self.edges[self.signals[b]]);
            }
        }
        self.glob = self.global();
        self.local_total = self.local_all();
        if self.grid.is_some() {
            if let Some(g) = self.grid.as_mut() {
                g.reset();
            }
            for i in 0..self.pose.len() {
                self.pads_into_grid(i, 1.0);
            }
            for k in 0..self.p.nets.len() {
                if self.p.nets[k].congest {
                    let d = self.demand(k, self.net_len[k]);
                    self.net_dem[k] = d;
                    if let Some(g) = self.grid.as_mut() {
                        g.add(&d.0, d.1, true);
                    }
                }
            }
            self.cong = self.grid.as_ref().map_or(0.0, Grid::total);
        }
    }

    /// The cost split by term, for the report: what the anneal paid for.
    pub fn breakdown(&self) -> Vec<(String, f64)> {
        let w = &self.p.weights;
        let dummy = R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
        let b = self.frame();
        let mut t = vec![
            ("wire".to_string(), self.net_cost.iter().sum::<f64>()),
            ("crossings".into(), w.crossing * self.cross.iter().map(|&c| c as f64).sum::<f64>()),
            ("congestion".into(), w.congestion * self.cong),
            ("area".into(), if self.p.fixed_outline { 0.0 } else { w.area * b.w() * b.h() }),
            ("edges".into(), self.p.edges.iter().map(|&(i, side, wt)| wt * Self::side_gap(&self.rect(i), &b, side)).sum()),
            ("edge_hints".into(), (0..self.hints.len()).filter(|&h| self.hints[h].0 == 2).map(|h| self.hint_cost(h, &b)).sum()),
            ("decaps".into(), (0..self.p.clusters.len()).map(|k| w.decap * self.cluster_match(k).0).sum()),
            ("claims".into(), (0..self.p.claims.len()).map(|k| self.claim_cost(k)).sum()),
            ("decap_limit".into(), (0..self.p.decap_limits.len()).map(|k| self.dlimit_cost(k)).sum()),
            ("groups".into(), (0..self.p.groups.len()).map(|g| self.group_cost(g)).sum()),
            ("near_apart".into(), (0..self.hints.len()).filter(|&h| self.hints[h].0 < 2).map(|h| self.hint_cost(h, &dummy)).sum()),
            ("templates".into(), (0..self.p.templates.len()).map(|k| self.template_cost(k)).sum()),
        ];
        let n = self.pose.len();
        let mut halo = 0.0;
        if self.p.fine.iter().any(Option::is_some) {
            for a in 0..n {
                for c in a + 1..n {
                    halo += self.halo_pair(a, c);
                }
            }
        }
        t.push(("halo".into(), halo));
        t
    }

    pub fn total(&self) -> f64 {
        self.net_cost.iter().sum::<f64>()
            + self.p.weights.crossing * self.cross.iter().map(|&c| c as f64).sum::<f64>()
            + self.glob
            + self.local_total
            + self.p.weights.congestion * self.cong
    }

    fn grown(&self, i: usize) -> R {
        self.rect(i).grow(self.p.half)
    }

    /// Overlap (mm^2) of every pair with a part in `parts`, each pair once.
    pub fn overlap(&self, parts: &[usize]) -> f64 {
        let n = self.pose.len();
        let mut t = 0.0;
        for (ai, &a) in parts.iter().enumerate() {
            let ra = self.grown(a);
            for b in 0..n {
                if b == a || parts[..ai].contains(&b) {
                    continue;
                }
                t += ra.overlap(&self.grown(b));
            }
        }
        t
    }
    pub fn overlap_all(&self) -> f64 {
        let n = self.pose.len();
        let g: Vec<R> = (0..n).map(|i| self.grown(i)).collect();
        let mut t = 0.0;
        for a in 0..n {
            for b in a + 1..n {
                t += g[a].overlap(&g[b]);
            }
        }
        t
    }

    fn touched_nets(&self, parts: &[usize]) -> Vec<usize> {
        let mut nets: Vec<usize> = parts.iter().flat_map(|&i| self.nets_of[i].iter().copied()).collect();
        nets.sort_unstable();
        nets.dedup();
        nets
    }

    /// Price the wire and crossing terms for the poses now set on `parts`
    /// against the committed ones, and the global terms.
    fn price(&self, parts: &[usize]) -> (f64, Pending) {
        let nets = self.touched_nets(parts);
        let mut delta = 0.0;
        let mut new_net = Vec::with_capacity(nets.len());
        for &k in &nets {
            let mut e = Vec::new();
            let (c, l) = self.net(k, if self.p.nets[k].kind == Kind::Signal { Some(&mut e) } else { None });
            delta += c - self.net_cost[k];
            new_net.push((k, c, l, e));
        }
        let ns = self.signals.len();
        let mut new_cross = Vec::new();
        let changed: Vec<usize> = new_net.iter().enumerate().filter(|(_, x)| self.p.nets[x.0].kind == Kind::Signal).map(|(j, _)| j).collect();
        for (ci, &j) in changed.iter().enumerate() {
            let (ka, _, _, ea) = &new_net[j];
            let sa = self.sig_slot[*ka];
            for sb in 0..ns {
                if sb == sa {
                    continue;
                }
                let kb = self.signals[sb];
                let other = changed.iter().position(|&jj| new_net[jj].0 == kb);
                if let Some(oi) = other {
                    if oi < ci {
                        continue;
                    }
                }
                let eb = match other {
                    Some(oi) => &new_net[changed[oi]].3,
                    None => &self.edges[kb],
                };
                let n = crossings(ea, eb);
                let slot = if sa < sb { sa * ns + sb } else { sb * ns + sa };
                delta += self.p.weights.crossing * (n as f64 - self.cross[slot] as f64);
                new_cross.push((slot, n));
            }
        }
        let g = self.global();
        delta += g - self.glob;
        (delta, Pending { new_net, new_cross, glob: g })
    }

    fn commit(&mut self, p: Pending) {
        for (k, c, l, e) in p.new_net {
            self.net_cost[k] = c;
            self.net_len[k] = l;
            if self.p.nets[k].kind == Kind::Signal {
                self.edges[k] = e;
            }
        }
        for (s, n) in p.new_cross {
            self.cross[s] = n;
        }
        self.glob = p.glob;
    }

    /// Keep a pose's courtyard inside the canvas.
    pub fn clamp(&self, i: usize, mut ps: Pose) -> Pose {
        let c = &self.p.canvas;
        let r = self.p.parts[i].crt[ps.q];
        ps.x = ps.x.max(c.x0 - r.x0).min((c.x1 - r.x1).max(c.x0 - r.x0));
        ps.y = ps.y.max(c.y0 - r.y0).min((c.y1 - r.y1).max(c.y0 - r.y0));
        ps
    }
}

struct Pending {
    new_net: Vec<(usize, f64, f64, Vec<Seg>)>,
    new_cross: Vec<(usize, u32)>,
    glob: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Shift,
    Turn,
    Swap,
    Jump,
    Dock,
    GShift,
    GTurn,
    GSwap,
    Escape,
}

/// Upstream `simple_anneal.py` `MOVES`.
const MOVES: [(Move, f64); 9] = [
    (Move::Shift, 0.40),
    (Move::Turn, 0.15),
    (Move::Swap, 0.10),
    (Move::Jump, 0.10),
    (Move::Dock, 0.10),
    (Move::GShift, 0.12),
    (Move::GTurn, 0.08),
    (Move::GSwap, 0.05),
    (Move::Escape, 0.05),
];

pub struct Outcome {
    /// The best overlap-free poses seen; `None` when none was.
    pub best: Option<Vec<Pose>>,
    pub best_cost: f64,
    pub moves: u64,
}

/// Satellite `s`'s pose when its anchor goes from `old` to `new`
/// (upstream `simple.follow`): same offset in the anchor's frame, turned
/// with it.
pub fn follow(old: Pose, new: Pose, s: Pose) -> Pose {
    let back = (4 - old.q % 4) % 4;
    let (rx, ry) = rot(s.x - old.x, s.y - old.y, back);
    let (nx, ny) = rot(rx, ry, new.q);
    Pose { x: new.x + nx, y: new.y + ny, q: (s.q + 4 + new.q - old.q % 4) % 4 }
}

pub struct Annealer<'a, 'b> {
    pub m: &'b mut Model<'a>,
    rng: Rng,
    mov: Vec<usize>,
    alike: Vec<Vec<usize>>,
    /// Satellites per part: decaps of its clusters, claimed parts, template
    /// members (always), the last two flagged.
    sats: Vec<Vec<(usize, bool)>>,
    free_groups: Vec<usize>,
    span: f64,
    lam: f64,
}

impl<'a, 'b> Annealer<'a, 'b> {
    pub fn new(m: &'b mut Model<'a>, seed: u64) -> Self {
        let p = m.p;
        let n = p.parts.len();
        let mov: Vec<usize> = (0..n).filter(|&i| !p.locked[i]).collect();
        let edge_part = |i: usize| p.edges.iter().any(|e| e.0 == i) || p.seat[i].is_some();
        let mut alike: Vec<Vec<usize>> = Vec::new();
        for &i in &mov {
            if edge_part(i) {
                continue;
            }
            let r = p.parts[i].crt[0];
            let key = ((r.w() * 1000.0).round() as i64, (r.h() * 1000.0).round() as i64, p.parts[i].numbers.len());
            match alike.iter_mut().find(|g| {
                let s = p.parts[g[0]].crt[0];
                ((s.w() * 1000.0).round() as i64, (s.h() * 1000.0).round() as i64, p.parts[g[0]].numbers.len()) == key
            }) {
                Some(g) => g.push(i),
                None => alike.push(vec![i]),
            }
        }
        alike.retain(|g| g.len() > 1);
        let mut sats: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
        let add = |a: usize, s: usize, always: bool, sats: &mut Vec<Vec<(usize, bool)>>| {
            if a != s && !p.locked[s] && !sats[a].iter().any(|x| x.0 == s) {
                sats[a].push((s, always));
            }
        };
        for c in &p.clusters {
            for site in &c.sites {
                for cap in &c.caps {
                    add(site.ic, cap.part, false, &mut sats);
                }
            }
        }
        for c in &p.claims {
            add(c.partner, c.subject, false, &mut sats);
        }
        for t in &p.templates {
            for mm in &t.members {
                add(t.anchor, mm.0, true, &mut sats);
            }
        }
        let free_groups: Vec<usize> = (0..p.groups.len()).filter(|&g| !p.groups[g].members.is_empty() && p.groups[g].members.iter().all(|&i| !p.locked[i] && !edge_part(i) && p.turns[i].is_empty())).collect();
        let b = p.board;
        let span = b.w().max(b.h()).max(1.0);
        Annealer { m, rng: Rng::new(seed), mov, alike, sats, free_groups, span, lam: LAM_START }
    }

    /// Upstream `scatter`: every movable part at a random place on the
    /// board; `clustered` gathers each group round one random point in a
    /// square of its members' area.
    pub fn scatter(&mut self, clustered: bool) {
        let b = self.m.p.board;
        for &i in &self.mov.clone() {
            let ps = self.m.pose[i];
            let r = self.m.p.parts[i].crt[ps.q];
            let x = self.rng.uniform(b.x0 - r.x0, (b.x1 - r.x1).max(b.x0 - r.x0));
            let y = self.rng.uniform(b.y0 - r.y0, (b.y1 - r.y1).max(b.y0 - r.y0));
            self.m.pose[i] = self.m.clamp(i, Pose { x, y, q: ps.q });
        }
        if clustered {
            for g in 0..self.m.p.groups.len() {
                let members: Vec<usize> = self.m.p.groups[g].members.iter().copied().filter(|&i| !self.m.p.locked[i]).collect();
                if members.len() < 2 {
                    continue;
                }
                let area: f64 = members.iter().map(|&i| self.m.rect(i)).map(|r| r.w() * r.h()).sum();
                let half = (area.sqrt() / 2.0).min(b.w() / 2.0).min(b.h() / 2.0);
                let gx = self.rng.uniform(b.x0 + half, (b.x1 - half).max(b.x0 + half));
                let gy = self.rng.uniform(b.y0 + half, (b.y1 - half).max(b.y0 + half));
                for &i in &members {
                    let ps = self.m.pose[i];
                    let r = self.m.p.parts[i].crt[ps.q];
                    let x = gx + self.rng.uniform(-half, half) - r.cx();
                    let y = gy + self.rng.uniform(-half, half) - r.cy();
                    self.m.pose[i] = self.m.clamp(i, Pose { x, y, q: ps.q });
                }
            }
        }
        // Seated parts start on their allowed turn, on their line.
        for i in 0..self.m.pose.len() {
            if !self.m.p.locked[i] && !self.m.p.turns[i].is_empty() && !self.m.p.turns[i].contains(&self.m.pose[i].q) {
                self.m.pose[i].q = self.m.p.turns[i][0];
            }
        }
        // Template members start at their template pose.
        for t in &self.m.p.templates {
            let a = self.m.pose[t.anchor];
            for &(mi, dx, dy, mq) in &t.members {
                if self.m.p.locked[mi] {
                    continue;
                }
                let (ox, oy) = rot(dx, dy, a.q);
                self.m.pose[mi] = Pose { x: a.x + ox, y: a.y + oy, q: (mq + a.q) % 4 };
            }
        }
        let frame = self.m.core();
        for i in 0..self.m.pose.len() {
            if !self.m.p.locked[i] {
                self.m.pose[i] = self.m.project(i, self.m.pose[i], &frame);
            }
        }
        self.m.reprice_all();
    }

    /// The move with every seated part put back on its line: the moved ones
    /// projected, and any other whose line moved because the frame did.
    fn complete(&mut self, mut new: Vec<(usize, Pose)>) -> Vec<(usize, Pose)> {
        if self.m.p.seat.iter().all(Option::is_none) {
            return new;
        }
        let was: Vec<(usize, Pose)> = new.iter().map(|&(i, _)| (i, self.m.pose[i])).collect();
        for &(i, ps) in &new {
            self.m.pose[i] = ps;
        }
        let frame = self.m.core();
        for e in new.iter_mut() {
            e.1 = self.m.project(e.0, e.1, &frame);
        }
        for i in 0..self.m.pose.len() {
            if self.m.p.seat[i].is_some() && !new.iter().any(|x| x.0 == i) {
                let cur = self.m.pose[i];
                let pr = self.m.project(i, cur, &frame);
                if pr != cur {
                    new.push((i, pr));
                }
            }
        }
        for (i, ps) in was {
            self.m.pose[i] = ps;
        }
        new
    }

    /// A quarter turn part `i` may take, other than its current one.
    fn other_turn(&mut self, i: usize) -> Option<usize> {
        let cur = self.m.pose[i].q;
        let allowed: Vec<usize> = if self.m.p.turns[i].is_empty() { (0..4).collect() } else { self.m.p.turns[i].clone() };
        let opts: Vec<usize> = allowed.into_iter().filter(|&q| q != cur).collect();
        if opts.is_empty() {
            None
        } else {
            Some(opts[self.rng.below(opts.len())])
        }
    }
    fn any_turn(&mut self, i: usize) -> usize {
        let t = &self.m.p.turns[i];
        if t.is_empty() {
            self.rng.below(4)
        } else {
            t[self.rng.below(t.len())]
        }
    }

    fn carry(&self, i: usize, new: Vec<(usize, Pose)>) -> Vec<(usize, Pose)> {
        let mut new = new;
        let old = self.m.pose[i];
        let Some(&(_, np)) = new.iter().find(|(j, _)| *j == i) else { return new };
        let r = self.m.rect(i);
        let reach = TETHER_MM + r.w().max(r.h());
        for &(s, always) in &self.sats[i] {
            if new.iter().any(|(j, _)| *j == s) {
                continue;
            }
            let sp = self.m.pose[s];
            if !always && (sp.x - old.x).hypot(sp.y - old.y) > reach {
                continue;
            }
            new.push((s, self.m.clamp(s, follow(old, np, sp))));
        }
        new
    }

    /// The wiring target of part `i`'s origin: a template member's template
    /// pose, a decap's nearest site, or the mean of the non-ground pins its
    /// nets reach on other parts (upstream `target`), less its centre.
    fn target(&self, i: usize) -> Option<Pose> {
        let m = &*self.m;
        let ps = m.pose[i];
        for t in &m.p.templates {
            if let Some(&(_, dx, dy, mq)) = t.members.iter().find(|x| x.0 == i) {
                let a = m.pose[t.anchor];
                let (ox, oy) = rot(dx, dy, a.q);
                return Some(Pose { x: a.x + ox, y: a.y + oy, q: (mq + a.q) % 4 });
            }
        }
        let r = m.p.parts[i].crt[ps.q];
        for c in &m.p.clusters {
            if let Some(cap) = c.caps.iter().find(|c| c.part == i) {
                let site = c.sites.iter().min_by(|a, b| m.cap_site(cap, a).total_cmp(&m.cap_site(cap, b)))?;
                let (px, py) = m.pin(site.ic, *site.pads.first()?);
                return Some(Pose { x: px - r.cx(), y: py - r.cy(), q: ps.q });
            }
        }
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0usize);
        for &k in &m.nets_of[i] {
            let net = &m.p.nets[k];
            if net.kind == Kind::Ground {
                continue;
            }
            for &(j, pad) in &net.pins {
                if j != i {
                    let (px, py) = m.pin(j, pad);
                    sx += px;
                    sy += py;
                    n += 1;
                }
            }
        }
        (n > 0).then(|| Pose { x: sx / n as f64 - r.cx(), y: sy / n as f64 - r.cy(), q: ps.q })
    }

    fn group_centre(&self, g: usize) -> (f64, f64) {
        let ms = &self.m.p.groups[g].members;
        let (mut x, mut y) = (0.0, 0.0);
        for &i in ms {
            let r = self.m.rect(i);
            x += r.cx();
            y += r.cy();
        }
        (x / ms.len() as f64, y / ms.len() as f64)
    }

    fn pick(&mut self) -> usize {
        self.mov[self.rng.below(self.mov.len())]
    }

    fn propose(&mut self, kind: Move, reach: f64) -> Option<Vec<(usize, Pose)>> {
        match kind {
            Move::Shift => {
                let i = self.pick();
                let ps = self.m.pose[i];
                let np = self.m.clamp(i, Pose { x: ps.x + self.rng.uniform(-reach, reach), y: ps.y + self.rng.uniform(-reach, reach), q: ps.q });
                Some(self.carry(i, vec![(i, np)]))
            }
            Move::Turn => {
                let cands: Vec<usize> = self.mov.iter().copied().filter(|&i| self.m.p.parts[i].can_turn && self.m.p.turns[i].len() != 1).collect();
                if cands.is_empty() {
                    return None;
                }
                let i = cands[self.rng.below(cands.len())];
                let ps = self.m.pose[i];
                let nq = self.other_turn(i)?;
                let (a, b) = (self.m.p.parts[i].crt[ps.q], self.m.p.parts[i].crt[nq]);
                let np = self.m.clamp(i, Pose { x: ps.x + a.cx() - b.cx(), y: ps.y + a.cy() - b.cy(), q: nq });
                Some(self.carry(i, vec![(i, np)]))
            }
            Move::Swap => {
                if self.alike.is_empty() {
                    return None;
                }
                let g = self.alike[self.rng.below(self.alike.len())].clone();
                let a = g[self.rng.below(g.len())];
                let mut b = g[self.rng.below(g.len())];
                if a == b {
                    b = g[(g.iter().position(|&x| x == a).unwrap_or(0) + 1) % g.len()];
                }
                let (pa, pb) = (self.m.pose[a], self.m.pose[b]);
                Some(vec![(a, self.m.clamp(a, Pose { x: pb.x, y: pb.y, q: pa.q })), (b, self.m.clamp(b, Pose { x: pa.x, y: pa.y, q: pb.q }))])
            }
            Move::Jump => {
                let i = self.pick();
                let mut t = self.target(i)?;
                if !self.m.p.turns[i].is_empty() && !self.m.p.turns[i].contains(&t.q) {
                    t.q = self.m.pose[i].q;
                }
                let np = self.m.clamp(i, Pose { x: t.x + self.rng.uniform(-0.5, 0.5), y: t.y + self.rng.uniform(-0.5, 0.5), q: t.q });
                Some(self.carry(i, vec![(i, np)]))
            }
            Move::Dock => {
                // Against the nearest partner pad on a random signal net.
                let i = self.pick();
                let nets: Vec<usize> = self.m.nets_of[i].iter().copied().filter(|&k| self.m.p.nets[k].kind == Kind::Signal).collect();
                if nets.is_empty() {
                    return None;
                }
                let k = nets[self.rng.below(nets.len())];
                let mine: Vec<(f64, f64)> = self.m.p.nets[k].pins.iter().filter(|x| x.0 == i).map(|&(_, pad)| self.m.pin(i, pad)).collect();
                let (mx, my) = *mine.first()?;
                let (j, jpad) = self.m.p.nets[k].pins.iter().filter(|x| x.0 != i).min_by(|a, b| {
                    let (ax, ay) = self.m.pin(a.0, a.1);
                    let (bx, by) = self.m.pin(b.0, b.1);
                    ((ax - mx).abs() + (ay - my).abs()).total_cmp(&((bx - mx).abs() + (by - my).abs()))
                })?;
                let (px, py) = self.m.pin(*j, *jpad);
                let rj = self.m.rect(*j);
                let q = self.any_turn(i);
                let side = self.rng.below(4);
                let r = self.m.p.parts[i].crt[q];
                let g = 2.0 * self.m.p.half + 1e-3;
                let (cx, cy) = match side {
                    0 => (rj.x0 - g - r.w() / 2.0, py),
                    1 => (rj.x1 + g + r.w() / 2.0, py),
                    2 => (px, rj.y0 - g - r.h() / 2.0),
                    _ => (px, rj.y1 + g + r.h() / 2.0),
                };
                let np = self.m.clamp(i, Pose { x: cx - r.cx(), y: cy - r.cy(), q });
                Some(self.carry(i, vec![(i, np)]))
            }
            Move::GShift => {
                let gs: Vec<usize> = (0..self.m.p.groups.len()).filter(|&g| self.m.p.groups[g].members.iter().filter(|&&i| !self.m.p.locked[i]).count() > 1).collect();
                if gs.is_empty() {
                    return None;
                }
                let g = gs[self.rng.below(gs.len())];
                let (dx, dy) = (self.rng.uniform(-reach, reach), self.rng.uniform(-reach, reach));
                Some(self.m.p.groups[g].members.iter().filter(|&&i| !self.m.p.locked[i]).map(|&i| {
                    let p = self.m.pose[i];
                    (i, self.m.clamp(i, Pose { x: p.x + dx, y: p.y + dy, q: p.q }))
                }).collect())
            }
            Move::GTurn => {
                if self.free_groups.is_empty() {
                    return None;
                }
                let g = self.free_groups[self.rng.below(self.free_groups.len())];
                let k = 1 + self.rng.below(3);
                let (cx, cy) = self.group_centre(g);
                let pivot_old = Pose { x: cx, y: cy, q: 0 };
                let pivot_new = Pose { x: cx, y: cy, q: k };
                Some(self.m.p.groups[g].members.iter().map(|&i| {
                    // Turn each member about the group centre.
                    let p = self.m.pose[i];
                    (i, self.m.clamp(i, follow(pivot_old, pivot_new, p)))
                }).collect())
            }
            Move::GSwap => {
                if self.free_groups.len() < 2 {
                    return None;
                }
                let a = self.free_groups[self.rng.below(self.free_groups.len())];
                let mut b = self.free_groups[self.rng.below(self.free_groups.len())];
                if a == b {
                    b = self.free_groups[(self.free_groups.iter().position(|&x| x == a).unwrap_or(0) + 1) % self.free_groups.len()];
                }
                let (ca, cb) = (self.group_centre(a), self.group_centre(b));
                let (dx, dy) = (cb.0 - ca.0, cb.1 - ca.1);
                let mut out: Vec<(usize, Pose)> = Vec::new();
                for (g, s) in [(a, 1.0), (b, -1.0)] {
                    for &i in &self.m.p.groups[g].members {
                        if out.iter().any(|x| x.0 == i) {
                            continue;
                        }
                        let p = self.m.pose[i];
                        out.push((i, self.m.clamp(i, Pose { x: p.x + s * dx, y: p.y + s * dy, q: p.q })));
                    }
                }
                Some(out)
            }
            Move::Escape => {
                let hit: Vec<usize> = self.mov.iter().copied().filter(|&i| self.m.overlap(&[i]) > 0.0).collect();
                if hit.is_empty() {
                    return None;
                }
                let i = hit[self.rng.below(hit.len())];
                let was = self.m.pose[i];
                let b = if self.m.p.fixed_outline { self.m.p.canvas } else { self.m.bbox() };
                let r = self.m.p.parts[i].crt[was.q];
                let mut best = (f64::INFINITY, was);
                for _ in 0..ESCAPE_TRIES {
                    let x = self.rng.uniform(b.x0 - r.x0, (b.x1 - r.x1).max(b.x0 - r.x0));
                    let y = self.rng.uniform(b.y0 - r.y0, (b.y1 - r.y1).max(b.y0 - r.y0));
                    let ps = self.m.clamp(i, Pose { x, y, q: was.q });
                    self.m.pose[i] = ps;
                    let ov = self.m.overlap(&[i]);
                    if ov < best.0 {
                        best = (ov, ps);
                    }
                }
                self.m.pose[i] = was;
                Some(vec![(i, best.1)])
            }
        }
    }

    /// Congestion: take `parts`' pads and `nets`' demand out of the grid (or
    /// put them in), at the current poses / committed demand.
    fn grid_move(&mut self, parts: &[usize], nets: &[usize], sign: f64, dem: &[(usize, R, f64)]) -> f64 {
        if self.m.grid.is_none() {
            return 0.0;
        }
        let mut d = 0.0;
        for &i in parts {
            d += self.m.pads_into_grid(i, sign);
        }
        for &(k, r, dens) in dem {
            if nets.contains(&k) {
                if let Some(g) = self.m.grid.as_mut() {
                    d += g.add(&r, sign * dens, true);
                }
            }
        }
        d
    }

    /// Price `new` (cost plus lam x overlap) and keep it by the Metropolis
    /// rule: `(kept, cost delta, overlap delta)`.
    fn trial(&mut self, new: &[(usize, Pose)], t: f64) -> (bool, f64, f64) {
        let parts: Vec<usize> = new.iter().map(|(i, _)| *i).collect();
        let was: Vec<Pose> = parts.iter().map(|&i| self.m.pose[i]).collect();
        let ob = self.m.overlap(&parts);
        let lb = self.m.local(&parts);
        let cnets: Vec<usize> = self.m.touched_nets(&parts).into_iter().filter(|&k| self.m.p.nets[k].congest).collect();
        let old_dem: Vec<(usize, R, f64)> = cnets.iter().map(|&k| (k, self.m.net_dem[k].0, self.m.net_dem[k].1)).collect();
        let mut dg = self.grid_move(&parts, &cnets, -1.0, &old_dem);
        for &(i, ps) in new {
            self.m.pose[i] = ps;
        }
        let d_ov = self.m.overlap(&parts) - ob;
        let dl = self.m.local(&parts) - lb;
        let (dc, pending) = self.m.price(&parts);
        let new_dem: Vec<(usize, R, f64)> = if self.m.grid.is_some() {
            cnets
                .iter()
                .map(|&k| {
                    let l = pending.new_net.iter().find(|x| x.0 == k).map_or(self.m.net_len[k], |x| x.2);
                    let (r, d) = self.m.demand(k, l);
                    (k, r, d)
                })
                .collect()
        } else {
            Vec::new()
        };
        dg += self.grid_move(&parts, &cnets, 1.0, &new_dem);
        let w_cong = self.m.p.weights.congestion;
        let cost = dc + dl + w_cong * dg;
        let delta = cost + self.lam * d_ov;
        if delta <= 0.0 || (t > 0.0 && self.rng.f64() < (-delta / t).exp()) {
            self.m.commit(pending);
            self.m.local_total += dl;
            self.m.cong += dg;
            for (k, r, d) in new_dem {
                self.m.net_dem[k] = (r, d);
            }
            return (true, cost, d_ov);
        }
        self.grid_move(&parts, &cnets, -1.0, &new_dem);
        for (k, &i) in parts.iter().enumerate() {
            self.m.pose[i] = was[k];
        }
        self.grid_move(&parts, &cnets, 1.0, &old_dem);
        (false, cost, d_ov)
    }

    fn start_temperature(&mut self) -> f64 {
        let reach = self.span / 6.0;
        let mut ds = Vec::new();
        for _ in 0..200 {
            let i = self.pick();
            let was = self.m.pose[i];
            let np = self.m.clamp(i, Pose { x: was.x + self.rng.uniform(-reach, reach), y: was.y + self.rng.uniform(-reach, reach), q: was.q });
            let lb = self.m.local(&[i]);
            self.m.pose[i] = np;
            let dl = self.m.local(&[i]) - lb;
            let (dc, _) = self.m.price(&[i]);
            self.m.pose[i] = was;
            let d = dc + dl;
            if d.abs() > 1e-9 {
                ds.push(d.abs());
            }
        }
        if ds.is_empty() {
            return 1.0;
        }
        ds.sort_by(f64::total_cmp);
        ds[ds.len() / 2] / -ACCEPT.ln()
    }

    pub fn run(&mut self, moves: u64) -> Outcome {
        if self.mov.is_empty() || moves == 0 {
            let clean = self.m.overlap_all() == 0.0;
            return Outcome { best: clean.then(|| self.m.pose.clone()), best_cost: self.m.total(), moves: 0 };
        }
        let t0 = self.start_temperature();
        let reach0 = self.span / 6.0;
        let late = (BIG_UNTIL * moves as f64) as u64;
        let mut kinds: Vec<Move> = MOVES.iter().map(|m| m.0).collect();
        if !self.mov.iter().any(|&i| self.m.p.parts[i].can_turn) {
            kinds.retain(|&k| k != Move::Turn);
        }
        if self.alike.is_empty() {
            kinds.retain(|&k| k != Move::Swap);
        }
        if !self.m.p.groups.iter().any(|g| g.members.len() > 1) {
            kinds.retain(|&k| k != Move::GShift);
        }
        if self.free_groups.is_empty() {
            kinds.retain(|&k| k != Move::GTurn);
        }
        if self.free_groups.len() < 2 {
            kinds.retain(|&k| k != Move::GSwap);
        }
        let weight = |k: &Move| MOVES.iter().find(|m| m.0 == *k).map_or(0.0, |m| m.1);
        let mut wts: Vec<f64> = kinds.iter().map(weight).collect();
        let mut cur = self.m.total();
        let mut ov = self.m.overlap_all();
        let mut best = if ov == 0.0 { (cur, Some(self.m.pose.clone())) } else { (f64::INFINITY, None) };
        for k in 0..moves {
            let frac = k as f64 / moves as f64;
            let t = if t0 > T_END { t0 * (T_END / t0).powf(frac) } else { T_END };
            self.lam = LAM_START * (LAM_END / LAM_START).powf(frac);
            let reach = (reach0 * (1.0 - frac).powi(2)).max(0.1);
            if k == late {
                for (j, kk) in kinds.iter().enumerate() {
                    if matches!(kk, Move::GTurn | Move::GSwap) {
                        wts[j] = 0.0;
                    }
                }
            }
            let kind = kinds[self.rng.weighted(&wts)];
            let Some(new) = self.propose(kind, reach) else { continue };
            let new = self.complete(new);
            let (kept, dc, d_ov) = self.trial(&new, t);
            if kept {
                cur += dc;
                ov = (ov + d_ov).max(0.0);
                if ov < CLEAN && cur < best.0 - 1e-9 {
                    ov = self.m.overlap_all();
                    if ov == 0.0 {
                        self.m.reprice_all();
                        cur = self.m.total();
                        if cur < best.0 - 1e-9 {
                            best = (cur, Some(self.m.pose.clone()));
                        }
                    }
                }
            }
            if k % 2048 == 2047 {
                self.m.reprice_all();
                cur = self.m.total();
                ov = self.m.overlap_all();
            }
        }
        Outcome { best: best.1, best_cost: best.0, moves }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mst_is_prim_l1() {
        let pts = [(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)];
        let mut e = Vec::new();
        assert_eq!(mst_l1(&pts, Some(&mut e)), 7.0);
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn crossing_is_proper_only() {
        assert!(crosses(&(0.0, 0.0, 2.0, 2.0), &(0.0, 2.0, 2.0, 0.0)));
        assert!(!crosses(&(0.0, 0.0, 2.0, 2.0), &(2.0, 2.0, 3.0, 0.0)));
    }

    #[test]
    fn follow_turns_with_the_anchor() {
        let old = Pose { x: 0.0, y: 0.0, q: 0 };
        let new = Pose { x: 10.0, y: 0.0, q: 1 };
        let s = follow(old, new, Pose { x: 1.0, y: 0.0, q: 0 });
        assert_eq!((s.x, s.y, s.q), (10.0, -1.0, 1));
        let back = follow(new, old, s);
        assert!((back.x - 1.0).abs() < 1e-12 && back.y.abs() < 1e-12 && back.q == 0);
    }

    #[test]
    fn gap_tree_is_zero_when_touching() {
        let a = R { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 };
        let b = R { x0: 1.0, y0: 0.0, x1: 2.0, y1: 1.0 };
        let c = R { x0: 5.0, y0: 0.0, x1: 6.0, y1: 1.0 };
        assert_eq!(gap_tree(&[a, b]), 0.0);
        assert_eq!(gap_tree(&[a, b, c]), 3.0);
    }
}
