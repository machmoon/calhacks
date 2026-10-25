//! Reactants: the propellant catalog (`data/rocket/propellants.json`) and
//! the element/enthalpy totals of a propellant mixture.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROPELLANTS_JSON: &str = include_str!("../../../data/rocket/propellants.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reactant {
    pub id: String,
    pub cea_name: String,
    pub role: String,
    pub formula: BTreeMap<String, f64>,
    pub molar_mass_g: f64,
    pub temperature_k: f64,
    pub enthalpy_j_mol: f64,
    pub density_kg_m3: f64,
    pub cryogenic: bool,
    /// Stored as a saturated liquid on its own vapour (N2O): the run tank
    /// blows down instead of being regulated (see `nitrous`).
    #[serde(default)]
    pub self_pressurizing: bool,
    pub url: String,
    pub retrieved: String,
    pub density_url: String,
}

#[derive(Deserialize)]
struct File {
    rows: Vec<Reactant>,
}

/// Every reactant row, validated: a row missing a field fails to load
/// (serde), and non-physical values are rejected here.
pub fn catalog() -> anyhow::Result<Vec<Reactant>> {
    let f: File = serde_json::from_str(PROPELLANTS_JSON)?;
    for r in &f.rows {
        anyhow::ensure!(r.molar_mass_g > 0.0 && r.density_kg_m3 > 0.0 && r.temperature_k > 0.0, "bad row {}", r.id);
        anyhow::ensure!(!r.url.is_empty() && !r.retrieved.is_empty(), "unsourced row {}", r.id);
        let m: f64 = r
            .formula
            .iter()
            .map(|(e, n)| n * atomic_mass_g(e).unwrap_or(f64::NAN))
            .sum();
        anyhow::ensure!((m - r.molar_mass_g).abs() / r.molar_mass_g < 2e-3, "formula mass {m} != {} for {}", r.molar_mass_g, r.id);
    }
    Ok(f.rows)
}

pub fn reactant(id: &str) -> anyhow::Result<Reactant> {
    let rows = catalog()?;
    let known: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    // "LOX" and "lox" name the same oxidizer: people (and models) capitalise
    rows.into_iter()
        .find(|r| r.id.eq_ignore_ascii_case(id.trim()))
        .ok_or_else(|| anyhow::anyhow!("no propellant '{id}'; known: {}", known.join(", ")))
}

/// Standard atomic weights, g/mol (IUPAC 2013 conventional values, as in
/// CEA `source/atomic_data.f90`).
pub fn atomic_mass_g(e: &str) -> Option<f64> {
    Some(match e {
        "H" => 1.00794,
        "C" => 12.0107,
        "N" => 14.0067,
        "O" => 15.9994,
        "AR" | "Ar" => 39.948,
        _ => return None,
    })
}

/// A propellant mixture by mass fraction.
#[derive(Debug, Clone)]
pub struct Mix {
    pub parts: Vec<(Reactant, f64)>,
}

impl Mix {
    /// Oxidiser/fuel by mass ratio; `fuel` may itself be a blend
    /// (e.g. 75 % ethanol / 25 % water by mass).
    pub fn bipropellant(ox: &Reactant, fuel: &[(Reactant, f64)], of: f64) -> Self {
        let wsum: f64 = fuel.iter().map(|(_, w)| w).sum();
        let mut parts = vec![(ox.clone(), of / (1.0 + of))];
        for (r, w) in fuel {
            parts.push((r.clone(), w / wsum / (1.0 + of)));
        }
        Self { parts }
    }
    /// mol of each element per kg of mixture.
    pub fn elements(&self) -> BTreeMap<String, f64> {
        let mut b = BTreeMap::new();
        for (r, w) in &self.parts {
            for (e, n) in &r.formula {
                *b.entry(e.clone()).or_insert(0.0) += w * n / (r.molar_mass_g * 1e-3);
            }
        }
        b
    }
    /// Enthalpy, J/kg (each reactant at its assigned temperature).
    pub fn enthalpy(&self) -> f64 {
        self.parts.iter().map(|(r, w)| w * r.enthalpy_j_mol / (r.molar_mass_g * 1e-3)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_loads_and_formulas_match_molar_mass() {
        let c = catalog().unwrap();
        assert!(c.iter().any(|r| r.id == "lox"));
        assert!(c.iter().any(|r| r.id == "ethanol"));
    }
}
