//! `.kicad_pcb` writer. Replaces the Python baseline's `board.py emit_kicad_pcb` /
//! `_library_footprint_block`.
//!
//! Structure follows atopile `src/faebryk/exporters/pcb/kicad/transformer.py`
//! (`insert_footprint` copies the library footprint and sets position, refs
//! and pad nets; `insert_track` / `insert_via` / `insert_zone` append copper;
//! `move_fp` adds the footprint angle to every pad and text angle) and the
//! sexpr-pcb spec (dev-docs.kicad.org/en/file-formats/sexpr-pcb). The file is
//! KiCad 10's own format (`version 20260206`, the version
//! `kicad-cli pcb upgrade` writes and the installed `.kicad_mod` files carry):
//! nets are named on each item (`(net "GND")`), with no net-number table,
//! which is how KiCad 10 itself saves a board (checked 2026-10-24 by
//! upgrading a 20240108 file with kicad-cli 10.0.6). Deviation from the
//! plan's 20240108 header, for that reason: the embedded library footprints
//! are 20260206 text.
//!
//! Conventions KiCad's reader imposes, each checked against kicad-cli 10.0.6
//! on 2026-10-24 (scratch boards, DRC positions):
//! * pad and text `(at x y a)`: position footprint-local, **angle absolute**
//!   (footprint angle + the pad's own);
//! * graphics (`fp_line`, `fp_rect`, ...): footprint-local;
//! * zones inside a footprint (the ESP32 antenna keepout): **board
//!   coordinates**, so their points go through [`Placed::to_board`];
//! * a bottom-side footprint stores its children already mirrored in X with
//!   F/B layers swapped, so KiCad's `pos + rotate(local)` equals
//!   [`Placed::to_board`] (mirror X, then rotate).

use std::collections::{BTreeSet, HashMap};
use wright_eda::sexpr::{uuid_for, Sexpr};
use wright_eda::{Board, Footprint, Netlist, Nm, PadShape, Placed, Pt, Side};

/// KiCad 10 board file format version.
pub const PCB_VERSION: &str = "20260206";

/// KiCad 9/10 layer table (ids as `kicad-cli pcb upgrade` renumbers them).
const LAYERS: &[(u32, &str, &str, Option<&str>)] = &[
    (0, "F.Cu", "signal", None),
    (2, "B.Cu", "signal", None),
    (9, "F.Adhes", "user", Some("F.Adhesive")),
    (11, "B.Adhes", "user", Some("B.Adhesive")),
    (13, "F.Paste", "user", None),
    (15, "B.Paste", "user", None),
    (5, "F.SilkS", "user", Some("F.Silkscreen")),
    (7, "B.SilkS", "user", Some("B.Silkscreen")),
    (1, "F.Mask", "user", None),
    (3, "B.Mask", "user", None),
    (17, "Dwgs.User", "user", Some("User.Drawings")),
    (19, "Cmts.User", "user", Some("User.Comments")),
    (21, "Eco1.User", "user", Some("User.Eco1")),
    (23, "Eco2.User", "user", Some("User.Eco2")),
    (25, "Edge.Cuts", "user", None),
    (27, "Margin", "user", None),
    (31, "F.CrtYd", "user", Some("F.Courtyard")),
    (29, "B.CrtYd", "user", Some("B.Courtyard")),
    (35, "F.Fab", "user", None),
    (33, "B.Fab", "user", None),
];

fn atom(s: impl Into<String>) -> Sexpr {
    Sexpr::atom(s)
}
fn st(s: impl Into<String>) -> Sexpr {
    Sexpr::str(s)
}
fn mm(v: Nm) -> Sexpr {
    atom(v.kicad())
}
fn l(head: &str, rest: Vec<Sexpr>) -> Sexpr {
    Sexpr::list(head, rest)
}
fn xy(head: &str, p: Pt) -> Sexpr {
    l(head, vec![mm(p.x), mm(p.y)])
}
fn uuid(seed: &str) -> Sexpr {
    l("uuid", vec![st(uuid_for(seed))])
}

/// An angle in degrees as KiCad writes it: normalised to [0, 360), shortest.
fn angle_text(a: f64) -> String {
    let a = a.rem_euclid(360.0);
    let r = (a * 1e6).round() / 1e6;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}").trim_end_matches('0').to_string()
    }
}

/// The net KiCad gives a no-connect pin: `SCH_PIN::GetDefaultNetName`
/// (eeschema/sch_pin.cpp), as the baseline's `board.py kicad_unconnected_net`
/// transcribed it: `unconnected-(REF-NAME-PadN)`, or `unconnected-(REF-PadN)`
/// when the name equals the number; `/` escapes to `{slash}`. A no-connect
/// pad left with no net reads to schematic parity as "Pad missing net
/// given by schematic" (33 of those on the baseline's 2026-09-14 demo board).
pub fn unconnected_net(refdes: &str, pin_name: &str, pad: &str) -> String {
    let esc = |t: &str| t.replace('/', "{slash}").replace(['\n', '\r'], "");
    if !pin_name.is_empty() && pin_name != pad {
        format!("unconnected-({refdes}-{}-Pad{})", esc(pin_name), esc(pad))
    } else {
        format!("unconnected-({refdes}-Pad{})", esc(pad))
    }
}

/// Pad number -> net name for one part (no-connect pins get KiCad's
/// `unconnected-(...)` name; pads the circuit does not name get no net).
pub fn pad_nets(netlist: &Netlist, part: usize) -> HashMap<String, String> {
    let p = &netlist.parts[part];
    p.pins
        .iter()
        .filter_map(|pin| match pin.net {
            Some(n) => Some((pin.pad.clone(), netlist.nets[n].name.clone())),
            None if pin.no_connect => Some((pin.pad.clone(), unconnected_net(&p.refdes, &pin.name, &pin.pad))),
            None => None,
        })
        .collect()
}

/// The schematic symbol instance uuid the board links each footprint to
/// (`(path "/<uuid>")`). Lane B should seed its symbol uuids the same way
/// (`uuid_for("sym/<refdes>")`) so KiCad's "Update PCB from Schematic"
/// matches by path; parity itself matches by reference.
pub fn symbol_uuid(refdes: &str) -> String {
    uuid_for(&format!("sym/{refdes}"))
}

/// A `.kicad_mod`-shaped s-expression for a footprint that carries no
/// library source (synthetic footprints in other lanes' tests).
fn synth_source(f: &Footprint) -> Sexpr {
    let text = |name: &str, layer: &str| {
        l("property", vec![st(name), st(if name == "Reference" { "REF**" } else { "" }), l("at", vec![atom("0"), atom("0")]), l("layer", vec![st(layer)]), l("effects", vec![l("font", vec![l("size", vec![atom("1"), atom("1")]), l("thickness", vec![atom("0.15")])])])])
    };
    let mut items = vec![atom("footprint"), st(f.lib_id.clone()), l("layer", vec![st("F.Cu")]), text("Reference", "F.Fab"), text("Value", "F.Fab")];
    items.push(l("fp_rect", vec![xy("start", f.courtyard.min), xy("end", f.courtyard.max), l("stroke", vec![l("width", vec![atom("0.05")]), l("type", vec![atom("solid")])]), l("fill", vec![atom("no")]), l("layer", vec![st("F.CrtYd")])]));
    let tht = f.pads.iter().any(|p| p.drill.is_some());
    items.push(l("attr", vec![atom(if tht { "through_hole" } else { "smd" })]));
    for p in &f.pads {
        let (kind, mut layers) = match (p.drill, p.plated) {
            (Some(_), true) => ("thru_hole", vec![st("*.Cu"), st("*.Mask")]),
            (Some(_), false) => ("np_thru_hole", vec![st("*.Cu"), st("*.Mask")]),
            (None, _) => ("smd", vec![st("F.Cu"), st("F.Paste"), st("F.Mask")]),
        };
        if p.drill.is_none() && !p.layers.is_empty() {
            layers = p.layers.iter().map(|l| st(l.kicad())).collect();
        }
        let shape = match p.shape {
            PadShape::Rect | PadShape::Other => "rect",
            PadShape::RoundRect(_) => "roundrect",
            PadShape::Circle => "circle",
            PadShape::Oval => "oval",
        };
        let mut at = vec![mm(p.at.x), mm(p.at.y)];
        if p.rot_deg != 0.0 {
            at.push(atom(angle_text(p.rot_deg)));
        }
        let mut v = vec![atom("pad"), st(p.number.clone()), atom(kind), atom(shape), l("at", at), l("size", vec![mm(p.size.0), mm(p.size.1)])];
        if let Some(d) = p.drill {
            v.push(l("drill", vec![mm(d)]));
        }
        v.push(l("layers", layers));
        if let PadShape::RoundRect(r) = p.shape {
            v.push(l("roundrect_rratio", vec![atom(format!("{r}"))]));
        }
        items.push(Sexpr::List(v));
    }
    Sexpr::List(items)
}

struct Ctx<'a> {
    placed: &'a Placed,
    bottom: bool,
    angle: f64,
    refdes: &'a str,
    value: &'a str,
    nets: HashMap<String, String>,
    poured: &'a BTreeSet<String>,
}

fn flip_layer(name: &str) -> String {
    if let Some(r) = name.strip_prefix("F.") {
        format!("B.{r}")
    } else if let Some(r) = name.strip_prefix("B.") {
        format!("F.{r}")
    } else {
        name.to_string()
    }
}

fn neg_atom(e: &Sexpr) -> Sexpr {
    match e {
        Sexpr::Atom(a) => a.parse::<f64>().map(|v| mm(-Nm::mm(v))).unwrap_or_else(|_| e.clone()),
        _ => e.clone(),
    }
}

fn num_at(e: &Sexpr, i: usize) -> f64 {
    e.num(i).unwrap_or(0.0)
}

/// Mirror X of points and swap F/B layers, recursively (bottom side only).
fn mirror(e: &Sexpr) -> Sexpr {
    match e {
        Sexpr::List(v) => match e.head() {
            Some("start" | "end" | "mid" | "center" | "xy") if v.len() >= 3 => {
                let mut o = v.clone();
                o[1] = neg_atom(&v[1]);
                Sexpr::List(o)
            }
            Some("layer" | "layers") => Sexpr::List(v.iter().map(|c| if let Sexpr::Str(s) = c { st(flip_layer(s)) } else { c.clone() }).collect()),
            _ => Sexpr::List(v.iter().map(mirror).collect()),
        },
        _ => e.clone(),
    }
}

/// `(at x y a)` of a pad or text: local position (mirrored on the bottom),
/// absolute angle.
fn place_at(at: &Sexpr, ctx: &Ctx) -> Sexpr {
    let x = at.items().get(1).cloned().unwrap_or(atom("0"));
    let y = at.items().get(2).cloned().unwrap_or(atom("0"));
    let a = num_at(at, 3);
    let local = if ctx.bottom { -a } else { a };
    let mut v = vec![atom("at"), if ctx.bottom { neg_atom(&x) } else { x }, y];
    let abs = ctx.angle + local;
    if angle_text(abs) != "0" || at.items().len() > 3 {
        v.push(atom(angle_text(abs)));
    }
    // `unlocked` and other trailing atoms are kept.
    v.extend(at.items().iter().skip(4).filter(|c| matches!(c, Sexpr::Atom(s) if s.parse::<f64>().is_err())).cloned());
    Sexpr::List(v)
}

/// A text-bearing child (`property`, `fp_text`): angle, mirror, layer.
fn place_text(e: &Sexpr, ctx: &Ctx) -> Sexpr {
    let mut v: Vec<Sexpr> = Vec::new();
    for (i, c) in e.items().iter().enumerate() {
        let c = match c.head() {
            Some("uuid" | "tstamp") => continue,
            Some("at") => place_at(c, ctx),
            Some("layer") if ctx.bottom => mirror(c),
            Some("effects") if ctx.bottom => {
                let mut fx: Vec<Sexpr> = c.items().to_vec();
                match fx.iter().position(|x| x.head() == Some("justify")) {
                    Some(j) => {
                        if !fx[j].items().iter().any(|a| matches!(a, Sexpr::Atom(s) if s == "mirror")) {
                            if let Sexpr::List(jv) = &mut fx[j] {
                                jv.push(atom("mirror"));
                            }
                        }
                    }
                    None => fx.push(l("justify", vec![atom("mirror")])),
                }
                Sexpr::List(fx)
            }
            _ => c.clone(),
        };
        // property "Reference"/"Value" text substitution
        if i == 2 && e.head() == Some("property") {
            match e.text(1) {
                Some("Reference") => {
                    v.push(st(ctx.refdes));
                    continue;
                }
                Some("Value") => {
                    v.push(st(ctx.value));
                    continue;
                }
                _ => {}
            }
        }
        v.push(c);
    }
    if e.head() == Some("property") {
        if let Some(name) = e.text(1) {
            v.push(uuid(&format!("prop/{}/{name}", ctx.refdes)));
        }
    }
    Sexpr::List(v)
}

fn place_pad(e: &Sexpr, ctx: &Ctx, index: usize) -> Sexpr {
    let number = e.text(1).unwrap_or("").to_string();
    let net = ctx.nets.get(&number).cloned();
    let mut v: Vec<Sexpr> = Vec::new();
    for (i, c) in e.items().iter().enumerate() {
        if i < 4 && !matches!(c, Sexpr::List(_)) {
            v.push(c.clone());
            continue;
        }
        match c.head() {
            Some("at") => v.push(place_at(c, ctx)),
            Some("net" | "uuid" | "tstamp" | "pinfunction" | "pintype") => {}
            Some("zone_connect") if net.as_ref().is_some_and(|n| ctx.poured.contains(n)) => {}
            _ => v.push(if ctx.bottom { mirror(c) } else { c.clone() }),
        }
    }
    if let Some(n) = &net {
        v.push(l("net", vec![st(n.clone())]));
        // Solid connection to a pour (KiCad `zone_connect 2`): the baseline's thermal
        // reliefs starved (DRC `starved_thermal`, five SMD pads on the
        // 2026-09-16 ESP32 board, J3 THT pad 3 on the 10/24 servo board).
        if ctx.poured.contains(n) {
            v.push(l("zone_connect", vec![atom("2")]));
        }
    }
    v.push(uuid(&format!("pad/{}/{index}:{number}", ctx.refdes)));
    Sexpr::List(v)
}

/// A zone inside a footprint (a keepout): KiCad stores its outline in
/// board coordinates.
fn place_fp_zone(e: &Sexpr, ctx: &Ctx) -> Sexpr {
    fn walk(e: &Sexpr, ctx: &Ctx) -> Sexpr {
        match e {
            Sexpr::List(v) => match e.head() {
                Some("xy") if v.len() >= 3 => xy("xy", ctx.placed.to_board(Pt::mm(num_at(e, 1), num_at(e, 2)))),
                Some("layer" | "layers") if ctx.bottom => mirror(e),
                _ => Sexpr::List(v.iter().map(|c| walk(c, ctx)).collect()),
            },
            _ => e.clone(),
        }
    }
    walk(e, ctx)
}

const DROP_TOP: &[&str] = &["version", "generator", "generator_version", "layer", "uuid", "at", "tedit", "tstamp", "path", "sheetname", "sheetfile", "locked", "placed"];

fn place_footprint_with(src: &Sexpr, ctx: &Ctx) -> anyhow::Result<Sexpr> {
    let p = ctx.placed;
    let side = if ctx.bottom { "B.Cu" } else { "F.Cu" };
    let mut at = vec![mm(p.at.x), mm(p.at.y)];
    if ctx.angle != 0.0 {
        at.push(atom(angle_text(ctx.angle)));
    }
    let mut head = vec![atom("footprint"), st(p.footprint.lib_id.clone()), l("layer", vec![st(side)]), uuid(&format!("fp/{}", ctx.refdes)), l("at", at)];
    let mut body: Vec<Sexpr> = Vec::new();
    let mut pad_index = 0;
    for c in src.items().iter().skip(2) {
        let Some(h) = c.head() else { continue };
        if DROP_TOP.contains(&h) {
            continue;
        }
        body.push(match h {
            "property" | "fp_text" => place_text(c, ctx),
            "pad" => {
                pad_index += 1;
                place_pad(c, ctx, pad_index - 1)
            }
            "zone" => place_fp_zone(c, ctx),
            "model" | "descr" | "tags" | "attr" | "embedded_fonts" | "embedded_files" | "duplicate_pad_numbers_are_jumpers" | "net_tie_pad_groups" | "solder_mask_margin" | "solder_paste_margin" | "solder_paste_ratio" | "clearance" | "zone_connect" | "private_layers" | "jumper_pad_groups" | "component_classes" => c.clone(),
            _ if ctx.bottom => mirror(c),
            _ => c.clone(),
        });
    }
    // path / sheetname go after the properties, where KiCad writes them.
    let k = body.iter().position(|c| !matches!(c.head(), Some("descr" | "tags" | "property"))).unwrap_or(body.len());
    body.insert(k, l("path", vec![st(format!("/{}", symbol_uuid(ctx.refdes)))]));
    body.insert(k + 1, l("sheetname", vec![st("/")]));
    head.extend(body);
    Ok(Sexpr::List(head))
}

fn mounting_hole(i: usize, at: Pt, d: Nm) -> Sexpr {
    // Shape of KiCad's MountingHole:MountingHole_*mm footprints; `board_only`
    // so parity does not ask the schematic for it.
    let r = format!("H{}", i + 1);
    let prop = |name: &str, val: &str, layer: &str| l("property", vec![st(name), st(val), l("at", vec![atom("0"), atom("0")]), l("layer", vec![st(layer)]), l("hide", vec![atom("yes")]), uuid(&format!("hole/{i}/{name}")), l("effects", vec![l("font", vec![l("size", vec![atom("1"), atom("1")]), l("thickness", vec![atom("0.15")])])])]);
    let cy = d.0 / 2 + 250_000;
    l(
        "footprint",
        vec![
            st("MountingHole:wright_NPTH"),
            l("layer", vec![st("F.Cu")]),
            uuid(&format!("hole/{i}")),
            xy("at", at),
            prop("Reference", &r, "F.SilkS"),
            prop("Value", "MountingHole", "F.Fab"),
            l("attr", vec![atom("exclude_from_pos_files"), atom("exclude_from_bom"), atom("board_only")]),
            l("fp_circle", vec![xy("center", Pt::new(0, 0)), xy("end", Pt::new(cy, 0)), l("stroke", vec![l("width", vec![atom("0.05")]), l("type", vec![atom("solid")])]), l("fill", vec![atom("no")]), l("layer", vec![st("F.CrtYd")])]),
            l("pad", vec![st(""), atom("np_thru_hole"), atom("circle"), xy("at", Pt::new(0, 0)), l("size", vec![mm(d), mm(d)]), l("drill", vec![mm(d)]), l("layers", vec![st("*.Cu"), st("*.Mask")]), uuid(&format!("hole/{i}/pad"))]),
        ],
    )
}

/// The whole board as an s-expression tree.
pub fn pcb_sexpr(netlist: &Netlist, board: &Board, title: &str) -> anyhow::Result<Sexpr> {
    let net_name = |n: usize| -> anyhow::Result<String> { netlist.nets.get(n).map(|x| x.name.clone()).ok_or_else(|| anyhow::anyhow!("net index {n} is outside the netlist ({} nets)", netlist.nets.len())) };
    let mut poured = BTreeSet::new();
    for z in &board.zones {
        poured.insert(net_name(z.net)?);
    }
    let mut seen = BTreeSet::new();
    for p in &board.parts {
        anyhow::ensure!(seen.insert(p.part), "netlist part {} is placed twice", p.part);
    }
    let mut items = vec![
        atom("kicad_pcb"),
        l("version", vec![atom(PCB_VERSION)]),
        l("generator", vec![st("wright")]),
        l("generator_version", vec![st("10.0")]),
        l("general", vec![l("thickness", vec![atom("1.6")]), l("legacy_teardrops", vec![atom("no")])]),
        l("paper", vec![st("A4")]),
        l("title_block", vec![l("title", vec![st(title)])]),
        l("layers", LAYERS.iter().map(|(id, name, kind, user)| {
            let mut v = vec![atom(id.to_string()), st(*name), atom(*kind)];
            if let Some(u) = user {
                v.push(st(*u));
            }
            Sexpr::List(v)
        }).collect()),
        l("setup", vec![l("pad_to_mask_clearance", vec![atom("0")]), l("allow_soldermask_bridges_in_footprints", vec![atom("no")])]),
    ];
    for p in &board.parts {
        let part = netlist.parts.get(p.part).ok_or_else(|| anyhow::anyhow!("board part refers to netlist part {} of {}", p.part, netlist.parts.len()))?;
        let src = if p.footprint.source.trim().is_empty() { synth_source(&p.footprint) } else { Sexpr::parse(&p.footprint.source).map_err(|e| anyhow::anyhow!("{}: footprint source: {e}", part.refdes))? };
        let ctx = Ctx { placed: p, bottom: p.side == Side::Bottom, angle: (p.rot.rem_euclid(4) * 90) as f64, refdes: &part.refdes, value: &part.value, nets: pad_nets(netlist, p.part), poured: &poured };
        items.push(place_footprint_with(&src, &ctx)?);
    }
    for (i, (at, d)) in board.holes.iter().enumerate() {
        items.push(mounting_hole(i, *at, *d));
    }
    items.push(l(
        "gr_rect",
        vec![xy("start", board.outline.min), xy("end", board.outline.max), l("stroke", vec![l("width", vec![atom("0.05")]), l("type", vec![atom("default")])]), l("fill", vec![atom("no")]), l("layer", vec![st("Edge.Cuts")]), uuid("edge")],
    ));
    for (i, t) in board.tracks.iter().enumerate() {
        items.push(l("segment", vec![xy("start", t.a), xy("end", t.b), l("width", vec![mm(t.width)]), l("layer", vec![st(t.layer.kicad())]), l("net", vec![st(net_name(t.net)?)]), uuid(&format!("seg/{i}"))]));
    }
    for (i, v) in board.vias.iter().enumerate() {
        items.push(l("via", vec![xy("at", v.at), l("size", vec![mm(v.diameter)]), l("drill", vec![mm(v.drill)]), l("layers", vec![st("F.Cu"), st("B.Cu")]), l("net", vec![st(net_name(v.net)?)]), uuid(&format!("via/{i}"))]));
    }
    for (i, z) in board.zones.iter().enumerate() {
        let name = net_name(z.net)?;
        anyhow::ensure!(z.outline.len() >= 3, "zone {i} ({name}) has {} outline points", z.outline.len());
        items.push(l(
            "zone",
            vec![
                l("net", vec![st(name.clone())]),
                l("layer", vec![st(z.layer.kicad())]),
                uuid(&format!("zone/{i}")),
                l("name", vec![st(format!("{name}_{}", z.layer.kicad()))]),
                l("hatch", vec![atom("edge"), atom("0.5")]),
                l("connect_pads", vec![l("clearance", vec![mm(z.clearance)])]),
                l("min_thickness", vec![mm(z.min_thickness)]),
                l("filled_areas_thickness", vec![atom("no")]),
                // Unfilled on purpose: KiCad computes the fill
                // (`pcb drc --refill-zones`), the baseline's `board.py write_board`.
                l("fill", vec![l("thermal_gap", vec![mm(z.clearance)]), l("thermal_bridge_width", vec![mm(z.thermal_spoke)])]),
                l("polygon", vec![l("pts", z.outline.iter().map(|p| xy("xy", *p)).collect())]),
            ],
        ));
    }
    items.push(l("embedded_fonts", vec![atom("no")]));
    Ok(Sexpr::List(items))
}

/// `.kicad_pcb` text.
pub fn write_pcb(netlist: &Netlist, board: &Board, title: &str) -> anyhow::Result<String> {
    Ok(pcb_sexpr(netlist, board, title)?.to_kicad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconnected_names_match_kicad() {
        // eeschema SCH_PIN::GetDefaultNetName; the CH340C case lane B checked.
        assert_eq!(unconnected_net("U2", "CTS", "9"), "unconnected-(U2-CTS-Pad9)");
        assert_eq!(unconnected_net("R1", "1", "1"), "unconnected-(R1-Pad1)");
        assert_eq!(unconnected_net("U1", "A/B", "3"), "unconnected-(U1-A{slash}B-Pad3)");
    }

    #[test]
    fn angles_are_normalised_and_short() {
        assert_eq!(angle_text(0.0), "0");
        assert_eq!(angle_text(-90.0), "270");
        assert_eq!(angle_text(450.0), "90");
        assert_eq!(angle_text(12.5), "12.5");
    }
}
