//! Explicit live smoke test: no model calls, one labeled infrastructure record.
#[cfg(feature = "azure")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    use wright_agents::{azure_blob::BlobStore, runs_db::{self, RunStore}};
    let blob = BlobStore::from_env()?.ok_or_else(|| anyhow::anyhow!("set WRIGHT_BLOB_URL"))?;
    let cosmos = runs_db::azure::CosmosStore::from_uri(&std::env::var("WRIGHT_RUNS_URI")?)?;
    let dir = std::env::temp_dir().join("wright-azure-storage-smoke");
    std::fs::create_dir_all(&dir)?;
    for name in ["fixture.kicad_pcb", "fixture.step", "report.md"] {
        std::fs::write(dir.join(name), format!("Wright storage smoke fixture: {name}\nNot an engineering design.\n"))?;
    }
    let mut record = runs_db::record_from_state(&serde_json::json!({
        "domain": "storage-test", "request": "Infrastructure upload/readback smoke test, not a hardware run", "spec": {}, "design": {}
    }), "azure-storage-smoke", Some(&dir)).unwrap();
    record.passed = false;
    cosmos.log_run(&record)?;
    let saved = cosmos.get_run(&record.id)?.ok_or_else(|| anyhow::anyhow!("Cosmos record missing"))?;
    anyhow::ensure!(saved.artifact_urls.len() == record.artifacts.len(), "artifact URL count mismatch");
    for (path, url) in record.artifacts.iter().zip(&saved.artifact_urls) {
        anyhow::ensure!(blob.download(url).await? == std::fs::read(path)?, "readback mismatch: {path}");
    }
    println!("PASS: Cosmos record + {} private Blob artifacts uploaded and byte-verified", saved.artifact_urls.len());
    Ok(())
}
#[cfg(not(feature = "azure"))]
fn main() { eprintln!("run with --features azure"); }
