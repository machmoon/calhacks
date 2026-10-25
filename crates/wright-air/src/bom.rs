//! The bill of materials: one line per thing to buy, priced from the
//! catalog row's cited page. An unpriced part is an unpriced line (the
//! budget check fails on it), never free. Everything that costs money is a
//! line here -- wire, connectors, hardware, filament -- because the budget
//! check reads exactly these rows.

use crate::catalog::{catalog, Row};
use crate::electrical::{sellable_awg, Electrical};
use crate::layout::Layout;
use crate::rules::*;
use crate::spec::BatteryChoice;
use wright_core::units::Money;
use wright_core::{Bom, BomLine};

fn line(cat: &str, row: &Row, qty: f64, unit: &str, note: &str) -> BomLine {
    BomLine {
        category: cat.into(),
        id: row.id.clone(),
        name: row.name.clone(),
        qty,
        unit: unit.into(),
        unit_price: row.price,
        url: row.provenance.url.clone(),
        note: note.into(),
    }
}

/// Printed grams by filament id, from the mass model (the slicer's grams
/// check this estimate).
pub fn printed_grams(lay: &Layout) -> Vec<(String, f64)> {
    let shell: f64 = lay.items.iter().filter(|i| i.name.ends_with("(printed)")).map(|i| i.mass.as_g()).sum();
    let fit: f64 = lay
        .items
        .iter()
        .filter(|i| ["lift motor mounts", "boom clamps", "tail joints", "pusher mount", "landing legs"].contains(&i.name))
        .map(|i| i.mass.as_g())
        .sum();
    let s = &lay.design.structure;
    if s.shell_filament == s.structural_filament {
        vec![(s.shell_filament.clone(), shell + fit)]
    } else {
        vec![(s.shell_filament.clone(), shell), (s.structural_filament.clone(), fit)]
    }
}

/// Tube lines: pieces cut from stock by first-fit decreasing.
fn tubes(lay: &Layout, out: &mut Vec<BomLine>, problems: &mut Vec<String>) {
    let s = &lay.design.structure;
    let semi = lay.semi_span().as_m();
    let mut need: Vec<(String, Vec<f64>)> = vec![];
    let mut push = |id: &str, l: f64, n: usize| {
        if let Some(e) = need.iter_mut().find(|e| e.0 == id) {
            e.1.extend(std::iter::repeat_n(l, n));
        } else {
            need.push((id.into(), vec![l; n]));
        }
    };
    push(&s.main_spar, MAIN_SPAR_REACH * semi, 2);
    push(&s.rear_spar, REAR_SPAR_REACH * semi, 2);
    push(&s.joiner, 2.0 * JOINER_HALF_FRAC * semi, 1);
    push(&s.boom_tube, (lay.boom_x1 - lay.boom_x0).as_m(), 2);
    let c = catalog();
    for (id, mut pieces) in need {
        let Some(t) = c.tubes.get(&id) else { continue };
        let stock = t.stock_length.as_m();
        pieces.sort_by(|a, b| b.total_cmp(a));
        if pieces[0] > stock + 1e-9 {
            problems.push(format!("{} is sold {:.0} mm long; a {:.0} mm piece cannot be cut from it", t.row.name, stock * 1000.0, pieces[0] * 1000.0));
        }
        let mut bins: Vec<f64> = vec![];
        for p in &pieces {
            if let Some(b) = bins.iter_mut().find(|b| **b >= *p) {
                *b -= p;
            } else {
                bins.push(stock - p);
            }
        }
        let cuts: Vec<String> = pieces.iter().map(|p| format!("{:.0}", p * 1000.0)).collect();
        out.push(line("tubes", &t.row, bins.len() as f64, "tubes", &format!("cut to {} mm", cuts.join(", "))));
    }
}

/// The hardware row for a BOM role: the design's choice (`hardware`) when it
/// names one with that role, else the catalog's default for the role.
pub fn hardware_for<'a>(lay: &Layout, role: &str) -> Option<&'a crate::catalog::Hardware> {
    let c = catalog();
    match lay.design.hardware.get(role) {
        Some(id) => c.hardware.get(id).filter(|h| h.role.as_deref() == Some(role)),
        None => c.hardware_by_role(role, lay.design.requirements.stock),
    }
}

pub fn bom(lay: &Layout, elec: Option<&Electrical>) -> (Bom, Vec<String>) {
    let c = catalog();
    let p = &lay.parts;
    let mut v = vec![];
    let mut problems = vec![];
    v.push(line("motors", &p.lift_motor.row, 4.0, "pcs", "lift; Motor1/2 spin CCW, 3/4 CW (ArduPilot Quad X)"));
    let mount = match (&p.lift_prop.hub, &p.lift_motor.prop_interface) {
        (Some(h), Some(i)) => crate::catalog::mount_fit(h, i).map(|how| format!("; mounts {how} (screws/adapters supplied with the prop and motor)")).unwrap_or_else(|e| format!("; DOES NOT MOUNT: {e}")),
        _ => "; mount unpublished".into(),
    };
    v.push(line("props", &p.lift_prop.row, 4.0, "pcs", &format!("2 CW + 2 CCW{mount}")));
    v.push(line("motors", &p.cruise_motor.row, 1.0, "pcs", "pusher"));
    v.push(line("props", &p.cruise_prop.row, 1.0, "pcs", "pusher"));
    for (pr, n) in [(p.lift_prop, 4.0), (p.cruise_prop, 1.0)] {
        if pr.folding && pr.intended.as_deref() != Some("multirotor") {
            v.push(BomLine {
                category: "props".into(),
                id: format!("{}_hub", pr.row.id),
                name: format!("{} spinner and hub", pr.row.name),
                qty: n,
                unit: "sets".into(),
                unit_price: pr.hub_price,
                url: pr.row.provenance.url.clone(),
                note: "folding prop centre piece".into(),
            });
        }
    }
    v.push(line("escs", &p.lift_esc.row, if p.lift_esc.channels >= 4 { 1.0 } else { 4.0 }, "pcs", "lift"));
    v.push(line("escs", &p.cruise_esc.row, 1.0, "pcs", "pusher"));
    match &lay.design.battery {
        BatteryChoice::Pack { id, count } => {
            if let Some(pk) = c.packs.get(id) {
                v.push(line("packs", &pk.row, *count as f64, "packs", if *count > 1 { "in parallel (match voltages within 0.05 V/cell before joining)" } else { "" }));
                if *count > 1 {
                    // a parallel harness: one mating plug per pack on a
                    // 12 AWG bus out to the aircraft's battery connector
                    let plug = pk.row.id.clone();
                    let kind = c.packs[&plug].connector.as_deref().unwrap_or("").split_whitespace().next().unwrap_or("").to_uppercase();
                    let mate = match kind.as_str() {
                        "XT60" => Some("amass_xt60h_connector_piece"),
                        "XT90" | "XT90S" => Some("amass_xt90s_connector_piece"),
                        _ => None,
                    };
                    match mate.and_then(|m| c.hardware.get(m)) {
                        Some(h) => v.push(line("hardware", &h.row, *count as f64, "pcs", &format!("parallel harness: one {kind} plug per pack"))),
                        None => problems.push(format!("no catalog connector to build a parallel harness for {kind:?} packs")),
                    }
                    match c.hardware.get("pyrodrone_silicone_wire_12awg_1m_pair") {
                        Some(h) => v.push(line("hardware", &h.row, 1.0, "1 m pairs", "parallel harness bus")),
                        None => problems.push("no 12 AWG wire row for the parallel harness".into()),
                    }
                }
            }
        }
        BatteryChoice::Cells { id, s, p: par } => {
            if let Some(cell) = c.cells.get(id) {
                v.push(line("cells", &cell.row, (s * par) as f64, "cells", &format!("{s}S{par}P, built by hand")));
                for role in ["nickel_strip", "heat_shrink_pack"] {
                    match hardware_for(lay, role) {
                        Some(h) => v.push(line("hardware", &h.row, 1.0, "pack", "pack building")),
                        None => problems.push(format!("no catalog row for {role}")),
                    }
                }
                if let Some(h) = c.hardware.get("strix_jst_xh_6s_balance_lead_12in") {
                    v.push(line("hardware", &h.row, 1.0, "pcs", "balance lead"));
                }
            }
        }
    }
    v.push(line("flight_controllers", &p.fc.row, 1.0, "pcs", ""));
    v.push(line("gps", &p.gps.row, 1.0, "pcs", ""));
    v.push(line("receivers", &p.receiver.row, 1.0, "pcs", ""));
    if let Some(a) = p.airspeed {
        v.push(line("airspeed", &a.row, 1.0, "pcs", ""));
    }
    v.push(line("servos", &p.servo.row, 5.0, "pcs", "2 ailerons, elevator, 2 rudders"));
    tubes(lay, &mut v, &mut problems);
    for (fid, g) in printed_grams(lay) {
        if let Some(f) = c.filaments.get(&fid) {
            let need = g * (1.0 + PRINT_WASTE);
            let spools = (need / f.spool.as_g()).ceil().max(1.0);
            v.push(line("filaments", &f.row, spools, "spools", &format!("{g:.0} g printed, {need:.0} g with waste")));
        }
    }
    if let Some(e) = elec {
        // wire by gauge, sold in 1 m red + 1 m black pairs
        let mut by: Vec<(u32, f64)> = vec![];
        for r in &e.runs {
            let m = r.length_m * r.conductors as f64;
            if let Some(x) = by.iter_mut().find(|x| x.0 == r.awg) {
                x.1 += m;
            } else {
                by.push((r.awg, m));
            }
        }
        by.sort_by_key(|x| x.0);
        for (g, m) in by {
            match sellable_awg(g).and_then(|id| c.hardware.get(&id)) {
                Some(h) => v.push(line("hardware", &h.row, (m / 2.0).ceil(), "1 m pairs", &format!("{m:.1} m of {g} AWG"))),
                None => v.push(BomLine {
                    category: "hardware".into(),
                    id: format!("silicone_wire_{g}awg"),
                    name: format!("{g} AWG silicone wire"),
                    qty: m,
                    unit: "m".into(),
                    unit_price: None,
                    url: String::new(),
                    note: "no seller in the catalog".into(),
                }),
            }
        }
        if let Some(h) = c.hardware.get(e.connector.3) {
            v.push(line("hardware", &h.row, 2.0, "pcs", &format!("{} battery connector, male + female", e.connector.0)));
        }
    }
    for (role, qty, note) in [
        ("heat_set_inserts", 1.0, "pack of 100"),
        ("screws_m3", 1.0, "assortment"),
        ("ca_glue", 1.0, ""),
        ("epoxy", 1.0, "spar joiner, booms"),
        ("servo_extensions", 5.0, "one per servo"),
        ("control_horns", 3.0, "five surfaces"),
        ("pushrods", 1.0, "set of 10"),
    ] {
        match hardware_for(lay, role) {
            Some(h) => v.push(line("hardware", &h.row, qty, "pcs", note)),
            None => problems.push(format!("no catalog row for {role}")),
        }
    }
    let _ = Money::usd(0.0);
    (Bom { lines: v }, problems)
}
