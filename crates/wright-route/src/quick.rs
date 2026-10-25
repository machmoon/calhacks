//! The seed judge: route each candidate placement fast and rank the
//! results, after KiCadRoutingTools `py_placer/judge_seeds.py` (route every
//! seed at `route.py --quick 2`) and `py_tools/quick_score.py`
//! (`judge_key`: blocking, then `joins_needed`, then vias, copper_mm,
//! segments, sorted ascending). The placement's own defects (floorplan,
//! assembly), which `judge_key` puts first, belong to lane C's legality
//! check and are not scored here. The winner is then routed in full mode;
//! the quick board itself is never shipped (judge_seeds.py: "never the
//! --quick board").

use crate::{route_with_stats, verify, RouteOptions};
use wright_eda::{Board, Netlist, RouteReport};

/// `quick_score` for one routed board. Field order is the ranking order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuickScore {
    /// Attempted nets that are not fully joined (KiCadRoutingTools
    /// `blocking`: unrouted nets plus broken joins; this router lifts a
    /// net's partial copper when it fails, so every open net counts once).
    pub blocking: usize,
    /// Joins the open nets still need: copper islands minus one per net,
    /// so a missing 9-pad rail (8) loses to one stranded pad (1).
    pub joins_needed: usize,
    pub vias: usize,
    pub copper_mm: f64,
    pub segments: usize,
}

impl QuickScore {
    /// `judge_key`, ascending is better. Copper is compared in whole
    /// micrometres so the key is a total order without float ties.
    pub fn key(&self) -> (usize, usize, usize, i64, usize) {
        (self.blocking, self.joins_needed, self.vias, (self.copper_mm * 1000.0).round() as i64, self.segments)
    }
}

pub fn score(netlist: &Netlist, board: &Board, report: &RouteReport) -> QuickScore {
    let comps = verify::components(netlist, board);
    let mut blocking = 0;
    let mut joins = 0;
    for &n in &report.attempted {
        let c = comps[n];
        let open = report.unrouted.iter().any(|u| u.net == n) || c > 1;
        if open {
            blocking += 1;
            // A net with pads missing from the board still needs every join.
            let need = if c == 0 { netlist.nets[n].pins.len().saturating_sub(1) } else { c - 1 };
            joins += need.max(1);
        }
    }
    QuickScore { blocking, joins_needed: joins, vias: board.vias.len(), copper_mm: board.copper_length().as_mm(), segments: board.tracks.len() }
}

/// One judged candidate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Judged {
    /// Index into the candidates passed to [`judge`].
    pub seed: usize,
    pub score: QuickScore,
    pub seconds: f64,
}

/// Route every candidate with `opts` (normally [`RouteOptions::quick`]) on
/// a copy and rank them best first. Deterministic: equal keys keep seed
/// order. The caller routes `boards[result[0].seed]` with full options.
pub fn judge(netlist: &Netlist, boards: &[Board], opts: &RouteOptions) -> Vec<Judged> {
    let mut out: Vec<Judged> = boards
        .iter()
        .enumerate()
        .map(|(seed, b)| {
            let mut b = b.clone();
            let (rep, _) = route_with_stats(netlist, &mut b, opts);
            Judged { seed, score: score(netlist, &b, &rep), seconds: rep.seconds }
        })
        .collect();
    out.sort_by_key(|j| (j.score.key(), j.seed));
    out
}

/// Judge `boards` at quick level, then route the winner in full mode in
/// place of the original. Returns the winner's index, its full report, and
/// the ranking.
pub fn judge_and_route(netlist: &Netlist, boards: &[Board], full: &RouteOptions) -> Option<(usize, Board, RouteReport, Vec<Judged>)> {
    let ranking = judge(netlist, boards, &RouteOptions::quick());
    let best = ranking.first()?.seed;
    let mut b = boards[best].clone();
    let rep = crate::route(netlist, &mut b, full);
    Some((best, b, rep, ranking))
}
