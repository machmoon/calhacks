//! The emitted copper as shapes, measured independently of the router's
//! grid: different-net clearance (all pairs, O(n^2); the slice boards are
//! small, docs/electronics-plan.md section 3), IPC-2221 ampacity and its
//! inverse, and the net-class table built from per-net currents.
//!
//! Clearance follows Horizon EDA
//! `src/board/board_rules_check_clearance_copper.cpp` in kind: every copper
//! item on a layer against every item of another net on that layer, the
//! limit the larger of the two nets' class clearances. Shapes are
//! conservative: rectangular and rounded pads are their rotated bounding
//! boxes (a rounded corner is never closer than its box), oval pads are
//! capsules, round pads and vias discs, tracks capsules: the same shapes as
//! the Haskell oracle (`haskell/src/Oracle/Pcb.hs` `padGeo`). Zones are left to
//! KiCad's DRC (it fills them, `--refill-zones`).

use wright_eda::{Board, Layer, NetClass, Netlist, Nm, PadShape, Pt, Rect, Rules};

/// IPC-2221A section 6.2 external-layer constant (baseline `electronics.py:386`).
pub const IPC_K_EXTERNAL: f64 = 0.048;
/// 1 oz copper thickness, mil (baseline `electronics.py:388`).
pub const COPPER_1OZ_MIL: f64 = 1.378;
/// Allowed temperature rise, C.
pub const RISE_C: f64 = 20.0;

/// Current an external track of `width_mm` carries for a `RISE_C` rise.
pub fn ampacity_a(width_mm: f64, oz: f64) -> f64 {
    ampacity_at(width_mm, RISE_C, oz)
}

/// IPC-2221A external: I = 0.048 dT^0.44 A^0.725, A in mil^2.
pub fn ampacity_at(width_mm: f64, rise_c: f64, oz: f64) -> f64 {
    let area_mil2 = width_mm / 0.0254 * COPPER_1OZ_MIL * oz;
    IPC_K_EXTERNAL * rise_c.powf(0.44) * area_mil2.powf(0.725)
}

/// IPC-2221A internal-conductor constant (a via barrel is enclosed in
/// laminate, so it gets the conservative half of the external constant).
pub const IPC_K_INTERNAL: f64 = 0.024;
/// Barrel plating, mm: IPC-6012 Class 2 minimum average hole-wall copper
/// (20 um), the thinnest a fab may ship.
pub const VIA_PLATING_MM: f64 = 0.020;

/// Current a plated via barrel of `drill_mm` carries for a `RISE_C` rise:
/// IPC-2221A internal on the barrel's cross-section `pi x drill x plating`.
pub fn via_ampacity_a(drill_mm: f64) -> f64 {
    let area_mil2 = std::f64::consts::PI * drill_mm * VIA_PLATING_MM / (0.0254 * 0.0254);
    IPC_K_INTERNAL * RISE_C.powf(0.44) * area_mil2.powf(0.725)
}

/// Narrowest external track width (mm) that carries `amps` (the inverse).
pub fn width_for_mm(amps: f64, oz: f64) -> f64 {
    let area_mil2 = (amps / (IPC_K_EXTERNAL * RISE_C.powf(0.44))).powf(1.0 / 0.725);
    area_mil2 / (COPPER_1OZ_MIL * oz) * 0.0254
}

/// Net classes from per-net currents: `Default` first (min track, every
/// net that fits it), then one class per distinct width, rounded up to
/// 0.05 mm (`W1.85mm`). Clearance and vias stay at the fab minimum.
pub fn classes_for(netlist: &Netlist, currents: &[f64], rules: &Rules) -> Vec<NetClass> {
    let mut default = NetClass { name: "Default".into(), track_width: rules.min_track, clearance: rules.min_clearance, via_diameter: rules.min_via_diameter, via_drill: rules.min_via_drill, nets: vec![] };
    let mut wide: Vec<NetClass> = Vec::new();
    for (i, _) in netlist.nets.iter().enumerate() {
        let need = width_for_mm(currents.get(i).copied().unwrap_or(0.0), rules.copper_oz);
        let w = Nm(((need * 20.0).ceil() / 20.0 * 1e6).round() as i64);
        if w <= rules.min_track {
            default.nets.push(i);
            continue;
        }
        match wide.iter_mut().find(|c| c.track_width == w) {
            Some(c) => c.nets.push(i),
            None => wide.push(NetClass { name: format!("W{}mm", w.kicad()), track_width: w, clearance: rules.min_clearance, via_diameter: rules.min_via_diameter, via_drill: rules.min_via_drill, nets: vec![i] }),
        }
    }
    wide.sort_by_key(|c| c.track_width);
    std::iter::once(default).chain(wide).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Axis-aligned box.
    Box(Rect),
    /// Segment with a radius (track, or a disc when a == b).
    Capsule { a: Pt, b: Pt, r: f64 },
}

/// One piece of copper. Same model as the Haskell oracle's
/// `Oracle.Pcb.copperItems` (pads, then tracks, then vias; a pad on no
/// netlist net is a net of its own; NPTH has no copper), so the two can
/// be cross-checked item for item.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// F.Cu, B.Cu or both.
    pub layers: Vec<Layer>,
    /// `Err(k)`: the k-th pad, on no net (a different net from everything).
    pub net: Result<usize, usize>,
    pub shape: Shape,
    /// "C3 pad 1", "track SW", "via GND".
    pub what: String,
}

/// Each pad's copper layers as the library `.kicad_mod` text says
/// (`Footprint.source`), in `fp.pads` order, footprint frame; `None` when
/// the source does not name the pad. `wright_eda::Pad` has no layer field,
/// so an SMD pad on the *back* of a top-side footprint (the B.Cu pad of
/// `TI_SO-PowerPAD-8_ThermalVias`) is invisible without this; kicad-cli
/// DRC found a +6V track shorted to it on the first real servo board
/// (2026-10-24 23:30).
pub fn source_pad_layers(fp: &wright_eda::Footprint) -> Vec<Option<Vec<Layer>>> {
    let Ok(root) = wright_eda::sexpr::Sexpr::parse(&fp.source) else { return vec![None; fp.pads.len()] };
    let mut src: Vec<(String, f64, f64, Vec<Layer>, bool)> = root
        .children("pad")
        .map(|p| {
            let at = p.child("at");
            let (x, y) = (at.and_then(|a| a.num(1)).unwrap_or(f64::NAN), at.and_then(|a| a.num(2)).unwrap_or(f64::NAN));
            let mut ls = Vec::new();
            for l in p.child("layers").map(|l| &l.items()[1..]).unwrap_or(&[]) {
                let t = match l {
                    wright_eda::sexpr::Sexpr::Atom(t) | wright_eda::sexpr::Sexpr::Str(t) => t.as_str(),
                    _ => "",
                };
                match t {
                    "F.Cu" => ls.push(Layer::FCu),
                    "B.Cu" => ls.push(Layer::BCu),
                    "*.Cu" | "F&B.Cu" => ls.extend([Layer::FCu, Layer::BCu]),
                    _ => {}
                }
            }
            ls.sort();
            ls.dedup();
            (p.text(1).unwrap_or("").to_string(), x, y, ls, false)
        })
        .collect();
    fp.pads
        .iter()
        .map(|pad| {
            let (px, py) = (pad.at.x.as_mm(), pad.at.y.as_mm());
            let hit = src.iter_mut().find(|s| !s.4 && s.0 == pad.number && (s.1 - px).abs() < 1e-6 && (s.2 - py).abs() < 1e-6)?;
            hit.4 = true;
            Some(hit.3.clone())
        })
        .collect()
}

/// Every pad, track and via of `board` as copper items.
pub fn items(netlist: &Netlist, board: &Board) -> Vec<Item> {
    let mut out = Vec::new();
    let mut k = 0;
    for p in &board.parts {
        let part = netlist.parts.get(p.part);
        let refdes = part.map(|x| x.refdes.as_str()).unwrap_or("?");
        let named = source_pad_layers(&p.footprint);
        let flip = |l: Layer| if p.side == wright_eda::Side::Bottom { if l == Layer::FCu { Layer::BCu } else { Layer::FCu } } else { l };
        for (pi, pad) in p.footprint.pads.iter().enumerate() {
            k += 1;
            let layers: Vec<Layer> = match (pad.drill, named.get(pi).cloned().flatten()) {
                (Some(_), _) if !pad.plated => continue, // NPTH: no copper
                (_, Some(ls)) if !ls.is_empty() => ls.into_iter().map(flip).collect(),
                _ => p.pad_layers(pad),
            };
            let net = netlist.nets.iter().position(|n| n.pins.iter().any(|q| q.part == p.part && q.pad == pad.number && !pad.number.is_empty())).ok_or(k);
            let r = p.pad_bbox_abs(pad);
            let c = p.pad_abs(pad);
            let (hx, hy) = ((r.max.x.0 - r.min.x.0) / 2, (r.max.y.0 - r.min.y.0) / 2);
            let shape = match pad.shape {
                PadShape::Circle => Shape::Capsule { a: c, b: c, r: pad.size.0 .0 as f64 / 2.0 },
                PadShape::Oval if hx >= hy => Shape::Capsule { a: Pt::new(c.x.0 - (hx - hy), c.y.0), b: Pt::new(c.x.0 + (hx - hy), c.y.0), r: hy as f64 },
                PadShape::Oval => Shape::Capsule { a: Pt::new(c.x.0, c.y.0 - (hy - hx)), b: Pt::new(c.x.0, c.y.0 + (hy - hx)), r: hx as f64 },
                _ => Shape::Box(r),
            };
            out.push(Item { layers, net, shape, what: format!("{refdes} pad {}", if pad.number.is_empty() { "(unnumbered)" } else { &pad.number }) });
        }
    }
    let name = |n: usize| netlist.nets.get(n).map(|x| x.name.as_str()).unwrap_or("?");
    for t in &board.tracks {
        out.push(Item { layers: vec![t.layer], net: Ok(t.net), shape: Shape::Capsule { a: t.a, b: t.b, r: t.width.0 as f64 / 2.0 }, what: format!("track {}", name(t.net)) });
    }
    for v in &board.vias {
        out.push(Item { layers: vec![Layer::FCu, Layer::BCu], net: Ok(v.net), shape: Shape::Capsule { a: v.at, b: v.at, r: v.diameter.0 as f64 / 2.0 }, what: format!("via {}", name(v.net)) });
    }
    out
}

fn f(p: Pt) -> (f64, f64) {
    (p.x.0 as f64, p.y.0 as f64)
}

fn point_seg(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

fn cross(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn segs_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn seg_seg(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    if segs_cross(a, b, c, d) {
        return 0.0;
    }
    point_seg(a, c, d).min(point_seg(b, c, d)).min(point_seg(c, a, b)).min(point_seg(d, a, b))
}

fn point_box(p: (f64, f64), r: &Rect) -> f64 {
    let dx = (r.min.x.0 as f64 - p.0).max(0.0).max(p.0 - r.max.x.0 as f64);
    let dy = (r.min.y.0 as f64 - p.1).max(0.0).max(p.1 - r.max.y.0 as f64);
    dx.hypot(dy)
}

fn seg_box(a: (f64, f64), b: (f64, f64), r: &Rect) -> f64 {
    if point_box(a, r) == 0.0 || point_box(b, r) == 0.0 {
        return 0.0;
    }
    let c = [f(r.min), (r.max.x.0 as f64, r.min.y.0 as f64), f(r.max), (r.min.x.0 as f64, r.max.y.0 as f64)];
    (0..4).map(|i| seg_seg(a, b, c[i], c[(i + 1) % 4])).fold(f64::INFINITY, f64::min)
}

/// Edge-to-edge distance, nm (0 when touching or overlapping).
pub fn distance(s: &Shape, t: &Shape) -> f64 {
    let d = match (s, t) {
        (Shape::Box(a), Shape::Box(b)) => {
            let dx = (a.min.x.0 - b.max.x.0).max(b.min.x.0 - a.max.x.0).max(0) as f64;
            let dy = (a.min.y.0 - b.max.y.0).max(b.min.y.0 - a.max.y.0).max(0) as f64;
            dx.hypot(dy)
        }
        (Shape::Box(bx), Shape::Capsule { a, b, r }) | (Shape::Capsule { a, b, r }, Shape::Box(bx)) => seg_box(f(*a), f(*b), bx) - r,
        (Shape::Capsule { a, b, r }, Shape::Capsule { a: c, b: d, r: q }) => seg_seg(f(*a), f(*b), f(*c), f(*d)) - r - q,
    };
    d.max(0.0)
}

/// The closest different-net pair and its margin against the larger
/// class clearance (mm, negative = violation). `None` when the board has
/// fewer than two nets' worth of copper.
#[derive(Debug, Clone, PartialEq)]
pub struct Worst {
    pub margin_mm: f64,
    pub gap_mm: f64,
    pub limit_mm: f64,
    pub a: String,
    pub b: String,
    pub violations: usize,
}

/// The clearance a pair needs: the larger of the two nets' class
/// clearances, at least the board minimum (oracle `classClearance`).
fn class_clearance(board: &Board, net: Result<usize, usize>) -> i64 {
    let c = match net {
        Ok(n) => board.class_of(n).map(|c| c.clearance.0).unwrap_or(0),
        Err(_) => 0,
    };
    c.max(board.rules.min_clearance.0)
}

/// `(gap_nm, required_nm, a, b)` for every different-net pair on a shared
/// layer, the gap clamped at 0 (oracle `gapPairs`).
pub fn gap_pairs(netlist: &Netlist, board: &Board) -> Vec<(f64, i64, usize, usize)> {
    let it = items(netlist, board);
    let mut out = Vec::new();
    for i in 0..it.len() {
        for j in i + 1..it.len() {
            let (a, b) = (&it[i], &it[j]);
            if a.net == b.net || !a.layers.iter().any(|l| b.layers.contains(l)) {
                continue;
            }
            out.push((distance(&a.shape, &b.shape), class_clearance(board, a.net).max(class_clearance(board, b.net)), i, j));
        }
    }
    out
}

/// Smallest different-net copper gap on the board, nm (oracle `minGap`);
/// `None` when no two nets share a layer.
pub fn min_gap_nm(netlist: &Netlist, board: &Board) -> Option<f64> {
    gap_pairs(netlist, board).into_iter().map(|p| p.0).reduce(f64::min)
}

/// Pairs closer than they need to be (oracle `clearanceViolations`).
pub fn clearance_violations(netlist: &Netlist, board: &Board) -> usize {
    gap_pairs(netlist, board).into_iter().filter(|p| p.0 < p.1 as f64).count()
}

/// Per net with tracks: (net, narrowest track nm, its IPC-2221 capacity A
/// at `RISE_C`), in net order (oracle `pcb_ampacity`).
pub fn narrowest_tracks(board: &Board) -> Vec<(usize, i64, f64)> {
    let mut nets: Vec<usize> = board.tracks.iter().map(|t| t.net).collect();
    nets.sort_unstable();
    nets.dedup();
    nets.into_iter()
        .map(|n| {
            let w = board.tracks.iter().filter(|t| t.net == n).map(|t| t.width.0).min().unwrap_or(0);
            (n, w, ampacity_a(w as f64 / 1e6, board.rules.copper_oz))
        })
        .collect()
}

pub fn clearance(netlist: &Netlist, board: &Board) -> Option<Worst> {
    let it = items(netlist, board);
    let pairs = gap_pairs(netlist, board);
    let violations = pairs.iter().filter(|p| p.0 < p.1 as f64).count();
    let w = pairs.iter().min_by(|x, y| (x.0 - x.1 as f64).total_cmp(&(y.0 - y.1 as f64)))?;
    Some(Worst { margin_mm: (w.0 - w.1 as f64) / 1e6, gap_mm: w.0 / 1e6, limit_mm: w.1 as f64 / 1e6, a: it[w.2].what.clone(), b: it[w.3].what.clone(), violations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_via_barrel_is_rated_on_its_plating() {
        // pi x 0.3 mm x 20 um = 29.2 mil^2; 0.024 x 20^0.44 x 29.2^0.725 = 1.03 A
        let i = via_ampacity_a(0.3);
        assert!((i - 1.03).abs() < 0.01, "{i}");
        assert!(via_ampacity_a(0.6) > i);
    }

    #[test]
    fn ipc2221_round_trips_and_matches_the_python_baseline() {
        // baseline electronics.py docstring: 1.0 mm carries 3.2 A at 20 C.
        assert!((ampacity_a(1.0, 1.0) - 3.2).abs() < 0.05, "{}", ampacity_a(1.0, 1.0));
        // baseline trace_capacity_a(0.8), Lane F's reference value
        assert!((ampacity_a(0.8, 1.0) - 2.7598527691900028).abs() < 1e-12);
        let w = width_for_mm(5.0, 1.0);
        assert!((ampacity_a(w, 1.0) - 5.0).abs() < 1e-9);
        assert!(w > 1.5 && w < 2.0, "5 A needs {w} mm");
    }

    #[test]
    fn distances_are_edge_to_edge() {
        let bx = Shape::Box(Rect::from_corners(Pt::mm(0.0, 0.0), Pt::mm(1.0, 1.0)));
        let disc = Shape::Capsule { a: Pt::mm(3.0, 0.5), b: Pt::mm(3.0, 0.5), r: 500_000.0 };
        assert!((distance(&bx, &disc) - 1.5e6).abs() < 1.0);
        let track = Shape::Capsule { a: Pt::mm(-1.0, 2.0), b: Pt::mm(2.0, 2.0), r: 100_000.0 };
        assert!((distance(&bx, &track) - 0.9e6).abs() < 1.0);
        let crossing = Shape::Capsule { a: Pt::mm(-1.0, 0.5), b: Pt::mm(2.0, 0.5), r: 100_000.0 };
        assert_eq!(distance(&bx, &crossing), 0.0);
        let b2 = Shape::Box(Rect::from_corners(Pt::mm(1.2, 0.0), Pt::mm(2.0, 1.0)));
        assert!((distance(&bx, &b2) - 0.2e6).abs() < 1.0);
    }
}
