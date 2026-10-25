//! One small live Build Desk run (costs CREAO credits):
//!   cargo run -p wright-creao --example desk_smoke
//! Reads CREAO_API_KEY and CREAO_APP_ID from the repo's .env. Prints the run
//! id, status, credits and the sourcing summary -- never the key.

use std::path::PathBuf;
use wright_core::{Bom, BomLine};
use wright_creao::{run_desk, Client, Config, DesignPacket, Poll};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cfg = Config::load(&root.join(".env"))?;
    eprintln!("key family {:?}, agent {}", cfg.key.family(), cfg.agent_id);
    let client = Client::new(cfg, wright_creao::transport::Reqwest::new()?);
    let line = |id: &str, name: &str, qty: f64| BomLine {
        category: "propulsion".into(),
        id: id.into(),
        name: name.into(),
        qty,
        unit: "ea".into(),
        unit_price: None,
        url: String::new(),
        note: String::new(),
    };
    let design = DesignPacket {
        vehicle: "Wright smoke test: 2-line BOM".into(),
        domain: "air".into(),
        summary: "Smoke test of the Wright -> CREAO Build Desk hand-off. Source these two parts only.".into(),
        bom: Bom { lines: vec![line("esc-30a", "30 A BLHeli_S ESC, 2-4S", 1.0), line("servo-9g", "9 g micro servo, metal gear (e.g. EMAX ES08MA II)", 2.0)] },
        checks: vec![],
        build_steps: vec!["Solder ESC to motor leads".into(), "Mount servos in wing bays".into()],
        files: vec![],
    };
    let out_dir = root.join("target/creao-smoke");
    let out = run_desk(&client, "creao-smoke", &design, &out_dir, Poll::default()).await?;
    println!("run {} {} credits {:?}", out.run.id, out.run.status, out.run.credits);
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "filled": out.sourcing.filled, "rejected": out.sourcing.rejected, "still_unpriced": out.sourcing.still_unpriced,
        "total_after_usd": out.sourcing.total_after_usd, "files": out.files,
    }))?);
    Ok(())
}
