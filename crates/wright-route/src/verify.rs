//! The router's own audit of the copper it reports: per-net connectivity by
//! union-find over pads, tracks and vias, and an all-pairs different-net
//! clearance check. `route` decides `routed` from [`connected`], not from
//! what the search believed, so a bug in the search shows up as an
//! unrouted net, never as a false "routed".
//!
//! Connection rules follow KiCad's connectivity (pcbnew
//! `connectivity/connectivity_items.cpp` `CN_ITEM` anchors, design read
//! only): a track joins a pad when its end point is inside the pad, joins a
//! track when its end point lies on the other track's copper, and joins a via
//! when its end point is inside the via; a via joins a pad when its centre
//! is inside the pad. Zones are not counted: KiCad fills them.

use crate::copper::{self, layer_bit, PadItem, F, B};
use crate::geom::{Shape, P};
use wright_eda::{Board, Netlist};

struct Dsu(Vec<usize>);
impl Dsu {
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = x;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

fn p(pt: wright_eda::Pt) -> P {
    P::new(pt.x.0 as f64, pt.y.0 as f64)
}

/// For each net: are all its pads joined by copper (tracks, vias, touching
/// pads)? Pads missing from the board are not seen here (see
/// [`missing_pads`]).
pub fn connected(netlist: &Netlist, board: &Board) -> Vec<bool> {
    components(netlist, board).iter().map(|&c| c <= 1).collect()
}

/// For each net: how many separate copper islands its pads form (0 for a
/// net with no pads on the board). `components - 1` is the joins it still
/// needs (KiCadRoutingTools `py_tools/board_score.py` `joins_needed`).
pub fn components(netlist: &Netlist, board: &Board) -> Vec<usize> {
    let (pads, _) = copper::pads(netlist, board);
    let nt = board.tracks.len();
    let nv = board.vias.len();
    let np = pads.len();
    // items: pads [0, np), tracks [np, np+nt), vias [np+nt, ..)
    let mut d = Dsu((0..np + nt + nv).collect());
    let tol = 1.0;
    // pad-pad touching (same layer)
    for a in 0..np {
        for b in a + 1..np {
            if pads[a].net.is_some() && pads[a].net == pads[b].net && pads[a].layers & pads[b].layers != 0 && pads[a].shape.dist_shape(&pads[b].shape) <= 0.0 {
                d.join(a, b);
            }
        }
    }
    let _ = tol;
    let inside = |pad: &PadItem, q: P| pad.contains_copper(q);
    for (ti, t) in board.tracks.iter().enumerate() {
        let lb = layer_bit(t.layer);
        let ends = [p(t.a), p(t.b)];
        for (pi, pad) in pads.iter().enumerate() {
            if pad.net == Some(t.net) && pad.layers & lb != 0 && ends.iter().any(|&q| inside(pad, q)) {
                d.join(pi, np + ti);
            }
        }
        for (ui, u) in board.tracks.iter().enumerate().skip(ti + 1) {
            if u.net != t.net || u.layer != t.layer {
                continue;
            }
            let su = copper::track_shape(u);
            let st = copper::track_shape(t);
            let touch = ends.iter().any(|&q| su.dist_seg(q, q) <= tol) || [p(u.a), p(u.b)].iter().any(|&q| st.dist_seg(q, q) <= tol);
            if touch {
                d.join(np + ti, np + ui);
            }
        }
        for (vi, v) in board.vias.iter().enumerate() {
            if v.net == t.net && ends.iter().any(|&q| copper::via_shape(v).dist_seg(q, q) <= tol) {
                d.join(np + ti, np + nt + vi);
            }
        }
    }
    for (vi, v) in board.vias.iter().enumerate() {
        let c = p(v.at);
        for (pi, pad) in pads.iter().enumerate() {
            if pad.net == Some(v.net) && inside(pad, c) {
                d.join(pi, np + nt + vi);
            }
        }
    }
    (0..netlist.nets.len())
        .map(|n| {
            let mut roots: Vec<usize> = (0..np).filter(|&i| pads[i].net == Some(n)).map(|i| d.find(i)).collect();
            roots.sort_unstable();
            roots.dedup();
            roots.len()
        })
        .collect()
}

/// Netlist pins of `net` whose pad is not on the board.
pub fn missing_pads(netlist: &Netlist, board: &Board, net: usize) -> Vec<String> {
    let mut out = Vec::new();
    for pin in &netlist.nets[net].pins {
        let on = board.parts.iter().any(|pl| pl.part == pin.part && pl.footprint.pads.iter().any(|pd| pd.number == pin.pad));
        if !on {
            out.push(format!("{}.{}", netlist.parts[pin.part].refdes, pin.pad));
        }
    }
    out
}

/// Different-net copper closer than its clearance, for every pair that
/// involves a track or via (pad-to-pad gaps are the placer's), drills
/// closer than KiCad's `min_hole_to_hole` where a via is one of the pair
/// (any nets), and tracks/vias closer to the board edge than
/// `edge_clearance`.
pub fn clearance_violations(netlist: &Netlist, board: &Board) -> Vec<String> {
    let (pads, holes) = copper::pads(netlist, board);
    let clr = |n: Option<usize>| n.map(|n| crate::router::rule_for(board, n).clr.0 as f64).unwrap_or(board.rules.min_clearance.0 as f64);
    struct It {
        shape: Shape,
        layers: u8,
        net: Option<usize>,
        what: String,
        copper: bool,
    }
    let mut items: Vec<It> = Vec::new();
    for pd in &pads {
        items.push(It { shape: pd.shape, layers: pd.layers, net: pd.net, what: format!("pad {}.{}", netlist.parts[pd.part].refdes, pd.number), copper: false });
    }
    for &(c, r) in &holes {
        items.push(It { shape: Shape::circle(c, r), layers: F | B, net: None, what: "hole".into(), copper: false });
    }
    let first_copper = items.len();
    for t in &board.tracks {
        items.push(It { shape: copper::track_shape(t), layers: layer_bit(t.layer), net: Some(t.net), what: format!("track {} {}", netlist.nets[t.net].name, t.layer.kicad()), copper: true });
    }
    for v in &board.vias {
        items.push(It { shape: copper::via_shape(v), layers: F | B, net: Some(v.net), what: format!("via {}", netlist.nets[v.net].name), copper: true });
    }
    let mut out = Vec::new();
    // Drill to drill (kicad-cli `hole_to_hole`), every pair with a via in it,
    // same net included: duplicate stitching vias showed up here.
    let mut drills: Vec<(P, f64, String, bool)> = pads.iter().filter(|p| p.drilled).map(|p| (p.centre, p.drill_r, format!("pad {}.{}", netlist.parts[p.part].refdes, p.number), false)).collect();
    drills.extend(holes.iter().map(|&(c, r)| (c, r, "hole".to_string(), false)));
    drills.extend(board.vias.iter().map(|v| (p(v.at), v.drill.0 as f64 / 2.0, format!("via {} at ({:.3}, {:.3})", netlist.nets[v.net].name, v.at.x.as_mm(), v.at.y.as_mm()), true)));
    for a in 0..drills.len() {
        for b in a + 1..drills.len() {
            let (x, y) = (&drills[a], &drills[b]);
            if !(x.3 || y.3) {
                continue;
            }
            let gap = x.0.dist(y.0) - x.1 - y.1;
            if gap < copper::HOLE_TO_HOLE - 1.0 {
                out.push(format!("hole_to_hole: {} and {}: {:.3} mm (needs {:.3})", x.2, y.2, gap.max(0.0) / 1e6, copper::HOLE_TO_HOLE / 1e6));
            }
        }
    }
    let o = board.outline;
    let edge = board.rules.edge_clearance.0 as f64;
    for it in &items[first_copper..] {
        let (lo, hi) = it.shape.bbox();
        let m = (lo.x - o.min.x.0 as f64).min(o.max.x.0 as f64 - hi.x).min(lo.y - o.min.y.0 as f64).min(o.max.y.0 as f64 - hi.y);
        if m < edge - 1.0 {
            out.push(format!("{} is {:.3} mm from the board edge (needs {:.3})", it.what, m / 1e6, edge / 1e6));
        }
    }
    for a in first_copper..items.len() {
        for b in 0..items.len() {
            if b >= first_copper && b <= a {
                continue;
            }
            let (x, y) = (&items[a], &items[b]);
            if !(x.copper || y.copper) || x.layers & y.layers == 0 || (x.net.is_some() && x.net == y.net) {
                continue;
            }
            let need = clr(x.net).max(clr(y.net));
            let dist = x.shape.dist_shape(&y.shape);
            if dist < need - 1.0 {
                out.push(format!("{} and {}: {:.4} mm apart, clearance {:.3} mm", x.what, y.what, dist / 1e6, need / 1e6));
            }
        }
    }
    out
}
