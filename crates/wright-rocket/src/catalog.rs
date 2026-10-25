//! The parts catalog (`data/rocket/catalog.json`). Every row carries a URL,
//! the date it was read and a price; fields a role depends on are required
//! when the catalog loads (a pressurant bottle without a volume is rejected
//! here, never read as zero litres later).

use serde_json::Value;

pub const CATALOG_JSON: &str = include_str!("../../../data/rocket/catalog.json");

#[derive(Debug, Clone)]
pub struct Row(pub Value);

impl Row {
    pub fn id(&self) -> &str {
        self.0["id"].as_str().unwrap_or("")
    }
    pub fn role(&self) -> &str {
        self.0["role"].as_str().unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.0["name"].as_str().unwrap_or("")
    }
    pub fn url(&self) -> &str {
        self.0["url"].as_str().unwrap_or("")
    }
    pub fn price(&self) -> Option<f64> {
        self.0["price_usd"].as_f64()
    }
    pub fn unit(&self) -> &str {
        self.0["unit"].as_str().unwrap_or("each")
    }
    /// A numeric field; `None` if absent (callers that need it must have
    /// listed it in `required`, so absence there is a load error).
    pub fn f(&self, k: &str) -> Option<f64> {
        self.0[k].as_f64()
    }
    pub fn s(&self, k: &str) -> Option<&str> {
        self.0[k].as_str()
    }
    pub fn in_stock(&self) -> bool {
        self.0["in_stock"].as_bool().unwrap_or(true)
    }
    /// Priced and not marked out of stock on its page (`in_stock: false`):
    /// what the search may choose. Unstated stock is accepted, as
    /// wright-air's default stock policy accepts `Stock::Unstated`.
    pub fn orderable(&self) -> bool {
        self.price().is_some() && self.in_stock()
    }
}

/// Fields each role needs for the models that read it.
fn required(role: &str) -> &'static [&'static str] {
    match role {
        "tube_aluminum" => &["od_mm", "wall_mm", "length_m"],
        "airframe_tube" => &["id_mm", "od_mm", "length_m"],
        "nosecone" => &["length_mm"],
        "fin_material" => &["thickness_mm", "area_m2"],
        "parachute" => &["diameter_in", "cd", "mass_g"],
        "pressurant_tank" => &["volume_l", "pressure_psi"],
        "regulator" => &["outlet_psi_max"],
        _ => &[],
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub rows: Vec<Row>,
    /// Rows refused at load, with the reason (never silently defaulted).
    pub rejected: Vec<(String, String)>,
}

impl Catalog {
    pub fn load() -> anyhow::Result<Self> {
        let v: Value = serde_json::from_str(CATALOG_JSON)?;
        let mut rows = Vec::new();
        let mut rejected = Vec::new();
        for r in v["rows"].as_array().ok_or_else(|| anyhow::anyhow!("catalog has no rows"))? {
            let row = Row(r.clone());
            anyhow::ensure!(!row.id().is_empty() && !row.url().is_empty(), "catalog row without id/url: {r}");
            anyhow::ensure!(row.0["retrieved"].is_string(), "catalog row {} has no retrieved date", row.id());
            if let Some(k) = required(row.role()).iter().find(|k| !row.f(k).is_some_and(|x| x.is_finite() && x > 0.0)) {
                rejected.push((row.id().to_string(), format!("role {} needs {k}", row.role())));
                continue;
            }
            rows.push(row);
        }
        Ok(Self { rows, rejected })
    }
    pub fn get(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id() == id)
    }
    pub fn role<'a>(&'a self, role: &'a str) -> impl Iterator<Item = &'a Row> + 'a {
        self.rows.iter().filter(move |r| r.role() == role)
    }
    /// The first orderable id of `prefs` (in order), else the first.
    pub fn first_orderable(&self, prefs: &[&str]) -> String {
        prefs.iter().find(|id| self.get(id).is_some_and(|r| r.orderable())).or(prefs.first()).map(|s| s.to_string()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_loads_with_required_fields() {
        let c = super::Catalog::load().unwrap();
        assert!(c.role("parachute").count() >= 4);
        assert!(c.role("tube_aluminum").count() >= 1);
    }
}
