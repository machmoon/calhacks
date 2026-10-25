//! The parts catalog (`data/air/catalog.json`), validated per role on load.
//!
//! Every row must carry the `url` its numbers came from and the date it was
//! read. A row missing a field its role needs is rejected here, with the
//! reason, and never reaches a design: the prototype picked an ESC with no
//! mass because a missing `mass_g` read as 0 g. A missing *price* is kept --
//! the part is real -- and fails the budget check of any design that uses it.

use crate::prop::{self, PropData};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use wright_core::units::{Charge, Current, Length, Mass, Money, Voltage};
use crate::spec::StockPolicy;
use wright_core::Provenance;

#[derive(Debug, Clone)]
pub struct Row {
    pub category: &'static str,
    pub id: String,
    pub name: String,
    pub provenance: Provenance,
    /// Price of ONE piece (a row priced per set is divided by its count).
    pub price: Option<Money>,
    /// On sale at all (not discontinued).
    pub available: bool,
    /// Stock as read on the cited page (`stock`, `stock_evidence`).
    pub stock: Stock,
    pub stock_evidence: Option<String>,
}

/// A row's stock state as its cited page showed it on `retrieved`. A row
/// that states none is `Unstated`, never `InStock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stock {
    InStock,
    Unstated,
    MadeToOrder,
    PreOrder,
    Backorder,
    OutOfStock,
    Discontinued,
}

impl Stock {
    pub fn parse(s: &str) -> Option<Stock> {
        Some(match s {
            "in_stock" => Stock::InStock,
            "unstated" => Stock::Unstated,
            "made_to_order" => Stock::MadeToOrder,
            "pre_order" => Stock::PreOrder,
            "backorder" => Stock::Backorder,
            "out_of_stock" => Stock::OutOfStock,
            "discontinued" => Stock::Discontinued,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Stock::InStock => "in stock",
            Stock::Unstated => "stock unstated",
            Stock::MadeToOrder => "made to order",
            Stock::PreOrder => "pre-order",
            Stock::Backorder => "backorder",
            Stock::OutOfStock => "out of stock",
            Stock::Discontinued => "discontinued",
        }
    }
    /// Whether a builder under `policy` may be sent to buy it.
    pub fn accepted(self, policy: StockPolicy) -> bool {
        match self {
            Stock::InStock => true,
            Stock::Unstated => policy != StockPolicy::InStock,
            Stock::MadeToOrder | Stock::PreOrder | Stock::Backorder => matches!(policy, StockPolicy::LeadTime | StockPolicy::Restock),
            Stock::OutOfStock => policy == StockPolicy::Restock,
            Stock::Discontinued => false,
        }
    }
}

impl Row {
    /// Priced and in a stock state the policy accepts: what the search may
    /// choose.
    pub fn orderable(&self, policy: StockPolicy) -> bool {
        self.price.is_some() && self.stock.accepted(policy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chemistry {
    LiIon,
    LiPo,
}

#[derive(Debug, Clone)]
pub struct Motor {
    pub row: Row,
    /// rpm per volt.
    pub kv: f64,
    /// Phase-to-phase resistance, ohm.
    pub resistance_ohm: f64,
    pub no_load_current: Current,
    pub mass: Mass,
    pub max_current: Current,
    pub cells_min: Option<u32>,
    pub cells_max: Option<u32>,
    pub body_diameter: Length,
    /// None: unpublished (geometry and drag use an envelope, `rules`).
    pub body_length: Option<Length>,
    pub prop_mount: Option<String>,
    pub prop_interface: Option<Interface>,
    /// The maker's thrust table (`thrust_table`): rows with a throttle.
    pub table: Vec<TableRow>,
    /// Supply voltage the motor makes good, <= 1: fitted so the model's
    /// free-running thrust meets the maker's 100 % throttle row with a prop
    /// in this catalog (`calibrate`); 1 when no such row exists.
    pub voltage_factor: f64,
    /// (prop id, table voltage) the factor was fitted on.
    pub calibrated_on: Option<(String, f64)>,
}

/// One row of a maker's thrust-stand table.
#[derive(Debug, Clone)]
pub struct TableRow {
    pub prop: String,
    pub voltage: f64,
    /// ESC duty 0..1.
    pub throttle: f64,
    /// From the supply.
    pub current_a: f64,
    pub thrust_g: f64,
    /// From the supply (V x I).
    pub power_w: f64,
}

impl Motor {
    /// The maker's rows measured with `prop` (matched by the prop row's
    /// `thrust_table_names`), by thrust; empty when the maker never tested it.
    pub fn rows_with(&self, prop: &Prop) -> Vec<&TableRow> {
        let mut v: Vec<&TableRow> = self.table.iter().filter(|t| prop.table_names.iter().any(|n| t.prop.contains(n.as_str()))).collect();
        v.sort_by(|a, b| a.thrust_g.total_cmp(&b.thrust_g));
        v
    }
}

/// How a prop or a motor takes the other: a centre bore / shaft, and or a
/// bolt circle. From the row's `hub` (prop) or `prop_interface` (motor)
/// object; None when the maker publishes none (the mount check then fails).
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    /// Prop: its centre bore. Motor: the diameter the prop seats on (its
    /// shaft, or the boss of a bolt-on mount).
    pub bore_mm: Option<f64>,
    /// Bolt circle: (count, screw e.g. "M3", circle diameter mm).
    pub bolts: Option<(u32, String, f64)>,
    /// Prop: bores its supplied adapter rings reduce it to. Motor: bores a
    /// supplied prop adapter presents (a collet or prop saver).
    pub adapters_mm: Vec<f64>,
    /// The page that states it.
    pub source: String,
}

fn interface(v: &Value, key: &str) -> Option<Interface> {
    let o = v.get(key)?;
    let source = s(o, "source").filter(|u| u.starts_with("http"))?;
    let bolts = match (f(o, "bolts"), s(o, "screw"), f(o, "circle_mm")) {
        (Some(n), Some(sc), Some(c)) => Some((n as u32, sc, c)),
        _ => None,
    };
    let adapters_mm = o.get("adapters_mm").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let i = Interface { bore_mm: f(o, "bore_mm"), bolts, adapters_mm, source };
    (i.bore_mm.is_some() || i.bolts.is_some() || !i.adapters_mm.is_empty()).then_some(i)
}

/// Whether a prop with hub `p` mounts on a motor presenting `m`: the same
/// bolt circle (screw, diameter within 0.3 mm, the motor's hole count a
/// multiple of the prop's) with the prop's bore
/// clearing the motor's boss; or, for a bore-only prop, a shaft or supplied
/// adapter it seats on (the prop's bore, or one of its rings, within 0.3 mm
/// over it). Ok(how) or Err(why).
pub fn mount_fit(p: &Interface, m: &Interface) -> Result<String, String> {
    const TOL: f64 = 0.3;
    if let Some((n, sc, c)) = &p.bolts {
        return match &m.bolts {
            // a 2-hole clamp uses opposite holes of a 4-hole circle
            Some((mn, msc, mc)) if *n > 0 && mn % n == 0 && msc.eq_ignore_ascii_case(sc) && (mc - c).abs() <= TOL => {
                match (p.bore_mm, m.bore_mm) {
                    (Some(pb), Some(mb)) if pb + 1e-9 < mb => Err(format!("bolt circle matches ({n}x {sc} on {c} mm) but the prop's {pb} mm bore is smaller than the motor's {mb} mm boss")),
                    _ => Ok(format!("{n}x {sc} on a {c} mm circle")),
                }
            }
            Some((mn, msc, mc)) => Err(format!("prop needs {n}x {sc} on {c} mm, motor has {mn}x {msc} on {mc} mm")),
            None => Err(format!("prop bolts on ({n}x {sc} on {c} mm); the motor has no published bolt circle")),
        };
    }
    let Some(pb) = p.bore_mm else { return Err("prop publishes neither a bore nor a bolt circle".into()) };
    let bores: Vec<f64> = std::iter::once(pb).chain(p.adapters_mm.iter().copied()).collect();
    let seats: Vec<f64> = m.bore_mm.into_iter().chain(m.adapters_mm.iter().copied()).collect();
    if seats.is_empty() {
        return Err("motor publishes no shaft or prop adapter for a bored prop".into());
    }
    for b in &bores {
        for sd in &seats {
            if *b + 1e-9 >= *sd && b - sd <= TOL {
                return Ok(format!("{b} mm bore on a {sd} mm shaft/adapter"));
            }
        }
    }
    Err(format!("prop bores {bores:?} mm fit none of the motor's {seats:?} mm"))
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub row: Row,
    pub diameter_in: f64,
    pub pitch_in: f64,
    pub blades: u32,
    /// Whether the product folds (the row says; not the proxy's family).
    pub folding: bool,
    /// "CW", "CCW", "pair" or unstated.
    pub rotation: Option<String>,
    pub intended: Option<String>,
    pub mass: Mass,
    /// Spinner and hub a folding prop needs, priced separately.
    pub hub_price: Option<Money>,
    pub data: PropData,
    pub hub: Option<Interface>,
    /// Substrings naming this prop in makers' thrust tables.
    pub table_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Esc {
    pub row: Row,
    pub channels: u32,
    pub cont_current: Current,
    pub burst_current: Option<Current>,
    /// Where it is documented that each motor's current can be limited in
    /// this ESC's firmware with its own sensor (None: it cannot, or unknown).
    pub current_limit_source: Option<String>,
    pub cells_min: Option<u32>,
    pub cells_max: Option<u32>,
    pub mass: Mass,
    pub dims: [Length; 3],
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub row: Row,
    pub chemistry: Chemistry,
    pub format: String,
    pub capacity: Charge,
    pub nominal: Voltage,
    pub max_v: Voltage,
    pub min_v: Voltage,
    pub mass: Mass,
    pub max_cont: Current,
    pub pulse: Option<Current>,
    pub resistance_ohm: f64,
    /// Open-circuit voltage near the end of a flight (~10 % state of charge).
    pub ocv_low: Voltage,
    pub dims: Vec<Length>,
}

#[derive(Debug, Clone)]
pub struct PackProduct {
    pub row: Row,
    pub chemistry: Chemistry,
    pub s: u32,
    pub p: u32,
    pub cell: Option<String>,
    pub capacity: Charge,
    pub mass: Mass,
    pub dims: [Length; 3],
    pub max_cont: Current,
    pub burst: Option<Current>,
    pub nominal: Option<Voltage>,
    pub connector: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TubeKind {
    Pultruded,
    RollWrapped,
}

#[derive(Debug, Clone)]
pub struct Tube {
    pub row: Row,
    pub kind: TubeKind,
    pub od: Length,
    /// None: a solid rod.
    pub id: Option<Length>,
    pub stock_length: Length,
    /// kg per metre: the row's weighed mass, or the section times density.
    pub kg_per_m: f64,
}

#[derive(Debug, Clone)]
pub struct Servo {
    pub row: Row,
    pub dims: [Length; 3],
    pub mass: Mass,
    pub stall_current: Option<Current>,
}

#[derive(Debug, Clone)]
pub struct FlightController {
    pub row: Row,
    pub pwm_outputs: u32,
    /// Servo-rail BEC continuous current, parsed from the row's BEC lines
    /// (0 when the board has none).
    pub servo_bec: Current,
    /// Its peak rating, when the maker states one.
    pub servo_bec_peak: Option<Current>,
    /// Cell range from the row's input voltage ("(3-6S LiPo)"); None unstated.
    pub cells: Option<(u32, u32)>,
    pub mass: Mass,
    pub dims: [Length; 3],
    pub ardupilot_target: Option<String>,
    pub mcu: String,
}

/// A small box that goes in the bay: GPS, receiver, airspeed sensor.
#[derive(Debug, Clone)]
pub struct Module {
    pub row: Row,
    pub mass: Mass,
    pub dims: [Length; 3],
    pub key_spec: String,
}

#[derive(Debug, Clone)]
pub struct Filament {
    pub row: Row,
    pub density_kg_m3: f64,
    pub spool: Mass,
}

#[derive(Debug, Clone)]
pub struct Hardware {
    pub row: Row,
    pub role: Option<String>,
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub motors: BTreeMap<String, Motor>,
    pub props: BTreeMap<String, Prop>,
    pub escs: BTreeMap<String, Esc>,
    pub cells: BTreeMap<String, Cell>,
    pub packs: BTreeMap<String, PackProduct>,
    pub tubes: BTreeMap<String, Tube>,
    pub servos: BTreeMap<String, Servo>,
    pub fcs: BTreeMap<String, FlightController>,
    pub gps: BTreeMap<String, Module>,
    pub receivers: BTreeMap<String, Module>,
    pub airspeed: BTreeMap<String, Module>,
    pub filaments: BTreeMap<String, Filament>,
    pub hardware: BTreeMap<String, Hardware>,
    /// (category, id, why) for every row the loader refused.
    pub rejected: Vec<(String, String, String)>,
}

pub fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| load(include_str!("../../../data/air/catalog.json")))
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}
fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}
fn dims3(v: &Value) -> Option<[Length; 3]> {
    let a = v.get("dims_mm")?.as_array()?;
    if a.len() < 3 {
        return None;
    }
    let x: Vec<f64> = a.iter().take(3).map(|e| e.as_f64()).collect::<Option<Vec<_>>>()?;
    if x.iter().any(|d| *d <= 0.0) {
        return None;
    }
    Some([Length::mm(x[0]), Length::mm(x[1]), Length::mm(x[2])])
}
fn need<T>(x: Option<T>, field: &str, missing: &mut Vec<String>) -> Option<T> {
    if x.is_none() {
        missing.push(field.to_string());
    }
    x
}

fn row(category: &'static str, v: &Value) -> Result<Row, String> {
    let id = s(v, "id").ok_or("no id")?;
    let url = s(v, "url").filter(|u| u.starts_with("http")).ok_or("no source url")?;
    let retrieved = s(v, "retrieved").ok_or("no retrieved date")?;
    let per = f(v, "price_qty").filter(|q| *q > 0.0).unwrap_or(
        if s(v, "price_unit").as_deref() == Some("pair") { 2.0 } else { 1.0 },
    );
    let name = format!("{} {}", s(v, "manufacturer").unwrap_or_default(), s(v, "model").unwrap_or_default())
        .trim()
        .to_string();
    let available = v.get("available").and_then(Value::as_bool).unwrap_or(true);
    let stock = match s(v, "stock") {
        None => Stock::Unstated,
        Some(t) => Stock::parse(&t).ok_or_else(|| format!("unknown stock state {t:?}"))?,
    };
    // a row marked unavailable is not on sale whatever its stock line says
    let stock = if available { stock } else { Stock::Discontinued };
    Ok(Row {
        category,
        id,
        name,
        provenance: Provenance { url, retrieved },
        price: f(v, "price_usd").map(|p| Money::usd(p / per)),
        available: stock != Stock::Discontinued,
        stock,
        stock_evidence: s(v, "stock_evidence"),
    })
}

fn chemistry(v: &Value) -> Chemistry {
    if s(v, "chemistry").unwrap_or_default().to_lowercase().contains("lipo") {
        Chemistry::LiPo
    } else {
        Chemistry::LiIon
    }
}

/// "Servo Vx 4.9V 4.5A cont (5.5A peak)" -> 5.5: the current right before
/// "a peak" / "a max" on the servo rail's line (None when unstated).
fn servo_bec_peak(lines: &[String]) -> Option<f64> {
    for line in lines {
        let t = line.to_lowercase();
        if !["vx", "servo"].iter().any(|w| t.contains(w)) {
            continue;
        }
        for key in ["a peak", "a max"] {
            if let Some(k) = t.find(key) {
                let head = &t[..k];
                let st = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + 1).unwrap_or(0);
                if let Ok(x) = head[st..].parse::<f64>() {
                    return Some(x);
                }
            }
        }
    }
    None
}

/// "Servo Vx 4.9V 4.5A cont" -> 4.5: the largest current on a BEC line that
/// names the servo rail.
fn servo_bec(lines: &[String]) -> f64 {
    let mut best: f64 = 0.0;
    for line in lines {
        let t = line.to_lowercase();
        if !["vx", "servo"].iter().any(|w| t.contains(w)) {
            continue;
        }
        // the continuous figure: the first "<n>A" on the line
        let b = t.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit() {
                let st = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                if i < b.len() && b[i] == b'a' {
                    if let Ok(x) = t[st..i].parse::<f64>() {
                        best = best.max(x);
                        break;
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    best
}

/// "(3-6S LiPo)" -> (3, 6).
fn cell_range(text: &str) -> Option<(u32, u32)> {
    let t = text.to_uppercase();
    let k = t.find("S LIPO").or_else(|| t.find("S)"))?;
    let head = &t[..k];
    let dash = head.rfind('-')?;
    let hi: u32 = head[dash + 1..].trim().parse().ok()?;
    let lo_start = head[..dash].rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
    let lo: u32 = head[lo_start..dash].parse().ok()?;
    Some((lo, hi))
}

fn load(text: &str) -> Catalog {
    let data: Value = serde_json::from_str(text).expect("data/air/catalog.json parses");
    let mut c = Catalog::default();
    let cat = |k: &str| data.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    macro_rules! each {
        ($name:literal, |$v:ident, $r:ident, $m:ident| $body:block) => {
            for $v in cat($name) {
                let id = s(&$v, "id").unwrap_or_else(|| "?".into());
                let $r = match row($name, &$v) {
                    Ok(r) => r,
                    Err(e) => {
                        c.rejected.push(($name.into(), id, e));
                        continue;
                    }
                };
                let mut $m: Vec<String> = Vec::new();
                #[allow(clippy::redundant_closure_call)]
                let res: Option<()> = (|| $body)();
                if res.is_none() || !$m.is_empty() {
                    c.rejected.push(($name.into(), id, format!("lacks {}", $m.join(", "))));
                }
            }
        };
    }
    each!("motors", |v, r, m| {
        let kv = need(f(&v, "kv"), "kv", &mut m);
        let res = need(f(&v, "resistance_ohm"), "resistance_ohm", &mut m);
        let i0 = need(f(&v, "no_load_current_a"), "no_load_current_a", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let imax = need(f(&v, "max_current_a"), "max_current_a", &mut m);
        let dia = need(f(&v, "body_diameter_mm"), "body_diameter_mm", &mut m);
        let mo = Motor {
            kv: kv?,
            resistance_ohm: res?,
            no_load_current: Current::a(i0?),
            mass: Mass::g(mass?),
            max_current: Current::a(imax?),
            cells_min: f(&v, "cells_min").map(|x| x as u32),
            cells_max: f(&v, "cells_max").map(|x| x as u32),
            body_diameter: Length::mm(dia?),
            body_length: f(&v, "body_length_mm").map(Length::mm),
            prop_mount: s(&v, "prop_mount"),
            prop_interface: interface(&v, "prop_interface"),
            voltage_factor: 1.0,
            calibrated_on: None,
            table: v
                .get("thrust_table")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|t| {
                            let th = s(t, "throttle")?.trim_end_matches('%').trim().parse::<f64>().ok()? / 100.0;
                            Some(TableRow { prop: s(t, "prop")?, voltage: f(t, "voltage")?, throttle: th, current_a: f(t, "current_a")?, thrust_g: f(t, "thrust_g")?, power_w: f(t, "power_w")? })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            row: r,
        };
        c.motors.insert(mo.row.id.clone(), mo);
        Some(())
    });
    each!("props", |v, r, m| {
        let d = need(f(&v, "diameter_in"), "diameter_in", &mut m)?;
        let p = need(f(&v, "pitch_in"), "pitch_in", &mut m)?;
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m)?;
        let src = s(&v, "data").unwrap_or_default();
        let data = if let Some(k) = src.strip_prefix("uiuc:") {
            prop::measured(k)
        } else if let Some(fam) = src.strip_prefix("proxy:") {
            prop::proxy(d, p, fam)
        } else {
            None
        };
        let data = need(data, "measured data (data: uiuc:/proxy:)", &mut m)?;
        let folding = v.get("folding").and_then(Value::as_bool).unwrap_or(data.measured_folding);
        let pr = Prop {
            diameter_in: d,
            pitch_in: p,
            blades: f(&v, "blades").map(|b| b as u32).unwrap_or(2),
            folding,
            rotation: s(&v, "rotation"),
            intended: s(&v, "intended"),
            mass: Mass::g(mass),
            hub_price: f(&v, "hub_price_usd").map(Money::usd),
            data,
            hub: interface(&v, "hub"),
            table_names: v.get("thrust_table_names").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            row: r,
        };
        c.props.insert(pr.row.id.clone(), pr);
        Some(())
    });
    each!("escs", |v, r, m| {
        let ch = need(f(&v, "channels"), "channels", &mut m);
        let cont = need(f(&v, "cont_current_a"), "cont_current_a", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let dims = need(dims3(&v), "dims_mm (all three)", &mut m);
        let e = Esc {
            channels: ch? as u32,
            cont_current: Current::a(cont?),
            burst_current: f(&v, "burst_current_a").map(Current::a),
            current_limit_source: s(&v, "per_motor_current_limit_source"),
            cells_min: f(&v, "cells_min").map(|x| x as u32),
            cells_max: f(&v, "cells_max").map(|x| x as u32),
            mass: Mass::g(mass?),
            dims: dims?,
            row: r,
        };
        c.escs.insert(e.row.id.clone(), e);
        Some(())
    });
    each!("cells", |v, r, m| {
        let cap = need(f(&v, "capacity_mah"), "capacity_mah", &mut m);
        let nom = need(f(&v, "nominal_v"), "nominal_v", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let ic = need(f(&v, "max_cont_a"), "max_cont_a", &mut m);
        let res = need(f(&v, "resistance_ohm"), "resistance_ohm", &mut m);
        let ocv = need(f(&v, "ocv_low_v"), "ocv_low_v", &mut m);
        let dims: Vec<Length> = v
            .get("dims_mm")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).map(Length::mm).collect())
            .unwrap_or_default();
        let dims = need((dims.len() >= 2).then_some(dims), "dims_mm", &mut m);
        let ce = Cell {
            chemistry: chemistry(&v),
            format: s(&v, "format").unwrap_or_default(),
            capacity: Charge::mah(cap?),
            nominal: Voltage::v(nom?),
            max_v: Voltage::v(f(&v, "max_v").unwrap_or(4.2)),
            min_v: Voltage::v(f(&v, "min_v").unwrap_or(2.5)),
            mass: Mass::g(mass?),
            max_cont: Current::a(ic?),
            pulse: f(&v, "pulse_a").map(Current::a),
            resistance_ohm: res?,
            ocv_low: Voltage::v(ocv?),
            dims: dims?,
            row: r,
        };
        c.cells.insert(ce.row.id.clone(), ce);
        Some(())
    });
    each!("lipo_packs", |v, r, m| {
        let s_ = need(f(&v, "cells_s"), "cells_s", &mut m);
        let cap = need(f(&v, "capacity_mah"), "capacity_mah", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let dims = need(dims3(&v), "dims_mm", &mut m);
        let cont = f(&v, "max_cont_a")
            .or_else(|| Some(f(&v, "c_rating")? * f(&v, "capacity_mah")? / 1000.0));
        let cont = need(cont, "max_cont_a or c_rating", &mut m);
        let pk = PackProduct {
            chemistry: chemistry(&v),
            s: s_? as u32,
            p: f(&v, "cells_p").unwrap_or(1.0) as u32,
            cell: s(&v, "cell"),
            capacity: Charge::mah(cap?),
            mass: Mass::g(mass?),
            dims: dims?,
            max_cont: Current::a(cont?),
            burst: f(&v, "burst_a").map(Current::a),
            nominal: f(&v, "nominal_v").map(Voltage::v),
            connector: s(&v, "connector"),
            row: r,
        };
        c.packs.insert(pk.row.id.clone(), pk);
        Some(())
    });
    each!("tubes", |v, r, m| {
        let od = need(f(&v, "od_mm"), "od_mm", &mut m)?;
        let len = need(f(&v, "length_mm"), "length_mm", &mut m)?;
        let id = f(&v, "id_mm").filter(|x| *x > 0.0);
        let kind = if s(&v, "kind").as_deref() == Some("pultruded") { TubeKind::Pultruded } else { TubeKind::RollWrapped };
        let kg_per_m = match (f(&v, "mass_g"), f(&v, "length_mm")) {
            (Some(g), Some(l)) if l > 0.0 => g / l,
            _ => {
                let i = id.unwrap_or(0.0);
                crate::rules::CF_PULTRUDED.density_kg_m3 * std::f64::consts::PI / 4.0 * (od * od - i * i) * 1e-6
            }
        };
        let t = Tube { kind, od: Length::mm(od), id: id.map(Length::mm), stock_length: Length::mm(len), kg_per_m, row: r };
        c.tubes.insert(t.row.id.clone(), t);
        Some(())
    });
    each!("servos", |v, r, m| {
        let dims = need(dims3(&v), "dims_mm (all three)", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let sv = Servo { dims: dims?, mass: Mass::g(mass?), stall_current: f(&v, "stall_current_a").map(Current::a), row: r };
        c.servos.insert(sv.row.id.clone(), sv);
        Some(())
    });
    each!("flight_controllers", |v, r, m| {
        let outs = need(f(&v, "pwm_outputs"), "pwm_outputs", &mut m);
        let mass = need(f(&v, "mass_g"), "mass_g", &mut m);
        let dims = need(dims3(&v), "dims_mm", &mut m);
        let bec: Vec<String> = v
            .get("bec")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let fc = FlightController {
            pwm_outputs: outs? as u32,
            servo_bec: Current::a(servo_bec(&bec)),
            servo_bec_peak: servo_bec_peak(&bec).map(Current::a),
            cells: s(&v, "input_v").and_then(|t| cell_range(&t)),
            mass: Mass::g(mass?),
            dims: dims?,
            ardupilot_target: s(&v, "ardupilot_target"),
            mcu: s(&v, "mcu").unwrap_or_default(),
            row: r,
        };
        c.fcs.insert(fc.row.id.clone(), fc);
        Some(())
    });
    for (name, slot) in [("gps", 0), ("receivers", 1), ("airspeed", 2)] {
        for v in cat(name) {
            let id = s(&v, "id").unwrap_or_else(|| "?".into());
            let cat_name: &'static str = match slot {
                0 => "gps",
                1 => "receivers",
                _ => "airspeed",
            };
            let r = match row(cat_name, &v) {
                Ok(r) => r,
                Err(e) => {
                    c.rejected.push((name.into(), id, e));
                    continue;
                }
            };
            let (Some(mass), Some(dims)) = (f(&v, "mass_g"), dims3(&v)) else {
                c.rejected.push((name.into(), id, "lacks mass_g or all three dims_mm".into()));
                continue;
            };
            let key_spec = format!("{} {}", s(&v, "key_spec").unwrap_or_default(), s(&v, "compass").unwrap_or_default());
            let md = Module { row: r, mass: Mass::g(mass), dims, key_spec };
            let map = match slot {
                0 => &mut c.gps,
                1 => &mut c.receivers,
                _ => &mut c.airspeed,
            };
            map.insert(md.row.id.clone(), md);
        }
    }
    each!("filaments", |v, r, m| {
        let rho = f(&v, "density_used_g_cm3").or_else(|| {
            let d = v.get("density_g_cm3")?;
            d.as_f64().or_else(|| d.as_array()?.first()?.as_f64())
        });
        let rho = need(rho, "density", &mut m)?;
        let fl = Filament { density_kg_m3: rho * 1000.0, spool: Mass::g(f(&v, "spool_g").unwrap_or(1000.0)), row: r };
        c.filaments.insert(fl.row.id.clone(), fl);
        Some(())
    });
    each!("hardware", |v, r, m| {
        let _ = &mut m;
        let h = Hardware { role: s(&v, "role"), row: r };
        c.hardware.insert(h.row.id.clone(), h);
        Some(())
    });
    calibrate(&mut c);
    c
}

/// The model spins a prop faster on a given supply than some makers measure
/// (T-Motor MN3110 470 kV on a 13x4.4: +97 % thrust free-running at the
/// table's 14.8 V). For each motor whose maker published a 100 % throttle
/// row with a prop in this catalog, the supply voltage is scaled so the
/// model's free-running thrust meets that row; never above 1 (a model that
/// under-predicts is left conservative). The first such row in table order
/// is the fit; the others validate it (`powertrain::thrust_tables`).
fn calibrate(c: &mut Catalog) {
    let rho = wright_core::units::Density::kg_m3(1.2);
    let ids: Vec<String> = c.motors.keys().cloned().collect();
    for id in ids {
        let fit = {
            let m = &c.motors[&id];
            m.table.iter().filter(|t| (t.throttle - 1.0).abs() < 1e-9).find_map(|t| {
                let (pid, p) = c.props.iter().find(|(_, p)| p.table_names.iter().any(|n| t.prop.contains(n.as_str())))?;
                let target = t.thrust_g * 9.80665e-3;
                let mut mm = m.clone();
                mm.voltage_factor = 1.0;
                if crate::powertrain::free_running(&mm, &p.data, rho, t.voltage).0.value() <= target {
                    return Some((1.0, pid.clone(), t.voltage));
                }
                let (mut lo, mut hi) = (0.2, 1.0);
                for _ in 0..50 {
                    let k = 0.5 * (lo + hi);
                    mm.voltage_factor = k;
                    if crate::powertrain::free_running(&mm, &p.data, rho, t.voltage).0.value() > target {
                        hi = k
                    } else {
                        lo = k
                    }
                }
                Some((0.5 * (lo + hi), pid.clone(), t.voltage))
            })
        };
        if let Some((k, pid, v)) = fit {
            let m = c.motors.get_mut(&id).unwrap();
            m.voltage_factor = k;
            m.calibrated_on = Some((pid, v));
        }
    }
}

impl Catalog {
    /// The row for a hardware role: the first (by id) a builder under
    /// `policy` can order, else the first at all (so the BOM still names it
    /// and `parts_orderable` fails with the reason).
    pub fn hardware_by_role(&self, role: &str, policy: StockPolicy) -> Option<&Hardware> {
        let mut all = self.hardware.values().filter(|h| h.role.as_deref() == Some(role));
        let first = all.clone().next();
        all.find(|h| h.row.orderable(policy)).or(first)
    }

    /// Any row by its BOM category and id.
    pub fn row(&self, category: &str, id: &str) -> Option<&Row> {
        match category {
            "motors" => self.motors.get(id).map(|x| &x.row),
            "props" => self.props.get(id).map(|x| &x.row),
            "escs" => self.escs.get(id).map(|x| &x.row),
            "cells" => self.cells.get(id).map(|x| &x.row),
            "packs" | "lipo_packs" => self.packs.get(id).map(|x| &x.row),
            "tubes" => self.tubes.get(id).map(|x| &x.row),
            "servos" => self.servos.get(id).map(|x| &x.row),
            "flight_controllers" => self.fcs.get(id).map(|x| &x.row),
            "gps" => self.gps.get(id).map(|x| &x.row),
            "receivers" => self.receivers.get(id).map(|x| &x.row),
            "airspeed" => self.airspeed.get(id).map(|x| &x.row),
            "filaments" => self.filaments.get(id).map(|x| &x.row),
            "hardware" => self.hardware.get(id).map(|x| &x.row),
            _ => None,
        }
    }
}

impl Motor {
    pub fn bell_length(&self) -> Length {
        self.body_length.unwrap_or(Length::m(crate::rules::UNKNOWN_BELL_LENGTH_M))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_loads_and_rejects_incomplete_rows() {
        let c = catalog();
        assert!(c.motors.contains_key("aos_supernova_2807_1400kv"));
        // iFlight XING2 rows publish no no-load current: rejected, never zero
        assert!(!c.motors.contains_key("iflight_xing2_2809_800kv"));
        assert!(c.rejected.iter().any(|r| r.1 == "iflight_xing2_2809_800kv"));
        // the Tekko32 Mini 45A has no published mass: rejected
        assert!(!c.escs.contains_key("holybro_tekko32_f4_4in1_mini_45a_am32"));
        assert!(c.packs.contains_key("upgrade_energy_gold_v1_6s2p_22ah_sa08"));
        let p = &c.props["gemfan_flash_7042_2blade"];
        assert!((p.row.price.unwrap().as_usd() - 4.99 / 4.0).abs() < 1e-9);
    }
    #[test]
    fn parses_fc_ratings() {
        let c = catalog();
        let fc = &c.fcs["corewing_f405_wing_v2"];
        assert_eq!(fc.cells, Some((3, 6)));
        assert!((fc.servo_bec.as_a() - 8.0).abs() < 1e-9);
        assert_eq!(c.fcs["speedybee_f405_wing_app"].servo_bec.as_a(), 4.5);
        assert_eq!(c.fcs["speedybee_f405_wing_app"].servo_bec_peak.map(|a| a.as_a()), Some(5.5));
        assert_eq!(c.fcs["matek_f405_wte"].servo_bec_peak.map(|a| a.as_a()), Some(10.0));
        assert_eq!(cell_range("power port max 6V"), None);
    }
}
