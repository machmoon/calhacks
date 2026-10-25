//! The `electronics` feature: the PCB half of the cross-check
//! (docs/electronics-plan.md section 4, Lane F).
//!
//! * `gen-pcb DIR` writes one SYNTHETIC board per circuit fixture in
//!   `evals/electronics/fixtures/`: the netlist is the real
//!   `Circuit::validate` output, but the footprints, placement and copper are
//!   made up here (a grid placement, L-shaped routes, every sixth net left
//!   unrouted on purpose) until lanes A, C and D produce real boards. They
//!   are serialised exactly as the oracle's contract says,
//!   `serde_json::to_string(&(netlist, board, route))`, so the oracle reads
//!   the frozen `wright_eda` types and nothing hand-written.
//! * `eval` answers the `pcb_*` cases with what the Rust exposes today:
//!   `Placed::pad_abs` / `pad_bbox_abs` / `courtyard_abs` and `Rect::overlaps`
//!   (wright-eda), the router's claim in `RouteReport`, and lane E's
//!   `wright_electronics::copper::{min_gap_nm, clearance_violations,
//!   narrowest_tracks, ampacity_at}` and `checks::regulator_vout` for
//!   pcb_clearance / pcb_ampacity / pcb_regulator.
//!
//! Agreement with the oracle means two independent readings agree; it does
//! not verify the Rust code.

use serde_json::{json, Map, Value};
use std::path::Path;
use wright_eda::{Board, Circuit, Footprint, Layer, NetClass, Netlist, Nm, Pad, PadShape, Placed, Pt, Rect, RouteReport, Rules, Side, Track, Unrouted, Via, Zone};
use wright_eda::ir::{NetRole, PartKind};

type Design = (Netlist, Board, RouteReport);

fn design(c: &Map<String, Value>) -> Result<Design, String> {
    let b = c.get("board").ok_or("board")?;
    serde_json::from_value(b.clone()).map_err(|e| format!("board JSON: {e}"))
}

fn refdes(n: &Netlist, p: &Placed) -> String {
    n.parts.get(p.part).map(|x| x.refdes.clone()).unwrap_or_else(|| format!("part{}", p.part))
}

pub fn eval(fn_: &str, c: &Map<String, Value>) -> Result<Value, String> {
    let (nl, board, route) = design(c)?;
    let mut out = Map::new();
    let mut put = |k: String, v: f64| {
        out.insert(k, json!(v));
    };
    match fn_ {
        "pcb_pads" => {
            for p in &board.parts {
                for (i, pad) in p.footprint.pads.iter().enumerate() {
                    let k = format!("{}#{i}", refdes(&nl, p));
                    let a = p.pad_abs(pad);
                    let r = p.pad_bbox_abs(pad);
                    put(format!("pad_x:{k}"), a.x.0 as f64);
                    put(format!("pad_y:{k}"), a.y.0 as f64);
                    put(format!("pad_minx:{k}"), r.min.x.0 as f64);
                    put(format!("pad_miny:{k}"), r.min.y.0 as f64);
                    put(format!("pad_maxx:{k}"), r.max.x.0 as f64);
                    put(format!("pad_maxy:{k}"), r.max.y.0 as f64);
                }
            }
        }
        "pcb_courtyard" => {
            let cs: Vec<Rect> = board.parts.iter().map(Placed::courtyard_abs).collect();
            let mut pairs = 0;
            for i in 0..cs.len() {
                for j in i + 1..cs.len() {
                    pairs += cs[i].overlaps(&cs[j]) as u32;
                }
            }
            put("overlap_pairs".into(), pairs as f64);
            for (p, r) in board.parts.iter().zip(&cs) {
                let k = refdes(&nl, p);
                put(format!("crtyd_minx:{k}"), r.min.x.0 as f64);
                put(format!("crtyd_miny:{k}"), r.min.y.0 as f64);
                put(format!("crtyd_maxx:{k}"), r.max.x.0 as f64);
                put(format!("crtyd_maxy:{k}"), r.max.y.0 as f64);
            }
        }
        "pcb_connectivity" => {
            // The router's claim: an attempted net is connected iff it is in
            // `routed`.
            for &n in &route.attempted {
                let name = &nl.nets[n].name;
                put(format!("conn:{name}"), route.routed.contains(&n) as u8 as f64);
            }
            put("unrouted_count".into(), route.unrouted.len() as f64);
        }
        "pcb_clearance" => {
            // wright_electronics::copper (lane E), same item model as Oracle.Pcb
            if let Some(g) = wright_electronics::copper::min_gap_nm(&nl, &board) {
                put("min_gap_nm".into(), g);
                put("violations".into(), wright_electronics::copper::clearance_violations(&nl, &board) as f64);
            }
        }
        "pcb_ampacity" => {
            let rise = c.get("rise_c").and_then(Value::as_f64).unwrap_or(20.0);
            let per = wright_electronics::copper::narrowest_tracks(&board);
            let cap = |w: i64| wright_electronics::copper::ampacity_at(w as f64 / 1e6, rise, board.rules.copper_oz);
            for &(n, w, _) in &per {
                let name = &nl.nets[n].name;
                put(format!("minw_nm:{name}"), w as f64);
                put(format!("amp_a:{name}"), cap(w));
            }
            if let Some(cs) = c.get("net_current_a").and_then(Value::as_array) {
                let cs: Vec<f64> = cs.iter().filter_map(Value::as_f64).collect();
                put("short_nets".into(), per.iter().filter(|&&(n, w, _)| n < cs.len() && cap(w) < cs[n]).count() as f64);
            }
        }
        "pcb_regulator" => match wright_electronics::checks::regulator_vout(&nl) {
            Ok((r1, r2, v)) => {
                put("vout_v".into(), v);
                put("r1_ohm".into(), r1);
                put("r2_ohm".into(), r2);
            }
            Err(e) => return Ok(json!({ "error": e })),
        },
        _ => return Err(format!("{fn_}: no public Rust function")),
    }
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// synthetic boards

fn pad(number: &str, x: f64, y: f64, w: f64, h: f64, shape: PadShape, drill: Option<f64>) -> Pad {
    Pad {
        number: number.into(),
        at: Pt::mm(x, y),
        rot_deg: 0.0,
        size: (Nm::mm(w), Nm::mm(h)),
        shape,
        drill: drill.map(Nm::mm),
        plated: drill.is_some(),
        layers: Vec::new(),
    }
}

/// A made-up land pattern with one pad per distinct pad number.
fn synthetic_footprint(kind: PartKind, pads: &[String]) -> Footprint {
    let n = pads.len();
    let mut out: Vec<Pad> = Vec::new();
    let lib = match kind {
        k if k.is_passive() && n == 2 => {
            // 0603-like (Resistor_SMD:R_0603_1608Metric pad numbers)
            out.push(pad(&pads[0], -0.825, 0.0, 0.8, 0.95, PadShape::RoundRect(0.25), None));
            out.push(pad(&pads[1], 0.825, 0.0, 0.8, 0.95, PadShape::RoundRect(0.25), None));
            "Synthetic:R_0603"
        }
        PartKind::Connector | PartKind::Battery | PartKind::Switch | PartKind::Testpoint => {
            // 1 x n through-hole header, pin 1 square
            for (i, p) in pads.iter().enumerate() {
                let shape = if i == 0 { PadShape::Rect } else { PadShape::Circle };
                out.push(pad(p, 0.0, i as f64 * 2.54, 1.7, 1.7, shape, Some(1.0)));
            }
            "Synthetic:Header_1xN"
        }
        _ => {
            // two-row SOIC-like, ovals, rows 5.4 mm apart, 1.27 mm pitch
            let rows = n.div_ceil(2);
            for (i, p) in pads.iter().enumerate() {
                let (x, row) = if i < rows { (-2.7, i) } else { (2.7, n - 1 - i) };
                let shape = if i % 3 == 2 { PadShape::Rect } else { PadShape::Oval };
                out.push(pad(p, x, row as f64 * 1.27, 1.55, 0.6, shape, None));
            }
            "Synthetic:SOIC_N"
        }
    };
    let bb = out.iter().map(Pad::local_bbox).reduce(|a, b| a.union(&b)).unwrap_or_default();
    Footprint { lib_id: lib.into(), pads: out, courtyard: bb.inflate(Nm::mm(0.25)), model3d: None, source: String::new() }
}

/// The copper layer a route must start on at this pad; `None` for a
/// through-hole pad (either layer).
fn layer_of(p: &Placed, pad: &Pad) -> Option<Layer> {
    pad.drill.is_none().then(|| p.copper_layer())
}

fn synthetic_board(name: &str, nl: &Netlist) -> (Board, RouteReport) {
    let rules = Rules::default();
    let fps: Vec<Footprint> = nl
        .parts
        .iter()
        .map(|p| {
            let mut ps: Vec<String> = p.pins.iter().map(|q| q.pad.clone()).collect();
            ps.sort_by(|a, b| a.parse::<u32>().ok().cmp(&b.parse::<u32>().ok()).then(a.cmp(b)));
            ps.dedup();
            synthetic_footprint(p.kind, &ps)
        })
        .collect();
    let cell = fps
        .iter()
        .map(|f| f.courtyard.width().0.max(f.courtyard.height().0))
        .max()
        .unwrap_or(0)
        + Nm::mm(3.0).0;
    let cols = (nl.parts.len() as f64).sqrt().ceil().max(1.0) as i64;
    let mut parts: Vec<Placed> = fps
        .into_iter()
        .enumerate()
        .map(|(i, footprint)| {
            let (cx, cy) = (i as i64 % cols, i as i64 / cols);
            Placed {
                part: i,
                footprint,
                at: Pt::new(Nm::mm(5.0).0 + cx * cell + cell / 2, Nm::mm(5.0).0 + cy * cell + cell / 2),
                rot: (i % 4) as i32,
                side: if i % 5 == 3 { Side::Bottom } else { Side::Top },
                locked: false,
            }
        })
        .collect();
    // One deliberate courtyard overlap on the LDO board.
    if name == "ldo" && parts.len() > 1 {
        parts[1].at = parts[0].at + Pt::mm(0.5, 0.3);
    }
    let rows = (nl.parts.len() as i64 + cols - 1) / cols;
    let outline = Rect::from_corners(Pt::new(0, 0), Pt::new(Nm::mm(10.0).0 + cols * cell, Nm::mm(10.0).0 + rows * cell));

    let power: Vec<usize> = (0..nl.nets.len()).filter(|&i| nl.nets[i].role != NetRole::Signal).collect();
    let signal: Vec<usize> = (0..nl.nets.len()).filter(|i| !power.contains(i)).collect();
    let classes = vec![
        NetClass { name: "Default".into(), track_width: Nm::mm(0.25), clearance: Nm::mm(0.2), via_diameter: Nm::mm(0.6), via_drill: Nm::mm(0.3), nets: signal },
        NetClass { name: "Power".into(), track_width: Nm::mm(0.8), clearance: Nm::mm(0.25), via_diameter: Nm::mm(0.8), via_drill: Nm::mm(0.4), nets: power.clone() },
    ];
    let mut board = Board { outline, parts, tracks: vec![], vias: vec![], zones: vec![], classes, rules, holes: vec![] };

    // The servo board pours GND on B.Cu instead of routing it.
    let pour_gnd = name.starts_with("servo");
    let mut route = RouteReport::default();
    for (n, net) in nl.nets.iter().enumerate() {
        if net.pins.len() < 2 {
            continue;
        }
        if pour_gnd && net.role == NetRole::Ground {
            board.zones.push(Zone { net: n, layer: Layer::BCu, outline: vec![outline.min, Pt { x: outline.max.x, y: outline.min.y }, outline.max, Pt { x: outline.min.x, y: outline.max.y }], clearance: Nm::mm(0.3), min_thickness: Nm::mm(0.25), thermal_spoke: Nm::mm(0.5) });
            route.poured.push(n);
            continue;
        }
        route.attempted.push(n);
        if n % 6 == 5 {
            route.unrouted.push(Unrouted { net: n, name: net.name.clone(), reason: "synthetic: left unrouted to exercise the report".into() });
            continue;
        }
        let class = board.class_of(n).cloned().expect("classes");
        let ends: Vec<(Pt, Option<Layer>)> = net
            .pins
            .iter()
            .filter_map(|pr| {
                let p = board.parts.iter().find(|p| p.part == pr.part)?;
                let pad = p.footprint.pads.iter().find(|q| q.number == pr.pad)?;
                Some((p.pad_abs(pad), layer_of(p, pad)))
            })
            .collect();
        let pref = if n % 2 == 0 { Layer::FCu } else { Layer::BCu };
        for w in ends.windows(2) {
            let ((a, la), (b, lb)) = (w[0], w[1]);
            let la = la.unwrap_or(pref);
            let lb = lb.unwrap_or(la);
            let corner = Pt { x: b.x, y: a.y };
            let mut seg = |p: Pt, q: Pt, layer: Layer| {
                if p != q {
                    board.tracks.push(Track { a: p, b: q, width: class.track_width, layer, net: n });
                }
            };
            seg(a, corner, la);
            if la != lb {
                board.vias.push(Via { at: corner, diameter: class.via_diameter, drill: class.via_drill, net: n });
            }
            seg(corner, b, lb);
        }
        route.routed.push(n);
    }
    (board, route)
}

/// Write `<name>.json` per circuit fixture into `out`.
pub fn gen(fixtures: &Path, out: &Path) -> anyhow::Result<Vec<String>> {
    std::fs::create_dir_all(out)?;
    let mut names = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(fixtures)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for f in files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let circuit = Circuit::from_json(&std::fs::read_to_string(&f)?)?;
        let nl = circuit.validate().map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        let (board, route) = synthetic_board(&name, &nl);
        let d: Design = (nl, board, route);
        std::fs::write(out.join(format!("{name}.json")), serde_json::to_string(&d)?)?;
        names.push(name);
    }
    Ok(names)
}
