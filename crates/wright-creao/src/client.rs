//! Start, poll and collect a CREAO agent run, on either documented route
//! family (docs.creao.ai/developer-api/api-paths):
//!
//! | | app-scoped API Trigger (`capi_`) | CREAO CLI (`cr_sk_`) |
//! |---|---|---|
//! | start | `POST /api/v1/apps/{appId}/runs` `{inputs}` | `POST /v1/runs` `{agent_id, input}` |
//! | poll | `GET /api/v1/apps/{appId}/runs/{id}` | `GET /v1/runs/{run_id}` |
//! | run id field | `id` | `run_id` |
//! | failure code | `errorCode` | `error.code` |
//!
//! Terminal statuses are `completed`, `failed`, `cancelled` on both.

use crate::config::{is_uuid, Config, KeyFamily};
use crate::error::CreaoError;
use crate::transport::{Method, Request, Transport};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Documented limit on serialized run input (create-async-run: "256 KB or smaller").
pub const MAX_INPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunArtifact {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "type")]
    pub mime: String,
    /// Stable CREAO URL; needs the key on every fetch, 302s to storage.
    pub url: String,
}

/// One run, normalized across both route families.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub id: String,
    pub status: String,
    pub text: Option<String>,
    pub artifacts: Vec<RunArtifact>,
    pub error_code: Option<String>,
    pub credits: Option<f64>,
    pub raw: serde_json::Value,
}

impl Run {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }

    pub fn from_json(v: serde_json::Value) -> Result<Self, CreaoError> {
        let s = |p: &str| v.pointer(p).and_then(|x| x.as_str()).map(str::to_string);
        let id = s("/id").or_else(|| s("/run_id")).ok_or_else(|| CreaoError::Decode("run has no id/run_id".into()))?;
        let status = s("/status").ok_or_else(|| CreaoError::Decode("run has no status".into()))?;
        if !is_uuid(&id) {
            return Err(CreaoError::Decode("run id is not a UUID".into()));
        }
        // result is JSON; the app path documents result.text, the CLI path
        // result.text in its examples. A bare string result is accepted too.
        let text = s("/result/text").or_else(|| v.get("result").and_then(|r| r.as_str()).map(str::to_string));
        let artifacts = match v.pointer("/result/artifacts") {
            Some(a) => serde_json::from_value(a.clone()).map_err(|e| CreaoError::Decode(format!("artifacts: {e}")))?,
            None => vec![],
        };
        let error_code = s("/errorCode").or_else(|| s("/error/code"));
        let credits = v.pointer("/usage/credits").and_then(|x| x.as_f64());
        Ok(Run { id, status, text, artifacts, error_code, credits, raw: v })
    }
}

/// How long to keep polling. A Build Desk run browses vendor pages, so minutes.
#[derive(Debug, Clone, Copy)]
pub struct Poll {
    pub first: Duration,
    pub max_interval: Duration,
    pub budget: Duration,
}

impl Default for Poll {
    fn default() -> Self {
        Poll { first: Duration::from_secs(3), max_interval: Duration::from_secs(20), budget: Duration::from_secs(20 * 60) }
    }
}

pub struct Client<T: Transport> {
    pub cfg: Config,
    pub transport: T,
}

impl<T: Transport> Client<T> {
    pub fn new(cfg: Config, transport: T) -> Self {
        Client { cfg, transport }
    }

    fn runs_url(&self) -> String {
        match self.cfg.key.family() {
            KeyFamily::AppTrigger => format!("{}/api/v1/apps/{}/runs", self.cfg.base_url, self.cfg.agent_id),
            KeyFamily::Account => format!("{}/v1/runs", self.cfg.base_url),
        }
    }

    async fn call(&self, req: Request) -> Result<serde_json::Value, CreaoError> {
        let r = self.transport.send(req, &self.cfg.key).await?;
        if !(200..300).contains(&r.status) {
            return Err(CreaoError::from_status(r.status, &r.body, r.retry_after));
        }
        serde_json::from_slice(&r.body).map_err(|e| CreaoError::Decode(e.to_string()))
    }

    /// Start a run with the agent's structured inputs. Returns the pending run.
    pub async fn start(&self, inputs: &serde_json::Value) -> Result<Run, CreaoError> {
        let size = serde_json::to_vec(inputs).map(|b| b.len()).unwrap_or(usize::MAX);
        if size > MAX_INPUT_BYTES {
            return Err(CreaoError::InputTooLarge(size));
        }
        let body = match self.cfg.key.family() {
            KeyFamily::AppTrigger => serde_json::json!({ "inputs": inputs }),
            KeyFamily::Account => serde_json::json!({ "agent_id": self.cfg.agent_id, "input": inputs }),
        };
        Run::from_json(self.call(Request { method: Method::Post, url: self.runs_url(), body: Some(body) }).await?)
    }

    pub async fn get(&self, run_id: &str) -> Result<Run, CreaoError> {
        if !is_uuid(run_id) {
            return Err(CreaoError::Decode("run id is not a UUID".into()));
        }
        let url = format!("{}/{}", self.runs_url(), run_id);
        Run::from_json(self.call(Request { method: Method::Get, url, body: None }).await?)
    }

    /// Poll until terminal. `completed` returns the run; `failed`/`cancelled`
    /// is an error carrying the run's code. 429 waits `Retry-After`.
    pub async fn wait(&self, run_id: &str, poll: Poll) -> Result<Run, CreaoError> {
        let start = tokio::time::Instant::now();
        let mut every = poll.first;
        loop {
            match self.get(run_id).await {
                Ok(run) if run.is_terminal() => {
                    return if run.status == "completed" {
                        Ok(run)
                    } else {
                        Err(CreaoError::RunFailed {
                            run: run.id.clone(),
                            status: run.status.clone(),
                            code: run.error_code.clone().unwrap_or_else(|| "UNKNOWN".into()),
                        })
                    };
                }
                Ok(_) => {}
                Err(CreaoError::RateLimited(after)) => every = every.max(Duration::from_secs(after.unwrap_or(30))),
                Err(e) => return Err(e),
            }
            if start.elapsed() + every > poll.budget {
                return Err(CreaoError::Timeout(run_id.to_string()));
            }
            tokio::time::sleep(every).await;
            every = (every * 3 / 2).min(poll.max_interval);
        }
    }

    /// Fetch a run artifact's bytes. Only CREAO's own origin ever sees the key.
    pub async fn download(&self, a: &RunArtifact) -> Result<Vec<u8>, CreaoError> {
        let ok_origin = format!("{}/", self.cfg.base_url);
        if !a.url.starts_with(&ok_origin) {
            return Err(CreaoError::ForeignUrl(a.url.split('?').next().unwrap_or("").to_string()));
        }
        let r = self.transport.send(Request { method: Method::Get, url: a.url.clone(), body: None }, &self.cfg.key).await?;
        if !(200..300).contains(&r.status) {
            return Err(CreaoError::from_status(r.status, &r.body, r.retry_after));
        }
        Ok(r.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiKey;
    use crate::transport::Fake;
    use serde_json::json;

    const APP: &str = "00000000-0000-4000-8000-000000000000";
    const RUN: &str = "11111111-1111-4111-8111-111111111111";

    fn client(key: &str, fake: Fake) -> Client<Fake> {
        Client::new(Config::new(ApiKey::new(key).unwrap(), APP).unwrap(), fake)
    }

    #[tokio::test]
    async fn app_trigger_start_uses_inputs_and_app_path() {
        let c = client("capi_k", Fake::new().reply(202, json!({"id": RUN, "status": "pending", "appId": APP})));
        let run = c.start(&json!({"design": "x"})).await.unwrap();
        assert_eq!(run.id, RUN);
        let req = &c.transport.requests()[0];
        assert_eq!(req.url, format!("https://agent.creao.ai/api/v1/apps/{APP}/runs"));
        assert_eq!(req.body.as_ref().unwrap(), &json!({"inputs": {"design": "x"}}));
        assert!(!req.url.contains("capi_"), "key must never be in the URL");
    }

    #[tokio::test]
    async fn account_start_uses_agent_id_and_input() {
        let c = client("cr_sk_k", Fake::new().reply(202, json!({"run_id": RUN, "status": "pending"})));
        c.start(&json!({"a": 1})).await.unwrap();
        let req = &c.transport.requests()[0];
        assert_eq!(req.url, "https://developer.creao.ai/v1/runs");
        assert_eq!(req.body.as_ref().unwrap(), &json!({"agent_id": APP, "input": {"a": 1}}));
    }

    #[tokio::test(start_paused = true)]
    async fn wait_polls_to_completion_and_reads_artifacts() {
        let art = json!({"id": "workspaces/u/t/files/build.html", "title": "Build page", "type": "text/html",
            "url": format!("https://agent.creao.ai/api/v1/runs/{RUN}/artifacts/download?artifactId=x")});
        let fake = Fake::new()
            .reply(200, json!({"id": RUN, "status": "pending"}))
            .reply(200, json!({"id": RUN, "status": "running"}))
            .reply(200, json!({"id": RUN, "status": "completed", "result": {"text": "done", "artifacts": [art]}}));
        let c = client("capi_k", fake);
        let run = c.wait(RUN, Poll::default()).await.unwrap();
        assert_eq!(run.text.as_deref(), Some("done"));
        assert_eq!(run.artifacts[0].title, "Build page");
        assert_eq!(c.transport.requests().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_run_is_an_error_with_its_code() {
        let c = client("capi_k", Fake::new().reply(200, json!({"id": RUN, "status": "failed", "errorCode": "EXECUTION_FAILED"})));
        match c.wait(RUN, Poll::default()).await {
            Err(CreaoError::RunFailed { code, .. }) => assert_eq!(code, "EXECUTION_FAILED"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn wait_gives_up_after_budget() {
        let mut fake = Fake::new();
        for _ in 0..100 {
            fake = fake.reply(200, json!({"id": RUN, "status": "running"}));
        }
        let c = client("capi_k", fake);
        let poll = Poll { first: Duration::from_secs(5), max_interval: Duration::from_secs(10), budget: Duration::from_secs(60) };
        assert!(matches!(c.wait(RUN, poll).await, Err(CreaoError::Timeout(_))));
    }

    #[tokio::test]
    async fn error_codes_are_typed() {
        let c = client("capi_k", Fake::new().reply(401, json!({"error": "AUTHENTICATION_REQUIRED"})));
        assert!(matches!(c.start(&json!({})).await, Err(CreaoError::Auth)));
        let c = client("capi_k", Fake::new().reply(403, json!({"error": "API_KEY_SCOPE_RESTRICTED"})));
        assert!(matches!(c.start(&json!({})).await, Err(CreaoError::Scope(_))));
        let c = client("capi_k", Fake::new().reply(402, json!({"error": "INSUFFICIENT_CREDITS", "available": 0, "required": 10})));
        assert!(matches!(c.start(&json!({})).await, Err(CreaoError::Credits { required: Some(r), .. }) if r == 10.0));
    }

    #[tokio::test]
    async fn oversized_input_never_leaves() {
        let c = client("capi_k", Fake::new());
        let big = json!({"x": "a".repeat(MAX_INPUT_BYTES)});
        assert!(matches!(c.start(&big).await, Err(CreaoError::InputTooLarge(_))));
        assert!(c.transport.requests().is_empty());
    }

    #[tokio::test]
    async fn key_is_never_sent_off_creao() {
        let c = client("capi_k", Fake::new());
        let a = RunArtifact { id: "x".into(), title: String::new(), mime: String::new(), url: "https://agent.creao.ai.evil.com/x".into() };
        assert!(matches!(c.download(&a).await, Err(CreaoError::ForeignUrl(_))));
        assert!(c.transport.requests().is_empty());
    }

    #[tokio::test]
    async fn run_ids_are_validated_before_use_in_paths() {
        let c = client("capi_k", Fake::new());
        assert!(c.get("../secrets").await.is_err());
        assert!(c.transport.requests().is_empty());
    }
}
