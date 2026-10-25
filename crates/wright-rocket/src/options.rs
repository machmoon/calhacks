//! Catalog lookups for agents (`Domain::catalog_paths` / `Domain::options`):
//! every design field whose value is a catalog id, and the rows it can take,
//! so agents pick ids instead of guessing them. Read only.

use crate::catalog::{Catalog, Row};
use crate::propellant;
use std::collections::BTreeMap;
use wright_core::CatalogOption;

/// (design path, what it chooses, catalog role(s) it draws from). Paths are
/// JSON paths into `RocketDesign`.
const TABLE: &[(&str, &str, &[&str])] = &[
    ("params.tank_tube", "6061-T6 tube for the propellant tanks and intertank (OD, wall, length)", &["tube_aluminum"]),
    ("params.airframe_tube", "fibreglass airframe tube for the recovery, pressurant and engine bays (ID, OD)", &["airframe_tube"]),
    ("params.nose", "nose cone (length, mass)", &["nosecone"]),
    ("params.fin_material", "G10 fin sheet (thickness: flutter)", &["fin_material"]),
    ("params.drogue", "drogue parachute (diameter, Cd, mass)", &["parachute"]),
    ("params.main", "main parachute (diameter, Cd, mass)", &["parachute"]),
    ("params.bottle", "GN2 pressurant bottle (volume, service pressure, mass, size)", &["pressurant_tank"]),
    ("params.regulator", "pressurant regulator (outlet range)", &["regulator"]),
    ("params.valve", "main propellant valve (pressure rating, temperature)", &["valve"]),
    ("params.altimeters[]", "dual-deploy altimeter", &["avionics"]),
    ("params.tracker", "GPS tracker", &["avionics"]),
    ("params.engine.oxidizer", "oxidizer (data/rocket/propellants.json)", &[]),
    ("params.engine.fuel", "fuel (data/rocket/propellants.json)", &[]),
];

pub fn paths() -> Vec<(&'static str, &'static str)> {
    TABLE.iter().map(|(p, w, _)| (*p, *w)).collect()
}

fn matches(q: Option<&str>, id: &str, label: &str) -> bool {
    match q.map(|q| q.trim().to_ascii_lowercase()) {
        None => true,
        Some(q) if q.is_empty() => true,
        Some(q) => id.to_ascii_lowercase().contains(&q) || label.to_ascii_lowercase().contains(&q),
    }
}

/// Every numeric or short text field of a row that is not bookkeeping:
/// the ratings and dimensions the models read.
fn specs(r: &Row) -> BTreeMap<String, String> {
    const SKIP: &[&str] = &["id", "role", "name", "url", "price_usd", "unit", "retrieved", "notes", "note", "in_stock"];
    let mut m = BTreeMap::new();
    if let Some(o) = r.0.as_object() {
        for (k, v) in o {
            if SKIP.contains(&k.as_str()) {
                continue;
            }
            match v {
                serde_json::Value::Number(n) => {
                    m.insert(k.clone(), n.to_string());
                }
                serde_json::Value::Bool(b) => {
                    m.insert(k.clone(), b.to_string());
                }
                serde_json::Value::String(s) if s.len() <= 60 => {
                    m.insert(k.clone(), s.clone());
                }
                _ => {}
            }
        }
    }
    m.insert("unit".into(), r.unit().to_string());
    m
}

fn row_option(r: &Row) -> CatalogOption {
    CatalogOption {
        id: r.id().to_string(),
        label: r.name().to_string(),
        specs: specs(r),
        price_usd: r.price(),
        in_stock: r.0["in_stock"].as_bool(),
        url: r.url().to_string(),
    }
}

/// Rows the field at `path` accepts, filtered by `query` (case-insensitive
/// substring of id or label). Unknown paths return nothing.
pub fn options(cat: &Catalog, path: &str, query: Option<&str>) -> Vec<CatalogOption> {
    let Some((_, _, roles)) = TABLE.iter().find(|(p, _, _)| *p == path) else {
        return vec![];
    };
    if path == "params.engine.oxidizer" || path == "params.engine.fuel" {
        let role = if path.ends_with("oxidizer") { "oxidizer" } else { "fuel" };
        let schema = <crate::Rocket as wright_core::Domain>::spec_schema();
        let offered: Vec<String> = schema["properties"][role]["enum"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        return propellant::catalog()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.role == role)
            .filter(|r| matches(query, &r.id, &r.cea_name))
            .map(|r| {
                let mut s = BTreeMap::new();
                s.insert("density_kg_m3".into(), format!("{}", r.density_kg_m3));
                s.insert("temperature_k".into(), format!("{}", r.temperature_k));
                s.insert("cryogenic".into(), r.cryogenic.to_string());
                s.insert("self_pressurizing".into(), r.self_pressurizing.to_string());
                s.insert("offered_in_spec_schema".into(), offered.contains(&r.id).to_string());
                CatalogOption { id: r.id.clone(), label: r.cea_name.clone(), specs: s, price_usd: None, in_stock: None, url: r.url.clone() }
            })
            .collect();
    }
    let kind_ok = |r: &Row| match path {
        "params.altimeters[]" => r.s("kind").is_some_and(|k| k.starts_with("altimeter")),
        "params.tracker" => r.s("kind").is_some_and(|k| k.contains("gps")),
        _ => true,
    };
    cat.rows
        .iter()
        .filter(|r| roles.contains(&r.role()))
        .filter(|r| kind_ok(r))
        .filter(|r| matches(query, r.id(), r.name()))
        .map(row_option)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_offers_rows_and_the_current_choice_is_among_them() {
        let cat = Catalog::load().unwrap();
        let d = <crate::Rocket as wright_core::Domain>::design(&crate::eval::requirements_spec()).unwrap();
        let v = serde_json::to_value(&d).unwrap();
        for (path, _) in paths() {
            let opts = options(&cat, path, None);
            assert!(!opts.is_empty(), "{path} offers nothing");
            // The design's own value at this path is one of the options.
            let field = path.trim_end_matches("[]").split('.').fold(&v, |acc, k| &acc[k]);
            let current = if path.ends_with("[]") { field[0].as_str() } else { field.as_str() };
            let current = current.unwrap_or_else(|| panic!("{path} has no value in the design"));
            assert!(opts.iter().any(|o| o.id == current), "{path}: {current} not offered");
        }
        // Ratings are in specs; query is case-insensitive on id or label.
        let valves = options(&cat, "params.valve", Some("CRYOGENIC"));
        assert!(!valves.is_empty() && valves.iter().all(|o| o.label.to_lowercase().contains("cryogenic") || o.id.contains("cryo")));
        assert!(valves.iter().any(|o| o.specs.contains_key("max_pressure_psi")));
        let tubes = options(&cat, "params.tank_tube", None);
        assert!(tubes.iter().all(|o| o.specs.contains_key("od_mm") && o.specs.contains_key("wall_mm")));
        assert!(options(&cat, "params.engine.oxidizer", Some("n2o")).iter().any(|o| o.id == "n2o" && o.specs["self_pressurizing"] == "true"));
        assert!(options(&cat, "no.such.path", None).is_empty());
    }
}
