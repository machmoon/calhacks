//! Final desktop archive: called after all producers (including native CAD)
//! exit, so guides and late PCB/CAD exports are included. No model calls.
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use wright_agents::runs_db::{self, RunRecord, RunStore};

pub fn record(dir: &Path, domain: &str, request: &str) -> Result<RunRecord> {
    let root = dir.canonicalize().context("run directory missing")?;
    let mut rec = runs_db::record_from_dir(&root).unwrap_or_else(|| {
        runs_db::record_from_state(&json!({"domain":domain,"request":request,"spec":{},"design":{}}),
            &root.file_name().unwrap_or_default().to_string_lossy(), Some(&root)).expect("domain supplied")
    });
    if let Ok(events) = std::fs::read_to_string(root.join("events.jsonl")) {
        for line in events.lines() {
            let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
            if event["type"] == "run_started" {
                if let Some(id) = event["run"].as_str() { rec.id = id.into(); }
                if let Some(d) = event["domain"].as_str() { rec.domain = d.into(); }
                if let Some(r) = event["request"].as_str() { rec.request = r.into(); }
            }
            if event["type"] == "artifact" {
                if let Some(path) = event["path"].as_str() { rec.artifacts.push(path.into()); }
            }
        }
    }
    rec.artifacts.sort();
    rec.artifacts.dedup();
    for artifact in &rec.artifacts {
        let resolved = Path::new(artifact).canonicalize().with_context(|| format!("missing artifact {artifact}"))?;
        anyhow::ensure!(resolved.starts_with(&root), "artifact outside run directory: {artifact}");
    }
    Ok(rec)
}

pub fn save(dir: &Path, domain: &str, request: &str, store: &dyn RunStore) -> Result<usize> {
    let rec = record(dir, domain, request)?;
    store.log_run(&rec)?;
    if store.describe().starts_with("https://") {
        let saved = store.get_run(&rec.id)?.context("cloud run missing after save")?;
        anyhow::ensure!(saved.artifact_urls.len() == rec.artifacts.len(),
            "cloud artifact count mismatch: expected {}, found {}; Blob uploads may be disabled", rec.artifacts.len(), saved.artifact_urls.len());
        return Ok(saved.artifact_urls.len());
    }
    Ok(rec.artifacts.len())
}

pub fn sync(args: &[String], data: &Path) -> Result<()> {
    let dir = Path::new(args.first().context("usage: wright runs sync DIR --domain DOMAIN --request TEXT")?);
    let flag = |name: &str| args.iter().position(|v| v == name).and_then(|i| args.get(i + 1)).map(String::as_str);
    let result = (|| {
        let store = runs_db::from_env(data)?.context("run storage disabled")?;
        let uri = std::env::var("WRIGHT_RUNS_URI").unwrap_or_default();
        anyhow::ensure!(!uri.starts_with("cosmos://") || store.describe().starts_with("https://"), "Azure unavailable; files remain local. Sign in with az login and retry wright runs sync");
        let count = save(dir, flag("--domain").unwrap_or("hardware"), flag("--request").unwrap_or(""), store.as_ref())?;
        Ok::<_, anyhow::Error>((count, store.describe()))
    })();
    let status = match &result {
        Ok((count, dest)) => json!({"status":"saved","artifact_count":count,"destination":dest}),
        Err(e) => json!({"status":"failed","error":format!("{e:#}"),"local_files_retained":true}),
    };
    std::fs::write(dir.join("storage-status.json"), serde_json::to_vec_pretty(&status)?)?;
    use std::io::Write;
    let text = match &result {
        Ok((count, dest)) => format!("Saved run and {count} artifacts to {dest}"),
        Err(e) => format!("Cloud save failed: {e:#}. Local files retained; retry wright runs sync {}", dir.display()),
    };
    writeln!(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl"))?, "{}", json!({"type":"agent_message","run":dir.file_name().unwrap_or_default().to_string_lossy(),"agent":"storage","text":text}))?;
    println!("{text}");
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_route_without_state_saves_late_cad_and_pcb() {
        let dir = std::env::temp_dir().join(format!("wright-final-archive-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cad")).unwrap();
        std::fs::write(dir.join("cad/assembly.FCStd"), "native fixture").unwrap();
        std::fs::write(dir.join("board.kicad_pcb"), "pcb fixture").unwrap();
        std::fs::write(dir.join("guide.md"), "late guide").unwrap();
        std::fs::write(dir.join("events.jsonl"), "{\"type\":\"run_started\",\"run\":\"native-test\",\"domain\":\"aircraft\"}\n").unwrap();
        let store = runs_db::JsonlStore::new(dir.join("db.jsonl"));
        assert_eq!(save(&dir,"hardware","test",&store).unwrap(),3);
        let saved = store.get_run("native-test").unwrap().unwrap();
        assert_eq!(saved.domain,"aircraft");
        assert!(!saved.passed);
        assert!(saved.artifacts.iter().any(|p| p.ends_with("assembly.FCStd")));
        std::fs::write(dir.join("events.jsonl"), format!("{}\n",json!({"type":"artifact","path":"/etc/hosts"}))).unwrap();
        assert!(record(&dir,"hardware","test").unwrap_err().to_string().contains("outside run"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
