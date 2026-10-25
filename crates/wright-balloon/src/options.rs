//! Catalog lookups for the agents' read-only `catalog` tool
//! (`Domain::catalog_paths` / `Domain::options`): every design field that
//! holds a catalog id, and the rows it may take.
//!
//! Each path offers only rows the domain accepts there: a tracker radio is
//! a part with a radio spec, a bus battery a battery, a rail a regulator,
//! so an id taken from a lookup never fails the analysis's own lookups
//! (`Catalog::part`, "is not a battery", "is not a GPS", ...).

use crate::catalog::{Catalog, LinkKind, Part, Role};
use std::collections::BTreeMap;
use wright_core::CatalogOption;

/// `(design path, what it chooses)`. `[]` marks a list field: the id goes
/// in any element of it.
pub const PATHS: &[(&str, &str)] = &[
    ("balloon", "latex envelope (burst diameter, mass)"),
    ("gas", "lift gas cylinder (helium or hydrogen)"),
    ("parachute", "descent parachute (diameter, Cd)"),
    ("line", "suspension line, the weak link (breaking strength)"),
    ("enclosure.insulation", "insulation wall material of the payload box"),
    ("items[].part", "any payload part: trackers, radios, GPS, cameras, computers, sensors, heater, hand warmers"),
    ("buses[].battery", "battery cell or pack of a power bus"),
    ("buses[].rails", "regulator / buck converter on a bus (one id per rail)"),
    ("trackers[].radio", "tracker radio (APRS, ELRS, LoRa, video, phone); must also be in items"),
    ("trackers[].gps", "GPS feeding a tracker (a phone serves as its own); must also be in items"),
    ("trackers[].ground", "ground station a tracker reports to"),
    ("buy_ground[]", "ground station bought for this design"),
];

/// A number as short text: no trailing zeros, at most 3 decimals.
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// An enum's snake_case catalog name ("lithium_primary", "hydrogen").
fn name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn link(l: LinkKind) -> &'static str {
    match l {
        LinkKind::Aprs => "aprs",
        LinkKind::Elrs => "elrs",
        LinkKind::Lora => "lora",
        LinkKind::Video => "video",
        LinkKind::Cellular => "cellular",
    }
}

/// What a part is, in the words an agent searches with ("camera", "heater",
/// "hand warmer", "buck converter"): appended to the label in brackets so
/// the id/label query finds it.
fn kind(p: &Part) -> String {
    if p.warmer.is_some() {
        return "hand warmer, heat".into();
    }
    if p.thermostat_c.is_some() {
        return "heater".into();
    }
    match p.role {
        Role::Computer => "computer".into(),
        Role::Camera => "camera".into(),
        Role::Gps => "gps".into(),
        Role::Radio => {
            let r = p.radio.as_ref().map_or("", |r| link(r.link));
            if p.gps.is_some() {
                format!("{r} tracker, radio + gps")
            } else {
                format!("{r} radio")
            }
        }
        Role::Battery => "battery".into(),
        Role::Regulator => {
            let r = p.regulator.as_ref();
            if r.is_some_and(|r| r.vin_min < r.vout && r.vin_max > r.vout) {
                "regulator, buck-boost".into()
            } else {
                "regulator, buck converter".into()
            }
        }
        Role::Sensor => "sensor".into(),
        Role::Servo => "servo".into(),
        Role::Phone => "phone, cellular tracker, gps".into(),
        Role::Mcu => "microcontroller".into(),
        Role::Switch => "switch".into(),
        Role::Hardware => "hardware".into(),
    }
}

fn part_option(p: &Part) -> CatalogOption {
    let mut s = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        s.insert(k.to_string(), v);
    };
    put("mass_g", num(p.mass_g));
    put("temp_range_c", format!("{}..{}{}", num(p.temp_min_c), num(p.temp_max_c), if p.temp_published { "" } else { " (assumed)" }));
    if p.power_w > 0.0 {
        put("power_w", num(p.power_w));
    }
    if let Some(v) = &p.supply {
        put("supply_v", format!("{}..{}", num(v.vin_min), num(v.vin_max)));
        if p.power_w > 0.0 {
            // the current at the bottom of its input range (the worst case)
            put("current_a_max", num(p.power_w / v.vin_min));
        }
    }
    if p.self_powered {
        put("self_powered", "true".into());
    }
    if let Some(b) = &p.battery {
        put("chemistry", name(&b.chemistry));
        put("voltage_v", format!("{} nom ({}..{})", num(b.v_nom), num(b.v_min), num(b.v_max)));
        put("capacity_ah", num(b.capacity_ah));
        put("current_a_max", num(b.max_current_a));
    }
    if let Some(r) = &p.regulator {
        put("vin_v", format!("{}..{}", num(r.vin_min), num(r.vin_max)));
        put("vout_v", num(r.vout));
        put("current_a_max", num(r.imax_a));
        put("efficiency", num(r.efficiency));
    }
    if let Some(r) = &p.radio {
        put("link", link(r.link).into());
        put("freq_mhz", num(r.freq_mhz));
        put("tx_dbm", format!("{}{}", num(r.tx_dbm), if r.tx_published { "" } else { " (assumed)" }));
        if r.needs_amateur_licence {
            put("needs_ham_licence", "true".into());
        }
    }
    if let Some(g) = &p.gps {
        put("gps_chip", g.chip.clone());
        put("gps_alt_limit_m", format!("{} airborne, {} default", num(g.alt_limit_airborne_m), num(g.alt_limit_default_m)));
    }
    if let Some(c) = &p.camera {
        put("records", match (c.video, c.photo) {
            (true, true) => "video, photo",
            (true, false) => "video",
            (false, true) => "photo",
            (false, false) => "none",
        }
        .into());
        if let Some(h) = c.record_h {
            put("record_h", num(h));
        }
    }
    if let Some(t) = p.thermostat_c {
        put("thermostat_off_c", num(t));
    }
    if let Some(w) = &p.warmer {
        put("rated_h", num(w.rated_h));
        put("activation_min", num(w.activation_min));
    }
    if p.microsd {
        put("needs_microsd", "true".into());
    }
    CatalogOption { id: p.id.clone(), label: format!("{} [{}]", p.name, kind(p)), specs: s, price_usd: p.row.price_usd, in_stock: None, url: p.row.url.clone() }
}

fn specs<const N: usize>(kv: [(&str, String); N]) -> BTreeMap<String, String> {
    kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Every row the choice at `path` can take, unfiltered; None for a path
/// that is not catalog-backed.
pub fn all(cat: &Catalog, path: &str) -> Option<Vec<CatalogOption>> {
    let parts = |keep: &dyn Fn(&Part) -> bool| cat.parts.iter().filter(|p| keep(p)).map(part_option).collect::<Vec<_>>();
    let stations = || {
        cat.ground
            .iter()
            .map(|g| {
                let mut s = specs([("link", link(g.link).into()), ("rx_sens_dbm", num(g.rx_sens_dbm)), ("antenna_gain_dbi", num(g.antenna_gain_dbi)), ("antenna_height_m", num(g.antenna_height_m))]);
                if let Some(t) = g.tx_dbm {
                    s.insert("tx_dbm".into(), num(t));
                }
                CatalogOption { id: g.id.clone(), label: format!("{} [{} ground station]", g.name, link(g.link)), specs: s, price_usd: g.row.price_usd, in_stock: None, url: g.row.url.clone() }
            })
            .collect::<Vec<_>>()
    };
    Some(match path {
        "balloon" => cat
            .balloons
            .iter()
            .map(|b| CatalogOption {
                id: b.id.clone(),
                label: b.name.clone(),
                specs: specs([("burst_diameter_m", num(b.burst_diameter_m)), ("mass_g", num(b.mass_g)), ("cd", num(b.cd))]),
                price_usd: b.row.price_usd,
                in_stock: None,
                url: b.row.url.clone(),
            })
            .collect(),
        "gas" => cat
            .gases
            .iter()
            .map(|g| CatalogOption {
                id: g.id.clone(),
                label: g.name.clone(),
                specs: specs([("gas", name(&g.gas)), ("purity", g.purity.to_string()), ("cylinder_m3", num(g.cylinder_m3))]),
                price_usd: g.row.price_usd,
                in_stock: None,
                url: g.row.url.clone(),
            })
            .collect(),
        "parachute" => cat
            .parachutes
            .iter()
            .map(|c| CatalogOption {
                id: c.id.clone(),
                label: format!("{} [chute]", c.name),
                specs: specs([("diameter_m", num(c.diameter_m)), ("cd", num(c.cd)), ("mass_g", num(c.mass_g))]),
                price_usd: c.row.price_usd,
                in_stock: None,
                url: c.row.url.clone(),
            })
            .collect(),
        "line" => cat
            .lines
            .iter()
            .map(|l| CatalogOption {
                id: l.id.clone(),
                label: l.name.clone(),
                specs: specs([
                    ("breaking_n", num(l.breaking_n)),
                    ("breaking_lbf", format!("{:.1}", wright_core::units::Force::n(l.breaking_n).as_lbf())),
                    ("g_per_m", num(l.g_per_m)),
                    ("price_per", "m".into()),
                ]),
                price_usd: l.price_usd_per_m,
                in_stock: None,
                url: l.url.clone(),
            })
            .collect(),
        "enclosure.insulation" => cat
            .insulation
            .iter()
            .map(|i| CatalogOption {
                id: i.id.clone(),
                label: format!("{} [insulation]", i.name),
                specs: specs([
                    ("k_w_mk", num(i.k_w_mk)),
                    ("density_kg_m3", num(i.density_kg_m3)),
                    ("sheet_thickness_mm", num(i.sheet_thickness_mm)),
                    ("price_per", "m^2 of sheet".into()),
                ]),
                price_usd: i.price_usd_per_m2,
                in_stock: None,
                url: i.url.clone(),
            })
            .collect(),
        "items[].part" => parts(&|_| true),
        "buses[].battery" => parts(&|p| p.role == Role::Battery && p.battery.is_some()),
        "buses[].rails" => parts(&|p| p.role == Role::Regulator && p.regulator.is_some()),
        "trackers[].radio" => parts(&|p| p.radio.as_ref().is_some_and(|r| r.link != LinkKind::Video)),
        // what the link analysis accepts as a position source
        "trackers[].gps" => parts(&|p| p.gps.is_some() || p.radio.as_ref().is_some_and(|r| r.link == LinkKind::Cellular)),
        "trackers[].ground" | "buy_ground[]" => stations(),
        _ => return None,
    })
}

/// The rows at `path` whose id or label contains `query`, ignoring case
/// (every row when the query is None or blank).
pub fn options(cat: &Catalog, path: &str, query: Option<&str>) -> Vec<CatalogOption> {
    let rows = all(cat, path).unwrap_or_default();
    let q = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
    match q {
        None => rows,
        Some(q) => rows.into_iter().filter(|o| o.id.to_lowercase().contains(&q) || o.label.to_lowercase().contains(&q)).collect(),
    }
}
