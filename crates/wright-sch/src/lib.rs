//! `.kicad_sch` emitter (lane B of docs/electronics-plan.md). Replaces the Python baseline's
//! `engine/silkscreen/schematic.py`: symbols are generated and embedded in
//! `lib_symbols` (opens with no library installed), each pin gets a short
//! wire stub to a *global* label named exactly like the board net (the baseline's
//! first parity run found `/NET` vs `NET` on every net), power and
//! no-connect flags are explicit, and refs come from `Netlist`.
//! Sheet frame is Y down; symbol libraries are Y up: [`pin_on_sheet`] is the
//! only function that crosses (the baseline's `_pin_on_sheet`).
//!
//! File shape follows the baseline's `emit_kicad_sch` (`schematic.py:1567`), which was
//! measured ERC-clean on KiCad 10.0.6, and the KiCad s-expression schematic
//! spec (dev-docs.kicad.org/en/file-formats/sexpr-schematic): format version
//! `20231120` (KiCad 8; KiCad 10 reads it without migration), one
//! `lib_symbols` entry per distinct symbol, `instances` on every symbol with
//! the project named after `title` so references stick, and
//! `sheet_instances`. `PWR_FLAG` goes on every net with a `power_in` pin and
//! no `power_out` pin (the baseline's `_power_flags`), on its own wire and label so it
//! draws clear of the parts.

mod symbol;

use std::collections::BTreeMap;
use symbol::{at, effects, flag_lib_symbol, lib_symbol, mm, shape_for, snap_up, text_w, xy, yes_no, Shape, SymPin, FLAG_LIB_ID, GRID, LIB, PITCH, TEXT};
use wright_eda::sexpr::{uuid_for, Sexpr};
use wright_eda::{Footprint, Netlist};

/// Wire stub between a pin and its label (the baseline's `_STUB_NM`).
const STUB: i64 = PITCH;
/// Space a label's arrow shape adds beyond its text.
const LABEL_PAD: i64 = 2 * TEXT;
/// Distance kept from every page edge (the baseline's `MARGIN_NM`, 20.32 mm).
const MARGIN: i64 = 16 * GRID;
/// Height kept clear above the bottom margin for KiCad's title block (the baseline's
/// `TITLE_BLOCK_H_NM`, 48.26 mm); kept across the whole width for simplicity.
const TITLE_H: i64 = 38 * GRID;
const GAP: i64 = 2 * PITCH;
const ROW_GAP: i64 = 3 * PITCH;
/// Landscape paper sizes, smallest first (the baseline's `PAPERS`, extended to A0).
const PAPERS: [(&str, i64, i64); 5] = [("A4", 297_000_000, 210_000_000), ("A3", 420_000_000, 297_000_000), ("A2", 594_000_000, 420_000_000), ("A1", 841_000_000, 594_000_000), ("A0", 1_189_000_000, 841_000_000)];

/// A pin's connection point on the sheet. **The one Y flip in this crate**:
/// symbols are drawn Y up, the sheet is Y down, and no instance is rotated.
pub fn pin_on_sheet(anchor: (i64, i64), pin_x: i64, pin_y: i64) -> (i64, i64) {
    (anchor.0 + pin_x, anchor.1 - pin_y)
}

enum Item {
    Part(usize),
    Flag(usize),
}

struct Cell {
    item: Item,
    left: i64,
    right: i64,
    up: i64,
    down: i64,
}

fn label_w(net: &str) -> i64 {
    text_w(net) + LABEL_PAD
}

fn part_cell(i: usize, shape: &Shape, netlist: &Netlist, value: &str, refdes: &str) -> Cell {
    let part = &netlist.parts[i];
    let net_of = |p: &SymPin| part.pins.iter().find(|q| q.pad == p.number).and_then(|q| q.net).map(|n| netlist.nets[n].name.as_str());
    let fields = text_w(value).max(text_w(refdes)) / 2 + GRID;
    let (mut left, mut right) = (shape.body_l.max(fields), shape.body_r.max(fields));
    for p in &shape.pins {
        let reach = p.x.abs() + net_of(p).map(|n| STUB + label_w(n)).unwrap_or(GRID);
        if p.out_dx() < 0 {
            left = left.max(reach);
        } else {
            right = right.max(reach);
        }
    }
    let field = shape.half_h + TEXT + GRID / 2 + TEXT;
    Cell { item: Item::Part(i), left, right, up: field, down: field }
}

fn flag_cell(n: usize, netlist: &Netlist) -> Cell {
    let half_text = text_w("PWR_FLAG") / 2 + GRID;
    Cell { item: Item::Flag(n), left: half_text, right: half_text.max(STUB + label_w(&netlist.nets[n].name)), up: 3 * GRID + TEXT, down: GRID }
}

/// Row-pack the cells on the smallest paper that holds them (the baseline's `_lay_out`:
/// ICs first, then the rest, A4 then up). Returns anchors per cell.
fn lay_out(cells: &[Cell]) -> (&'static str, Vec<(i64, i64)>) {
    'paper: for (k, &(name, w, h)) in PAPERS.iter().enumerate() {
        let (x_max, y_max) = (w - MARGIN, h - MARGIN - TITLE_H);
        let mut out = Vec::with_capacity(cells.len());
        let (mut x, mut top, mut bottom) = (MARGIN, MARGIN, MARGIN);
        for c in cells {
            if x + c.left + c.right > x_max && x > MARGIN {
                x = MARGIN;
                top = bottom + ROW_GAP;
            }
            let ax = snap_up(x + c.left);
            let ay = snap_up(top + c.up);
            if (ax + c.right > x_max || ay + c.down > y_max) && k + 1 < PAPERS.len() {
                continue 'paper;
            }
            bottom = bottom.max(ay + c.down);
            x = ax + c.right + GAP;
            out.push((ax, ay));
        }
        return (name, out);
    }
    unreachable!("the last paper always returns")
}

fn prop(key: &str, val: &str, x: i64, y: i64, eff: Sexpr) -> Sexpr {
    Sexpr::list("property", vec![Sexpr::str(key), Sexpr::str(val), at(x, y, 0), eff])
}

fn uuid(seed: &str) -> Sexpr {
    Sexpr::list("uuid", vec![Sexpr::str(uuid_for(seed))])
}

#[allow(clippy::too_many_arguments)]
fn instance(lib_id: &str, x: i64, y: i64, on_board: bool, seed: &str, props: Vec<Sexpr>, pins: &[String], project: &str, sheet: &str, refdes: &str) -> Sexpr {
    let mut v = vec![
        Sexpr::list("lib_id", vec![Sexpr::str(lib_id)]),
        at(x, y, 0),
        Sexpr::list("unit", vec![Sexpr::atom("1")]),
        yes_no("exclude_from_sim", false),
        yes_no("in_bom", on_board),
        yes_no("on_board", on_board),
        yes_no("dnp", false),
        uuid(seed),
    ];
    v.extend(props);
    for p in pins {
        v.push(Sexpr::list("pin", vec![Sexpr::str(p.clone()), uuid(&format!("{seed}/pin/{p}"))]));
    }
    v.push(Sexpr::list(
        "instances",
        vec![Sexpr::list(
            "project",
            vec![Sexpr::str(project), Sexpr::list("path", vec![Sexpr::str(format!("/{sheet}")), Sexpr::list("reference", vec![Sexpr::str(refdes)]), Sexpr::list("unit", vec![Sexpr::atom("1")])])],
        )],
    ));
    Sexpr::list("symbol", v)
}

fn wire(a: (i64, i64), b: (i64, i64), seed: &str) -> Sexpr {
    Sexpr::list(
        "wire",
        vec![
            Sexpr::list("pts", vec![xy(a.0, a.1), xy(b.0, b.1)]),
            Sexpr::list("stroke", vec![Sexpr::list("width", vec![Sexpr::atom("0")]), Sexpr::list("type", vec![Sexpr::atom("default")])]),
            uuid(seed),
        ],
    )
}

/// A global label at `p`; `dx` is the direction its wire arrived going, so
/// the text runs away from the part.
fn global_label(net: &str, p: (i64, i64), dx: i64, seed: &str) -> Sexpr {
    let (angle, justify) = if dx > 0 { (0, "left") } else { (180, "right") };
    Sexpr::list(
        "global_label",
        vec![
            Sexpr::str(net),
            Sexpr::list("shape", vec![Sexpr::atom("passive")]),
            at(p.0, p.1, angle),
            yes_no("fields_autoplaced", true),
            effects(Some(justify), false),
            uuid(seed),
            Sexpr::list("property", vec![Sexpr::str("Intersheetrefs"), Sexpr::str("${INTERSHEET_REFS}"), at(p.0, p.1, angle), effects(Some(justify), true)]),
        ],
    )
}

/// `.kicad_sch` text. `footprints[i]` is part `i`'s footprint (its
/// `lib_id` goes in the symbol's Footprint property, for parity). `title`
/// is the title block's title and the project name in every symbol's
/// `instances`, so it should be the file stem (lane E passes the stem).
pub fn write_schematic(netlist: &Netlist, footprints: &[Footprint], title: &str) -> anyhow::Result<String> {
    let mut problems = Vec::new();
    if footprints.len() != netlist.parts.len() {
        problems.push(format!("{} footprints for {} parts (one per part, in part order)", footprints.len(), netlist.parts.len()));
    }
    for p in &netlist.parts {
        for pin in &p.pins {
            match pin.net {
                Some(n) if n >= netlist.nets.len() => problems.push(format!("{}.{} names net {n}, past the {} nets", p.refdes, pin.name, netlist.nets.len())),
                None if !pin.no_connect => problems.push(format!("{}.{} is on no net and not no_connect", p.refdes, pin.name)),
                _ => {}
            }
        }
    }
    anyhow::ensure!(problems.is_empty(), "wright-sch: {}", problems.join("; "));

    // Shapes: passives of one kind share a definition; a sanitised name that
    // collides with a different shape gets a suffix.
    let mut defs: BTreeMap<String, Shape> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut shapes: Vec<Shape> = Vec::new();
    for part in &netlist.parts {
        let base = symbol::base_name(part);
        let mut k = 1;
        let shape = loop {
            let id = if k == 1 { format!("{LIB}:{base}") } else { format!("{LIB}:{base}_{k}") };
            let s = shape_for(part, netlist, id.clone());
            match defs.get(&id) {
                None => {
                    defs.insert(id.clone(), s.clone());
                    order.push(id);
                    break s;
                }
                Some(d) if *d == s => break s,
                Some(_) => k += 1,
            }
        };
        shapes.push(shape);
    }

    // Nets needing a PWR_FLAG: a power_in pin and no power_out pin.
    let mut has_in = vec![false; netlist.nets.len()];
    let mut has_out = vec![false; netlist.nets.len()];
    for (i, part) in netlist.parts.iter().enumerate() {
        for sp in &shapes[i].pins {
            if let Some(n) = part.pins.iter().find(|q| q.pad == sp.number).and_then(|q| q.net) {
                has_in[n] |= sp.electrical == "power_in";
                has_out[n] |= sp.electrical == "power_out";
            }
        }
    }
    let flag_nets: Vec<usize> = (0..netlist.nets.len()).filter(|&n| has_in[n] && !has_out[n]).collect();

    // Cells: ICs, then other devices, then passives, then flags.
    let mut idx: Vec<usize> = (0..netlist.parts.len()).collect();
    idx.sort_by_key(|&i| {
        let k = netlist.parts[i].kind;
        (if k == wright_eda::PartKind::Ic { 0 } else if k.is_passive() { 2 } else { 1 }, i)
    });
    let mut cells: Vec<Cell> = idx.iter().map(|&i| part_cell(i, &shapes[i], netlist, &netlist.parts[i].value, &netlist.parts[i].refdes)).collect();
    cells.extend(flag_nets.iter().map(|&n| flag_cell(n, netlist)));
    let (paper, anchors) = lay_out(&cells);

    let sheet = uuid_for(&format!("{title}/sheet"));
    let mut top = vec![
        Sexpr::list("version", vec![Sexpr::atom("20231120")]),
        Sexpr::list("generator", vec![Sexpr::str("wright")]),
        Sexpr::list("generator_version", vec![Sexpr::str("0.1")]),
        Sexpr::list("uuid", vec![Sexpr::str(sheet.clone())]),
        Sexpr::list("paper", vec![Sexpr::str(paper)]),
        Sexpr::list("title_block", vec![Sexpr::list("title", vec![Sexpr::str(title)]), Sexpr::list("rev", vec![Sexpr::str("A")]), Sexpr::list("company", vec![Sexpr::str("Wright")])]),
    ];
    let mut libs: Vec<Sexpr> = order.iter().map(|id| lib_symbol(&defs[id])).collect();
    if !flag_nets.is_empty() {
        libs.push(flag_lib_symbol());
    }
    top.push(Sexpr::list("lib_symbols", libs));

    let (mut syms, mut wires, mut labels, mut ncs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (cell, &(ax, ay)) in cells.iter().zip(&anchors) {
        match cell.item {
            Item::Part(i) => {
                let part = &netlist.parts[i];
                let shape = &shapes[i];
                // same seed as wright_kicad::pcb::symbol_uuid, so the footprint (path "/<uuid>") links by path
                let seed = format!("sym/{}", part.refdes);
                let fy = shape.half_h + TEXT + GRID / 2;
                let props = vec![
                    prop("Reference", &part.refdes, ax, ay - fy, effects(None, false)),
                    prop("Value", &part.value, ax, ay + fy, effects(None, false)),
                    prop("Footprint", &footprints[i].lib_id, ax, ay, effects(None, true)),
                    prop("Datasheet", "", ax, ay, effects(None, true)),
                ];
                let pins: Vec<String> = shape.pins.iter().map(|p| p.number.clone()).collect();
                syms.push(instance(&shape.lib_id, ax, ay, true, &seed, props, &pins, title, &sheet, &part.refdes));
                for sp in &shape.pins {
                    let pin = part.pins.iter().find(|q| q.pad == sp.number).expect("shape pins come from the part");
                    let p = pin_on_sheet((ax, ay), sp.x, sp.y);
                    let pseed = format!("{title}/{}/{}", part.refdes, sp.number);
                    match pin.net {
                        Some(n) => {
                            let e = (p.0 + sp.out_dx() * STUB, p.1);
                            wires.push(wire(p, e, &format!("{pseed}/wire")));
                            labels.push(global_label(&netlist.nets[n].name, e, sp.out_dx(), &format!("{pseed}/label")));
                        }
                        None => ncs.push(Sexpr::list("no_connect", vec![Sexpr::list("at", vec![mm(p.0), mm(p.1)]), uuid(&format!("{pseed}/nc"))])),
                    }
                }
            }
            Item::Flag(n) => {
                let k = flag_nets.iter().position(|&f| f == n).unwrap_or(0) + 1;
                let refdes = format!("#FLG{k:02}");
                let net = &netlist.nets[n].name;
                let seed = format!("{title}/flag/{net}");
                let props = vec![
                    prop("Reference", &refdes, ax, ay - 1_905_000, effects(None, true)),
                    prop("Value", "PWR_FLAG", ax, ay - 3 * GRID, effects(None, false)),
                    prop("Footprint", "", ax, ay, effects(None, true)),
                    prop("Datasheet", "", ax, ay, effects(None, true)),
                ];
                syms.push(instance(FLAG_LIB_ID, ax, ay, false, &seed, props, &["1".to_string()], title, &sheet, &refdes));
                let e = (ax + STUB, ay);
                wires.push(wire((ax, ay), e, &format!("{seed}/wire")));
                labels.push(global_label(net, e, 1, &format!("{seed}/label")));
            }
        }
    }
    top.extend(syms);
    top.extend(wires);
    top.extend(labels);
    top.extend(ncs);
    top.push(Sexpr::list("sheet_instances", vec![Sexpr::list("path", vec![Sexpr::str("/"), Sexpr::list("page", vec![Sexpr::str("1")])])]));
    Ok(Sexpr::list("kicad_sch", top).to_kicad())
}

#[cfg(test)]
mod tests;
