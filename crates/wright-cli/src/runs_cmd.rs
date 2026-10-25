//! `wright runs import [DIR...] [--data DIR]` and `wright runs search TEXT
//! [--domain D] [--spec SPEC.json] [-n N] [--json]`: the runs database of
//! prior efforts (`wright_agents::runs_db`; store chosen by
//! `WRIGHT_RUNS_URI`, default `data/runs-db.jsonl`).

use anyhow::Context;
use std::path::{Path, PathBuf};
use wright_agents::runs_db::{self, JsonlStore, Query, RunStore};

pub fn runs_cmd(args: &[String], data_dir: &Path) -> anyhow::Result<()> {
    if args.first().map(String::as_str) == Some("sync") { return crate::run_archive::sync(&args[1..], data_dir); }
    let store = runs_db::from_env(data_dir)?.context("WRIGHT_RUNS_URI=off: the runs database is disabled")?;
    match args.first().map(String::as_str) {
        Some("import") => import(&args[1..], data_dir, store.as_ref()),
        Some("search") => search(&args[1..], store.as_ref()),
        _ => {
            eprintln!("usage:\n  wright runs import [DIR...] [--data DIR]   # default: demo out runs demo-prev + the data dir's lessons/regressions\n  wright runs search \"TEXT\" [--domain D] [--spec SPEC.json] [-n N] [--json]");
            std::process::exit(2)
        }
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// Positional args, skipping flags and their values.
fn positionals(args: &[String], with_value: &[&str]) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if with_value.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with("--") {
            out.push(a.clone());
        }
    }
    out
}

fn import(args: &[String], data_dir: &Path, store: &dyn RunStore) -> anyhow::Result<()> {
    let data = flag(args, "--data").map(PathBuf::from).unwrap_or_else(|| data_dir.to_path_buf());
    let repo = data.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut roots: Vec<PathBuf> = positionals(args, &["--data"]).into_iter().map(PathBuf::from).collect();
    if roots.is_empty() {
        roots = ["demo", "out", "runs", "demo-prev"].iter().map(|d| repo.join(d)).filter(|d| d.is_dir()).collect();
    }
    let mut recs = vec![];
    for r in &roots {
        let found = runs_db::import_tree(r);
        println!("{}: {} run(s)", r.display(), found.len());
        recs.extend(found);
    }
    let extra = runs_db::import_data_dir(&data)?;
    println!("{}: {} lesson/regression record(s)", data.display(), extra.len());
    recs.extend(extra);
    let n = store.log_batch(&recs)?;
    // the local file is append-only; an import is when it is compacted
    let kept = match store.describe() {
        p if Path::new(&p).is_file() => JsonlStore::new(p).compact()?,
        _ => store.list_runs(None)?.len(),
    };
    println!("wrote {n} record(s) to {} ({kept} distinct)", store.describe());
    for r in recs.iter().filter(|r| r.kind == runs_db::RecordKind::Run) {
        let verdict = r.verdict();
        println!("  {:<28} {:<9} {:<11} {} artifacts  {}", r.id, r.domain, verdict, r.artifacts.len(), r.source);
    }
    Ok(())
}

fn search(args: &[String], store: &dyn RunStore) -> anyhow::Result<()> {
    let text = positionals(args, &["--domain", "--spec", "-n"]).join(" ");
    let spec = match flag(args, "--spec") {
        Some(p) => Some(serde_json::from_str(&std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?)?),
        None => None,
    };
    let n: usize = flag(args, "-n").and_then(|v| v.parse().ok()).unwrap_or(5);
    let q = Query { domain: flag(args, "--domain").map(String::from), text, spec };
    let hits = store.search_runs(&q, n)?;
    if args.iter().any(|a| a == "--json") {
        let v: Vec<_> = hits.iter().map(|h| serde_json::json!({"score": h.score, "exact_spec": h.exact_spec, "record": h.record})).collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if hits.is_empty() {
        println!("no prior effort matches (store {})", store.describe());
    }
    for h in hits {
        let r = &h.record;
        let verdict = r.verdict();
        let exact = if h.exact_spec { " exact-spec" } else { "" };
        println!("{:>6.2}{exact}  {:?} {} [{}] {}  {}", h.score, r.kind, r.id, r.domain, verdict, r.request.chars().take(110).collect::<String>());
        for f in r.failures.iter().take(2) {
            println!("        - {}", f.chars().take(140).collect::<String>());
        }
    }
    Ok(())
}
