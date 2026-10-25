//! Wright's Claude harness: agents whose only actions are deterministic
//! engineering tools, over any `wright_core::Domain`.
//!
//! * [`client`] -- the Messages API over `serde_json::Value`, retries, and
//!   the [`Client`] that prices every response and refuses calls past the
//!   [`budget`]; transports for the Claude API, Claude on Microsoft
//!   Foundry, the signed-in `claude` CLI ([`cli`]) and a scripted fake
//!   ([`fake`]).
//! * [`agent`] -- one agent's append-only tool loop with malformed-input
//!   requery.
//! * [`tools`] -- [`DomainTools`], the object-safe view of a domain.
//! * [`harness`] -- orchestrator, specialists and hostile reviewer;
//!   [`harness::run`] designs one vehicle and streams `wright_core::Event`s.
//!
//! Choosing who pays: `WRIGHT_TRANSPORT=api` (default, `ANTHROPIC_API_KEY`),
//! `foundry` (`ANTHROPIC_FOUNDRY_API_KEY` + `ANTHROPIC_FOUNDRY_RESOURCE`),
//! or `cli` (the `claude` login). See `docs/sponsors.md`.

pub mod agent;
#[cfg(feature = "azure")]
pub mod azure_blob;
pub mod azure_cli;
/// Offline benchmark of the agent team: scripted scenarios in `bench/scenarios/*.json`.
pub mod bench;
pub mod budget;
/// Checkpoints after every orchestrator step, `--resume`, and a person's
/// pause / approve / stop gate (lane 10).
pub mod checkpoint;
pub mod cli;
/// Subscription-backed Astra proposals for the native CAD specialist.
pub mod codex;
pub mod client;
/// Spec elicitation + feasibility probe before the team starts (lane 7).
pub mod elicit;
pub mod fake;
pub mod harness;
/// Parallel specialists: RFC 6902 patches per fork, conflict detection, merge with check-regression fallback, design versions (lane 9).
pub mod merge;
pub mod oss;
/// Cross-run lessons memory: `lessons-<domain>.jsonl` beside the regressions, BM25-retrieved into the prompts.
pub mod memory;
/// The orchestrator's engineering plan: a task DAG with owners, checks and stale-propagation.
pub mod plan;
pub mod price;
pub mod prompts;
/// Hostile reviewer upgrade: attack taxonomy, evidence, rebuttal, metrics.
pub mod review;
/// Per-role model + effort table with env/flag overrides (cost + latency, 10/24 night).
pub mod roles;
/// Specialists see failing + nearly failing checks; `full_report` fetches the rest.
pub mod slim;
/// Runs database: prior efforts (request, spec, design, checks, failures) behind a `RunStore` trait; JSONL default, Azure Cosmos behind `azure`.
pub mod runs_db;
pub mod state;
/// Sourcing tools over any domain's catalog: `catalog_search`, `alternates`, `bom_diff`, id gate (lane 8).
pub mod sourcing;
/// Design-of-experiments `sweep` tool (grid / Latin hypercube + Pareto front).
pub mod sweep;
pub mod tools;
/// Tool-error taxonomy, repair budget, OTel GenAI spans (`<run>/trace.jsonl`).
pub mod trace;

pub use budget::Budget;
pub use client::{transport_from_env, Client, Transport};
pub use harness::{run, Models, RunConfig, RunSummary, StopReason};
pub use state::{DesignState, Regression};
pub use tools::{tools, DomainTools};

/// A broadcast channel big enough that the harness's own events do not push
/// a person's feedback out before the orchestrator reads it.
pub fn channel() -> (tokio::sync::broadcast::Sender<wright_core::Event>, tokio::sync::broadcast::Receiver<wright_core::Event>) {
    tokio::sync::broadcast::channel(4096)
}
