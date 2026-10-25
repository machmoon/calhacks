//! Datasheet templates: a chip's support parts seated around it once, then
//! carried rigidly with it by the anneal, with a drift cost if one strays.
//! KiCadRoutingTools v0.23.7 `py_placer/placement/templates.py` (`fit`,
//! `pad_deviation`, `member_target`) and the template term in `simple.py`
//! (`template_cost`, weight 100 per mm in `place_simple.py`).
//!
//! `fit` as upstream seats a sketch: the chip at the origin unturned;
//! members in order, nearest the chip first; for each member its legs are
//! its pads that share a net with an already seated pad; over each quarter
//! turn, the origin minimising the summed pad-edge gap from each leg to its
//! nearest partner pad is searched on a 0.1 mm grid within 4 mm of the
//! partners, then refined on 0.01 mm; a pose must keep the courtyard gap
//! (spacing + 0.001 mm) from every seated part; a small tie-break favours
//! poses near the chip. A member with no legal pose fails the fit.
//!
//! Upstream fits a sketch the agent drew from the datasheet figure. Wright
//! has no sketch for the built-in TPS5450 stage (SLVS757 'Layout': input
//! caps at VIN/GND, boot cap across BOOT/PH, catch diode and inductor at
//! PH, divider at VSENSE), so the members are the chip's direct neighbours
//! (parts sharing a non-ground net with it, connectors excluded) and the
//! fit itself places them against the pins they share, which is what the
//! figure shows.

use crate::geom::{PartGeom, R};
use crate::intent::{Member, Template, DECAP_MAX_MM, TEMPLATE_TOLERANCE_MM};
use std::collections::BTreeSet;
use wright_eda::ir::NetRole;
use wright_eda::{Netlist, PartKind};

pub const SEARCH_MM: f64 = 4.0;
/// Per mm of decap loop past the limit less the margin (upstream's
/// `decap_limit` weight, `place_simple.py`).
const LOOP_WEIGHT: f64 = 10.0;
const LOOP_MARGIN_MM: f64 = 2.0;
/// A member's pads on one net and the chip's pad centres on it.
type LoopLeg = (Vec<usize>, Vec<(f64, f64)>);

/// `part`'s pad indices on `net`.
fn g_pads(n: &Netlist, geoms: &[PartGeom], part: usize, net: usize) -> Vec<usize> {
    let p = &n.parts[part];
    geoms[part].numbers.iter().enumerate().filter(|(_, num)| p.pins.iter().any(|q| &q.pad == *num && q.net == Some(net))).map(|(k, _)| k).collect()
}

/// The built-in member rule: parts sharing a non-ground net with `chip`,
/// most shared nets first, then part order. Connectors and `exclude` are
/// left out.
pub fn neighbours(n: &Netlist, chip: usize, exclude: &BTreeSet<usize>) -> Vec<usize> {
    let nets: BTreeSet<usize> = n.parts[chip].pins.iter().filter_map(|p| p.net).filter(|&k| n.nets[k].role != NetRole::Ground).collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, p) in n.parts.iter().enumerate() {
        if i == chip || exclude.contains(&i) || matches!(p.kind, PartKind::Connector | PartKind::Ic | PartKind::Battery | PartKind::Switch | PartKind::Testpoint) {
            continue;
        }
        let shared = p.pins.iter().filter(|x| x.net.is_some_and(|k| nets.contains(&k))).count();
        if shared > 0 {
            out.push((shared, i));
        }
    }
    out.sort_by_key(|&(s, i)| (std::cmp::Reverse(s), i));
    out.into_iter().map(|x| x.1).collect()
}

fn net_of(n: &Netlist, part: usize, pad: &str) -> Option<usize> {
    n.parts[part].pins.iter().find(|p| p.pad == pad).and_then(|p| p.net)
}

/// Seat `members` around `chip`; the row upstream's `fit` returns.
/// `gap_mm` is the courtyard gap every seated pair keeps.
pub fn fit(n: &Netlist, geoms: &[PartGeom], chip: usize, members: &[usize], gap_mm: f64, source: &str) -> Result<Template, String> {
    // (part, x, y, q)
    let mut seated: Vec<(usize, f64, f64, usize)> = vec![(chip, 0.0, 0.0, 0)];
    let mut out = Vec::new();
    let gap = gap_mm;
    for &m in members {
        let g = &geoms[m];
        // Legs: (member pad index, partner pad rects in board frame).
        let mut legs: Vec<(usize, Vec<R>)> = Vec::new();
        for (k, num) in g.numbers.iter().enumerate() {
            let Some(net) = net_of(n, m, num) else { continue };
            // The chip's own pins first (the datasheet figure seats each
            // part against the chip); other seated parts only when the
            // member shares no net with the chip, so no part chains away.
            let mut partners = Vec::new();
            for (j, sn) in geoms[chip].numbers.iter().enumerate() {
                if net_of(n, chip, sn) == Some(net) {
                    partners.push(geoms[chip].pad_rects[0][j]);
                }
            }
            if partners.is_empty() {
                for &(s, x, y, q) in seated.iter().skip(1) {
                    for (j, sn) in geoms[s].numbers.iter().enumerate() {
                        if net_of(n, s, sn) == Some(net) {
                            partners.push(geoms[s].pad_rects[q][j].at(x, y));
                        }
                    }
                }
            }
            if !partners.is_empty() {
                legs.push((k, partners));
            }
        }
        if legs.is_empty() {
            return Err(format!("{}: no pad shares a net with the seated parts", n.parts[m].refdes));
        }
        // A decoupling cap of the chip (on one of its power rails and its
        // ground) also keeps its loop, centre to centre as the verifier
        // measures it, inside the limit with a margin: the edge gap alone
        // is zero anywhere along a large PowerPAD.
        let ground = n.parts[chip].pins.iter().filter_map(|p| p.net).find(|&k| n.nets[k].role == NetRole::Ground);
        let mut loop_legs: Vec<LoopLeg> = Vec::new();
        if n.parts[m].kind == PartKind::Capacitor {
            if let Some(g) = ground {
                let rail = n.parts[m].pins.iter().filter_map(|p| p.net).find(|&k| n.nets[k].role == NetRole::Power && n.parts[chip].pins.iter().any(|q| q.net == Some(k)));
                if let Some(r) = rail.filter(|_| n.parts[m].pins.iter().any(|p| p.net == Some(g))) {
                    for net in [r, g] {
                        let mine: Vec<usize> = g_pads(n, geoms, m, net);
                        let theirs: Vec<(f64, f64)> = g_pads(n, geoms, chip, net).iter().map(|&j| geoms[chip].pads[0][j]).collect();
                        loop_legs.push((mine, theirs));
                    }
                }
            }
        }
        let region = legs.iter().flat_map(|l| l.1.iter()).fold(None, |acc: Option<R>, r| Some(acc.map_or(*r, |a| a.union(r)))).unwrap_or(R { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 });
        let walls: Vec<R> = seated.iter().map(|&(s, x, y, q)| geoms[s].crt[q].at(x, y)).collect();
        let cost = |x: f64, y: f64, q: usize| -> Option<f64> {
            let c = g.crt[q].at(x, y).grow(gap);
            if walls.iter().any(|w| w.overlap(&c) > 0.0) {
                return None;
            }
            let mut s = 0.0;
            for (k, ps) in &legs {
                let pr = g.pad_rects[q][*k].at(x, y);
                s += ps.iter().map(|p| p.gap(&pr)).fold(f64::INFINITY, f64::min);
            }
            if !loop_legs.is_empty() {
                let mut l = 0.0;
                for (mine, theirs) in &loop_legs {
                    l += mine
                        .iter()
                        .flat_map(|&k| {
                            let (px, py) = g.pads[q][k];
                            theirs.iter().map(move |&(tx, ty)| (x + px - tx).hypot(y + py - ty))
                        })
                        .fold(f64::INFINITY, f64::min);
                }
                s += LOOP_WEIGHT * (l - (DECAP_MAX_MM - LOOP_MARGIN_MM)).max(0.0);
            }
            let cc = g.crt[q].at(x, y);
            Some(s + 1e-3 * (cc.cx().abs() + cc.cy().abs()))
        };
        let mut best: Option<(f64, f64, f64, usize)> = None;
        for q in 0..4 {
            let c = g.crt[q];
            let reach = SEARCH_MM + c.w().max(c.h());
            let (x0, x1) = (region.x0 - reach, region.x1 + reach);
            let (y0, y1) = (region.y0 - reach, region.y1 + reach);
            let (nx, ny) = (((x1 - x0) / 0.1) as i64, ((y1 - y0) / 0.1) as i64);
            for ix in 0..=nx {
                for iy in 0..=ny {
                    let (x, y) = (x0 + ix as f64 * 0.1, y0 + iy as f64 * 0.1);
                    if let Some(v) = cost(x, y, q) {
                        if best.is_none_or(|b| v < b.0 - 1e-12) {
                            best = Some((v, x, y, q));
                        }
                    }
                }
            }
        }
        let Some((_, bx, by, bq)) = best else {
            return Err(format!("{}: no legal pose within {SEARCH_MM} mm of its partners", n.parts[m].refdes));
        };
        let mut fine = (f64::INFINITY, bx, by);
        for ix in -10..=10 {
            for iy in -10..=10 {
                let (x, y) = (bx + ix as f64 * 0.01, by + iy as f64 * 0.01);
                if let Some(v) = cost(x, y, bq) {
                    if v < fine.0 - 1e-12 {
                        fine = (v, x, y);
                    }
                }
            }
        }
        seated.push((m, fine.1, fine.2, bq));
        out.push(Member { refdes: n.parts[m].refdes.clone(), at: [(fine.1 * 1e3).round() / 1e3, (fine.2 * 1e3).round() / 1e3], rot: bq as i32 * 90, note: None });
    }
    Ok(Template {
        anchor: n.parts[chip].refdes.clone(),
        members: out,
        tolerance_mm: TEMPLATE_TOLERANCE_MM,
        source: Some(source.to_string()),
        kind: Some("switching stage".into()),
        why: Some("fitted by wright-place (templates.py fit)".into()),
    })
}
