//! Fly Wright's eval design on a real GFS forecast and animate it.
//!
//!   cargo run -p wright-balloon --example sim --release [LAUNCH_UTC] [--offline]
//!
//! Reads the design from evals/balloon/result.json (run the eval first),
//! asks the public Tawhiri API (predict::TAWHIRI_API, SondeHub) for the
//! flight with this design's ascent rate, burst altitude and sea-level
//! descent rate from the eval's launch site, caches the answer in
//! data/balloon/predictions/, and writes evals/balloon/sim/ (tracks.csv,
//! comparison.json, flight.mp4, flight.png, manifest.json).

use anyhow::{Context, Result};
use std::path::Path;
use wright_balloon::{catalog, predict, reference, sim, Design};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let offline = args.iter().any(|a| a == "--offline");
    let when = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "2026-10-25T16:00:00Z".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/balloon");
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("result.json")).context("run the eval first")?)?;
    let d: Design = serde_json::from_value(r["wright"]["design"].clone())?;
    let spec: wright_balloon::Spec = serde_json::from_value(r["spec"].clone()).unwrap_or_else(|_| reference::mission_spec(30_000.0, None));
    let cat = catalog::catalog();
    let cache = sim::predictions_dir().join(format!("{}.json", when.replace(':', "")));
    let pred: predict::Prediction = if offline { serde_json::from_str(&std::fs::read_to_string(&cache)?)? } else { sim::fetch(&spec, &d, cat, &when)? };
    println!("endpoint {}; GFS dataset {}", predict::TAWHIRI_API, pred.dataset);
    let manifest = sim::write(&spec, &d, cat, &pred, &root.join("sim"))?;
    println!("{}", manifest.brief());
    Ok(())
}
