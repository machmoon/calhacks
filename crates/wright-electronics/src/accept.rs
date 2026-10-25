//! The acceptance rule for a board, written to match
//! `lean/Wright/Board.lean` `accept` / `accept_iff` term for term:
//!
//! ```text
//! accept b = true <->
//!   (forall n in attempted, status n is not unrouted)          -- nets_ok
//!   /\ (forall n in nets, 2 <= pads n -> n in attempted)        -- covered
//!   /\ (forall p in pins, onNet p \/ declaredNC p)              -- pins_ok
//!   /\ erc = ran 0 /\ drc = ran 0 /\ parity = ran 0             -- tools_ok
//! ```
//!
//! The Rust-to-Lean reading: `status n` is `routed` / `poured` from the
//! `RouteReport`, and `unrouted` when the net is in `unrouted` *or* is in
//! none of the three lists (an attempted net nobody reported is not a
//! routed one); `pads n` is the netlist pin count; a `CliVerdict` is
//! `ran (errors [+ unconnected for DRC])` when `ran`, else `didNotRun`.
//! Lean proves the rule's properties on its model; it does not verify this
//! code. The 15-check report is stricter (geometry, ratings, budget) and is
//! what the harness judges; `accept` is the board-level gate both share.

use wright_eda::{Netlist, RouteReport};
use wright_kicad::CliVerdict;

/// Lean `ToolVerdict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVerdict {
    Ran(usize),
    DidNotRun,
}

impl ToolVerdict {
    pub fn clean(self) -> bool {
        self == ToolVerdict::Ran(0)
    }
    /// `name` is "erc", "drc" or "parity"; a missing verdict did not run.
    pub fn of(verdicts: &[CliVerdict], name: &str) -> ToolVerdict {
        match verdicts.iter().find(|v| v.check == name) {
            Some(v) if v.ran => ToolVerdict::Ran(v.errors + if name == "drc" { v.unconnected } else { 0 }),
            _ => ToolVerdict::DidNotRun,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    pub nets_ok: bool,
    pub covered: bool,
    pub pins_ok: bool,
    pub tools_ok: bool,
    /// Why not, one line per failed conjunct.
    pub why: Vec<String>,
}

impl Acceptance {
    pub fn accepted(&self) -> bool {
        self.nets_ok && self.covered && self.pins_ok && self.tools_ok
    }
}

fn unrouted(r: &RouteReport, n: usize) -> bool {
    r.unrouted.iter().any(|u| u.net == n) || !(r.routed.contains(&n) || r.poured.contains(&n))
}

pub fn accept(netlist: &Netlist, route: &RouteReport, verdicts: &[CliVerdict]) -> Acceptance {
    let mut why = Vec::new();
    let bad: Vec<&str> = route.attempted.iter().filter(|&&n| unrouted(route, n)).map(|&n| netlist.nets.get(n).map(|x| x.name.as_str()).unwrap_or("?")).collect();
    let nets_ok = bad.is_empty();
    if !nets_ok {
        why.push(format!("unrouted: {}", bad.join(", ")));
    }
    let uncovered: Vec<&str> = netlist.nets.iter().enumerate().filter(|(i, n)| n.pins.len() >= 2 && !route.attempted.contains(i)).map(|(_, n)| n.name.as_str()).collect();
    let covered = uncovered.is_empty();
    if !covered {
        why.push(format!("never attempted: {}", uncovered.join(", ")));
    }
    let floating: Vec<String> = netlist.parts.iter().flat_map(|p| p.pins.iter().filter(|q| q.net.is_none() && !q.no_connect).map(move |q| format!("{}.{}", p.refdes, q.name))).collect();
    let pins_ok = floating.is_empty();
    if !pins_ok {
        why.push(format!("floating pins: {}", floating.join(", ")));
    }
    let tools: Vec<(&str, ToolVerdict)> = ["erc", "drc", "parity"].iter().map(|n| (*n, ToolVerdict::of(verdicts, n))).collect();
    let tools_ok = tools.iter().all(|(_, v)| v.clean());
    for (n, v) in &tools {
        match v {
            ToolVerdict::DidNotRun => why.push(format!("{n} did not run")),
            ToolVerdict::Ran(k) if *k > 0 => why.push(format!("{n}: {k} errors")),
            _ => {}
        }
    }
    Acceptance { nets_ok, covered, pins_ok, tools_ok, why }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_eda::{Circuit, Unrouted};

    fn netlist() -> Netlist {
        Circuit::from_json(r#"{"devices":{"J":{"kind":"connector","pins":{"1":"1","2":"2"}}},"passives":{"R":{"type":"resistor","value":"1k"}},"nets":{"A":["J.1","R.1"],"B":["J.2","R.2"]}}"#).unwrap().validate().unwrap()
    }
    fn v(check: &str, ran: bool, errors: usize, unconnected: usize) -> CliVerdict {
        CliVerdict { check: check.into(), ran, tool_version: None, errors, warnings: 0, unconnected, details: vec![], report: None }
    }
    fn clean() -> Vec<CliVerdict> {
        vec![v("erc", true, 0, 0), v("drc", true, 0, 0), v("parity", true, 0, 0)]
    }
    fn routed() -> RouteReport {
        RouteReport { attempted: vec![0, 1], routed: vec![0, 1], ..Default::default() }
    }

    #[test]
    fn the_lean_theorems_hold_on_the_rust_rule() {
        let n = netlist();
        assert!(accept(&n, &routed(), &clean()).accepted());
        // tool_not_run_rejects / tool_errors_reject
        for (i, bad) in [v("erc", false, 0, 0), v("drc", true, 0, 1), v("parity", true, 2, 0)].into_iter().enumerate() {
            let mut vs = clean();
            vs[i] = bad;
            assert!(!accept(&n, &routed(), &vs).accepted());
        }
        assert!(!accept(&n, &routed(), &clean()[..2]).accepted(), "a missing verdict did not run");
        // unrouted_rejects (and an attempted net nobody reported)
        let mut r = routed();
        r.routed = vec![0];
        r.unrouted.push(Unrouted { net: 1, name: "B".into(), reason: "x".into() });
        assert!(!accept(&n, &r, &clean()).accepted());
        let mut r = routed();
        r.routed = vec![0];
        assert!(!accept(&n, &r, &clean()).accepted());
        // uncovered_rejects: the empty-attempted case (the Python baseline's 100 % bug)
        assert!(!accept(&n, &RouteReport::default(), &clean()).accepted());
        // a poured net is not unrouted
        let r = RouteReport { attempted: vec![0, 1], routed: vec![0], poured: vec![1], ..Default::default() };
        assert!(accept(&n, &r, &clean()).accepted());
    }
}
