//! A thin Messages API client: request and response are `serde_json::Value`
//! (there is no official Rust SDK; `docs/research/harness.md`, "Rust").
//!
//! Every call goes through one [`Transport`], so the harness cannot tell
//! which of these answered it:
//!
//! * [`HttpTransport`] with [`Provider::Anthropic`] -- `api.anthropic.com`,
//!   prompt caching and exact cost accounting; the default.
//! * [`HttpTransport`] with [`Provider::Foundry`] -- the same request body
//!   against Claude on Microsoft Foundry (Azure). Endpoint and headers are
//!   the Anthropic Python SDK's Foundry client
//!   (anthropics/anthropic-sdk-python @202db72,
//!   `src/anthropic/lib/foundry.py:150-175` for the
//!   `https://{resource}.services.ai.azure.com/anthropic/` base URL and the
//!   `ANTHROPIC_FOUNDRY_*` variables, `:283-290` for sending the key as both
//!   `x-api-key` and `api-key`).
//! * [`crate::cli::CliTransport`] -- the signed-in `claude` binary, so a run
//!   can bill a claude.ai subscription instead of API credits.
//! * [`crate::fake::ScriptedTransport`] -- offline tests.
//!
//! Retries: 429 (rate limit), 529 (overloaded) and the transient 5xx, with
//! exponential backoff that honours `retry-after`; the Anthropic SDKs retry
//! the same set (`max_retries` default 2, 408/409/429/5xx). Nothing else is
//! retried: a 400 is a bug in the request and resending it spends nothing
//! useful.

use crate::budget::{Budget, BudgetError};
use crate::price::{worst_case_usd, worst_case_with_fallbacks_usd, Usage};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use wright_core::Event;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// `fallbacks: "default"` re-runs a classifier-declined request on another
/// model server-side (`shared/model-migration.md`, "Migrating to Claude Opus
/// 5" -> New API features). Claude API only; not on Foundry.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// One Messages request: the JSON body plus any `anthropic-beta` values.
#[derive(Debug, Clone)]
pub struct Request {
    pub body: Value,
    pub betas: Vec<String>,
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum ApiError {
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
    #[error("claude cli: {0}")]
    Cli(String),
    #[error("not configured: {0}")]
    Config(String),
    #[error("scripted transport ran out of responses after {0} calls")]
    ScriptExhausted(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    Foundry,
    ClaudeCli,
    CodexCli,
    Fake,
}

impl Provider {
    /// Whether a call spends money the global ledger guards. The CLI bills
    /// a Claude or ChatGPT subscription; its calls still count against the run
    /// limit (priced as if they were API calls) so a runaway loop stops.
    pub fn bills_credits(self) -> bool {
        !matches!(self, Provider::ClaudeCli | Provider::CodexCli)
    }
    pub fn server_fallbacks(self) -> bool {
        matches!(self, Provider::Anthropic)
    }
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Foundry => "foundry",
            Provider::ClaudeCli => "claude-cli",
            Provider::CodexCli => "codex-cli",
            Provider::Fake => "fake",
        }
    }
}

pub trait Transport: Send + Sync {
    fn provider(&self) -> Provider;
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>>;
    /// The span recorder behind this transport, if it is a
    /// [`crate::trace::TracedTransport`] (lane 5: `trace.rs`).
    fn tracer(&self) -> Option<Arc<crate::trace::Tracer>> {
        None
    }
    /// Transport-held conversation state worth checkpointing (the CLI's
    /// persistent session ids, `crate::cli`); `Null` when there is none.
    fn export_sessions(&self) -> Value {
        Value::Null
    }
    /// Put back what [`Transport::export_sessions`] returned (`--resume`).
    fn import_sessions(&self, _sessions: &Value) {}
}

/// Statuses worth another try: rate limit, overload, transient server.
pub fn retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// Delay before retry `attempt` (0-based): `retry-after` when the server
/// sent one, else 1 s doubling to a 32 s cap, plus up to 250 ms of jitter so
/// parallel specialists do not retry in lockstep.
pub fn backoff(attempt: u32, retry_after_s: Option<f64>) -> Duration {
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() % 250)
        .unwrap_or(0) as u64;
    let base = match retry_after_s {
        Some(s) if s.is_finite() && s >= 0.0 => Duration::from_secs_f64(s.min(120.0)),
        _ => Duration::from_secs(1u64 << attempt.min(5)),
    };
    base + Duration::from_millis(jitter)
}

pub struct HttpTransport {
    http: reqwest::Client,
    provider: Provider,
    url: String,
    key: String,
    pub max_retries: u32,
}

impl HttpTransport {
    pub fn anthropic(key: String) -> Self {
        Self::new(Provider::Anthropic, API_URL.into(), key)
    }

    /// Claude on Microsoft Foundry. `base` is the resource's
    /// `.../anthropic/` URL; `/v1/messages` is appended.
    pub fn foundry(base: &str, key: String) -> Self {
        let base = base.trim_end_matches('/');
        Self::new(Provider::Foundry, format!("{base}/v1/messages"), key)
    }

    fn new(provider: Provider, url: String, key: String) -> Self {
        let http = reqwest::Client::builder()
            // Non-streaming calls; the SDKs' default is 10 minutes.
            .timeout(Duration::from_secs(600))
            .build()
            .expect("reqwest client with rustls");
        HttpTransport { http, provider, url, key, max_retries: 4 }
    }

    /// `ANTHROPIC_API_KEY` for the Claude API.
    pub fn anthropic_from_env() -> Result<Self, ApiError> {
        match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) if !k.trim().is_empty() => Ok(Self::anthropic(k)),
            _ => Err(ApiError::Config("ANTHROPIC_API_KEY is not set".into())),
        }
    }

    /// `ANTHROPIC_FOUNDRY_API_KEY` plus `ANTHROPIC_FOUNDRY_BASE_URL` or
    /// `ANTHROPIC_FOUNDRY_RESOURCE` (mutually exclusive, as in the SDK).
    pub fn foundry_from_env() -> Result<Self, ApiError> {
        let key = std::env::var("ANTHROPIC_FOUNDRY_API_KEY")
            .map_err(|_| ApiError::Config("ANTHROPIC_FOUNDRY_API_KEY is not set".into()))?;
        let base = std::env::var("ANTHROPIC_FOUNDRY_BASE_URL").ok();
        let resource = std::env::var("ANTHROPIC_FOUNDRY_RESOURCE").ok();
        let base = match (base, resource) {
            (Some(_), Some(_)) => {
                return Err(ApiError::Config(
                    "ANTHROPIC_FOUNDRY_BASE_URL and ANTHROPIC_FOUNDRY_RESOURCE are mutually exclusive".into(),
                ))
            }
            (Some(b), None) => b,
            (None, Some(r)) => format!("https://{r}.services.ai.azure.com/anthropic/"),
            (None, None) => {
                return Err(ApiError::Config(
                    "set ANTHROPIC_FOUNDRY_RESOURCE or ANTHROPIC_FOUNDRY_BASE_URL".into(),
                ))
            }
        };
        Ok(Self::foundry(&base, key))
    }

    async fn attempt(&self, req: &Request) -> Result<(u16, Option<f64>, String), reqwest::Error> {
        let mut rb = self
            .http
            .post(&self.url)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .header("x-api-key", &self.key);
        if self.provider == Provider::Foundry {
            rb = rb.header("api-key", &self.key);
        }
        if !req.betas.is_empty() {
            rb = rb.header("anthropic-beta", req.betas.join(","));
        }
        let resp = rb.json(&req.body).send().await?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok());
        Ok((status, retry_after, resp.text().await?))
    }
}

impl Transport for HttpTransport {
    fn provider(&self) -> Provider {
        self.provider
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            let mut attempt = 0;
            loop {
                let last = attempt >= self.max_retries;
                match self.attempt(req).await {
                    Ok((200, _, text)) => {
                        return serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("bad JSON: {e}")))
                    }
                    Ok((status, ra, text)) if retryable(status) && !last => {
                        let _ = text;
                        tokio::time::sleep(backoff(attempt, ra)).await;
                    }
                    Ok((status, _, body)) => return Err(ApiError::Http { status, body }),
                    Err(e) if (e.is_timeout() || e.is_connect()) && !last => {
                        tokio::time::sleep(backoff(attempt, None)).await;
                    }
                    Err(e) => return Err(ApiError::Network(e.to_string())),
                }
                attempt += 1;
            }
        })
    }
}

/// The transport `WRIGHT_TRANSPORT` names: `cli` (default: the signed-in
/// Claude Code subscription, Pat 10/24, so demos run Fable on it), `api`,
/// `foundry`. Never inferred from whichever credentials happen to be on
/// disk: a paid run must not quietly change who pays (the rule the Python
/// baseline's CLI backend follows, `engine/silkscreen/agents/claude_cli.py`).
///
/// With `WRIGHT_OSS_ENDPOINT` set, `gpt-oss-*` model ids go to Foundry
/// instead (`crate::oss`); every other role keeps this transport.
pub fn transport_from_env() -> Result<Arc<dyn Transport>, ApiError> {
    let primary: Arc<dyn Transport> = match std::env::var("WRIGHT_TRANSPORT").unwrap_or_default().as_str() {
        "api" | "anthropic" => Arc::new(HttpTransport::anthropic_from_env()?),
        "foundry" | "azure" => Arc::new(HttpTransport::foundry_from_env()?),
        "" | "cli" | "claude-cli" => Arc::new(crate::cli::CliTransport::from_env()),
        other => return Err(ApiError::Config(format!("WRIGHT_TRANSPORT={other}: expected api, foundry or cli"))),
    };
    Ok(crate::oss::wrap_from_env(primary))
}

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error(transparent)]
    Budget(#[from] BudgetError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("ledger: {0}")]
    Ledger(String),
}

/// Transport + budget + the run's event stream. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    pub transport: Arc<dyn Transport>,
    pub budget: Arc<Mutex<Budget>>,
    pub events: broadcast::Sender<Event>,
    pub run: String,
}

impl Client {
    pub fn new(transport: Arc<dyn Transport>, budget: Budget, events: broadcast::Sender<Event>, run: impl Into<String>) -> Self {
        Client { transport, budget: Arc::new(Mutex::new(budget)), events, run: run.into() }
    }

    pub fn spent(&self) -> f64 {
        self.budget.lock().unwrap().run_spent
    }

    /// One `POST /v1/messages`, refused up front if it could cross a limit,
    /// priced from the response's own `usage` and `model` afterwards.
    ///
    /// The worst case is reserved under the budget mutex before the call is
    /// sent (so concurrent calls on clones of this client cannot all fit the
    /// same room), priced at the dearest model the request can be answered
    /// by when `fallbacks` is sent, and settled or released afterwards; a
    /// response without a readable `usage` is booked at that worst case.
    pub async fn create(&self, body: Value) -> Result<Value, CallError> {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let provider = self.transport.provider();
        let global = provider.bills_credits();
        let fallback = provider.server_fallbacks()
            && (model.starts_with("claude-sonnet-5-5") || model.starts_with("claude-opus-5") || model.starts_with("claude-fable-5"))
            && std::env::var("WRIGHT_FALLBACKS").map(|v| v != "0").unwrap_or(true);
        let next = if fallback { worst_case_with_fallbacks_usd(&model, &body) } else { worst_case_usd(&model, &body) };
        let mut hold = Hold::new(self.budget.clone(), next, global)?;

        let mut req = Request { body, betas: vec![] };
        if fallback {
            req.body["fallbacks"] = json!("default");
            req.betas.push(FALLBACK_BETA.into());
        }
        // Any error below drops `hold`, which releases the reservation.
        let resp = match self.transport.send(&req).await {
            // A beta this key cannot use must not end the run: resend once
            // without it.
            Err(ApiError::Http { status: 400, body }) if fallback && body.contains("fallback") => {
                req.body.as_object_mut().unwrap().remove("fallbacks");
                req.betas.clear();
                self.transport.send(&req).await?
            }
            r => r?,
        };

        let usage = Usage::parse(&resp["usage"]);
        let priced_as = resp["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(&model).to_string();
        let (booked, run_spent, input, output, limit) = {
            let mut b = self.budget.lock().unwrap();
            let r = b.settle(hold.take(), &priced_as, usage.as_ref(), global);
            let booked = r.map_err(|e| CallError::Ledger(e.to_string()))?;
            (booked, b.run_spent, b.run_input_tokens, b.run_output_tokens, b.run_limit)
        };
        if usage.is_none() {
            let _ = self.events.send(Event::AgentMessage {
                run: self.run.clone(),
                agent: "budget".into(),
                text: format!(
                    "{priced_as} response had no readable usage ({}); booked its authorized worst case ${booked:.4}",
                    short(&resp["usage"])
                ),
            });
        } else if booked > next + 1e-12 {
            let _ = self.events.send(Event::AgentMessage {
                run: self.run.clone(),
                agent: "budget".into(),
                text: format!("{priced_as} cost ${booked:.4}, above the ${next:.4} authorized for {model}"),
            });
        }
        let _ = self.events.send(Event::Spend {
            run: self.run.clone(),
            input_tokens: input,
            output_tokens: output,
            usd: run_spent,
            budget_usd: limit,
        });
        Ok(resp)
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 80 {
        format!("{}...", &s[..s.floor_char_boundary(80)])
    } else {
        s
    }
}

/// A reservation on the shared budget, released on drop unless taken: an
/// error, a panic, or the caller dropping the future mid-request all give
/// the room back.
struct Hold {
    budget: Arc<Mutex<Budget>>,
    amount: f64,
    global: bool,
    live: bool,
}

impl Hold {
    fn new(budget: Arc<Mutex<Budget>>, next: f64, global: bool) -> Result<Self, BudgetError> {
        let amount = budget.lock().unwrap().reserve(next, global)?;
        Ok(Hold { budget, amount, global, live: true })
    }
    /// Hand the reservation to `Budget::settle`.
    fn take(&mut self) -> f64 {
        self.live = false;
        self.amount
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.live {
            if let Ok(mut b) = self.budget.lock() {
                b.release(self.amount, self.global);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy() {
        assert!(retryable(429) && retryable(529) && retryable(503));
        assert!(!retryable(400) && !retryable(401) && !retryable(404));
        assert!(backoff(0, None) >= Duration::from_secs(1));
        assert!(backoff(3, None) >= Duration::from_secs(8));
        assert!(backoff(9, None) < Duration::from_secs(33));
        let ra = backoff(0, Some(7.0));
        assert!(ra >= Duration::from_secs(7) && ra < Duration::from_millis(7250));
    }

    #[test]
    fn foundry_url_matches_the_sdk() {
        let t = HttpTransport::foundry("https://example-resource.services.ai.azure.com/anthropic/", "k".into());
        assert_eq!(t.url, "https://example-resource.services.ai.azure.com/anthropic/v1/messages");
        assert_eq!(t.provider(), Provider::Foundry);
        assert!(!Provider::Foundry.server_fallbacks());
        assert!(!Provider::ClaudeCli.bills_credits());
    }
}
