//! The one place a battery choice is resolved, so layout, power model, BOM
//! and parameter file agree on its mass, box, energy, current and price.

use crate::catalog::{catalog, Cell, Chemistry};
use crate::powertrain::Pack;
use crate::rules::{CELLS_P_MAX, PACK_COUNT_MAX};
use crate::spec::{BatteryChoice, StockPolicy};
use wright_core::units::{Charge, Current, Length, Money, Voltage};

#[derive(Debug, Clone)]
pub struct Battery {
    pub pack: Pack,
    /// Outer box: (along x, across y, up z).
    pub size: [Length; 3],
    /// Total price, None if any piece is unpriced.
    pub price: Option<Money>,
    pub label: String,
}

/// The cell a bought pack is built from: the catalog's when it knows it,
/// else one derived from the pack (capacity per parallel cell, nominal per
/// series cell, the resistance that sags a cell 0.5 V at its share of the
/// pack's rated current: ASSUMPTION, the prototype's rule).
fn pack_cell(pk: &crate::catalog::PackProduct) -> Cell {
    let c = catalog();
    if let Some(cell) = pk.cell.as_ref().and_then(|id| c.cells.get(id)) {
        return cell.clone();
    }
    let lipo = pk.chemistry == Chemistry::LiPo;
    let p = pk.p.max(1) as f64;
    let nominal = pk.nominal.map(|v| v.value() / pk.s as f64).unwrap_or(if lipo { 3.7 } else { 3.6 });
    Cell {
        row: pk.row.clone(),
        chemistry: pk.chemistry,
        format: "pack".into(),
        capacity: Charge::ah(pk.capacity.as_ah() / p),
        nominal: Voltage::v(nominal),
        max_v: Voltage::v(4.2),
        min_v: Voltage::v(if lipo { 3.3 } else { 2.8 }),
        mass: pk.mass / (pk.s as f64 * p),
        max_cont: pk.max_cont / p,
        pulse: pk.burst.map(|b| b / p),
        resistance_ohm: 0.5 / (pk.max_cont.value() / p),
        ocv_low: Voltage::v(if lipo { 3.6 } else { 3.45 }),
        dims: vec![],
    }
}

/// A hand-built pack's box: cylinders along x in one or two ranks, the
/// section the most nearly square grid; pouches stacked through their
/// thickness. 2 mm of wrap and strip on every face.
pub fn cell_pack_box(cell: &Cell, s: u32, p: u32) -> [Length; 3] {
    let n = (s * p) as f64;
    let wrap = 0.002;
    let d: Vec<f64> = cell.dims.iter().map(|l| l.as_m()).collect();
    if d.len() == 2 {
        let (dia, len) = (d[0].min(d[1]), d[0].max(d[1]));
        let ranks = if n >= 8.0 { 2.0 } else { 1.0 };
        let per = (n / ranks).ceil();
        let rows = per.sqrt().floor().max(1.0);
        let cols = (per / rows).ceil();
        return [Length::m(ranks * len + wrap), Length::m(cols * dia + wrap), Length::m(rows * dia + wrap)];
    }
    let mut v = d.clone();
    v.sort_by(|a, b| b.total_cmp(a));
    let (a, b, t) = (v[0], v[1], v[2]);
    let stack = t * n;
    if stack > 1.5 * b {
        [Length::m(a + wrap), Length::m(2.0 * b + wrap), Length::m(stack / 2.0 + wrap)]
    } else {
        [Length::m(a + wrap), Length::m(b + wrap), Length::m(stack + wrap)]
    }
}

pub fn resolve(choice: &BatteryChoice) -> Result<Battery, String> {
    let c = catalog();
    match choice {
        BatteryChoice::Pack { id, count } => {
            let pk = c.packs.get(id).ok_or_else(|| format!("no pack {id} in the catalog"))?;
            // the same count the BOM bills: never silently raised to 1
            let n = *count;
            if !(1..=PACK_COUNT_MAX).contains(&n) {
                return Err(format!("battery count {n}: a pack design takes 1-{PACK_COUNT_MAX} packs in parallel"));
            }
            let cell = pack_cell(pk);
            let mut pack = Pack::from_cells(&cell, pk.s, pk.p.max(1) * n);
            pack.mass = pk.mass * n as f64;
            pack.max_cont = pk.max_cont * n as f64;
            pack.burst = pk.burst.unwrap_or(pk.max_cont) * n as f64;
            pack.label = if n > 1 { format!("{n} x {}", pk.row.name) } else { pk.row.name.clone() };
            let mut d: Vec<f64> = pk.dims.iter().map(|l| l.as_m()).collect();
            d.sort_by(|a, b| b.total_cmp(a));
            let mut size = [d[0], d[1], n as f64 * d[2]];
            if size[2] > 1.6 * size[1] {
                size = [d[0], 2.0 * d[1], n.div_ceil(2) as f64 * d[2]];
            }
            Ok(Battery {
                label: format!("{} ({}S{}P)", pack.label, pack.s, pack.p),
                size: size.map(Length::m),
                price: pk.row.price.map(|p| p * n as f64),
                pack,
            })
        }
        BatteryChoice::Cells { id, s, p } => {
            let cell = c.cells.get(id).ok_or_else(|| format!("no cell {id} in the catalog"))?;
            if !(1..=CELLS_P_MAX).contains(p) || *s == 0 {
                return Err(format!("battery {s}S{p}P: a hand-built pack takes 1-{CELLS_P_MAX} cells in parallel and at least one in series"));
            }
            let pack = Pack::from_cells(cell, *s, *p);
            Ok(Battery {
                label: pack.label.clone(),
                size: cell_pack_box(cell, *s, *p),
                price: cell.row.price.map(|pr| pr * (s * p) as f64),
                pack,
            })
        }
    }
}

/// Every battery a builder can order for an `s`-cell aircraft: each priced,
/// pack product the stock policy accepts, 1-3 in parallel, each priced cell built 1P-8P.
pub fn options(s: u32, policy: StockPolicy) -> Vec<BatteryChoice> {
    let c = catalog();
    let mut out = vec![];
    for pk in c.packs.values() {
        if pk.s == s && pk.row.orderable(policy) {
            for n in 1..=PACK_COUNT_MAX {
                out.push(BatteryChoice::Pack { id: pk.row.id.clone(), count: n });
            }
        }
    }
    for cell in c.cells.values() {
        if cell.row.orderable(policy) {
            for p in 1..=CELLS_P_MAX {
                out.push(BatteryChoice::Cells { id: cell.row.id.clone(), s, p });
            }
        }
    }
    out
}

/// The rated continuous current a cell-built pack needs a burst figure for.
pub fn burst_current(b: &Battery) -> Current {
    b.pack.burst.max(b.pack.max_cont)
}

