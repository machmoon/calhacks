//! wright-fea: run the FEA cases and the validation.
//!
//!     wright-fea validate <dir>                  closed-form cases -> <dir>/validation.json
//!     wright-fea air <evals/vtol/result.json> <out_dir>
//!     wright-fea rocket <design.json> <out_dir>
//!     wright-fea balloon <build_dir> <out_dir>
//!
//! Each domain run writes <out_dir>/fea/manifest.json.

use anyhow::{bail, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize| -> Result<PathBuf> { a.get(i).map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("usage: see `wright-fea` docs")) };
    match a.get(1).map(|s| s.as_str()) {
        Some("validate") => {
            let dir = arg(2)?;
            let rows = wright_fea::validate::run(&dir)?;
            std::fs::write(dir.join("validation.json"), serde_json::to_string_pretty(&rows)?)?;
            for r in &rows {
                println!("{:<34} {:<38} FEA {:>10.4}  ref {:>10.4}  {:+6.2} %  ({:.1} s, {})", r.case, r.quantity, r.fea, r.reference, r.error_pct, r.seconds, r.mesh);
            }
        }
        Some(dom @ ("air" | "rocket" | "balloon")) => {
            let input = arg(2)?;
            let out = arg(3)?;
            let quick = a.iter().any(|x| x == "--no-media");
            let dir = out.join("fea");
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&input)?)?;
            let t0 = std::time::Instant::now();
            let m = match dom {
                #[cfg(feature = "air")]
                "air" => {
                    // a vtol_eval result.json (wright.design) or a bare design
                    let d = if v["wright"]["design"].is_object() { v["wright"]["design"].clone() } else { v };
                    wright_fea::cases::air::run(&serde_json::from_value(d)?, &dir, !quick)?
                }
                "rocket" => wright_fea::cases::rocket::run(&v, &dir, !quick)?,
                _ => wright_fea::cases::balloon::run(&v, input.parent().unwrap_or(&out), &dir, !quick)?,
            };
            m.write(&dir)?;
            println!("{}\n({:.0} s) {}", m.brief(), t0.elapsed().as_secs_f64(), dir.join("manifest.json").display());
        }
        _ => bail!("usage: wright-fea validate|air|rocket|balloon ..."),
    }
    Ok(())
}
