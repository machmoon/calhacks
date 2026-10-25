//! `wright merge SHARD.json... [-o OUT.json]`: put the result files of a
//! sharded run (`wright bench --shard i/n`, `rocket_matrix --shard i/n
//! --json`) back together, the way SWE-bench's harness writes one
//! `report.json` per instance and then aggregates them
//! (`swebench/harness/run_evaluation.py::run_instance` writes
//! `log_dir/report.json`; `main` ends in `make_run_report`).
//!
//! Two shapes, told apart by their keys:
//! * bench results (`{"scenarios": [...]}`, `wright_agents::bench::results_json`):
//!   the scenarios are concatenated in the order the files are given and the
//!   headline counts recomputed by the same `results_json`, so merging the
//!   shards of a run gives the same file the unsharded run writes.
//! * row files (`{"kind": "...", "rows": [{"id": ...}, ...]}`): rows
//!   concatenated; every file must have the same `kind`.
//!
//! A duplicated id is an error (two shards ran the same job, or one file was
//! given twice), not a silent overwrite.

use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub fn merge(inputs: &[Value]) -> anyhow::Result<Value> {
    anyhow::ensure!(!inputs.is_empty(), "nothing to merge");
    let mut seen = BTreeSet::new();
    let mut dup = |id: &str| -> anyhow::Result<()> {
        if !seen.insert(id.to_string()) {
            bail!("`{id}` appears in more than one input (a shard ran twice, or a file was given twice)");
        }
        Ok(())
    };
    if inputs.iter().all(|v| v.get("scenarios").is_some()) {
        let mut outcomes: Vec<wright_agents::bench::Outcome> = vec![];
        for v in inputs {
            let part: Vec<wright_agents::bench::Outcome> = serde_json::from_value(v["scenarios"].clone()).context("a bench results file's scenarios")?;
            for o in &part {
                dup(&o.id)?;
            }
            outcomes.extend(part);
        }
        return Ok(wright_agents::bench::results_json(&outcomes));
    }
    if inputs.iter().all(|v| v.get("rows").is_some()) {
        let kind = inputs[0].get("kind").cloned().unwrap_or(Value::Null);
        let mut rows = vec![];
        for v in inputs {
            if v.get("kind").cloned().unwrap_or(Value::Null) != kind {
                bail!("inputs mix kinds {kind} and {}", v.get("kind").unwrap_or(&Value::Null));
            }
            for r in v["rows"].as_array().context("`rows` is not an array")? {
                dup(r.get("id").and_then(Value::as_str).context("a row has no string `id`")?)?;
                rows.push(r.clone());
            }
        }
        return Ok(json!({ "kind": kind, "shards": inputs.len(), "rows": rows }));
    }
    bail!("inputs are neither all bench results (`scenarios`) nor all row files (`rows`)")
}

pub fn merge_cmd(args: &[String]) -> anyhow::Result<()> {
    let (mut files, mut out) = (vec![], None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = it.next().map(PathBuf::from),
            other if other.starts_with('-') => bail!("unknown flag {other}"),
            other => files.push(PathBuf::from(other)),
        }
    }
    anyhow::ensure!(!files.is_empty(), "usage: wright merge SHARD.json... [-o OUT.json]");
    let inputs = files.iter().map(|p| read(p)).collect::<anyhow::Result<Vec<_>>>()?;
    let merged = merge(&inputs)?;
    let text = serde_json::to_string_pretty(&merged)?;
    match out {
        Some(p) => {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))?;
            if let (Some(passed), Some(total)) = (merged["passed"].as_u64(), merged["total"].as_u64()) {
                println!("{passed}/{total} scenarios pass");
            }
            println!("merged {} files -> {}", files.len(), p.display());
        }
        None => println!("{text}"),
    }
    Ok(())
}

fn read(p: &Path) -> anyhow::Result<Value> {
    let t = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&t).with_context(|| format!("parsing {}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::shard::Shard;

    fn scenarios() -> Vec<wright_agents::bench::Scenario> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/scenarios");
        wright_agents::bench::load_dir(&dir).unwrap()
    }

    /// The bench run in 4 shards and merged is the unsharded run's results
    /// file, field for field.
    #[test]
    fn sharded_bench_merges_to_the_full_run() {
        let all = scenarios();
        assert!(all.len() >= 4);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let full = wright_agents::bench::results_json(&rt.block_on(wright_agents::bench::run_all(&all, &crate::bench::resolve)));
        let parts: Vec<Value> = (1..=4)
            .map(|c| {
                let mine = Shard { current: c, total: 4 }.select(all.clone());
                wright_agents::bench::results_json(&rt.block_on(wright_agents::bench::run_all(&mine, &crate::bench::resolve)))
            })
            .collect();
        assert_eq!(parts.iter().map(|p| p["total"].as_u64().unwrap()).sum::<u64>(), all.len() as u64);
        let merged = merge(&parts).unwrap();
        assert_eq!(merged["total"], full["total"]);
        assert_eq!(merged["passed"], full["passed"]);
        assert_eq!(merged["scenarios"], full["scenarios"]);
        assert!((merged["spent_usd"].as_f64().unwrap() - full["spent_usd"].as_f64().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let a = json!({"kind": "rocket_matrix", "rows": [{"id": "a"}, {"id": "b"}]});
        let b = json!({"kind": "rocket_matrix", "rows": [{"id": "c"}]});
        let m = merge(&[a.clone(), b]).unwrap();
        assert_eq!(m["rows"].as_array().unwrap().len(), 3);
        assert_eq!(m["shards"], 2);
        assert!(merge(&[a.clone(), a]).unwrap_err().to_string().contains("more than one input"));
    }

    #[test]
    fn mixed_shapes_are_refused() {
        let rows = json!({"kind": "rocket_matrix", "rows": []});
        let other = json!({"kind": "balloon", "rows": []});
        let bench = json!({"passed": 0, "total": 0, "spent_usd": 0.0, "scenarios": []});
        assert!(merge(&[rows.clone(), other]).is_err());
        assert!(merge(&[rows, bench.clone()]).is_err());
        assert_eq!(merge(&[bench]).unwrap()["total"], 0);
        assert!(merge(&[]).is_err());
    }
}
