//! Decoupling capacitors and the loop each one closes.
//!
//! The rule is the Python baseline's `engine/silkscreen/board.py:1039 decoupling_caps`
//! (itself tscircuit's `rfcs/2026-07-31-automatic-decoupling-capacitor-
//! detection-and-enforcement.md`): a capacitor between a supply rail and
//! ground is a decoupling cap when the rail reaches an IC (`kind == ic`;
//! a connector's VBUS pin is a source, not a load), and it belongs next to
//! that IC's supply pin. When several caps share one rail they are dealt
//! out in turn (fewest served first, then pin order), caps in name order.
//!
//! The metric is the baseline's `engine/tests/test_board_decoupling.py:103 _loop_mm`,
//! which `scripts/board_eval.py` reports as `decoupling_loop_mm`: per cap,
//! rail pad to the IC supply pin plus ground pad to the IC's own ground pin,
//! straight-line, each to the nearest pad carrying that number (SOT-223's
//! tab repeats pin 2).

use std::collections::BTreeMap;
use wright_eda::ir::NetRole;
use wright_eda::{Board, Netlist, PartKind};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Decoupling {
    pub cap: usize,
    /// The cap's pad on the rail and on ground.
    pub cap_rail_pad: String,
    pub cap_ground_pad: String,
    pub ic: usize,
    pub ic_supply_pad: String,
    /// The IC's own ground pin, when it has one on the same ground net.
    pub ic_ground_pad: Option<String>,
}

pub fn decoupling_caps(netlist: &Netlist) -> Vec<Decoupling> {
    let mut served: BTreeMap<(usize, String), usize> = BTreeMap::new();
    let mut out = Vec::new();
    for (ci, part) in netlist.parts.iter().enumerate() {
        if part.kind != PartKind::Capacitor || part.pins.len() != 2 {
            continue;
        }
        let legs: Vec<(String, Option<usize>)> = part.pins.iter().map(|p| (p.pad.clone(), p.net)).collect();
        let role = |n: Option<usize>| n.map(|n| netlist.nets[n].role);
        let (rail_leg, ground_leg) = match (role(legs[0].1), role(legs[1].1)) {
            (Some(NetRole::Power), Some(NetRole::Ground)) => (&legs[0], &legs[1]),
            (Some(NetRole::Ground), Some(NetRole::Power)) => (&legs[1], &legs[0]),
            _ => continue,
        };
        let (rail, ground) = (rail_leg.1.unwrap_or(0), ground_leg.1.unwrap_or(0));
        let ic_pins: Vec<(usize, String)> =
            netlist.nets[rail].pins.iter().filter(|p| netlist.parts[p.part].kind == PartKind::Ic).map(|p| (p.part, p.pad.clone())).collect();
        if ic_pins.is_empty() {
            continue;
        }
        let pick = ic_pins.iter().enumerate().min_by_key(|(i, k)| (served.get(*k).copied().unwrap_or(0), *i)).map(|(_, k)| k.clone()).unwrap_or_else(|| ic_pins[0].clone());
        *served.entry(pick.clone()).or_insert(0) += 1;
        let ic_ground_pad = netlist.nets[ground].pins.iter().find(|p| p.part == pick.0).map(|p| p.pad.clone());
        out.push(Decoupling { cap: ci, cap_rail_pad: rail_leg.0.clone(), cap_ground_pad: ground_leg.0.clone(), ic: pick.0, ic_supply_pad: pick.1, ic_ground_pad });
    }
    out
}

/// Nearest distance (nm) between any pad numbered `a` on part `pa` and any
/// pad numbered `b` on part `pb`; `None` when either is missing.
fn pad_gap(board: &Board, pa: usize, a: &str, pb: usize, b: &str) -> Option<f64> {
    let find = |part: usize| board.parts.iter().find(|p| p.part == part);
    let (ppa, ppb) = (find(pa)?, find(pb)?);
    let mut best: Option<f64> = None;
    for x in ppa.footprint.pads.iter().filter(|p| p.number == a) {
        for y in ppb.footprint.pads.iter().filter(|p| p.number == b) {
            let d = ppa.pad_abs(x).dist(ppb.pad_abs(y));
            best = Some(best.map_or(d, |v: f64| v.min(d)));
        }
    }
    best
}

/// Each cap's loop, nm, in [`decoupling_caps`] order. A missing pad counts
/// as an infinite loop, never as zero.
pub fn loops_nm(board: &Board, decaps: &[Decoupling]) -> Vec<f64> {
    decaps
        .iter()
        .map(|d| {
            let rail = pad_gap(board, d.cap, &d.cap_rail_pad, d.ic, &d.ic_supply_pad).unwrap_or(f64::INFINITY);
            let gnd = match &d.ic_ground_pad {
                Some(g) => pad_gap(board, d.cap, &d.cap_ground_pad, d.ic, g).unwrap_or(f64::INFINITY),
                None => 0.0,
            };
            rail + gnd
        })
        .collect()
}

/// The baseline's `decoupling_loop_mm`: the sum over every decoupling cap.
pub fn decoupling_loop_mm(netlist: &Netlist, board: &Board) -> f64 {
    loops_nm(board, &decoupling_caps(netlist)).iter().sum::<f64>() / 1e6
}

/// Minimum-cost assignment of every row to a distinct column (rows <=
/// columns): the Hungarian method in its shortest-augmenting-path form
/// (Kuhn-Munkres; the O(n^2 m) potentials version of e-maxx.ru "Hungarian
/// algorithm"), standing in for upstream's scipy `linear_sum_assignment`
/// in `simple.py cluster_match`. Returns the column of each row.
pub fn assign(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    if n == 0 {
        return Vec::new();
    }
    let m = cost[0].len();
    assert!(m >= n, "assign: {n} rows, {m} columns");
    let inf = f64::INFINITY;
    let (mut u, mut v) = (vec![0.0; n + 1], vec![0.0; m + 1]);
    let (mut p, mut way) = (vec![0usize; m + 1], vec![0usize; m + 1]);
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![inf; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = inf;
            let mut j1 = 0;
            for j in 1..=m {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = vec![0; n];
    for j in 1..=m {
        if p[j] != 0 {
            out[p[j] - 1] = j - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_is_optimal() {
        let c = vec![vec![4.0, 1.0, 3.0], vec![2.0, 0.0, 5.0], vec![3.0, 2.0, 2.0]];
        let a = assign(&c);
        let total: f64 = a.iter().enumerate().map(|(i, &j)| c[i][j]).sum();
        assert_eq!(total, 5.0);
        // Rectangular: two rows, three columns.
        assert_eq!(assign(&[vec![5.0, 1.0, 9.0], vec![1.0, 2.0, 9.0]]), vec![1, 0]);
    }
}
