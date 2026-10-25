//! The balloon catalog (`data/balloon/catalog.json`): envelopes, chutes,
//! gas, insulation, lines, flight parts and ground stations.
//!
//! Every row carries `url` and `retrieved`. Fields a row's role depends on
//! are required when the catalog loads: a battery with no capacity, a load
//! with no power figure or a radio with no TX power is a load error, never
//! a zero (the Python baseline's optimizer once picked an ESC whose missing mass read 0 g).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const CATALOG_JSON: &str = include_str!("../../../data/balloon/catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub url: String,
    pub retrieved: String,
    /// None: no published price; any design using it fails the cost check.
    pub price_usd: Option<f64>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalloonRow {
    pub id: String,
    pub name: String,
    pub mass_g: f64,
    /// Manufacturer burst diameter, m.
    pub burst_diameter_m: f64,
    /// Drag coefficient used by the CUSF calculator for this size.
    pub cd: f64,
    /// Where the burst diameter comes from.
    pub burst_source: String,
    #[serde(flatten)]
    pub row: Row,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChuteRow {
    pub id: String,
    pub name: String,
    /// Nominal (flat or inflated, as the maker states) diameter, m.
    pub diameter_m: f64,
    pub cd: f64,
    pub mass_g: f64,
    #[serde(flatten)]
    pub row: Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiftGas {
    Helium,
    Hydrogen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasRow {
    pub id: String,
    pub name: String,
    pub gas: LiftGas,
    /// Mole fraction of lift gas; the rest is taken as air.
    pub purity: f64,
    /// Gas per cylinder at 1 atm, 21 C (the industry's "cubic feet"), m^3.
    pub cylinder_m3: f64,
    #[serde(flatten)]
    pub row: Row,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsulationRow {
    pub id: String,
    pub name: String,
    /// Thermal conductivity, W/(m K).
    pub k_w_mk: f64,
    pub density_kg_m3: f64,
    /// Price per m^2 of sheet at `sheet_thickness_mm`.
    pub price_usd_per_m2: Option<f64>,
    pub sheet_thickness_mm: f64,
    pub url: String,
    pub retrieved: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineRow {
    pub id: String,
    pub name: String,
    /// Rated breaking strength, N.
    pub breaking_n: f64,
    pub g_per_m: f64,
    pub price_usd_per_m: Option<f64>,
    pub url: String,
    pub retrieved: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Computer,
    Camera,
    Gps,
    Radio,
    Battery,
    Regulator,
    Sensor,
    Servo,
    Phone,
    Mcu,
    Switch,
    Hardware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chemistry {
    Lipo,
    LiIon,
    LithiumPrimary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatterySpec {
    pub chemistry: Chemistry,
    pub v_nom: f64,
    pub v_min: f64,
    pub v_max: f64,
    pub capacity_ah: f64,
    pub max_current_a: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulatorSpec {
    pub vin_min: f64,
    pub vin_max: f64,
    pub vout: f64,
    pub imax_a: f64,
    pub efficiency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplySpec {
    pub vin_min: f64,
    pub vin_max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// AX.25 AFSK 1200 on 144.390 MHz (amateur licence required).
    Aprs,
    /// ExpressLRS 900 telemetry: needs an uplink from a ground TX module.
    Elrs,
    /// Plain LoRa beacon.
    Lora,
    /// Analog video transmitter.
    Video,
    /// A phone's cellular modem.
    Cellular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioSpec {
    pub link: LinkKind,
    pub freq_mhz: f64,
    pub tx_dbm: f64,
    /// Receive sensitivity, needed for a two-way link (ELRS uplink).
    pub sens_dbm: Option<f64>,
    pub antenna_gain_dbi: f64,
    pub needs_amateur_licence: bool,
    /// Whether `tx_dbm` is a published figure (false: an assumption, said in `note`).
    pub tx_published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsSpec {
    pub chip: String,
    /// Altitude limit in an airborne dynamic model, m.
    pub alt_limit_airborne_m: f64,
    /// Altitude limit in the power-on default model, m.
    pub alt_limit_default_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraSpec {
    pub video: bool,
    pub photo: bool,
    /// Recording endurance on its own battery at room temperature, h
    /// (None: it records through a host computer on a bus).
    pub record_h: Option<f64>,
}

/// An air-activated chemical warmer: no electrical load; its heat is
/// derived in `rules::warmer_power_sl_w` and falls with pressure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmerSpec {
    /// Rated heating duration after opening, h (maker's figure).
    pub rated_h: f64,
    /// Time from opening to full heat, min (maker's figure; modelled as a ramp).
    pub activation_min: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub mass_g: f64,
    pub dims_mm: [f64; 3],
    /// Operating temperature range, C.
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    /// Average electrical draw while on, W (0 for passive parts).
    pub power_w: f64,
    /// Whether `temp_min_c` is published (false: assumed, said in `note`).
    pub temp_published: bool,
    pub supply: Option<SupplySpec>,
    pub battery: Option<BatterySpec>,
    pub regulator: Option<RegulatorSpec>,
    pub radio: Option<RadioSpec>,
    pub gps: Option<GpsSpec>,
    pub camera: Option<CameraSpec>,
    /// The part carries its own battery (camera, phone, tracker with AAs);
    /// it is not on a design bus.
    #[serde(default)]
    pub self_powered: bool,
    /// A heater switched off at or above this box temperature, C.
    #[serde(default)]
    pub thermostat_c: Option<f64>,
    /// A chemical hand warmer (heat without a battery).
    #[serde(default)]
    pub warmer: Option<WarmerSpec>,
    /// Records to or boots from a microSD card that is bought separately.
    #[serde(default)]
    pub microsd: bool,
    #[serde(flatten)]
    pub row: Row,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundStation {
    pub id: String,
    pub name: String,
    pub link: LinkKind,
    pub rx_sens_dbm: f64,
    /// TX power for two-way links (ELRS uplink), dBm.
    pub tx_dbm: Option<f64>,
    pub antenna_gain_dbi: f64,
    pub antenna_height_m: f64,
    #[serde(flatten)]
    pub row: Row,
}

/// What an extra BOM row is counted per.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtraPer {
    /// One per flight (fill regulator, chute harness).
    Flight,
    /// One per gas cylinder (rental).
    Cylinder,
    /// One per part that needs a microSD card (`Part::microsd`).
    Microsd,
}

/// Things every flight buys that are not in the payload box: the fill
/// gear, cylinder rental, the chute harness, memory cards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraRow {
    pub id: String,
    pub name: String,
    pub per: ExtraPer,
    /// Mass added to the flight train (0 for ground gear), g.
    pub mass_g: f64,
    #[serde(flatten)]
    pub row: Row,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub balloons: Vec<BalloonRow>,
    pub parachutes: Vec<ChuteRow>,
    pub gases: Vec<GasRow>,
    pub insulation: Vec<InsulationRow>,
    pub lines: Vec<LineRow>,
    pub parts: Vec<Part>,
    pub ground: Vec<GroundStation>,
    #[serde(default)]
    pub extras: Vec<ExtraRow>,
}

fn need(ok: bool, id: &str, what: &str) -> Result<()> {
    if !ok {
        bail!("catalog row {id}: {what}");
    }
    Ok(())
}

impl Catalog {
    pub fn parse(json: &str) -> Result<Self> {
        let c: Catalog = serde_json::from_str(json).context("data/balloon/catalog.json")?;
        c.validate()?;
        Ok(c)
    }

    /// Role-dependent required fields; positive finite physical values.
    pub fn validate(&self) -> Result<()> {
        let pos = |v: f64| v.is_finite() && v > 0.0;
        let mut ids = std::collections::BTreeSet::new();
        let mut uniq = |id: &str| -> Result<()> { need(ids.insert(id.to_string()), id, "duplicate id") };
        for b in &self.balloons {
            uniq(&b.id)?;
            need(pos(b.mass_g) && pos(b.burst_diameter_m) && pos(b.cd), &b.id, "mass, burst diameter and cd must be > 0")?;
            need(!b.row.url.is_empty(), &b.id, "no url")?;
        }
        for c in &self.parachutes {
            uniq(&c.id)?;
            need(pos(c.diameter_m) && pos(c.cd) && pos(c.mass_g), &c.id, "diameter, cd, mass must be > 0")?;
        }
        for g in &self.gases {
            uniq(&g.id)?;
            need(g.purity > 0.5 && g.purity <= 1.0 && pos(g.cylinder_m3), &g.id, "purity in (0.5,1], cylinder volume > 0")?;
        }
        for i in &self.insulation {
            uniq(&i.id)?;
            need(pos(i.k_w_mk) && pos(i.density_kg_m3), &i.id, "k and density > 0")?;
        }
        for l in &self.lines {
            uniq(&l.id)?;
            need(pos(l.breaking_n) && pos(l.g_per_m), &l.id, "breaking strength and mass > 0")?;
        }
        for e in &self.extras {
            uniq(&e.id)?;
            need(e.mass_g.is_finite() && e.mass_g >= 0.0 && !e.row.url.is_empty() && !e.row.retrieved.is_empty(), &e.id, "extra: mass >= 0, url and retrieved")?;
        }
        for g in &self.ground {
            uniq(&g.id)?;
            if g.link == LinkKind::Elrs {
                need(g.tx_dbm.is_some(), &g.id, "an ELRS ground station needs tx_dbm (uplink)")?;
            }
        }
        for p in &self.parts {
            uniq(&p.id)?;
            need(pos(p.mass_g), &p.id, "mass must be > 0")?;
            need(p.dims_mm.iter().all(|d| pos(*d)), &p.id, "dims must be > 0")?;
            need(p.temp_min_c < p.temp_max_c, &p.id, "temperature range")?;
            need(p.power_w.is_finite() && p.power_w >= 0.0, &p.id, "power_w")?;
            need(!p.row.url.is_empty() && !p.row.retrieved.is_empty(), &p.id, "url and retrieved")?;
            match p.role {
                Role::Battery => {
                    let b = p.battery.as_ref().with_context(|| format!("{}: battery spec missing", p.id))?;
                    need(pos(b.capacity_ah) && b.v_min < b.v_max && pos(b.max_current_a), &p.id, "battery fields")?;
                }
                Role::Regulator => {
                    let r = p.regulator.as_ref().with_context(|| format!("{}: regulator spec missing", p.id))?;
                    need(r.vin_min < r.vin_max && pos(r.imax_a) && r.efficiency > 0.0 && r.efficiency <= 1.0, &p.id, "regulator fields")?;
                }
                Role::Radio => {
                    let r = p.radio.as_ref().with_context(|| format!("{}: radio spec missing", p.id))?;
                    need(pos(r.freq_mhz), &p.id, "radio frequency")?;
                    if r.link == LinkKind::Elrs {
                        need(r.sens_dbm.is_some(), &p.id, "ELRS radio needs sens_dbm")?;
                    }
                }
                Role::Gps => {
                    let g = p.gps.as_ref().with_context(|| format!("{}: gps spec missing", p.id))?;
                    need(pos(g.alt_limit_airborne_m) && pos(g.alt_limit_default_m), &p.id, "gps limits")?;
                }
                Role::Camera => {
                    need(p.camera.is_some(), &p.id, "camera spec missing")?;
                }
                _ => {}
            }
            // An electrical load that is not self-powered must say what it accepts.
            if p.power_w > 0.0 && !p.self_powered && p.role != Role::Regulator {
                need(p.supply.is_some(), &p.id, "a bus-powered load needs a supply range")?;
            }
            if p.self_powered {
                need(p.battery.is_some(), &p.id, "a self-powered part needs its battery spec")?;
            }
            if let Some(w) = &p.warmer {
                need(pos(w.rated_h) && w.activation_min.is_finite() && w.activation_min >= 0.0, &p.id, "warmer rated_h > 0 and activation_min >= 0")?;
            }
        }
        Ok(())
    }

    pub fn balloon(&self, id: &str) -> Result<&BalloonRow> {
        self.balloons.iter().find(|r| r.id == id).with_context(|| format!("no balloon {id}"))
    }
    pub fn chute(&self, id: &str) -> Result<&ChuteRow> {
        self.parachutes.iter().find(|r| r.id == id).with_context(|| format!("no parachute {id}"))
    }
    pub fn gas(&self, id: &str) -> Result<&GasRow> {
        self.gases.iter().find(|r| r.id == id).with_context(|| format!("no gas {id}"))
    }
    pub fn insulation(&self, id: &str) -> Result<&InsulationRow> {
        self.insulation.iter().find(|r| r.id == id).with_context(|| format!("no insulation {id}"))
    }
    pub fn line(&self, id: &str) -> Result<&LineRow> {
        self.lines.iter().find(|r| r.id == id).with_context(|| format!("no line {id}"))
    }
    pub fn part(&self, id: &str) -> Result<&Part> {
        self.parts.iter().find(|r| r.id == id).with_context(|| format!("no part {id}"))
    }
    pub fn station(&self, id: &str) -> Result<&GroundStation> {
        self.ground.iter().find(|r| r.id == id).with_context(|| format!("no ground station {id}"))
    }
}

/// The catalog compiled into the crate (validated once).
pub fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| Catalog::parse(CATALOG_JSON).expect("data/balloon/catalog.json failed validation"))
}
