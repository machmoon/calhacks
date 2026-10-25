//! The electronics domain's closed set of checks (wright_core::check rules:
//! an unevaluated check fails, missing evidence fails, NaN fails).
//! Each names the independent model that measures it; see
//! docs/electronics-plan.md section 4 for which are recomputed by the
//! Haskell oracle and which are kicad-cli's.

use wright_core::CheckId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The circuit validates: every terminal resolves (wright_eda::ir).
    NetlistValid,
    /// Every pin is on a net or declared no-connect.
    PinsAccounted,
    /// Every part has a KiCad library footprint with every pin's pad.
    FootprintsResolved,
    /// No two courtyards overlap, all pairs (Geometry).
    CourtyardClearance,
    /// Every attempted net routed or poured; an unrouted net fails by name.
    Routed,
    /// Min copper-to-copper distance between different nets >= clearance,
    /// recomputed from the emitted copper, not the router's grid (Geometry).
    CopperClearance,
    /// Each net's current-carrying copper carries its design current:
    /// body track width (IPC-2221 external, 20 C rise) plus the short-neck
    /// rule, or a pour's stitching vias and thermal reliefs (Physics).
    TraceAmpacity,
    /// Regulator output from its feedback divider within tolerance of the
    /// requirement (datasheet formula, Physics).
    RegulatorSetpoint,
    /// Regulator / connector / inductor current ratings vs load (Rating).
    PartRatings,
    /// Decoupling cap to its IC supply pin path length (Geometry).
    DecouplingLoop,
    /// Board outline fits the vehicle bay the spec names (Requirement).
    BoardFitsBay,
    /// kicad-cli sch erc: 0 errors (Simulation = external tool).
    Erc,
    /// kicad-cli pcb drc --refill-zones: 0 errors, 0 unconnected.
    Drc,
    /// kicad-cli pcb drc --schematic-parity: 0 issues.
    Parity,
    /// Every BOM line priced; total within budget.
    Budget,
}

impl CheckId for Check {
    const ALL: &'static [Self] = &[
        Check::NetlistValid,
        Check::PinsAccounted,
        Check::FootprintsResolved,
        Check::CourtyardClearance,
        Check::Routed,
        Check::CopperClearance,
        Check::TraceAmpacity,
        Check::RegulatorSetpoint,
        Check::PartRatings,
        Check::DecouplingLoop,
        Check::BoardFitsBay,
        Check::Erc,
        Check::Drc,
        Check::Parity,
        Check::Budget,
    ];
    fn name(self) -> &'static str {
        match self {
            Check::NetlistValid => "netlist_valid",
            Check::PinsAccounted => "pins_accounted",
            Check::FootprintsResolved => "footprints_resolved",
            Check::CourtyardClearance => "courtyard_clearance",
            Check::Routed => "routed",
            Check::CopperClearance => "copper_clearance",
            Check::TraceAmpacity => "trace_ampacity",
            Check::RegulatorSetpoint => "regulator_setpoint",
            Check::PartRatings => "part_ratings",
            Check::DecouplingLoop => "decoupling_loop",
            Check::BoardFitsBay => "board_fits_bay",
            Check::Erc => "erc",
            Check::Drc => "drc",
            Check::Parity => "parity",
            Check::Budget => "budget",
        }
    }
    fn describe(self) -> &'static str {
        match self {
            Check::NetlistValid => "the circuit validates: every terminal names a real part and pin",
            Check::PinsAccounted => "every pin is on a net or declared no_connect",
            Check::FootprintsResolved => "every part has a KiCad library footprint carrying each of its pads",
            Check::CourtyardClearance => "no two courtyards overlap (all pairs)",
            Check::Routed => "every net is routed or poured; unrouted nets fail by name",
            Check::CopperClearance => "different-net copper is at least the net-class clearance apart",
            Check::TraceAmpacity => "each net's narrowest track carries its design current at a 20 C rise (IPC-2221)",
            Check::RegulatorSetpoint => "regulator output from its divider is within tolerance of the requirement",
            Check::PartRatings => "regulator, inductor, diode and connector ratings cover the load",
            Check::DecouplingLoop => "each decoupling cap is within the loop limit of its supply pin",
            Check::BoardFitsBay => "the board outline fits the bay the spec names",
            Check::Erc => "kicad-cli ERC reports 0 errors",
            Check::Drc => "kicad-cli DRC (zones refilled) reports 0 errors and 0 unconnected",
            Check::Parity => "kicad-cli schematic parity reports 0 issues",
            Check::Budget => "every BOM line is priced and the total is within budget",
        }
    }
}

// ------------------------------------------------------------ the judge

use crate::templates::{self, Template};
use crate::{Design, Spec};
use wright_core::{Finished, Report, Source};
use wright_eda::ir::{NetRole, PartKind};
use wright_eda::{Netlist, Pt};

/// Decoupling loop limit per cap, mm: rail pad to the IC supply pin plus
/// ground pad to the IC ground pin (baseline `test_board_decoupling._loop_mm`).
/// A Wright rule, not a datasheet figure (TI SLVS757 section 10.1 asks for
/// the input cap "as close as possible" without a number); the Python baseline's boards
/// measured 9.2-60.9 mm summed (docs/measurements placer-2026-10-24).
pub const DECAP_LOOP_MAX_MM: f64 = 10.0;
/// Regulator setpoint tolerance against the requirement (ASSUMPTION: 2 %,
/// the 1 % divider plus the reference's initial accuracy, nominal values).
pub const SETPOINT_TOL: f64 = 0.02;
/// Capacitor voltage derating: rated >= 1.5 x the most it sees (plan
/// section 6, lane E; common ceramic DC-bias practice).
pub const CAP_DERATE: f64 = 1.5;
/// TPS5450 switching frequency, Hz (SLVS757: fixed 500 kHz).
pub const TPS5450_FSW_HZ: f64 = 500e3;

/// Run every check. kicad-cli runs on files freshly written from this
/// design into a temp directory, so its verdicts belong to these bytes.
pub fn check(spec: &Spec, d: &Design) -> Finished {
    let mut r: Report<Check> = Report::new();
    let n = &d.netlist;
    netlist_valid(&mut r, d);
    pins_accounted(&mut r, n, d);
    footprints_resolved(&mut r, n, d);
    courtyard(&mut r, n, d);
    routed(&mut r, n, d);
    copper(&mut r, n, d);
    ampacity(&mut r, spec, n, d);
    setpoint(&mut r, spec, n);
    ratings(&mut r, spec, n, d);
    decoupling(&mut r, n, d);
    bay(&mut r, spec, d);
    kicad(&mut r, d);
    budget(&mut r, spec, d);
    r.finish()
}

fn netlist_valid(r: &mut Report<Check>, d: &Design) {
    match d.circuit.validate() {
        Ok(n) if n == d.netlist => r.margin(Check::NetlistValid, 0.0, "problems", Source::Requirement, format!("{} parts, {} nets, every terminal resolved", n.parts.len(), n.nets.len())),
        Ok(_) => r.margin(Check::NetlistValid, -1.0, "problems", Source::Requirement, "the design's netlist is not the one its circuit validates to"),
        Err(e) => r.margin(Check::NetlistValid, -(e.problems.len() as f64), "problems", Source::Requirement, e.to_string()),
    }
}

fn pins_accounted(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    let mut bad: Vec<String> = n.parts.iter().flat_map(|p| p.pins.iter().filter(|q| q.net.is_none() && !q.no_connect).map(move |q| format!("{}.{}", p.refdes, q.name))).collect();
    // every part on the board exactly once
    for (i, p) in n.parts.iter().enumerate() {
        let k = d.board.parts.iter().filter(|b| b.part == i).count();
        if k != 1 {
            bad.push(format!("{} placed {k} times", p.refdes));
        }
    }
    let nc: usize = n.parts.iter().map(|p| p.pins.iter().filter(|q| q.no_connect).count()).sum();
    let detail = if bad.is_empty() { format!("every pin on a net or declared no_connect ({nc} no_connect), every part placed once") } else { bad.join(", ") };
    r.margin(Check::PinsAccounted, -(bad.len() as f64), "pins", Source::Requirement, detail);
}

fn footprints_resolved(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    if d.board.parts.is_empty() {
        r.missing(Check::FootprintsResolved, Source::Geometry, "the board has no parts");
        return;
    }
    let mut bad = Vec::new();
    for b in &d.board.parts {
        let Some(p) = n.parts.get(b.part) else {
            bad.push(format!("board part {} is not in the netlist", b.part));
            continue;
        };
        let f = &b.footprint;
        if !f.lib_id.contains(':') || f.source.trim().is_empty() {
            bad.push(format!("{}: {:?} is not a KiCad library footprint with its .kicad_mod text", p.refdes, f.lib_id));
        }
        for q in &p.pins {
            if !f.pads.iter().any(|x| x.number == q.pad) {
                bad.push(format!("{}: {} has no pad {}", p.refdes, f.lib_id, q.pad));
            }
        }
    }
    let detail = if bad.is_empty() { format!("{} parts on KiCad library footprints, every pin's pad present", d.board.parts.len()) } else { bad.join("; ") };
    r.margin(Check::FootprintsResolved, -(bad.len() as f64), "parts", Source::Geometry, detail);
}

fn gap(a: &wright_eda::Rect, b: &wright_eda::Rect) -> f64 {
    // separation (>= 0) or minus the overlap depth
    let sx = (a.min.x.0 - b.max.x.0).max(b.min.x.0 - a.max.x.0) as f64;
    let sy = (a.min.y.0 - b.max.y.0).max(b.min.y.0 - a.max.y.0) as f64;
    if sx < 0.0 && sy < 0.0 {
        sx.max(sy) / 1e6
    } else {
        sx.max(0.0).hypot(sy.max(0.0)) / 1e6
    }
}

fn courtyard(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    let b = &d.board;
    if b.parts.is_empty() {
        r.missing(Check::CourtyardClearance, Source::Geometry, "the board has no parts");
        return;
    }
    let refd = |i: usize| n.parts.get(b.parts[i].part).map(|p| p.refdes.clone()).unwrap_or_default();
    let mut worst = (f64::INFINITY, String::new());
    let mut overlaps = 0;
    for i in 0..b.parts.len() {
        let ci = b.parts[i].courtyard_abs();
        // inside the outline by the edge clearance
        let o = b.outline;
        let edge = [ci.min.x.0 - o.min.x.0, ci.min.y.0 - o.min.y.0, o.max.x.0 - ci.max.x.0, o.max.y.0 - ci.max.y.0].into_iter().min().unwrap_or(0) as f64 / 1e6;
        if edge < worst.0 {
            worst = (edge, format!("{} courtyard {edge:.3} mm inside the outline", refd(i)));
        }
        for j in i + 1..b.parts.len() {
            if b.parts[i].side != b.parts[j].side {
                continue;
            }
            let g = gap(&ci, &b.parts[j].courtyard_abs());
            if g < 0.0 {
                overlaps += 1;
            }
            if g < worst.0 {
                worst = (g, format!("{} - {} courtyards {g:.3} mm apart", refd(i), refd(j)));
            }
        }
    }
    r.margin(Check::CourtyardClearance, worst.0, "mm", Source::Geometry, format!("closest: {}; {overlaps} overlapping pairs of {} parts", worst.1, b.parts.len()));
}

fn routed(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    let rt = &d.route;
    let need: Vec<usize> = n.nets.iter().enumerate().filter(|(_, x)| x.pins.len() >= 2).map(|(i, _)| i).collect();
    let mut bad: Vec<String> = Vec::new();
    for &i in &need {
        if !rt.attempted.contains(&i) && !rt.poured.contains(&i) {
            bad.push(format!("{} never attempted", n.nets[i].name));
        }
    }
    for u in &rt.unrouted {
        bad.push(format!("{} unrouted: {}", u.name, u.reason));
    }
    for &i in &rt.attempted {
        if !rt.routed.contains(&i) && !rt.poured.contains(&i) && !rt.unrouted.iter().any(|u| u.net == i) {
            bad.push(format!("{} attempted but neither routed, poured nor reported", n.nets.get(i).map(|x| x.name.as_str()).unwrap_or("?")));
        }
    }
    if need.is_empty() {
        r.missing(Check::Routed, Source::Geometry, "the netlist has no net with two pads");
        return;
    }
    let done = need.iter().filter(|i| (rt.routed.contains(i) || rt.poured.contains(i)) && !rt.unrouted.iter().any(|u| u.net == **i)).count();
    let detail = if bad.is_empty() { format!("{done}/{} nets routed or poured ({} poured)", need.len(), rt.poured.len()) } else { format!("{done}/{} nets: {}", need.len(), bad.join("; ")) };
    r.margin(Check::Routed, -(bad.len() as f64), "nets", Source::Geometry, detail);
}

fn copper(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    match crate::copper::clearance(n, &d.board) {
        None => r.missing(Check::CopperClearance, Source::Geometry, "no copper of two different nets to measure"),
        Some(w) => r.margin(
            Check::CopperClearance,
            w.margin_mm,
            "mm",
            Source::Geometry,
            format!("closest different-net copper: {} - {} {:.3} mm (limit {:.3} mm); {} violating pairs; {} tracks, {} vias recomputed from the emitted shapes", w.a, w.b, w.gap_mm, w.limit_mm, w.violations, d.board.tracks.len(), d.board.vias.len()),
        ),
    }
}

/// True when the track lies inside one pad of its own net (a neck-down in
/// the pad's own area, which the plan allows).
fn inside_own_pad(n: &Netlist, d: &Design, t: &wright_eda::Track) -> bool {
    let inside = |p: Pt, r: &wright_eda::Rect| p.x >= r.min.x && p.x <= r.max.x && p.y >= r.min.y && p.y <= r.max.y;
    d.board.parts.iter().any(|b| {
        let Some(part) = n.parts.get(b.part) else { return false };
        b.footprint.pads.iter().any(|pad| {
            let on = part.pins.iter().any(|q| q.pad == pad.number && q.net == Some(t.net));
            let bb = b.pad_bbox_abs(pad);
            on && inside(t.a, &bb) && inside(t.b, &bb)
        })
    })
}

/// Design currents from the spec's requirements (so a changed requirement
/// is judged against the copper already laid), else the design's own.
fn currents(spec: &Spec, d: &Design) -> Vec<f64> {
    match &spec.template {
        Some(t) => templates::net_currents(Some(t), &d.netlist),
        None => d.net_current_a.clone(),
    }
}

/// Longest neck-down stub the short-neck rule accepts, mm. A Wright rule
/// (lane E, 10/24), not an IPC figure: IPC-2221's curve is for long
/// conductors, a stub this short between a pad and full-width copper is
/// cooled by both; it must also be no narrower than the pad it enters, so
/// the constriction is the part's own pad, which the part's rating covers.
pub const NECK_MAX_MM: f64 = 2.0;

/// Class width of net `i` (the router's target width), else the board minimum.
fn class_width(d: &Design, i: usize) -> wright_eda::Nm {
    d.board.classes.iter().find(|c| c.nets.contains(&i)).map(|c| c.track_width.max(d.board.rules.min_track)).unwrap_or(d.board.rules.min_track)
}

/// The own-net pad a track end sits in: `(refdes.pad, short side)`.
fn pad_at(n: &Netlist, d: &Design, net: usize, p: Pt) -> Option<(String, wright_eda::Nm)> {
    let inside = |p: Pt, r: &wright_eda::Rect| p.x >= r.min.x && p.x <= r.max.x && p.y >= r.min.y && p.y <= r.max.y;
    d.board.parts.iter().find_map(|b| {
        let part = n.parts.get(b.part)?;
        b.footprint.pads.iter().find_map(|pad| {
            let on = part.pins.iter().any(|q| q.pad == pad.number && q.net == Some(net));
            (on && inside(p, &b.pad_bbox_abs(pad))).then(|| (format!("{}.{}", part.refdes, pad.number), pad.size.0.min(pad.size.1)))
        })
    })
}

/// Trace ampacity, measured on the copper that carries the current:
///
/// * a **tracked** net: its body width (narrowest track at or above its
///   class width, i.e. away from the pads) against the design current, plus
///   the short-neck rule on every narrower stub ([`NECK_MAX_MM`], no
///   narrower than the pad it enters); a narrow track that is not such a
///   stub counts as body;
/// * a **poured** net (a zone on it, listed `poured`): the current crosses
///   into the pour at each connector pad on it, through the stitching vias
///   within 1.5 mm (IPC-2221 internal on the barrel,
///   [`crate::copper::via_ampacity_a`]) plus, for a through-hole pad, its
///   thermal relief (2 spokes, KiCad's minimum). Each such pad must carry
///   its connector's return current: the design currents of the
///   connector's other nets, each split evenly over the connectors on
///   that net. Fan-out tracks are not judged; the regulator's internal
///   return loop is not modelled (said in the detail).
fn ampacity(r: &mut Report<Check>, spec: &Spec, n: &Netlist, d: &Design) {
    let oz = d.board.rules.copper_oz;
    let cur = currents(spec, d);
    let mut worst: Option<(f64, String)> = None;
    let mut unjudged = Vec::new();
    let mut necks = Vec::new();
    let mut pours = Vec::new();
    let mut take = |m: f64, s: String| {
        if worst.as_ref().is_none_or(|(w, _)| m < *w) {
            worst = Some((m, s));
        }
    };
    for (i, net) in n.nets.iter().enumerate() {
        if net.pins.len() < 2 {
            continue;
        }
        let amps = cur.get(i).copied().unwrap_or(f64::NAN);
        if !amps.is_finite() {
            unjudged.push(format!("{}: no design current", net.name));
            continue;
        }
        let zone = d.board.zones.iter().find(|z| z.net == i);
        if let (Some(z), true) = (zone, d.route.poured.contains(&i)) {
            let spoke = crate::copper::ampacity_a(z.thermal_spoke.as_mm(), oz);
            // connectors on a net share its current evenly
            let conns_on = |k: usize| n.parts.iter().filter(|p| p.kind == PartKind::Connector && p.pins.iter().any(|q| q.net == Some(k))).count().max(1) as f64;
            let mut judged = 0usize;
            for b in &d.board.parts {
                let Some(part) = n.parts.get(b.part) else { continue };
                if part.kind != PartKind::Connector {
                    continue;
                }
                let mine: Vec<&wright_eda::Pad> = b.footprint.pads.iter().filter(|pad| part.pins.iter().any(|q| q.pad == pad.number && q.net == Some(i))).collect();
                if mine.is_empty() {
                    continue;
                }
                // the return current = what the connector's other pins carry
                let mut others: Vec<usize> = part.pins.iter().filter_map(|q| q.net).filter(|&k| k != i).collect();
                others.sort_unstable();
                others.dedup();
                let share = others.iter().map(|&k| cur.get(k).copied().unwrap_or(f64::NAN) / conns_on(k)).sum::<f64>() / mine.len() as f64;
                if !share.is_finite() {
                    unjudged.push(format!("{}: {} return current unknown", net.name, part.refdes));
                    continue;
                }
                for pad in mine {
                    let bb = b.pad_bbox_abs(pad);
                    let reach = 1_500_000;
                    let near = |v: &&wright_eda::Via| v.net == i && v.at.x.0 >= bb.min.x.0 - reach && v.at.x.0 <= bb.max.x.0 + reach && v.at.y.0 >= bb.min.y.0 - reach && v.at.y.0 <= bb.max.y.0 + reach;
                    let vias: Vec<f64> = d.board.vias.iter().filter(near).map(|v| crate::copper::via_ampacity_a(v.drill.as_mm())).collect();
                    let tht = pad.drill.is_some() && pad.plated && b.pad_layers(pad).contains(&z.layer);
                    let cap = vias.iter().sum::<f64>() + if tht { 2.0 * spoke } else { 0.0 };
                    let how = match (tht, vias.len()) {
                        (true, 0) => format!("2 x {} mm thermal spokes", z.thermal_spoke.kicad()),
                        (true, k) => format!("2 x {} mm spokes + {k} vias", z.thermal_spoke.kicad()),
                        (false, k) => format!("{k} stitching vias within 1.5 mm"),
                    };
                    judged += 1;
                    pours.push(format!("{}.{} {how} {cap:.2} A vs {share:.2} A", part.refdes, pad.number));
                    take(cap - share, format!("{} return at {}.{} into the {:?} pour: {how} carry {cap:.2} A, its return current {share:.2} A", net.name, part.refdes, pad.number, z.layer));
                }
            }
            if judged == 0 {
                unjudged.push(format!("{}: poured, no connector pad on it to judge", net.name));
            }
            continue;
        }
        let cw = class_width(d, i);
        let mut body: Option<wright_eda::Nm> = None;
        for t in d.board.tracks.iter().filter(|t| t.net == i && !inside_own_pad(n, d, t)) {
            let len = t.a.dist(t.b) / 1e6;
            let stub = if t.width < cw { pad_at(n, d, i, t.a).or_else(|| pad_at(n, d, i, t.b)) } else { None };
            match stub {
                Some((pad, short)) if len <= NECK_MAX_MM + 1e-6 && t.width.0 + 1_000 >= short.0 => {
                    necks.push(format!("{} {} mm x {len:.2} mm into {pad}", net.name, t.width.kicad()));
                }
                Some((pad, short)) => {
                    take(-(len - NECK_MAX_MM).max(0.0) - (short.0 - t.width.0).max(0) as f64 / 1e6, format!("{} neck {} mm x {len:.2} mm into {pad} (pad short side {} mm) breaks the short-neck rule (<= {NECK_MAX_MM} mm long, >= the pad)", net.name, t.width.kicad(), short.kicad()));
                }
                None => body = Some(body.map_or(t.width, |b| b.min(t.width))),
            }
        }
        let (cap, how) = match (body, zone) {
            (Some(w), _) => (crate::copper::ampacity_a(w.as_mm(), oz), format!("{} mm body track", w.kicad())),
            // a poured-only net the router did not list: its narrowest copper
            // is a thermal relief, two spokes (KiCad's default minimum)
            (None, Some(z)) => (2.0 * crate::copper::ampacity_a(z.thermal_spoke.as_mm(), oz), format!("pour, 2 x {} mm thermal spokes", z.thermal_spoke.kicad())),
            (None, None) if !necks.is_empty() => continue,
            (None, None) => {
                unjudged.push(format!("{}: no copper on the board", net.name));
                continue;
            }
        };
        take(cap - amps, format!("{}: {how} carries {cap:.2} A at a 20 C rise, design current {amps:.2} A", net.name));
        // a layer change on a tracked net: any one via may carry the whole
        // current (vias are not known to be in parallel), so the weakest counts
        if let Some(v) = d.board.vias.iter().filter(|v| v.net == i).min_by_key(|v| v.drill.0) {
            let k = d.board.vias.iter().filter(|v| v.net == i).count();
            let vc = crate::copper::via_ampacity_a(v.drill.as_mm());
            take(vc - amps, format!("{}: {k} via(s), the weakest {} mm drill carries {vc:.2} A (IPC-2221 internal, 20 um plating), design current {amps:.2} A", net.name, v.drill.kicad()));
        }
    }
    let assumed = if spec.template.is_none() { "; ASSUMPTION: agent circuit, 0.5 A per power net, 0.1 A per signal" } else { "" };
    let mut neck_note = if necks.is_empty() { String::new() } else { format!("; {} necks within the short-neck rule: {}", necks.len(), necks.join(", ")) };
    if !pours.is_empty() {
        neck_note.push_str(&format!("; poured nets judged where current enters the pour at each connector (vias IPC-2221 internal, 20 um plating; thermal reliefs 2 spokes, KiCad's minimum; fan-outs not judged; the regulator's internal return loop is not modelled): {}", pours.join(", ")));
    }
    match (worst, unjudged.is_empty()) {
        (Some((m, s)), true) => r.margin(Check::TraceAmpacity, m, "A", Source::Physics, format!("tightest {s} (IPC-2221, {oz} oz){neck_note}{assumed}")),
        (_, false) => r.missing(Check::TraceAmpacity, Source::Physics, format!("cannot judge {}", unjudged.join("; "))),
        (None, true) => r.missing(Check::TraceAmpacity, Source::Physics, "no net with two pads"),
    }
}

/// `(R1, R2, Vout)` of the first TPS5450's VSENSE divider: exactly one
/// resistor from VSENSE to a non-ground net (R1) and one to ground (R2),
/// Vout = 1.221 x (1 + R1/R2) (SLVS757). Same reading as the Haskell
/// oracle's `Oracle.Pcb.regulatorVout`.
pub fn regulator_vout(n: &Netlist) -> Result<(f64, f64, f64), String> {
    let u = n.parts.iter().find(|p| p.value.starts_with("TPS5450") || p.name.starts_with("TPS5450")).ok_or("no TPS5450 in the netlist")?;
    let fb = pin_net(u, "VSENSE").ok_or("TPS5450 VSENSE is on no net")?;
    let mut up = Vec::new();
    let mut down = Vec::new();
    for p in n.parts.iter().filter(|p| p.kind == PartKind::Resistor && p.pins.iter().any(|q| q.net == Some(fb))) {
        let Some(v) = templates::parse_ohm(&p.value) else { continue };
        for o in p.pins.iter().filter_map(|q| q.net).filter(|&x| x != fb) {
            if n.nets[o].role == NetRole::Ground {
                down.push(v)
            } else {
                up.push(v)
            }
        }
    }
    match (up.as_slice(), down.as_slice()) {
        ([r1], [r2]) => Ok((*r1, *r2, templates::tps5450_vout(*r1, *r2))),
        _ => Err(format!("VSENSE divider is not one resistor to the output and one to ground ({} up, {} down)", up.len(), down.len())),
    }
}

/// Part (by circuit name prefix) and its pin-name -> net index.
fn find_ic<'a>(n: &'a Netlist, prefix: &str) -> Option<&'a wright_eda::ir::Part> {
    n.parts.iter().find(|p| p.kind == PartKind::Ic && p.name.starts_with(prefix))
}

fn pin_net(p: &wright_eda::ir::Part, pin: &str) -> Option<usize> {
    p.pins.iter().find(|q| q.name == pin).and_then(|q| q.net)
}

fn setpoint(r: &mut Report<Check>, spec: &Spec, n: &Netlist) {
    let Some(t) = &spec.template else {
        r.missing(Check::RegulatorSetpoint, Source::Physics, "agent circuit: no regulator rule for it (only template circuits have one)");
        return;
    };
    let (target, got, how) = match *t {
        Template::ServoPower { vout_v, .. } => {
            let (r1, r2, v) = match regulator_vout(n) {
                Ok(x) => x,
                Err(e) => {
                    r.missing(Check::RegulatorSetpoint, Source::Physics, e);
                    return;
                }
            };
            (vout_v, v, format!("TPS5450 Vout = 1.221 x (1 + {} / {}) = {v:.3} V (SLVS757)", templates::ohm_text(r1), templates::ohm_text(r2)))
        }
        Template::Ldo { vout_v, .. } => {
            let Some(u) = find_ic(n, "AMS1117-") else {
                r.missing(Check::RegulatorSetpoint, Source::Physics, "no AMS1117 on the board");
                return;
            };
            let Some(v) = u.name.trim_start_matches("AMS1117-").parse::<f64>().ok() else {
                r.missing(Check::RegulatorSetpoint, Source::Physics, format!("{} names no fixed output voltage", u.name));
                return;
            };
            (vout_v, v, format!("{} fixed output {v} V", u.name))
        }
    };
    let err = (got - target).abs() / target;
    r.margin(Check::RegulatorSetpoint, SETPOINT_TOL - err, "fraction", Source::Physics, format!("{how}; requirement {target} V, error {:.2} % (limit {:.0} %)", err * 100.0, SETPOINT_TOL * 100.0));
}

fn ratings(r: &mut Report<Check>, spec: &Spec, n: &Netlist, d: &Design) {
    let Some(t) = &spec.template else {
        r.missing(Check::PartRatings, Source::Rating, "agent circuit: no load model to rate parts against");
        return;
    };
    let lib_of = |i: usize| d.board.parts.iter().find(|b| b.part == i).map(|b| b.footprint.lib_id.clone()).unwrap_or_default();
    let mut items: Vec<(f64, String)> = Vec::new(); // (rating / need - 1, text)
    let mut missing: Vec<String> = Vec::new();
    fn need(items: &mut Vec<(f64, String)>, missing: &mut Vec<String>, p: &wright_eda::ir::Part, row: Option<&crate::catalog::Row>, key: &str, load: f64, what: &str) {
        match row.and_then(|x| x.rating(key)) {
            Some(v) => items.push((v / load - 1.0, format!("{} {what}: {key} {v} vs {load:.2}", p.refdes))),
            None => missing.push(format!("{} {key} not on file ({})", p.refdes, row.map(|x| x.rating_source.as_str()).unwrap_or("no catalog row"))),
        }
    }
    let currents = &currents(spec, d);
    let (vin_max, vout, iout) = match *t {
        Template::ServoPower { vin_max_v, vout_v, iout_a, .. } => (vin_max_v, vout_v, iout_a),
        Template::Ldo { vin_v, vout_v, iout_a } => (vin_v, vout_v, iout_a),
    };
    // the most each net sees, V (switch node and bootstrap bounded by VIN)
    let volts = |i: usize| -> f64 {
        let x = &n.nets[i];
        if x.role == NetRole::Ground {
            0.0
        } else if x.name == templates::output_net(t) {
            vout
        } else {
            vin_max
        }
    };
    for (pi, p) in n.parts.iter().enumerate() {
        let row = crate::catalog::lookup(p, &lib_of(pi));
        match p.kind {
            PartKind::Ic => {
                need(&mut items, &mut missing, p, row, "iout_max_a", iout, "regulator output");
                need(&mut items, &mut missing, p, row, "vin_max_v", vin_max, "input");
            }
            PartKind::Inductor => {
                let ripple = match *t {
                    Template::ServoPower { .. } => {
                        let lh = parse_henry(&p.value).unwrap_or(f64::NAN);
                        (vin_max - vout) * vout / (vin_max * lh * TPS5450_FSW_HZ)
                    }
                    Template::Ldo { .. } => 0.0,
                };
                need(&mut items, &mut missing, p, row, "isat_a", iout + ripple / 2.0, "peak current (Iout + ripple/2 at Vin max, 500 kHz)");
            }
            PartKind::Diode => {
                need(&mut items, &mut missing, p, row, "if_a", iout, "forward current (full load, conservative)");
                need(&mut items, &mut missing, p, row, "vrrm_v", vin_max, "reverse voltage");
                // cathode (KiCad pad 1) on the switch node for a catch diode
                if let (Some(k), Some(sw)) = (p.pins.iter().find(|q| q.pad == "1").and_then(|q| q.net), n.net_index("SW")) {
                    if k != sw && p.pins.iter().any(|q| q.net == Some(sw)) {
                        items.push((-1.0, format!("{} cathode (pad 1) is on {}, not SW: the catch diode is reversed", p.refdes, n.nets[k].name)));
                    }
                }
            }
            PartKind::Capacitor => {
                let nets: Vec<usize> = p.pins.iter().filter_map(|q| q.net).collect();
                // conservative: the highest net voltage on either pin, the other side at 0 V
                let v = nets.iter().map(|&i| volts(i)).fold(0.0, f64::max);
                need(&mut items, &mut missing, p, row, "voltage_v", (CAP_DERATE * v).max(1e-9), &format!("{CAP_DERATE} x {v:.1} V"));
            }
            PartKind::Connector => {
                // per contact: each net's current over that net's connector pins
                let share = p
                    .pins
                    .iter()
                    .filter_map(|q| q.net)
                    .map(|i| {
                        let k = n.nets[i].pins.iter().filter(|pr| n.parts[pr.part].kind == PartKind::Connector).count().max(1);
                        currents.get(i).copied().unwrap_or(f64::NAN) / k as f64
                    })
                    .fold(0.0, f64::max);
                let key = if row.is_some_and(|x| x.ratings.contains_key("current_a")) { "current_a" } else { "current_per_pin_a" };
                need(&mut items, &mut missing, p, row, key, share, "contact current");
            }
            _ => {}
        }
    }
    if !missing.is_empty() {
        r.missing(Check::PartRatings, Source::Rating, format!("{} of {} ratings not on file: {}", missing.len(), missing.len() + items.len(), missing.join("; ")));
        return;
    }
    let Some(w) = items.iter().min_by(|a, b| a.0.total_cmp(&b.0)) else {
        r.missing(Check::PartRatings, Source::Rating, "no rated part on the board");
        return;
    };
    r.margin(Check::PartRatings, w.0, "ratio", Source::Rating, format!("tightest: {}; {} ratings checked", w.1, items.len()));
}

/// `15uH` -> 1.5e-5 (baseline `footprints.py:908` `_parse_value`).
pub fn parse_henry(text: &str) -> Option<f64> {
    let t = text.split_whitespace().next()?;
    let t = t.trim_end_matches(['H', 'h']);
    let (num, m) = match t.chars().last()? {
        'u' | 'µ' => (&t[..t.len() - t.chars().last()?.len_utf8()], 1e-6),
        'n' => (&t[..t.len() - 1], 1e-9),
        'm' => (&t[..t.len() - 1], 1e-3),
        _ => (t, 1.0),
    };
    num.parse::<f64>().ok().map(|v| v * m).filter(|v| *v > 0.0)
}

fn decoupling(r: &mut Report<Check>, n: &Netlist, d: &Design) {
    let b = &d.board;
    let pads = |part: usize, net: usize| -> Vec<Pt> {
        let Some(bp) = b.parts.iter().find(|x| x.part == part) else { return vec![] };
        let p = &n.parts[part];
        bp.footprint.pads.iter().filter(|pad| p.pins.iter().any(|q| q.pad == pad.number && q.net == Some(net))).map(|pad| bp.pad_abs(pad)).collect()
    };
    let nearest = |a: &[Pt], c: &[Pt]| a.iter().flat_map(|x| c.iter().map(move |y| x.dist(*y))).fold(f64::INFINITY, f64::min) / 1e6;
    let mut loops: Vec<(f64, String)> = Vec::new();
    let mut bare: Vec<String> = Vec::new();
    for (ui, u) in n.parts.iter().enumerate().filter(|(_, p)| p.kind == PartKind::Ic) {
        let grounds: Vec<usize> = u.pins.iter().filter_map(|q| q.net).filter(|&i| n.nets[i].role == NetRole::Ground).collect();
        let Some(&g) = grounds.first() else { continue };
        let mut rails: Vec<usize> = u.pins.iter().filter_map(|q| q.net).filter(|&i| n.nets[i].role == NetRole::Power).collect();
        rails.sort();
        rails.dedup();
        for rail in rails {
            let caps: Vec<usize> = n.parts.iter().enumerate().filter(|(_, c)| c.kind == PartKind::Capacitor && c.pins.iter().any(|q| q.net == Some(rail)) && c.pins.iter().any(|q| q.net == Some(g))).map(|(i, _)| i).collect();
            if caps.is_empty() {
                bare.push(format!("{} {} has no decoupling cap", u.refdes, n.nets[rail].name));
            }
            for c in caps {
                let l = nearest(&pads(c, rail), &pads(ui, rail)) + nearest(&pads(c, g), &pads(ui, g));
                loops.push((l, format!("{} -> {} {}", n.parts[c].refdes, u.refdes, n.nets[rail].name)));
            }
        }
    }
    if loops.is_empty() && bare.is_empty() {
        r.missing(Check::DecouplingLoop, Source::Geometry, "no IC supply pin on a power net to decouple");
        return;
    }
    if loops.iter().any(|(l, _)| !l.is_finite()) {
        r.missing(Check::DecouplingLoop, Source::Geometry, "a decoupling cap or its IC is not on the board");
        return;
    }
    let total: f64 = loops.iter().map(|x| x.0).sum();
    let worst = loops.iter().max_by(|a, b| a.0.total_cmp(&b.0));
    let mut m = worst.map(|w| DECAP_LOOP_MAX_MM - w.0).unwrap_or(f64::INFINITY);
    if !bare.is_empty() {
        m = m.min(-DECAP_LOOP_MAX_MM);
    }
    let detail = format!(
        "{}{}; total {total:.1} mm over {} caps (limit {DECAP_LOOP_MAX_MM} mm per cap)",
        worst.map(|w| format!("worst {} {:.1} mm", w.1, w.0)).unwrap_or_default(),
        if bare.is_empty() { String::new() } else { format!("; {}", bare.join("; ")) },
        loops.len()
    );
    r.margin(Check::DecouplingLoop, m, "mm", Source::Geometry, detail);
}

fn bay(r: &mut Report<Check>, spec: &Spec, d: &Design) {
    let Some((bw, bh)) = spec.max_board_mm else {
        r.missing(Check::BoardFitsBay, Source::Requirement, "the spec names no bay size (max_board_mm)");
        return;
    };
    let (w, h) = (d.board.outline.width().as_mm(), d.board.outline.height().as_mm());
    let straight = (bw - w).min(bh - h);
    let turned = (bw - h).min(bh - w);
    let m = straight.max(turned);
    r.margin(Check::BoardFitsBay, m, "mm", Source::Requirement, format!("board {w:.1} x {h:.1} mm in a {bw} x {bh} mm bay{}", if turned > straight { " (turned 90 degrees)" } else { "" }));
}

/// kicad-cli verdicts for this design, cached by the bytes written (the
/// writers are deterministic, so the same design gives the same files).
pub fn kicad_verdicts(d: &Design) -> Result<Vec<wright_kicad::CliVerdict>, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<u64, Vec<wright_kicad::CliVerdict>>>> = Mutex::new(None);
    let files = crate::pipeline::kicad_texts(d).map_err(|e| format!("{e:#}"))?;
    let mut h = DefaultHasher::new();
    files.hash(&mut h);
    wright_kicad::kicad_cli().hash(&mut h);
    let key = h.finish();
    if let Some(v) = CACHE.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&key).cloned())) {
        return Ok(v);
    }
    let dir = std::env::temp_dir().join(format!("wright-electronics-{key:016x}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (pcb, sch) = crate::pipeline::write_texts(&dir, &files).map_err(|e| format!("{e:#}"))?;
    let v = wright_kicad::verify(&pcb, &sch);
    if v.iter().all(|x| x.ran) {
        if let Ok(mut c) = CACHE.lock() {
            c.get_or_insert_with(HashMap::new).insert(key, v.clone());
        }
    }
    Ok(v)
}

fn kicad(r: &mut Report<Check>, d: &Design) {
    let v = match kicad_verdicts(d) {
        Ok(v) => v,
        Err(e) => {
            for c in [Check::Erc, Check::Drc, Check::Parity] {
                r.missing(c, Source::Simulation, format!("KiCad files could not be written: {e}"));
            }
            return;
        }
    };
    for (c, name) in [(Check::Erc, "erc"), (Check::Drc, "drc"), (Check::Parity, "parity")] {
        let Some(x) = v.iter().find(|x| x.check == name) else {
            r.missing(c, Source::Simulation, format!("kicad-cli returned no {name} verdict"));
            continue;
        };
        if !x.ran {
            r.missing(c, Source::Simulation, format!("kicad-cli {name} did not run: {}", x.details.join("; ")));
            continue;
        }
        let bad = x.errors + if name == "drc" { x.unconnected } else { 0 };
        let first = if x.details.is_empty() { String::new() } else { format!("; {}", x.details.iter().take(4).cloned().collect::<Vec<_>>().join("; ")) };
        let ver = x.tool_version.as_deref().unwrap_or("?");
        let detail = match name {
            "drc" => format!("kicad-cli {ver} pcb drc --refill-zones: {} errors, {} unconnected, {} warnings{first}", x.errors, x.unconnected, x.warnings),
            _ => format!("kicad-cli {ver} {name}: {} errors, {} warnings{first}", x.errors, x.warnings),
        };
        r.margin(c, -(bad as f64), "errors", Source::Simulation, detail);
    }
}

fn budget(r: &mut Report<Check>, spec: &Spec, d: &Design) {
    let catalog = crate::catalog::bom(&d.netlist, &d.board);
    let same = |b: &wright_core::Bom| b.lines.len() == catalog.lines.len() && b.lines.iter().zip(&catalog.lines).all(|(x, y)| x.id == y.id && x.qty == y.qty);
    let bom = match &d.sourced {
        Some(b) if same(b) => b.clone(),
        Some(_) => {
            r.missing(Check::Budget, Source::Requirement, "the sourced BOM's lines are not this design's lines");
            return;
        }
        None => catalog,
    };
    let unpriced: Vec<String> = bom.unpriced().iter().map(|l| format!("{} x{}", l.id, l.qty)).collect();
    if !unpriced.is_empty() {
        r.missing(Check::Budget, Source::Requirement, format!("{} of {} BOM lines unpriced (no retrieved price): {}", unpriced.len(), bom.lines.len(), unpriced.join(", ")));
        return;
    }
    let total = bom.total().as_usd();
    match spec.budget_usd {
        Some(b) => r.margin(Check::Budget, b - total, "USD", Source::Requirement, format!("${total:.2} of ${b:.2}")),
        None => r.missing(Check::Budget, Source::Requirement, format!("every line priced (${total:.2}) but the spec names no budget")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_lists_every_check_once() {
        let mut names: Vec<_> = Check::ALL.iter().map(|c| c.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Check::ALL.len());
        assert_eq!(Check::ALL.len(), 15);
    }
}
