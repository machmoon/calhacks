//! `wright-fab BUILD_DIR [--out OUT_DIR] [--domain D] [--no-films] [--max-prints N]`
//! writes OUT_DIR/fab (OUT_DIR defaults to BUILD_DIR) and its manifest.json.
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut build = None;
    let mut out = None;
    let mut domain = None;
    let mut opt = wright_fab::fab::Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().map(PathBuf::from),
            "--domain" => domain = it.next().cloned(),
            "--no-films" => opt.films = false,
            "--max-prints" => opt.max_prints = it.next().and_then(|s| s.parse().ok()).unwrap_or(8),
            _ => build = Some(PathBuf::from(a)),
        }
    }
    let build = build.ok_or_else(|| anyhow::anyhow!("usage: wright-fab BUILD_DIR [--out OUT_DIR] [--domain aircraft|rocket|balloon] [--no-films] [--max-prints N]"))?;
    let design: serde_json::Value = std::fs::read(build.join("design.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(serde_json::Value::Null);
    let domain = domain.unwrap_or_else(|| wright_fab::fab::guess_domain(&build, &design));
    let fab = out.unwrap_or_else(|| build.clone()).join("fab");
    let t = std::time::Instant::now();
    let man = wright_fab::fab::run(&domain, &build, &design, &fab, &opt)?;
    let path = man.write(&fab)?;
    println!("{}", man.brief());
    println!("wrote {} ({:.0} s)", path.display(), t.elapsed().as_secs_f64());
    Ok(())
}
