//! Generated schematic symbols, in the symbol library's **Y-up** frame.
//!
//! Follows the Python baseline's `engine/silkscreen/schematic.py` (`_device_shape`,
//! `_connector_shape`, `_passive_shape`, `_lib_symbol`, `_flag_lib_symbol`)
//! and, for the glyph geometry and the `lib_symbols` entry shape, KiCad's own
//! library files read from the 10.0.6 bundle: `symbols/Device.kicad_sym`
//! (`R`, `C`, `L`, `D`), `symbols/Connector_Generic.kicad_sym` (`Conn_01x03`:
//! every pin on the left edge pointing right) and `symbols/power.kicad_sym`
//! (`PWR_FLAG`: one `power_out` pin at the origin, length 0, drawn upward).
//!
//! One deviation from the baseline, stated: two-terminal parts are drawn horizontally
//! (pin 1 left, pin 2 right) in the library itself rather than as KiCad's
//! vertical `Device:R` placed at 90 degrees. Every pin on the sheet then
//! leaves sideways, every label is horizontal, and no instance is ever
//! rotated, so the only frame crossing left is the Y flip in
//! [`crate::pin_on_sheet`].

use wright_eda::ir::{NetRole, Part, PartKind};
use wright_eda::sexpr::Sexpr;
use wright_eda::{Netlist, Nm};

/// KiCad's schematic connection grid (50 mil). Every pin end, wire end and
/// label anchor this crate writes is a multiple of it, or nothing connects.
pub const GRID: i64 = 1_270_000;
pub const PITCH: i64 = 2 * GRID;
/// Text height of every field and label on the sheet.
pub const TEXT: i64 = GRID;

pub fn snap_up(v: i64) -> i64 {
    (v + GRID - 1).div_euclid(GRID) * GRID
}

/// One pin, symbol-local, Y up. `(x, y)` is the connection point (the end
/// away from the body); `angle` is the KiCad pin direction from that point
/// toward the body: 0 = pin on the left edge, 180 = pin on the right edge.
#[derive(Debug, Clone, PartialEq)]
pub struct SymPin {
    pub number: String,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub angle: i32,
    pub len: i64,
    /// KiCad electrical type: `passive`, `power_in` or `power_out`.
    pub electrical: &'static str,
}

impl SymPin {
    /// Unit X direction a wire leaves this pin, away from the body. Pins are
    /// only ever horizontal, so there is no Y component to flip.
    pub fn out_dx(&self) -> i64 {
        if self.angle == 180 {
            1
        } else {
            -1
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// `wright:<name>`.
    pub lib_id: String,
    pub ref_prefix: String,
    pub pins: Vec<SymPin>,
    pub graphics: Vec<Sexpr>,
    /// Half-height of the body (fields sit just outside it).
    pub half_h: i64,
    /// Body extent left and right of the origin (positive), pins excluded.
    pub body_l: i64,
    pub body_r: i64,
    pub hide_pin_numbers: bool,
    pub hide_pin_names: bool,
}

pub const LIB: &str = "wright";

/// A symbol name KiCad's `LIB_ID` accepts (the baseline's `_sanitise`).
pub fn sanitise(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-+".contains(c) { c } else { '_' }).collect();
    if s.is_empty() {
        "PART".into()
    } else {
        s
    }
}

pub fn mm(v: i64) -> Sexpr {
    Sexpr::atom(Nm(v).kicad())
}

pub fn xy(x: i64, y: i64) -> Sexpr {
    Sexpr::list("xy", vec![mm(x), mm(y)])
}

pub fn at(x: i64, y: i64, angle: i32) -> Sexpr {
    Sexpr::list("at", vec![mm(x), mm(y), Sexpr::atom(angle.to_string())])
}

pub fn yes_no(head: &str, v: bool) -> Sexpr {
    Sexpr::list(head, vec![Sexpr::atom(if v { "yes" } else { "no" })])
}

/// `(effects (font (size 1.27 1.27)) [(justify ..)] [hide])`, the KiCad 8
/// form (8's parser rejects a property-level `(hide yes)`; the baseline measured it).
pub fn effects(justify: Option<&str>, hide: bool) -> Sexpr {
    let mut v = vec![Sexpr::list("font", vec![Sexpr::list("size", vec![mm(TEXT), mm(TEXT)])])];
    if let Some(j) = justify {
        v.push(Sexpr::list("justify", vec![Sexpr::atom(j)]));
    }
    if hide {
        v.push(Sexpr::atom("hide"));
    }
    Sexpr::list("effects", v)
}

fn stroke() -> Sexpr {
    Sexpr::list("stroke", vec![Sexpr::list("width", vec![Sexpr::atom("0.254")]), Sexpr::list("type", vec![Sexpr::atom("default")])])
}

fn fill(kind: &str) -> Sexpr {
    Sexpr::list("fill", vec![Sexpr::list("type", vec![Sexpr::atom(kind)])])
}

fn rect(x0: i64, y0: i64, x1: i64, y1: i64, fill_kind: &str) -> Sexpr {
    Sexpr::list("rectangle", vec![Sexpr::list("start", vec![mm(x0), mm(y0)]), Sexpr::list("end", vec![mm(x1), mm(y1)]), stroke(), fill(fill_kind)])
}

fn polyline(pts: &[(i64, i64)], fill_kind: &str) -> Sexpr {
    Sexpr::list("polyline", vec![Sexpr::list("pts", pts.iter().map(|&(x, y)| xy(x, y)).collect()), stroke(), fill(fill_kind)])
}

fn circle(cx: i64, cy: i64, r: i64) -> Sexpr {
    Sexpr::list("circle", vec![Sexpr::list("center", vec![mm(cx), mm(cy)]), Sexpr::list("radius", vec![mm(r)]), stroke(), fill("none")])
}

fn arc(s: (i64, i64), m: (i64, i64), e: (i64, i64)) -> Sexpr {
    Sexpr::list("arc", vec![Sexpr::list("start", vec![mm(s.0), mm(s.1)]), Sexpr::list("mid", vec![mm(m.0), mm(m.1)]), Sexpr::list("end", vec![mm(e.0), mm(e.1)]), stroke(), fill("none")])
}

/// Numeric pins in numeric order, then the rest by text (the baseline's `_pin_sort_key`).
fn pin_key(number: &str) -> (u8, u64, String) {
    match number.parse::<u64>() {
        Ok(n) => (0, n, String::new()),
        Err(_) => (1, 0, number.to_string()),
    }
}

/// Electrical type of an IC pin. A pin on a ground or supply net is a power
/// input; a regulator's output pin on a supply net is a power output, so its
/// rail needs no `PWR_FLAG` (two `power_out` on one net is ERC `pin_to_pin`,
/// the baseline measured it 2026-09-14). Everything else is passive, as every baseline
/// generated pin was: a symbol with invented input/output types would make
/// ERC report the generator's guesses as the design's faults.
fn ic_electrical(name: &str, role: Option<NetRole>) -> &'static str {
    let n = name.to_ascii_uppercase();
    match role {
        Some(NetRole::Power) if matches!(n.as_str(), "VOUT" | "VO" | "OUT" | "OUTPUT") => "power_out",
        Some(NetRole::Power | NetRole::Ground) => "power_in",
        _ => "passive",
    }
}

const LEFT_TOKENS: [&str; 9] = ["VCC", "VDD", "VIN", "VBUS", "VSS", "GND", "EN", "NRST", "RESET"];

/// Pin name chars to width, an over-estimate of KiCad's stroke font (the baseline's
/// `_CHAR_W_NM`: measured 0.94-1.24 mm per glyph at 1.27 mm).
pub fn text_w(s: &str) -> i64 {
    s.chars().count() as i64 * TEXT
}

/// The rectangular IC (the baseline's `_device_shape`): power and reset pins pulled to
/// the left, the rest split to fill both sides; rails at the top of each
/// side, grounds at the bottom.
fn ic_shape(part: &Part, netlist: &Netlist, lib_id: String) -> Shape {
    let role_of = |pin: &wright_eda::ir::Pin| pin.net.map(|n| netlist.nets[n].role);
    let mut items: Vec<&wright_eda::ir::Pin> = part.pins.iter().collect();
    items.sort_by_key(|p| (pin_key(&p.pad), p.name.clone()));
    let is_left = |p: &&wright_eda::ir::Pin| {
        let n = p.name.to_ascii_uppercase();
        let n = n.trim_start_matches(['+', '-', '/', '~']);
        LEFT_TOKENS.iter().any(|t| n.starts_with(t))
    };
    let mut left: Vec<&wright_eda::ir::Pin> = items.iter().copied().filter(is_left).collect();
    let rest: Vec<&wright_eda::ir::Pin> = items.iter().copied().filter(|p| !is_left(p)).collect();
    let half = items.len().div_ceil(2).saturating_sub(left.len()).min(rest.len());
    left.extend_from_slice(&rest[..half]);
    let mut right: Vec<&wright_eda::ir::Pin> = rest[half..].to_vec();
    let band = |p: &&wright_eda::ir::Pin| match role_of(p) {
        Some(NetRole::Power) => 0,
        Some(NetRole::Ground) => 2,
        _ => 1,
    };
    left.sort_by_key(band); // stable: number order within a band
    right.sort_by_key(band);

    let rows = left.len().max(right.len()).max(1) as i64;
    let half_h = snap_up((rows + 1) * PITCH / 2);
    let lw = left.iter().map(|p| text_w(&p.name)).max().unwrap_or(0);
    let rw = right.iter().map(|p| text_w(&p.name)).max().unwrap_or(0);
    let half_w = snap_up((lw.max(rw) + TEXT).max(2 * PITCH));
    let len = PITCH;
    let mut pins = Vec::new();
    for (side, x, angle) in [(&left, -(half_w + len), 0), (&right, half_w + len, 180)] {
        for (i, p) in side.iter().enumerate() {
            pins.push(SymPin { number: p.pad.clone(), name: p.name.clone(), x, y: half_h - PITCH * (i as i64 + 1), angle, len, electrical: ic_electrical(&p.name, role_of(p)) });
        }
    }
    Shape { lib_id, ref_prefix: part.kind.prefix().into(), pins, graphics: vec![rect(-half_w, -half_h, half_w, half_h, "background")], half_h, body_l: half_w, body_r: half_w, hide_pin_numbers: false, hide_pin_names: false }
}

/// KiCad `Connector_Generic:Conn_01xNN`: every pin on the left edge in
/// number order, a contact stub per pin inside a narrow body.
fn connector_shape(part: &Part, lib_id: String) -> Shape {
    let mut items: Vec<&wright_eda::ir::Pin> = part.pins.iter().collect();
    items.sort_by_key(|p| (pin_key(&p.pad), p.name.clone()));
    let n = items.len().max(1) as i64;
    let half_h = snap_up((n + 1) * PITCH / 2);
    let names_are_numbers = items.iter().all(|p| p.name == p.pad);
    let name_w = if names_are_numbers { 0 } else { items.iter().map(|p| text_w(&p.name)).max().unwrap_or(0) + TEXT };
    let right = snap_up(GRID + name_w);
    let left = GRID;
    let len = 2 * PITCH - left; // connection point at x = -5.08 as in Conn_01x03
    let mut pins = Vec::new();
    let mut graphics = vec![rect(-left, -half_h, right, half_h, "background")];
    for (i, p) in items.iter().enumerate() {
        let y = half_h - PITCH * (i as i64 + 1);
        graphics.push(rect(-left, y - 127_000, 0, y + 127_000, "none"));
        pins.push(SymPin { number: p.pad.clone(), name: p.name.clone(), x: -(left + len), y, angle: 0, len, electrical: "passive" });
    }
    Shape { lib_id, ref_prefix: part.kind.prefix().into(), pins, graphics, half_h, body_l: left, body_r: right, hide_pin_numbers: false, hide_pin_names: names_are_numbers }
}

/// Two-terminal glyphs, drawn horizontally: connection points at +/-3.81 mm
/// (KiCad `Device:R` puts them there, vertically).
fn two_pin_shape(part: &Part, lib_id: String) -> Shape {
    const END: i64 = 3 * GRID;
    let (graphics, inner, half_h, names): (Vec<Sexpr>, i64, i64, [&str; 2]) = match part.kind {
        PartKind::Resistor => (vec![rect(-2 * GRID, -800_000, 2 * GRID, 800_000, "none")], 2 * GRID, GRID, ["~", "~"]),
        PartKind::Ferrite => (vec![rect(-2 * GRID, -800_000, 2 * GRID, 800_000, "outline")], 2 * GRID, GRID, ["~", "~"]),
        PartKind::Capacitor => (
            vec![polyline(&[(-508_000, -2_032_000), (-508_000, 2_032_000)], "none"), polyline(&[(508_000, -2_032_000), (508_000, 2_032_000)], "none")],
            508_000,
            2 * GRID,
            ["~", "~"],
        ),
        PartKind::Inductor => {
            let r = 635_000;
            let g = (0..4).map(|k| {
                let x0 = -2 * GRID + k * 2 * r;
                arc((x0, 0), (x0 + r, r), (x0 + 2 * r, 0))
            });
            (g.collect(), 2 * GRID, GRID, ["~", "~"])
        }
        // Cathode (pin 1) left with the bar, anode (pin 2) right: KiCad
        // Device:D / Device:LED (pin 1 = K) and the plan's corrected convention.
        PartKind::Diode | PartKind::Led => (
            vec![polyline(&[(GRID, GRID), (GRID, -GRID), (-GRID, 0), (GRID, GRID)], "none"), polyline(&[(-GRID, GRID), (-GRID, -GRID)], "none")],
            GRID,
            GRID,
            ["K", "A"],
        ),
        PartKind::Switch => (vec![circle(-1_778_000, 0, 508_000), circle(1_778_000, 0, 508_000), polyline(&[(-2_286_000, GRID), (2_286_000, GRID)], "none")], 2_286_000, GRID, ["", ""]),
        PartKind::Battery => (
            vec![polyline(&[(-508_000, -2_032_000), (-508_000, 2_032_000)], "none"), rect(508_000, -1_016_000, 762_000, 1_016_000, "outline")],
            508_000,
            2 * GRID,
            ["", ""],
        ),
        _ => (vec![rect(-2 * GRID, -GRID, 2 * GRID, GRID, "none")], 2 * GRID, GRID, ["~", "~"]),
    };
    let mut items: Vec<&wright_eda::ir::Pin> = part.pins.iter().collect();
    items.sort_by_key(|p| (pin_key(&p.pad), p.name.clone()));
    let pins = items
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = if names[i].is_empty() { p.name.clone() } else { names[i].to_string() };
            let (x, angle) = if i == 0 { (-END, 0) } else { (END, 180) };
            SymPin { number: p.pad.clone(), name, x, y: 0, angle, len: END - inner, electrical: "passive" }
        })
        .collect();
    Shape { lib_id, ref_prefix: part.kind.prefix().into(), pins, graphics, half_h, body_l: inner, body_r: inner, hide_pin_numbers: part.kind.is_passive(), hide_pin_names: true }
}

/// KiCad `Connector:TestPoint`-like: an open circle on a lead.
fn testpoint_shape(part: &Part, lib_id: String) -> Shape {
    let p = &part.pins[0];
    let pins = vec![SymPin { number: p.pad.clone(), name: p.name.clone(), x: -PITCH, y: 0, angle: 0, len: PITCH - 762_000, electrical: "passive" }];
    Shape { lib_id, ref_prefix: part.kind.prefix().into(), pins, graphics: vec![circle(0, 0, 762_000)], half_h: GRID, body_l: 762_000, body_r: 762_000, hide_pin_numbers: true, hide_pin_names: true }
}

/// The shape for one part. Two-terminal kinds with any other pin count fall
/// back to the IC rectangle rather than dropping pins.
pub fn shape_for(part: &Part, netlist: &Netlist, lib_id: String) -> Shape {
    match part.kind {
        PartKind::Ic => ic_shape(part, netlist, lib_id),
        PartKind::Connector => connector_shape(part, lib_id),
        PartKind::Testpoint if part.pins.len() == 1 => testpoint_shape(part, lib_id),
        k if (k.is_passive() || matches!(k, PartKind::Switch | PartKind::Battery)) && part.pins.len() == 2 => two_pin_shape(part, lib_id),
        _ => ic_shape(part, netlist, lib_id),
    }
}

/// Shared name for every passive of a kind (ten decoupling caps share one
/// definition, as in KiCad); devices get one symbol each.
pub fn base_name(part: &Part) -> String {
    match part.kind {
        PartKind::Resistor => "R".into(),
        PartKind::Capacitor => "C".into(),
        PartKind::Inductor => "L".into(),
        PartKind::Diode => "D".into(),
        PartKind::Led => "LED".into(),
        PartKind::Ferrite => "FerriteBead".into(),
        _ => sanitise(&part.name),
    }
}

/// The `lib_symbols` entry (the baseline's `_lib_symbol`). KiCad keys it by the full
/// `Lib:Name` and names the unit sub-symbols after the bare name.
pub fn lib_symbol(shape: &Shape) -> Sexpr {
    let name = shape.lib_id.split_once(':').map(|(_, n)| n).unwrap_or(&shape.lib_id).to_string();
    let mut v = vec![Sexpr::str(shape.lib_id.clone())];
    if shape.hide_pin_numbers {
        v.push(Sexpr::list("pin_numbers", vec![Sexpr::atom("hide")]));
    }
    let mut pn = vec![Sexpr::list("offset", vec![Sexpr::atom("0.508")])];
    if shape.hide_pin_names {
        pn.push(Sexpr::atom("hide"));
    }
    v.push(Sexpr::list("pin_names", pn));
    v.push(yes_no("exclude_from_sim", false));
    v.push(yes_no("in_bom", true));
    v.push(yes_no("on_board", true));
    let fy = shape.half_h + TEXT + GRID / 2;
    v.push(Sexpr::list("property", vec![Sexpr::str("Reference"), Sexpr::str(shape.ref_prefix.clone()), at(0, fy, 0), effects(None, false)]));
    v.push(Sexpr::list("property", vec![Sexpr::str("Value"), Sexpr::str(name.clone()), at(0, -fy, 0), effects(None, false)]));
    v.push(Sexpr::list("property", vec![Sexpr::str("Footprint"), Sexpr::str(""), at(0, 0, 0), effects(None, true)]));
    v.push(Sexpr::list("property", vec![Sexpr::str("Datasheet"), Sexpr::str(""), at(0, 0, 0), effects(None, true)]));
    let mut body = vec![Sexpr::str(format!("{name}_0_1"))];
    body.extend(shape.graphics.iter().cloned());
    v.push(Sexpr::list("symbol", body));
    let mut unit = vec![Sexpr::str(format!("{name}_1_1"))];
    for p in &shape.pins {
        unit.push(Sexpr::list(
            "pin",
            vec![
                Sexpr::atom(p.electrical),
                Sexpr::atom("line"),
                at(p.x, p.y, p.angle),
                Sexpr::list("length", vec![mm(p.len)]),
                Sexpr::list("name", vec![Sexpr::str(p.name.clone()), effects(None, false)]),
                Sexpr::list("number", vec![Sexpr::str(p.number.clone()), effects(None, false)]),
            ],
        ));
    }
    v.push(Sexpr::list("symbol", unit));
    Sexpr::list("symbol", v)
}

pub const FLAG_LIB_ID: &str = "wright:PWR_FLAG";

/// KiCad `power:PWR_FLAG`, copied in shape: one hidden `power_out` pin at the
/// origin pointing up, length 0, the flag drawn above it.
pub fn flag_lib_symbol() -> Sexpr {
    Sexpr::list(
        "symbol",
        vec![
            Sexpr::str(FLAG_LIB_ID),
            Sexpr::list("power", vec![]),
            Sexpr::list("pin_numbers", vec![Sexpr::atom("hide")]),
            Sexpr::list("pin_names", vec![Sexpr::list("offset", vec![Sexpr::atom("0")]), Sexpr::atom("hide")]),
            yes_no("exclude_from_sim", false),
            yes_no("in_bom", true),
            yes_no("on_board", true),
            Sexpr::list("property", vec![Sexpr::str("Reference"), Sexpr::str("#FLG"), at(0, 1_905_000, 0), effects(None, true)]),
            Sexpr::list("property", vec![Sexpr::str("Value"), Sexpr::str("PWR_FLAG"), at(0, 3 * GRID, 0), effects(None, false)]),
            Sexpr::list("property", vec![Sexpr::str("Footprint"), Sexpr::str(""), at(0, 0, 0), effects(None, true)]),
            Sexpr::list("property", vec![Sexpr::str("Datasheet"), Sexpr::str(""), at(0, 0, 0), effects(None, true)]),
            Sexpr::list(
                "symbol",
                vec![
                    Sexpr::str("PWR_FLAG_0_0"),
                    Sexpr::list(
                        "pin",
                        vec![
                            Sexpr::atom("power_out"),
                            Sexpr::atom("line"),
                            at(0, 0, 90),
                            Sexpr::list("length", vec![Sexpr::atom("0")]),
                            Sexpr::list("name", vec![Sexpr::str("pwr"), effects(None, false)]),
                            Sexpr::list("number", vec![Sexpr::str("1"), effects(None, false)]),
                        ],
                    ),
                ],
            ),
            Sexpr::list("symbol", vec![Sexpr::str("PWR_FLAG_0_1"), polyline(&[(0, 0), (0, GRID), (-1_016_000, 1_905_000), (0, PITCH), (1_016_000, 1_905_000), (0, GRID)], "none")]),
        ],
    )
}
