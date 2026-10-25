//! `wright trace RUN_DIR [--json] [--tool NAME]`: what a run's agents did,
//! from `RUN_DIR/trace.jsonl` (OTLP/JSON spans written by
//! `wright_agents::trace`): per model calls, p50/p95 latency, USD; per tool
//! calls, p50/p95 latency, error rate and error kinds. `--json` prints the
//! same numbers as one JSON object; `--tool NAME` lists that tool's spans.

use anyhow::{bail, Context};
use std::path::PathBuf;
use wright_agents::trace::{percentile, read_spans, summarize};

pub fn trace_cmd(args: &[String]) -> anyhow::Result<()> {
    let Some(dir) = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from) else {
        bail!("usage: wright trace RUN_DIR [--json] [--tool NAME]")
    };
    let path = if dir.is_dir() { dir.join("trace.jsonl") } else { dir.clone() };
    let spans = read_spans(&path).with_context(|| format!("reading {}", path.display()))?;
    if spans.is_empty() {
        bail!("{} holds no spans (was the run started with -o DIR on a build that traces?)", path.display());
    }
    if let Some(i) = args.iter().position(|a| a == "--tool") {
        let name = args.get(i + 1).context("--tool needs a tool name")?;
        for s in spans.iter().filter(|s| s.str("gen_ai.tool.name") == Some(name.as_str())) {
            println!(
                "{:>8.1} ms  {:<18} {:<10} {}",
                s.duration().as_secs_f64() * 1e3,
                s.str("gen_ai.agent.name").unwrap_or(""),
                s.str("error.type").unwrap_or("ok"),
                s.status_message
            );
        }
        return Ok(());
    }
    let sum = summarize(&spans);
    if args.iter().any(|a| a == "--json") {
        let tools: serde_json::Map<String, serde_json::Value> = sum
            .tools
            .iter()
            .map(|(k, t)| {
                (
                    k.clone(),
                    serde_json::json!({
                        "calls": t.calls, "errors": t.errors, "by_error": t.by_error,
                        "p50_ms": percentile(&t.latencies_ms, 50.0), "p95_ms": percentile(&t.latencies_ms, 95.0),
                    }),
                )
            })
            .collect();
        let models: serde_json::Map<String, serde_json::Value> = sum
            .models
            .iter()
            .map(|(k, m)| {
                (
                    k.clone(),
                    serde_json::json!({
                        "calls": m.calls, "errors": m.errors, "input_tokens": m.input_tokens, "output_tokens": m.output_tokens,
                        "cache_read_tokens": m.cache_read_tokens, "cost_usd": m.cost_usd,
                        "p50_ms": percentile(&m.latencies_ms, 50.0), "p95_ms": percentile(&m.latencies_ms, 95.0),
                    }),
                )
            })
            .collect();
        println!("{}", serde_json::json!({"spans": sum.spans, "models": models, "tools": tools}));
    } else {
        print!("{sum}");
    }
    Ok(())
}
