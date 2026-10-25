//! The lane E part catalog (`evals/electronics/catalog.json`, compiled in)
//! and the BOM built from it. A board part matches a row by mpn (devices),
//! or by kind + value prefix + footprint (passives); an unmatched part is
//! still a BOM line, unpriced, so the budget check names it. Line grouping
//! follows KiCad's `bom_csv_grouped_by_value.py` (`myEqu`: same value and
//! footprint is one line, designators joined), as baseline `sourcing.py`
//! `grouped_bom_csv` did.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use wright_core::units::Money;
use wright_core::{Bom, BomLine};
use wright_eda::ir::{Part, PartKind};

const CATALOG_JSON: &str = include_str!("../../../evals/electronics/catalog.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub mpn: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub footprint: Option<String>,
    #[serde(default)]
    pub footprint_prefix: Option<String>,
    #[serde(default)]
    pub value_prefix: Option<String>,
    /// Name -> rating; `null` = not on file.
    #[serde(default)]
    pub ratings: BTreeMap<String, Option<f64>>,
    #[serde(default)]
    pub rating_source: String,
    #[serde(default)]
    pub datasheet: String,
    pub price_usd: Option<f64>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub retrieved: Option<String>,
}

impl Row {
    /// A rating, `None` when it is not on file.
    pub fn rating(&self, name: &str) -> Option<f64> {
        self.ratings.get(name).copied().flatten().filter(|v| v.is_finite())
    }
}

#[derive(Debug, Deserialize)]
struct File {
    parts: Vec<Row>,
}

pub fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| serde_json::from_str::<File>(CATALOG_JSON).map(|f| f.parts).unwrap_or_default())
}

pub fn kind_name(k: PartKind) -> &'static str {
    match k {
        PartKind::Ic => "ic",
        PartKind::Connector => "connector",
        PartKind::Battery => "battery",
        PartKind::Switch => "switch",
        PartKind::Testpoint => "testpoint",
        PartKind::Resistor => "resistor",
        PartKind::Capacitor => "capacitor",
        PartKind::Inductor => "inductor",
        PartKind::Diode => "diode",
        PartKind::Led => "led",
        PartKind::Ferrite => "ferrite",
    }
}

/// The catalog row for a part on footprint `lib_id`.
pub fn lookup(part: &Part, lib_id: &str) -> Option<&'static Row> {
    let kind = kind_name(part.kind);
    let fp_ok = |r: &Row| match (&r.footprint, &r.footprint_prefix) {
        (Some(f), _) => f == lib_id,
        (None, Some(p)) => lib_id.starts_with(p.as_str()),
        (None, None) => true,
    };
    let rows = rows();
    if let Some(m) = part.mpn.as_deref() {
        if let Some(r) = rows.iter().find(|r| r.kind == kind && r.mpn.as_deref() == Some(m)) {
            return Some(r);
        }
    }
    if !part.kind.is_passive() {
        // a device by its name (`AMS1117-3.3`) or a generic row by footprint
        if let Some(r) = rows.iter().find(|r| r.kind == kind && r.mpn.as_deref() == Some(part.name.as_str())) {
            return Some(r);
        }
        return rows.iter().find(|r| r.kind == kind && r.mpn.is_none() && fp_ok(r) && (r.footprint.is_some() || r.footprint_prefix.is_some()));
    }
    rows.iter().find(|r| r.kind == kind && fp_ok(r) && r.value_prefix.as_deref().is_some_and(|v| part.value.starts_with(v)))
}

/// One BOM group while collecting: catalog row, kind, footprint, designators.
type Line = (Option<&'static Row>, String, String, Vec<String>);

/// One BOM line per (catalog row or value, footprint), plus the bare PCB.
pub fn bom(netlist: &wright_eda::Netlist, board: &wright_eda::Board) -> Bom {
    let mut groups: BTreeMap<(String, String), Line> = BTreeMap::new();
    for p in &board.parts {
        let Some(part) = netlist.parts.get(p.part) else { continue };
        let lib = p.footprint.lib_id.clone();
        let row = lookup(part, &lib);
        let key = (row.map(|r| r.id.clone()).unwrap_or_else(|| format!("{}|{}", part.value, lib)), lib.clone());
        let e = groups.entry(key).or_insert((row, kind_name(part.kind).to_string(), format!("{} {}", part.value, lib.split(':').next_back().unwrap_or("")), vec![]));
        e.3.push(part.refdes.clone());
    }
    let mut lines: Vec<BomLine> = groups
        .into_iter()
        .map(|((id, lib), (row, cat, name, refs))| BomLine {
            category: cat,
            id,
            name: row.and_then(|r| r.mpn.clone()).map(|m| format!("{m} ({name})")).unwrap_or(name),
            qty: refs.len() as f64,
            unit: "ea".into(),
            unit_price: row.and_then(|r| r.price_usd).map(Money::usd),
            url: row.map(|r| r.url.clone()).unwrap_or_default(),
            note: format!("{}; {lib}{}", refs.join(" "), if row.and_then(|r| r.price_usd).is_none() { "; unpriced: no retrieved price" } else { "" }),
        })
        .collect();
    let pcb = rows().iter().find(|r| r.kind == "pcb");
    let (w, h) = (board.outline.width().as_mm(), board.outline.height().as_mm());
    lines.push(BomLine {
        category: "pcb".into(),
        id: "pcb-2layer".into(),
        name: format!("2-layer PCB {w:.1} x {h:.1} mm, 1 oz"),
        qty: 1.0,
        unit: "ea".into(),
        unit_price: pcb.and_then(|r| r.price_usd).map(Money::usd),
        url: pcb.map(|r| r.url.clone()).unwrap_or_default(),
        note: "bare board; unpriced until a fab quote is retrieved".into(),
    });
    Bom { lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_rows_are_unique() {
        let r = rows();
        assert!(r.len() >= 10, "catalog.json did not parse ({} rows)", r.len());
        let mut ids: Vec<_> = r.iter().map(|x| x.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), r.len());
        // a price is never invented: every priced row says where and when
        for x in r {
            if x.price_usd.is_some() {
                assert!(!x.url.is_empty() && x.retrieved.is_some(), "{} priced without url/retrieved", x.id);
            }
        }
    }

    #[test]
    fn servo_parts_find_their_rows() {
        let n = crate::templates::expand(&crate::templates::Template::ServoPower { vin_min_v: 14.0, vin_max_v: 16.8, vout_v: 6.0, iout_a: 5.0, servos: 5 }).unwrap().validate().unwrap();
        let fp = |p: &Part| -> String {
            let c = crate::templates::expand(&crate::templates::Template::ServoPower { vin_min_v: 14.0, vin_max_v: 16.8, vout_v: 6.0, iout_a: 5.0, servos: 5 }).unwrap();
            c.devices.get(&p.name).and_then(|d| d.package.clone()).or_else(|| c.passives.get(&p.name).and_then(|d| d.package.clone())).unwrap()
        };
        let id = |r: &str| lookup(n.part(r).unwrap(), &fp(n.part(r).unwrap())).map(|x| x.id.as_str());
        let by_name = |name: &str| n.parts.iter().find(|p| p.name == name).unwrap().refdes.clone();
        assert_eq!(id(&by_name("TPS5450DDA")), Some("tps5450ddar"));
        assert_eq!(id(&by_name("J_BAT")), Some("xt30pw-m"));
        assert_eq!(id(&by_name("J_ELEVATOR")), Some("header-1x0n-2.54"));
        assert_eq!(id(&by_name("C_IN1")), Some("grm32er71h106ka12"));
        assert_eq!(id(&by_name("D_CATCH")), Some("ss54"));
        assert_eq!(id(&by_name("L_BUCK")), Some("srp1245a-150m"));
        assert_eq!(id(&by_name("R_FB2")), Some("res-0603-1pct"));
    }
}
