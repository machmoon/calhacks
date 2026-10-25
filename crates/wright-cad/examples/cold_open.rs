//! Measure a real native document through the public bounded Session API.
//! Usage: cargo run -p wright-cad --example cold_open -- SOURCE.FCStd NEW_OUTPUT_DIR
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::{path::PathBuf, time::Instant};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 { bail!("usage: cold_open SOURCE.FCStd NEW_OUTPUT_DIR"); }
    let source = std::fs::canonicalize(&args[0]).context("source path")?;
    let output = PathBuf::from(&args[1]);
    if output.exists() { bail!("output directory must be new to guarantee a cold Session"); }
    std::fs::create_dir_all(&output)?;
    let before = std::fs::read(&source)?;
    let mut session = wright_cad::Session::new(output.join("cad"));
    let started = Instant::now();
    let result = session.action("open_document", json!({"path":source}));
    let elapsed = started.elapsed().as_secs_f64();
    let unchanged = std::fs::read(&source)? == before;
    let mut record = json!({
        "source":source,"public_api":"wright_cad::Session::action(open_document)",
        "elapsed_seconds":elapsed,"source_file_unchanged":unchanged,
        "open_request_limit_seconds":300,"ordinary_worker_limit_seconds":60,
        "timing_scope":"Cold worker startup, open validation and automatic checkpoint; no timeout override",
        "status":if result.is_ok(){"completed"}else{"failed"}
    });
    match &result {
        Ok(value) => { record["result"] = value.clone(); }
        Err(error) => { record["error"] = json!(format!("{error:#}")); }
    }
    std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&record)?)?;
    eprintln!("Evidence: {}", output.join("result.json").display());
    if !unchanged { bail!("source file changed"); }
    result.map(|_| ())
}
