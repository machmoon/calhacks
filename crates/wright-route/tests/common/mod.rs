//! Test support: footprints read from the installed KiCad library (the
//! authority; lane A's resolver will replace this), synthetic footprints
//! with the same numbers for offline tests, and a deterministic shelf
//! placement standing in for lane C.
#![allow(dead_code)]

use std::path::PathBuf;
use wright_eda::ir::NetRole;
use wright_eda::sexpr::Sexpr;
use wright_eda::{Board, Footprint, Layer, NetClass, Netlist, Nm, Pad, PadShape, Placed, Pt, Rect, Rules, Side};

pub fn kicad_footprints() -> Option<PathBuf> {
    let p = std::env::var("WRIGHT_KICAD_SHARE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport"));
    let f = p.join("footprints");
    f.is_dir().then_some(f)
}

/// Pads and courtyard of `Lib:Name` from the KiCad library. SMD pads on
/// the back layers are skipped (the IR has no per-pad layer).
pub fn load(root: &std::path::Path, id: &str) -> Footprint {
    let (lib, name) = id.split_once(':').unwrap();
    let path = root.join(format!("{lib}.pretty")).join(format!("{name}.kicad_mod"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let s = Sexpr::parse(&text).unwrap();
    let mut pads = Vec::new();
    for p in s.children("pad") {
        let number = p.text(1).unwrap_or("").to_string();
        let kind = p.text(2).unwrap_or("smd");
        let shape = p.text(3).unwrap_or("rect");
        let at = p.child("at").unwrap();
        let size = p.child("size").unwrap();
        let layers: Vec<&str> = p.child("layers").map(|l| (1..l.items().len()).filter_map(|i| l.text(i)).collect()).unwrap_or_default();
        let cu: Vec<Layer> = layers
            .iter()
            .flat_map(|l| match *l {
                "F.Cu" => vec![Layer::FCu],
                "B.Cu" => vec![Layer::BCu],
                "*.Cu" | "F&B.Cu" => vec![Layer::FCu, Layer::BCu],
                _ => vec![],
            })
            .collect();
        if cu.is_empty() && kind != "np_thru_hole" {
            continue; // paste / mask only
        }
        let drill = p.child("drill").map(|d| {
            // (drill d) or (drill oval w h)
            let v = if d.text(1) == Some("oval") { d.num(2).unwrap().min(d.num(3).unwrap_or(f64::MAX)) } else { d.num(1).unwrap() };
            Nm::mm(v)
        });
        let rr = p.child("roundrect_rratio").and_then(|r| r.num(1)).unwrap_or(0.25);
        pads.push(Pad {
            number,
            at: Pt::mm(at.num(1).unwrap(), at.num(2).unwrap()),
            rot_deg: at.num(3).unwrap_or(0.0),
            size: (Nm::mm(size.num(1).unwrap()), Nm::mm(size.num(2).unwrap())),
            shape: match shape {
                "rect" => PadShape::Rect,
                "roundrect" => PadShape::RoundRect(rr),
                "circle" => PadShape::Circle,
                "oval" => PadShape::Oval,
                _ => PadShape::Other,
            },
            drill: if kind == "smd" { None } else { drill },
            plated: kind == "thru_hole",
            layers: cu,
        });
    }
    let mut cy: Option<Rect> = None;
    for g in s.items() {
        if g.child("layer").and_then(|l| l.text(1)) != Some("F.CrtYd") {
            continue;
        }
        let mut pts = Vec::new();
        for k in ["start", "end", "center", "mid"] {
            if let Some(c) = g.child(k) {
                pts.push(Pt::mm(c.num(1).unwrap(), c.num(2).unwrap()));
            }
        }
        if let Some(ps) = g.child("pts") {
            for xy in ps.children("xy") {
                pts.push(Pt::mm(xy.num(1).unwrap(), xy.num(2).unwrap()));
            }
        }
        for p in pts {
            let r = Rect::from_corners(p, p);
            cy = Some(cy.map(|c| c.union(&r)).unwrap_or(r));
        }
    }
    let courtyard = cy.unwrap_or_else(|| pads.iter().map(|p| p.local_bbox()).reduce(|a, b| a.union(&b)).unwrap().inflate(Nm::mm(0.25)));
    Footprint { lib_id: id.into(), pads, courtyard, model3d: None, source: String::new() }
}

/// Footprint id per part for the Python baseline's fixtures (lane A's packages.json will
/// own this table).
pub fn footprint_id(netlist: &Netlist, part: usize) -> String {
    use wright_eda::PartKind::*;
    let p = &netlist.parts[part];
    let pkg = p.package.as_deref().unwrap_or("");
    if pkg.contains(':') {
        return pkg.to_string();
    }
    let s = match (p.kind, p.name.as_str(), pkg) {
        (_, _, "PinHeader_1x03_P2.54mm") => "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical",
        (_, _, "PinHeader_1x06_P2.54mm") => "Connector_PinHeader_2.54mm:PinHeader_1x06_P2.54mm_Vertical",
        (_, _, "XT30_2P") => "Connector_AMASS:AMASS_XT30U-M_1x02_P5.0mm_Vertical",
        (_, _, "JST_PH_2P") => "Connector_JST:JST_PH_B2B-PH-K_1x02_P2.00mm_Vertical",
        (_, _, "SW_SPST_TL3305A") => "Button_Switch_SMD:SW_SPST_TL3305A",
        (_, _, "USB_C_Receptacle_USB2.0_16P") => "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12",
        (_, "TPS5450DDA", _) => "Package_SO:TI_SO-PowerPAD-8_ThermalVias",
        (_, "AMS1117-3.3", _) => "Package_TO_SOT_SMD:SOT-223-3_TabPin2",
        (_, "CH340C", _) => "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm",
        (_, "ESP32-WROOM-32E", _) => "RF_Module:ESP32-WROOM-32E",
        (Ic, _, _) => "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm",
        (Resistor, _, _) => "Resistor_SMD:R_0603_1608Metric",
        (Capacitor, _, _) if p.value.starts_with("220u") || p.value.starts_with("100u") => "Capacitor_SMD:C_1206_3216Metric",
        (Capacitor, _, _) => "Capacitor_SMD:C_0805_2012Metric",
        (Inductor, _, _) => "Inductor_SMD:L_12x12mm_H6mm",
        (Diode, _, _) if p.value.contains("5A") => "Diode_SMD:D_SMC",
        (Diode, _, _) => "Diode_SMD:D_SOD-123",
        (Led, _, _) => "LED_SMD:LED_0603_1608Metric",
        _ => panic!("no footprint for {} ({:?} {pkg})", p.refdes, p.kind),
    };
    s.to_string()
}

pub fn fixture(name: &str) -> Netlist {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals/electronics/fixtures");
    let text = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
    wright_eda::Circuit::from_json(&text).unwrap().validate().unwrap()
}

pub const FIXTURES: [&str; 7] = ["blinker", "blinker10", "esp32_devboard", "ldo", "ref25", "servo_power_6v5a", "two_chip_usb"];

/// Design current per net (A). The servo board's numbers: five stalled
/// servos draw 5 A on +6V and through the inductor (SW); the 4S input is
/// 30 W / 0.9 / 12.8 V = 2.6 A DC and 3.4 A RMS through the switch, so VBAT
/// is sized for 4.3 A, which IPC-2221 gives as 1.5 mm. Other boards: power rails 0.5 A,
/// signals 0.1 A. Ground is carried by the B.Cu pour; its tracks are
/// fan-outs sized at the 0.4 mm ground floor.
pub fn currents(netlist: &Netlist) -> Vec<f64> {
    netlist
        .nets
        .iter()
        .map(|n| match (n.name.as_str(), n.role) {
            ("+6V" | "SW", _) => 5.0,
            ("VBAT", _) => 4.3,
            (_, NetRole::Power) => 0.5,
            _ => 0.1,
        })
        .collect()
}

pub fn classes(netlist: &Netlist, rules: &Rules) -> Vec<NetClass> {
    wright_route::ipc::classes_for_currents(netlist, rules, &currents(netlist))
}

/// Shelf placement: parts largest first into rows, `gap` between
/// courtyards, row width about 1.4 x the square root of the total area.
/// Deterministic; a stand-in for lane C.
pub fn shelf(netlist: &Netlist, fps: Vec<Footprint>, gap: Nm) -> Board {
    let mut idx: Vec<usize> = (0..fps.len()).collect();
    let area = |f: &Footprint| f.courtyard.width().0 as f64 * f.courtyard.height().0 as f64;
    idx.sort_by(|&a, &b| area(&fps[b]).total_cmp(&area(&fps[a])).then(a.cmp(&b)));
    let total: f64 = fps.iter().map(|f| (f.courtyard.width().0 + gap.0) as f64 * (f.courtyard.height().0 + gap.0) as f64).sum();
    let row_w = (total.sqrt() * 1.4) as i64;
    let margin = Nm::mm(2.0).0 + gap.0;
    let (mut x, mut y, mut row_h, mut max_x) = (margin, margin, 0i64, 0i64);
    let mut parts = Vec::new();
    for i in idx {
        let c = fps[i].courtyard;
        let (w, h) = (c.width().0, c.height().0);
        if x > margin && x + w > margin + row_w {
            x = margin;
            y += row_h + gap.0;
            row_h = 0;
        }
        let at = Pt::new(x - c.min.x.0, y - c.min.y.0);
        parts.push(Placed { part: i, footprint: fps[i].clone(), at, rot: 0, side: Side::Top, locked: false });
        x += w + gap.0;
        row_h = row_h.max(h);
        max_x = max_x.max(x);
    }
    let _ = netlist;
    let outline = Rect::from_corners(Pt::new(0, 0), Pt::new(max_x - gap.0 + margin, y + row_h + margin));
    Board { outline, parts, tracks: vec![], vias: vec![], zones: vec![], classes: vec![], rules: Rules::default(), holes: vec![] }
}

pub fn fixture_board(root: &std::path::Path, name: &str) -> (Netlist, Board) {
    let nl = fixture(name);
    let fps: Vec<Footprint> = (0..nl.parts.len()).map(|i| load(root, &footprint_id(&nl, i))).collect();
    let rules = Rules::default();
    let cls = classes(&nl, &rules);
    let widest = cls.iter().map(|c| c.track_width).max().unwrap();
    // Room for two of the widest tracks between parts.
    let gap = Nm(Nm::mm(1.5).0.max(2 * widest.0 + Nm::mm(0.6).0));
    let mut b = shelf(&nl, fps, gap);
    b.classes = cls;
    (nl, b)
}

/// A two-pad chip footprint (KiCad R_0603_1608Metric numbers).
pub fn chip0603() -> Footprint {
    let pad = |n: &str, x: f64| Pad { number: n.into(), at: Pt::mm(x, 0.0), rot_deg: 0.0, size: (Nm::mm(0.8), Nm::mm(0.95)), shape: PadShape::RoundRect(0.25), drill: None, plated: false, layers: vec![] };
    Footprint { lib_id: "Resistor_SMD:R_0603_1608Metric".into(), pads: vec![pad("1", -0.825), pad("2", 0.825)], courtyard: Rect::from_corners(Pt::mm(-1.48, -0.73), Pt::mm(1.48, 0.73)), model3d: None, source: String::new() }
}

/// 1xN 2.54 mm header (KiCad PinHeader_1xNN_P2.54mm_Vertical numbers).
pub fn header(n: usize) -> Footprint {
    let pads = (0..n)
        .map(|i| Pad { number: (i + 1).to_string(), at: Pt::mm(0.0, 2.54 * i as f64), rot_deg: 0.0, size: (Nm::mm(1.7), Nm::mm(1.7)), shape: if i == 0 { PadShape::Rect } else { PadShape::Oval }, drill: Some(Nm::mm(1.0)), plated: true, layers: vec![] })
        .collect();
    Footprint { lib_id: format!("Connector_PinHeader_2.54mm:PinHeader_1x{n:02}_P2.54mm_Vertical"), pads, courtyard: Rect::from_corners(Pt::mm(-1.8, -1.8), Pt::mm(1.8, 2.54 * (n - 1) as f64 + 1.8)), model3d: None, source: String::new() }
}
