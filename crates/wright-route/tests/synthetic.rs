//! Offline tests on hand-built boards (no KiCad library needed).

mod common;
use wright_eda::ir::{Net, NetRole, Part, PartKind, Pin, PinRef};
use wright_eda::{Board, NetClass, Netlist, Nm, Placed, Pt, Rect, Rules, Side};
use wright_route::{route, route_with_stats, verify, QuickScore, RouteOptions};

fn part(refdes: &str, kind: PartKind, pins: &[(&str, Option<usize>)]) -> Part {
    Part {
        refdes: refdes.into(),
        name: refdes.into(),
        kind,
        value: "x".into(),
        package: None,
        mpn: None,
        pins: pins.iter().map(|(p, n)| Pin { name: p.to_string(), pad: p.to_string(), net: *n, no_connect: n.is_none() }).collect(),
    }
}

fn net(name: &str, role: NetRole, pins: &[(usize, &str)]) -> Net {
    Net { name: name.into(), role, pins: pins.iter().map(|(p, pad)| PinRef { part: *p, pad: pad.to_string() }).collect() }
}

fn board(parts: Vec<Placed>, w: f64, h: f64, classes: Vec<NetClass>) -> Board {
    Board { outline: Rect::from_corners(Pt::mm(0.0, 0.0), Pt::mm(w, h)), parts, tracks: vec![], vias: vec![], zones: vec![], classes, rules: Rules::default(), holes: vec![] }
}

fn at(part: usize, fp: wright_eda::Footprint, x: f64, y: f64, rot: i32) -> Placed {
    Placed { part, footprint: fp, at: Pt::mm(x, y), rot, side: Side::Top, locked: false }
}

/// Two resistors in series between two header pins, plus ground.
fn small() -> (Netlist, Board) {
    let nl = Netlist {
        parts: vec![
            part("J1", PartKind::Connector, &[("1", Some(0)), ("2", Some(2))]),
            part("R1", PartKind::Resistor, &[("1", Some(0)), ("2", Some(1))]),
            part("R2", PartKind::Resistor, &[("1", Some(1)), ("2", Some(2))]),
        ],
        nets: vec![net("VIN", NetRole::Power, &[(0, "1"), (1, "1")]), net("MID", NetRole::Signal, &[(1, "2"), (2, "1")]), net("GND", NetRole::Ground, &[(2, "2"), (0, "2")])],
    };
    let rules = Rules::default();
    let cls = wright_route::ipc::classes_for_currents(&nl, &rules, &[3.0, 0.1, 0.1]);
    let b = board(vec![at(0, common::header(2), 3.0, 3.0, 0), at(1, common::chip0603(), 10.0, 3.0, 0), at(2, common::chip0603(), 14.0, 6.0, 1)], 20.0, 12.0, cls);
    (nl, b)
}

#[test]
fn routes_a_small_board_with_class_widths_and_clean_copper() {
    let (nl, mut b) = small();
    let (rep, st) = route_with_stats(&nl, &mut b, &RouteOptions::default());
    assert_eq!(rep.attempted, vec![0, 1, 2]);
    assert_eq!(rep.routed, vec![0, 1, 2], "{:?}", rep.unrouted);
    assert!(rep.unrouted.is_empty());
    assert_eq!(rep.poured, vec![2]);
    assert_eq!(b.zones.len(), 1);
    assert!(verify::connected(&nl, &b).iter().all(|&c| c));
    assert_eq!(verify::clearance_violations(&nl, &b), Vec::<String>::new());
    // VIN at 3 A gets the IPC-2221 width (0.9 mm), the signal 0.2 mm.
    let vin = st.nets.iter().find(|s| s.name == "VIN").unwrap();
    assert!((vin.body_width_mm - wright_route::ipc::width_for(3.0, 1.0, 20.0).as_mm()).abs() < 1e-9);
    assert!(b.tracks.iter().filter(|t| t.net == 1).all(|t| t.width == Nm::mm(0.2)));
}

#[test]
fn an_impossible_net_is_reported_unrouted_with_a_reason_never_routed() {
    // A 9 mm rail cannot fit an 8 mm tall board at all.
    let (nl, mut b) = small();
    b.classes[1].track_width = Nm::mm(9.0);
    b.outline = Rect::from_corners(Pt::mm(0.0, 0.0), Pt::mm(16.0, 8.0));
    let rep = route(&nl, &mut b, &RouteOptions::default());
    assert!(rep.attempted.contains(&0));
    let u = rep.unrouted.iter().find(|u| u.net == 0).expect("VIN must be unrouted");
    assert!(!u.reason.is_empty());
    assert!(!rep.routed.contains(&0));
    // nothing is reported routed without copper that joins its pads
    let joined = verify::connected(&nl, &b);
    for &n in &rep.routed {
        assert!(joined[n]);
    }
}

#[test]
fn a_missing_pad_is_unrouted_by_name() {
    let (nl, mut b) = small();
    b.parts.remove(2); // R2 not placed
    let rep = route(&nl, &mut b, &RouteOptions::default());
    let u = rep.unrouted.iter().find(|u| u.name == "MID").unwrap();
    assert!(u.reason.contains("R2.1"), "{}", u.reason);
    assert!(rep.completion() < 1.0);
}

#[test]
fn empty_outline_routes_nothing_and_says_so() {
    let (nl, mut b) = small();
    b.outline = Rect::default();
    let rep = route(&nl, &mut b, &RouteOptions::default());
    assert_eq!(rep.attempted.len(), 3);
    assert_eq!(rep.unrouted.len(), 3);
    assert_eq!(rep.completion(), 0.0);
}

/// Four nets that must cross between two 4-pin headers: needs both layers.
fn crossing() -> (Netlist, Board) {
    let nets = (0..4).map(|i| net(&format!("S{i}"), NetRole::Signal, &[(0, &(i + 1).to_string()), (1, &(4 - i).to_string())])).collect();
    let pins = |rev: bool| (0..4).map(|i| ((i + 1).to_string(), if rev { 3 - i } else { i })).collect::<Vec<_>>();
    let mk = |r: &str, rev: bool| {
        let p = pins(rev);
        part(r, PartKind::Connector, &p.iter().map(|(a, n)| (a.as_str(), Some(*n))).collect::<Vec<_>>())
    };
    let nl = Netlist { parts: vec![mk("J1", false), mk("J2", true)], nets };
    let b = board(vec![at(0, common::header(4), 3.0, 3.0, 0), at(1, common::header(4), 15.0, 3.0, 0)], 18.0, 14.0, vec![]);
    (nl, b)
}

#[test]
fn crossing_nets_route_with_vias_and_stay_clear() {
    let (nl, mut b) = crossing();
    let rep = route(&nl, &mut b, &RouteOptions::default());
    assert_eq!(rep.completion(), 1.0, "{:?}", rep.unrouted);
    assert!(verify::clearance_violations(&nl, &b).is_empty());
    assert!(b.tracks.iter().any(|t| t.layer == wright_eda::Layer::BCu));
}

#[test]
fn quick_mode_scores_in_judge_key_order_and_picks_the_routable_seed() {
    let (nl, good) = crossing();
    // A seed whose outline was shrunk under J1/J2's last pins: those pins
    // have no lattice node near them, so two nets stay open.
    let mut bad = good.clone();
    bad.outline = Rect::from_corners(Pt::mm(0.0, 0.0), Pt::mm(18.0, 8.6));
    let ranking = wright_route::judge(&nl, &[bad.clone(), good.clone()], &RouteOptions::quick());
    assert_eq!(ranking[0].seed, 1, "{ranking:?}");
    assert_eq!(ranking[0].score.blocking, 0);
    assert!(ranking[1].score.blocking > 0 && ranking[1].score.joins_needed >= ranking[1].score.blocking);
    // the full route of the winner
    let (best, b, rep, _) = wright_route::judge_and_route(&nl, &[bad, good], &RouteOptions::default()).unwrap();
    assert_eq!(best, 1);
    assert_eq!(rep.completion(), 1.0);
    assert!(!b.tracks.is_empty());
    // key order: blocking beats joins beats vias beats copper beats segments
    let s = |b, j, v, c, g| QuickScore { blocking: b, joins_needed: j, vias: v, copper_mm: c, segments: g };
    let mut v = vec![s(1, 1, 0, 0.0, 0), s(0, 0, 3, 1.0, 9), s(0, 0, 2, 9.0, 99), s(0, 0, 2, 9.0, 98), s(1, 8, 0, 0.0, 0)];
    v.sort_by_key(|x| x.key());
    assert_eq!(v, vec![s(0, 0, 2, 9.0, 98), s(0, 0, 2, 9.0, 99), s(0, 0, 3, 1.0, 9), s(1, 1, 0, 0.0, 0), s(1, 8, 0, 0.0, 0)]);
}

#[test]
fn deterministic_on_a_synthetic_board() {
    let (nl, b0) = crossing();
    let (mut a, mut b) = (b0.clone(), b0);
    route(&nl, &mut a, &RouteOptions::default());
    route(&nl, &mut b, &RouteOptions::default());
    assert_eq!(a, b);
}

#[test]
fn a_back_layer_pad_is_an_obstacle_on_its_own_layer_only() {
    use wright_eda::Layer;
    // The TI PowerPAD case: a B.Cu-only GND pad under a top-side part.
    let mut fp = common::chip0603();
    let mut back = fp.pads[0].clone();
    back.number = "9".into();
    back.at = Pt::mm(0.0, 0.0);
    back.size = (Nm::mm(6.0), Nm::mm(6.0));
    back.layers = vec![Layer::BCu];
    fp.pads.push(back);
    let nl = Netlist {
        parts: vec![part("U1", PartKind::Ic, &[("1", Some(0)), ("2", Some(1)), ("9", Some(1))])],
        nets: vec![net("A", NetRole::Signal, &[(0, "1")]), net("GND", NetRole::Ground, &[(0, "2"), (0, "9")])],
    };
    let top = board(vec![at(0, fp.clone(), 10.0, 10.0, 0)], 20.0, 20.0, vec![]);
    let (pads, _) = wright_route::copper::pads(&nl, &top);
    let p9 = pads.iter().find(|p| p.number == "9").unwrap();
    assert_eq!(p9.layers, wright_route::copper::B);
    assert_eq!(p9.net, Some(1));
    // the same part on the bottom side: its back pad is on F.Cu
    let mut bot = top.clone();
    bot.parts[0].side = Side::Bottom;
    let (pads, _) = wright_route::copper::pads(&nl, &bot);
    assert_eq!(pads.iter().find(|p| p.number == "9").unwrap().layers, wright_route::copper::F);
    // a B.Cu track of another net over it is a clearance violation
    let mut bad = top.clone();
    bad.tracks.push(wright_eda::Track { a: Pt::mm(5.0, 10.0), b: Pt::mm(15.0, 10.0), width: Nm::mm(0.2), layer: Layer::BCu, net: 0 });
    assert!(!verify::clearance_violations(&nl, &bad).is_empty());
    // and the router keeps other nets' B.Cu copper off it
    let mut b = top;
    let rep = route(&nl, &mut b, &RouteOptions::default());
    assert_eq!(rep.completion(), 1.0, "{:?}", rep.unrouted);
    assert!(verify::clearance_violations(&nl, &b).is_empty());
}

#[test]
fn stitching_and_router_vias_keep_hole_to_hole_spacing() {
    use wright_route::copper::HOLE_TO_HOLE;
    // Many F.Cu SMD ground pads packed together: each wants a stitch via,
    // and neighbours would otherwise pick the same spot.
    let n = 6;
    let parts: Vec<Part> = (0..n).map(|i| part(&format!("C{}", i + 1), PartKind::Capacitor, &[("1", Some(0)), ("2", Some(1))])).collect();
    let nets = vec![
        net("+5V", NetRole::Power, &(0..n).map(|i| (i, "1")).collect::<Vec<_>>()),
        net("GND", NetRole::Ground, &(0..n).map(|i| (i, "2")).collect::<Vec<_>>()),
    ];
    let nl = Netlist { parts, nets };
    let placed = (0..n).map(|i| at(i, common::chip0603(), 6.0, 4.0 + 1.6 * i as f64, 0)).collect();
    let mut b = board(placed, 14.0, 16.0, vec![]);
    let (rep, st) = route_with_stats(&nl, &mut b, &RouteOptions::default());
    assert_eq!(rep.completion(), 1.0, "{:?}", rep.unrouted);
    assert!(st.stitching_vias > 0);
    for (i, a) in b.vias.iter().enumerate() {
        for c in &b.vias[i + 1..] {
            let gap = a.at.dist(c.at) - (a.drill.0 + c.drill.0) as f64 / 2.0;
            assert!(gap >= HOLE_TO_HOLE - 1.0, "vias at {:?} and {:?}: {gap} nm", a.at, c.at);
        }
    }
    assert!(verify::clearance_violations(&nl, &b).is_empty(), "{:?}", verify::clearance_violations(&nl, &b));
    // A board routed twice must not get a second via on top of the first
    // (the router treats copper already on the board as fixed).
    let mut again = b.clone();
    let before = again.vias.len();
    route(&nl, &mut again, &RouteOptions::default());
    for v in &again.vias[before..] {
        for w in &again.vias[..before] {
            assert!(v.at.dist(w.at) - (v.drill.0 + w.drill.0) as f64 / 2.0 >= HOLE_TO_HOLE - 1.0);
        }
    }
    // and the self-check catches a duplicated via
    let mut dup = b.clone();
    dup.vias.push(dup.vias[0].clone());
    assert!(verify::clearance_violations(&nl, &dup).iter().any(|v| v.starts_with("hole_to_hole")));
}

#[test]
fn copper_keeps_the_board_edge_clearance_from_rules() {
    // Ground pads close to the left edge (pad edge 0.75 mm from Edge.Cuts):
    // their stitch vias and tracks must not go between the pad and the edge.
    for edge_mm in [0.5, 0.7] {
        let n = 3;
        let parts: Vec<Part> = (0..n).map(|i| part(&format!("C{}", i + 1), PartKind::Capacitor, &[("1", Some(0)), ("2", Some(1))])).collect();
        let nets = vec![net("SIG", NetRole::Signal, &(0..n).map(|i| (i, "2")).collect::<Vec<_>>()), net("GND", NetRole::Ground, &(0..n).map(|i| (i, "1")).collect::<Vec<_>>())];
        let nl = Netlist { parts, nets };
        // pad 1 centre x = 1.575 - 0.825 = 0.75 + 0.4 half-width
        let placed = (0..n).map(|i| at(i, common::chip0603(), 1.975, 3.0 + 4.0 * i as f64, 0)).collect();
        let mut b = board(placed, 10.0, 14.0, vec![]);
        b.rules.edge_clearance = Nm::mm(edge_mm);
        let rep = route(&nl, &mut b, &RouteOptions::default());
        assert_eq!(rep.completion(), 1.0, "{edge_mm}: {:?}", rep.unrouted);
        let e = b.rules.edge_clearance.0 as f64;
        for t in &b.tracks {
            let m = [t.a, t.b].iter().map(|p| (p.x.0 as f64).min(10e6 - p.x.0 as f64).min(p.y.0 as f64).min(14e6 - p.y.0 as f64)).fold(f64::MAX, f64::min) - t.width.0 as f64 / 2.0;
            assert!(m >= e - 1.0, "{edge_mm}: {} track {:.3} mm from the edge", nl.nets[t.net].name, m / 1e6);
        }
        for v in &b.vias {
            let m = (v.at.x.0 as f64).min(10e6 - v.at.x.0 as f64).min(v.at.y.0 as f64).min(14e6 - v.at.y.0 as f64) - v.diameter.0 as f64 / 2.0;
            assert!(m >= e - 1.0, "{edge_mm}: via {:.3} mm from the edge", m / 1e6);
        }
        // the zone is inset by the same rule
        assert!(b.zones[0].outline.iter().all(|p| p.x.0 >= e as i64 && p.y.0 >= e as i64));
        assert!(verify::clearance_violations(&nl, &b).is_empty(), "{:?}", verify::clearance_violations(&nl, &b));
    }
}

#[test]
fn a_track_ending_in_a_roundrect_corner_is_not_connected() {
    // SOT-223 pad: 2 x 1.5 mm, rratio 0.25 -> 0.375 mm corner radius.
    let mut fp = common::chip0603();
    for p in fp.pads.iter_mut() {
        p.size = (Nm::mm(2.0), Nm::mm(1.5));
        p.shape = wright_eda::PadShape::RoundRect(0.25);
    }
    fp.pads[1].at = Pt::mm(3.0, 0.0);
    let nl = Netlist { parts: vec![part("U1", PartKind::Ic, &[("1", Some(0)), ("2", Some(0))])], nets: vec![net("A", NetRole::Signal, &[(0, "1"), (0, "2")])] };
    let mut b = board(vec![at(0, fp, 5.0, 5.0, 0)], 12.0, 10.0, vec![]);
    // pad 1 spans x 4..6, y 4.25..5.75; (5.99, 5.74) is in its bbox corner, outside the rounding
    let t = |a: (f64, f64), c: (f64, f64)| wright_eda::Track { a: Pt::mm(a.0, a.1), b: Pt::mm(c.0, c.1), width: Nm::mm(0.2), layer: wright_eda::Layer::FCu, net: 0 };
    b.tracks = vec![t((5.99, 5.74), (8.0, 5.0))];
    assert!(!verify::connected(&nl, &b)[0]);
    b.tracks = vec![t((5.0, 5.0), (8.0, 5.0))];
    assert!(verify::connected(&nl, &b)[0]);
    // and the router's own result passes its check
    let mut r = b.clone();
    r.tracks.clear();
    let rep = route(&nl, &mut r, &RouteOptions::default());
    assert_eq!(rep.completion(), 1.0);
}
