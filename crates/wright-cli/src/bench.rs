//! `wright bench [SCENARIO_DIR] [-o RESULTS.json]`: the offline agent-team
//! benchmark (`wright_agents::bench`). Scripted models, a toy domain by
//! default, no network and no spend; prints the table and writes the JSON.
//! `--shard i/n` runs one slice (`wright_core::shard`, after Playwright's
//! `--shard`) so containers can split the scenarios; `wright merge` joins
//! the slices (docs/aci.md).

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wright_agents::DomainTools;

fn repo_file(rel: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    std::fs::canonicalize(&repo).unwrap_or(repo)
}

/// A scenario's `domain` to its tools; `toy` is resolved by the bench itself.
pub(crate) fn resolve(name: &str) -> Option<Arc<dyn DomainTools>> {
    match name {
        #[cfg(feature = "air")]
        "aircraft" => Some(wright_agents::tools::<wright_air::Aircraft>()),
        "rocket" => Some(wright_agents::tools::<wright_rocket::Rocket>()),
        "balloon" => Some(wright_agents::tools::<wright_balloon::Balloon>()),
        "electronics" => Some(wright_agents::tools::<wright_electronics::Electronics>()),
        _ => None,
    }
}

pub fn bench_cmd(args: &[String]) -> anyhow::Result<()> {
    let (mut dir, mut out, mut shard) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = it.next().map(PathBuf::from),
            "--shard" => shard = Some(it.next().context("--shard wants i/n")?.parse::<wright_core::shard::Shard>()?),
            s if s.starts_with("--shard=") => shard = Some(s["--shard=".len()..].parse::<wright_core::shard::Shard>()?),
            other if other.starts_with('-') => anyhow::bail!("unknown flag {other}"),
            other => dir = Some(PathBuf::from(other)),
        }
    }
    let dir = dir.unwrap_or_else(|| repo_file("bench/scenarios"));
    let out = out.unwrap_or_else(|| match shard {
        Some(s) => repo_file("bench").join(format!("results-{}.json", s.tag())),
        None => repo_file("bench").join("results.json"),
    });
    let scenarios = wright_agents::bench::load_dir(&dir)?;
    anyhow::ensure!(!scenarios.is_empty(), "no *.json scenarios in {}", dir.display());
    let total = scenarios.len();
    let scenarios = match shard {
        Some(s) => {
            let r = s.range(total);
            println!("shard {s}: scenarios {}..{} of {total}", r.start + 1, r.end);
            s.select(scenarios)
        }
        None => scenarios,
    };
    let rt = tokio::runtime::Runtime::new()?;
    let outcomes = rt.block_on(wright_agents::bench::run_all(&scenarios, &resolve));
    print!("{}", wright_agents::bench::table(&outcomes));
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&wright_agents::bench::results_json(&outcomes))?).with_context(|| format!("writing {}", out.display()))?;
    println!("results: {}", out.display());
    if outcomes.iter().all(|o| o.passed) {
        Ok(())
    } else {
        std::process::exit(1)
    }
}
