//! Exact BOM-to-supplier-CAD discovery. This report does not import geometry.
use serde::Deserialize;
use serde_json::{json, Value};
use wright_core::Bom;

#[derive(Deserialize)]
struct Registry {
    schema_version: u32,
    components: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    model: String,
    file: String,
    sha256: String,
    #[serde(default)]
    bom_catalog_ids: Vec<String>,
    native_import_validation: String,
}

/// The runtime component_catalog/import_component tools revalidate the source
/// hash and native geometry. An embedded registry match alone is not that proof.
pub fn report(bom: &Bom) -> anyhow::Result<Value> {
    report_with_registry(bom, include_str!("../../../cad/components/catalog.json"))
}

fn report_with_registry(bom: &Bom, text: &str) -> anyhow::Result<Value> {
    let registry: Registry = serde_json::from_str(text)?;
    anyhow::ensure!(registry.schema_version == 1, "unsupported CAD catalog schema");
    let mut ids = std::collections::HashSet::new();
    for entry in &registry.components {
        anyhow::ensure!(ids.insert(&entry.id), "duplicate CAD component ID");
    }
    let rows: Vec<Value> = bom.lines.iter().map(|line| {
        // No prefix/fuzzy match or folding-prop hub suffix removal: an assembly
        // or related part is not the exact selected component.
        let candidates: Vec<Value> = registry.components.iter()
            .filter(|entry| entry.bom_catalog_ids.contains(&line.id))
            .map(|entry| json!({
                "component_id": entry.id, "model": entry.model,
                "source_path_relative_to_cad_components": entry.file,
                "expected_sha256": entry.sha256,
                "recorded_import_status": entry.native_import_validation,
                "lookup": {"action": "component_catalog", "query": entry.id},
            })).collect();
        let status = if candidates.is_empty() { "no_exact_registered_model" }
            else if candidates.len() == 1 { "exact_registry_match_requires_runtime_validation" }
            else { "multiple_exact_entries_require_selection" };
        json!({"bom_id": line.id, "category": line.category, "name": line.name,
            "quantity": line.qty, "unit": line.unit, "role": line.note,
            "status": status, "candidates": candidates})
    }).collect();
    Ok(json!({"schema_version": 1,
        "scope": "Exact registry discovery only. No geometry imported, BOM substitutions, mounting datums or fit validation performed. Runtime tools must verify files and geometry before import; placement must be explicitly established.",
        "registry": "cad/components/catalog.json (build-time snapshot)",
        "rows": rows}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::BomLine;

    fn bom(ids: &[&str]) -> Bom {
        Bom { lines: ids.iter().map(|id| BomLine {
            category: "parts".into(), id: (*id).into(), name: (*id).into(),
            qty: 1.0, unit: "pcs".into(), unit_price: None, url: String::new(),
            note: String::new(),
        }).collect() }
    }

    #[test]
    fn exact_mapping_does_not_substitute_related_parts() {
        let b = bom(&["tmotor_mn2806_evo_650kv", "tmotor_mn2806_650kv",
            "tmotor_p12x4_cf", "tmotor_p12x4_cf_hub", "aos_supernova_2807"]);
        let before = serde_json::to_value(&b).unwrap();
        let result = report(&b).unwrap();
        let rows = result["rows"].as_array().unwrap();
        assert_eq!(rows[0]["candidates"][0]["component_id"], "tmotor_mn2806_evo");
        assert_eq!(rows[2]["candidates"][0]["component_id"], "tmotor_p12x4");
        for i in [1, 3, 4] { assert_eq!(rows[i]["status"], "no_exact_registered_model"); }
        assert_eq!(serde_json::to_value(&b).unwrap(), before);
    }

    #[test]
    fn unsupported_models_and_ambiguity_remain_explicit() {
        let entry = json!({"id":"board_a", "model":"Board", "file":"board.step",
            "sha256":"test", "bom_catalog_ids":["board"],
            "native_import_validation":"unsupported_mixed_geometry"});
        let mut second = entry.clone(); second["id"] = json!("board_b");
        let registry = json!({"schema_version":1,"components":[entry, second]});
        let result = report_with_registry(&bom(&["board"]), &registry.to_string()).unwrap();
        assert_eq!(result["rows"][0]["status"], "multiple_exact_entries_require_selection");
        assert_eq!(result["rows"][0]["candidates"][0]["recorded_import_status"], "unsupported_mixed_geometry");
    }
}
