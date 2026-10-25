//! What a person asks for ([`Spec`]) and what Wright resolves it to
//! ([`Design`]).
//!
//! JSON is the boundary: lengths there are millimetres in fields named
//! `_mm`, angles degrees in `_deg`; inside the crate they become
//! `wright_core::units` quantities at once.
//!
//! A `Spec` may name nothing but requirements: `design` then leaves every
//! choice open and [`crate::search`] seeds and searches them. Anything named
//! in `design` is kept as written (how a published build is reproduced).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    /// Range with the energy reserve kept, km (None: no minimum beyond a
    /// feasible mission).
    #[serde(default)]
    pub range_km: Option<f64>,
    #[serde(default)]
    pub endurance_min: Option<f64>,
    #[serde(default)]
    pub payload_g: f64,
    #[serde(default = "d_span")]
    pub max_span_mm: f64,
    #[serde(default = "d_printer")]
    pub printer: String,
    #[serde(default)]
    pub budget_usd: Option<f64>,
    #[serde(default)]
    pub altitude_m: f64,
    /// Hover is checked this much hotter than ISA, K.
    #[serde(default = "d_hot")]
    pub hot_day_dk: f64,
    #[serde(default = "d_stall")]
    pub max_stall_ms: f64,
    /// Pack voltage class, cells in series.
    #[serde(default = "d_cells")]
    pub cells_s: u32,
    /// An airspeed sensor on board (ArduPilot's TECS and stall prevention
    /// want one on a long-range aircraft).
    #[serde(default = "d_true")]
    pub airspeed_sensor: bool,
    /// Longest acceptable total print time, hours (None: no limit).
    #[serde(default)]
    pub max_print_hours: Option<f64>,
    /// Which stock states a bought part may be in (catalog `stock`).
    #[serde(default)]
    pub stock: StockPolicy,
}

/// How long a builder will wait for parts. Every bought part's catalog row
/// states its `stock` as read on its cited page; the search only chooses,
/// and `parts_orderable` only passes, rows this policy accepts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StockPolicy {
    /// Only rows whose page said in stock.
    InStock,
    /// In stock, or listed for sale with a price and no stock line (the
    /// default: a priced product page is evidence of sale, not of stock;
    /// `parts_orderable` names how many such rows a design uses).
    #[default]
    Listed,
    /// Also made to order, pre-order and backorder (weeks of lead time).
    LeadTime,
    /// Also out of stock (waiting for a restock that may never come).
    Restock,
}

fn d_span() -> f64 {
    1200.0
}
fn d_printer() -> String {
    "bambu_a1".into()
}
fn d_hot() -> f64 {
    15.0
}
fn d_stall() -> f64 {
    14.0
}
fn d_cells() -> u32 {
    6
}
fn d_true() -> bool {
    true
}

impl Requirements {
    /// Refuse requirements no aircraft can be designed to (instead of
    /// ignoring them or producing NaN downstream).
    pub fn validate(&self) -> Result<(), String> {
        let fin = |x: f64| x.is_finite();
        if !fin(self.payload_g) || self.payload_g < 0.0 {
            return Err(format!("payload_g must be a finite mass >= 0, got {}", self.payload_g));
        }
        if !fin(self.max_span_mm) || self.max_span_mm <= 0.0 {
            return Err(format!("max_span_mm must be > 0, got {}", self.max_span_mm));
        }
        if !fin(self.hot_day_dk) || !fin(self.altitude_m) || !fin(self.max_stall_ms) || self.max_stall_ms <= 0.0 {
            return Err("hot_day_dk, altitude_m and max_stall_ms must be finite (max_stall_ms > 0)".into());
        }
        for (k, v) in [("range_km", self.range_km), ("endurance_min", self.endurance_min), ("budget_usd", self.budget_usd), ("max_print_hours", self.max_print_hours)] {
            if v.is_some_and(|x| !fin(x) || x < 0.0) {
                return Err(format!("{k} must be finite and >= 0"));
            }
        }
        // the envelope the schema states (rules::REQUIREMENT_BOUNDS)
        for &(k, lo, hi, source) in crate::rules::REQUIREMENT_BOUNDS {
            let Some(x) = self.number(k) else { continue };
            if x < lo || hi.is_some_and(|h| x > h) {
                let hi = hi.map_or("-".to_string(), |h| h.to_string());
                return Err(format!("{k} = {x} is outside [{lo}, {hi}] ({source})"));
            }
        }
        // the hot-day limit falls with altitude (MIL-HDBK-310 5.3.1.1.2)
        let dk_max = self.hot_day_dk_max();
        if self.hot_day_dk > dk_max + 1e-9 {
            let isa_c = crate::atmosphere::isa(wright_core::units::Length::m(self.altitude_m), 0.0).temperature.value() - 273.15;
            return Err(format!(
                "hot_day_dk = {} at a {} m field is {:.1} C, hotter than the MIL-HDBK-310 5.3.1.1.2 worldwide 1 % high temperature there ({:.1} C, ISA {:.1} C + {:.1} K)",
                self.hot_day_dk,
                self.altitude_m,
                isa_c + self.hot_day_dk,
                crate::rules::hot_1pct_c(self.altitude_m),
                isa_c,
                dk_max
            ));
        }
        Ok(())
    }

    /// The hottest day, K above ISA, the envelope allows at this field:
    /// MIL-HDBK-310's 1 % high temperature at its altitude less ISA's.
    pub fn hot_day_dk_max(&self) -> f64 {
        let isa_c = crate::atmosphere::isa(wright_core::units::Length::m(self.altitude_m), 0.0).temperature.value() - 273.15;
        crate::rules::hot_1pct_c(self.altitude_m) - isa_c
    }

    /// A numeric requirement by its JSON name (None when unset).
    fn number(&self, field: &str) -> Option<f64> {
        match field {
            "altitude_m" => Some(self.altitude_m),
            "hot_day_dk" => Some(self.hot_day_dk),
            "payload_g" => Some(self.payload_g),
            "max_span_mm" => Some(self.max_span_mm),
            "range_km" => self.range_km,
            "endurance_min" => self.endurance_min,
            "budget_usd" => self.budget_usd,
            "max_stall_ms" => Some(self.max_stall_ms),
            "cells_s" => Some(self.cells_s as f64),
            "max_print_hours" => self.max_print_hours,
            _ => None,
        }
    }
}

impl Default for Requirements {
    fn default() -> Self {
        serde_json::from_str("{}").expect("defaults")
    }
}

/// How the pack is bought: a product (several in parallel) or built from cells.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatteryChoice {
    Pack { id: String, count: u32 },
    Cells { id: String, s: u32, p: u32 },
}

/// Choices a spec may fix; None is left to the search.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Choices {
    pub span_mm: Option<f64>,
    pub root_chord_mm: Option<f64>,
    pub tip_chord_mm: Option<f64>,
    pub airfoil: Option<String>,
    pub lift_motor: Option<String>,
    pub lift_prop: Option<String>,
    pub lift_esc: Option<String>,
    pub cruise_motor: Option<String>,
    pub cruise_prop: Option<String>,
    pub cruise_esc: Option<String>,
    /// Per-motor AM32 current limit in the lift ESCs, A (even, 2-200).
    #[serde(default, deserialize_with = "ids::current_limit")]
    pub lift_current_limit_a: Option<f64>,
    pub battery: Option<BatteryChoice>,
    pub fc: Option<String>,
    pub gps: Option<String>,
    pub receiver: Option<String>,
    /// Some(None) = explicitly no airspeed sensor.
    pub airspeed: Option<Option<String>>,
    pub servo: Option<String>,
    pub main_spar: Option<String>,
    pub boom_tube: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    #[serde(default = "d_name")]
    pub name: String,
    #[serde(default)]
    pub requirements: Requirements,
    #[serde(default)]
    pub design: Choices,
}

fn d_name() -> String {
    "vtol".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Wing {
    pub span_mm: f64,
    pub root_chord_mm: f64,
    pub tip_chord_mm: f64,
    #[serde(deserialize_with = "ids::wing_airfoil")]
    pub airfoil: String,
    pub incidence_deg: f64,
    pub washout_deg: f64,
    pub dihedral_deg: f64,
    pub aileron_chord_frac: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tail {
    #[serde(deserialize_with = "ids::tail_airfoil")]
    pub airfoil: String,
    /// Horizontal and vertical tail volume coefficients the areas follow.
    pub vh: f64,
    pub vv: f64,
    pub aspect_ratio_v: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Propulsion {
    #[serde(deserialize_with = "ids::lift_motor")]
    pub lift_motor: String,
    #[serde(deserialize_with = "ids::lift_prop")]
    pub lift_prop: String,
    #[serde(deserialize_with = "ids::lift_esc")]
    pub lift_esc: String,
    #[serde(deserialize_with = "ids::cruise_motor")]
    pub cruise_motor: String,
    #[serde(deserialize_with = "ids::cruise_prop")]
    pub cruise_prop: String,
    #[serde(deserialize_with = "ids::cruise_esc")]
    pub cruise_esc: String,
    /// Lift motors canted alternately for yaw authority, deg.
    pub lift_tilt_deg: f64,
    /// Per-motor current limit set in each lift ESC's firmware (AM32
    /// `limits.current`, 2 A steps), A; None: no limit. Needs an ESC whose
    /// row states a per-motor limit (`Esc::current_limit_source`).
    #[serde(default, deserialize_with = "ids::current_limit")]
    pub lift_current_limit_a: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Avionics {
    #[serde(deserialize_with = "ids::fc")]
    pub fc: String,
    #[serde(deserialize_with = "ids::gps")]
    pub gps: String,
    #[serde(deserialize_with = "ids::receiver")]
    pub receiver: String,
    #[serde(deserialize_with = "ids::airspeed")]
    pub airspeed: Option<String>,
    #[serde(deserialize_with = "ids::servo")]
    pub servo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Structure {
    #[serde(deserialize_with = "ids::main_spar")]
    pub main_spar: String,
    #[serde(deserialize_with = "ids::rear_spar")]
    pub rear_spar: String,
    /// Carries the root moment across the centreline (sleeve or insert).
    #[serde(deserialize_with = "ids::joiner")]
    pub joiner: String,
    #[serde(deserialize_with = "ids::boom_tube")]
    pub boom_tube: String,
    pub leg_thickness_mm: f64,
    #[serde(deserialize_with = "ids::shell_filament")]
    pub shell_filament: String,
    #[serde(deserialize_with = "ids::structural_filament")]
    pub structural_filament: String,
}

/// A fully resolved aircraft: every part named, every dimension set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Design {
    pub name: String,
    pub requirements: Requirements,
    pub wing: Wing,
    pub tail: Tail,
    pub propulsion: Propulsion,
    #[serde(deserialize_with = "ids::battery")]
    pub battery: BatteryChoice,
    pub avionics: Avionics,
    pub structure: Structure,
    /// Consumables and hardware by BOM role ("ca_glue", "epoxy",
    /// "heat_set_inserts", ...) -> catalog hardware id. A role left out takes
    /// the first row with that role the stock policy accepts.
    #[serde(default, deserialize_with = "ids::hardware")]
    pub hardware: std::collections::BTreeMap<String, String>,
}

/// Catalog-backed ids are checked when a design is read (an agent's edit is
/// a deserialization), so a made-up id is refused with the valid ones.
pub mod ids {
    use serde::{Deserialize, Deserializer};

    pub fn valid(path: &str) -> Vec<String> {
        let c = crate::catalog::catalog();
        let keys = |m: Vec<&String>| m.into_iter().cloned().collect::<Vec<_>>();
        match path {
            "wing.airfoil" | "tail.airfoil" => crate::airfoil::names().into_iter().map(str::to_string).collect(),
            "motor" => keys(c.motors.keys().collect()),
            "prop" => keys(c.props.keys().collect()),
            "esc" => keys(c.escs.keys().collect()),
            "fc" => keys(c.fcs.keys().collect()),
            "gps" => keys(c.gps.keys().collect()),
            "receiver" => keys(c.receivers.keys().collect()),
            "airspeed" => keys(c.airspeed.keys().collect()),
            "servo" => keys(c.servos.keys().collect()),
            "tube" => keys(c.tubes.keys().collect()),
            "filament" => keys(c.filaments.keys().collect()),
            "pack" => keys(c.packs.keys().collect()),
            "cell" => keys(c.cells.keys().collect()),
            _ => vec![],
        }
    }

    pub fn check(kind: &str, path: &str, id: &str) -> Result<(), String> {
        let v = valid(kind);
        if v.iter().any(|x| x == id) {
            return Ok(());
        }
        let shown = if v.len() > 40 { format!("{} ... ({} in all)", v[..40].join(", "), v.len()) } else { v.join(", ") };
        Err(format!("unknown {path} id {id:?}: valid ids are {shown} (see `catalog {path}`)"))
    }

    macro_rules! field {
        ($name:ident, $kind:literal, $path:literal) => {
            pub fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
                let s = String::deserialize(d)?;
                check($kind, $path, &s).map_err(serde::de::Error::custom)?;
                Ok(s)
            }
        };
    }
    field!(wing_airfoil, "wing.airfoil", "wing.airfoil");
    field!(tail_airfoil, "tail.airfoil", "tail.airfoil");
    field!(lift_motor, "motor", "propulsion.lift_motor");
    field!(cruise_motor, "motor", "propulsion.cruise_motor");
    field!(lift_prop, "prop", "propulsion.lift_prop");
    field!(cruise_prop, "prop", "propulsion.cruise_prop");
    field!(lift_esc, "esc", "propulsion.lift_esc");
    field!(cruise_esc, "esc", "propulsion.cruise_esc");
    field!(fc, "fc", "avionics.fc");
    field!(gps, "gps", "avionics.gps");
    field!(receiver, "receiver", "avionics.receiver");
    field!(servo, "servo", "avionics.servo");
    field!(main_spar, "tube", "structure.main_spar");
    field!(rear_spar, "tube", "structure.rear_spar");
    field!(joiner, "tube", "structure.joiner");
    field!(boom_tube, "tube", "structure.boom_tube");
    field!(shell_filament, "filament", "structure.shell_filament");
    field!(structural_filament, "filament", "structure.structural_filament");

    pub fn airspeed<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        let s = Option::<String>::deserialize(d)?;
        if let Some(id) = &s {
            check("airspeed", "avionics.airspeed", id).map_err(serde::de::Error::custom)?;
        }
        Ok(s)
    }

    /// `hardware`: each role must be one the catalog's hardware rows name,
    /// and each id a hardware row with that role.
    pub fn hardware<'de, D: Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, String>, D::Error> {
        let m = std::collections::BTreeMap::<String, String>::deserialize(d)?;
        let c = crate::catalog::catalog();
        let mut roles: Vec<&str> = c.hardware.values().filter_map(|h| h.role.as_deref()).collect();
        roles.sort();
        roles.dedup();
        for (role, id) in &m {
            if !roles.contains(&role.as_str()) {
                return Err(serde::de::Error::custom(format!("unknown hardware role {role:?}: valid roles are {} (see `catalog hardware.<role>`)", roles.join(", "))));
            }
            let ok: Vec<&str> = c.hardware.values().filter(|h| h.role.as_deref() == Some(role.as_str())).map(|h| h.row.id.as_str()).collect();
            if !ok.contains(&id.as_str()) {
                return Err(serde::de::Error::custom(format!("unknown hardware.{role} id {id:?}: valid ids are {} (see `catalog hardware.{role}`)", ok.join(", "))));
            }
        }
        Ok(m)
    }

    /// `lift_current_limit_a`: a number, or the option id `catalog` lists
    /// for it ("24"); an even 2-200 A (AM32 sets it in 2 A steps).
    pub fn current_limit<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum NumOrText {
            N(f64),
            T(String),
        }
        let v = match Option::<NumOrText>::deserialize(d)? {
            None => return Ok(None),
            Some(NumOrText::N(x)) => x,
            Some(NumOrText::T(t)) => t.trim().trim_end_matches('A').trim().parse::<f64>().map_err(|_| serde::de::Error::custom(format!("lift_current_limit_a {t:?} is not a number of amps")))?,
        };
        if !(2.0..=200.0).contains(&v) || (v / 2.0).fract() != 0.0 {
            return Err(serde::de::Error::custom(format!("lift_current_limit_a {v}: an even number of amps, 2-200 (AM32 sets it in 2 A steps; see `catalog propulsion.lift_current_limit_a`)")));
        }
        Ok(Some(v))
    }

    pub fn battery<'de, D: Deserializer<'de>>(d: D) -> Result<super::BatteryChoice, D::Error> {
        let b = super::BatteryChoice::deserialize(d)?;
        match &b {
            super::BatteryChoice::Pack { id, count } => check("pack", "battery", id).and_then(|_| {
                (1..=crate::rules::PACK_COUNT_MAX).contains(count).then_some(()).ok_or_else(|| format!("battery count {count}: 1-{} packs in parallel", crate::rules::PACK_COUNT_MAX))
            }),
            super::BatteryChoice::Cells { id, s, p } => check("cell", "battery", id).and_then(|_| {
                ((1..=crate::rules::CELLS_P_MAX).contains(p) && *s >= 1).then_some(()).ok_or_else(|| format!("battery {s}S{p}P: 1-{} cells in parallel, at least 1 in series", crate::rules::CELLS_P_MAX))
            }),
        }
        .map_err(serde::de::Error::custom)?;
        Ok(b)
    }
}
