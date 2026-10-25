//! gpt-oss on Microsoft Foundry beside Claude: route by model id.
//!
//! Roles pick models (`crate::roles`, e.g.
//! `WRIGHT_MODEL_SPECIALIST=gpt-oss-120b`); [`Split`] sends any `gpt-oss-*`
//! request to [`OssTransport`] and everything else to the primary transport
//! (the Claude CLI by default). If Foundry fails, the request goes to the
//! primary with [`Split::fallback`] as its model, so a run never stalls on
//! Azure. Acceptance is unchanged: whichever model proposes, the checks decide.
//!
//! [`OssTransport`] speaks OpenAI Chat Completions to the Foundry v1 endpoint
//! (`https://RESOURCE.openai.azure.com/openai/v1/chat/completions`) with an
//! Entra token from the Azure CLI (`crate::azure_cli`, scope
//! `https://cognitiveservices.azure.com/.default`), and translates the
//! harness's Messages-API body and response the way LiteLLM's adapter does
//! (BerriAI/litellm `litellm/llms/anthropic/pass_through/adapters/transformation.py`,
//! `translate_anthropic_messages_to_openai`: `tool_result` blocks become
//! `role: "tool"` messages placed before the user's remaining content,
//! `tool_use` blocks become `tool_calls` with JSON-string arguments;
//! `_translate_openai_finish_reason_to_anthropic`: stop -> end_turn,
//! length -> max_tokens, tool_calls -> tool_use). Images are replaced by a
//! text marker: gpt-oss reads text only.

use crate::azure_cli::Token;
use crate::client::{backoff, retryable, ApiError, BoxFut, Provider, Request, Transport};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const PREFIX: &str = "gpt-oss";
const SCOPE: &str = "https://cognitiveservices.azure.com/.default";

/// Is this model id served by Foundry gpt-oss?
pub fn is_oss(model: &str) -> bool {
    model.starts_with(PREFIX)
}

pub struct OssTransport {
    url: String,
    token: Token,
    http: reqwest::Client,
    max_retries: u32,
}

impl OssTransport {
    /// `base` is the resource endpoint, e.g. `https://NAME.openai.azure.com`.
    pub fn new(base: &str, token: Token) -> Self {
        let url = format!("{}/openai/v1/chat/completions", base.trim_end_matches('/'));
        OssTransport { url, token, http: reqwest::Client::new(), max_retries: 3 }
    }

    /// `WRIGHT_OSS_ENDPOINT` (required) and `WRIGHT_OSS_TOKEN` (optional;
    /// else the Azure CLI). `None` when no endpoint is set.
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("WRIGHT_OSS_ENDPOINT").ok().filter(|s| !s.is_empty())?;
        Some(Self::new(&base, Token::new(std::env::var("WRIGHT_OSS_TOKEN").ok(), SCOPE)))
    }

    async fn attempt(&self, body: &Value) -> Result<(u16, Option<f64>, String), ApiError> {
        let token = self.token.clone();
        let bearer = tokio::task::spawn_blocking(move || token.get())
            .await
            .map_err(|e| ApiError::Config(format!("Azure token task: {e}")))?
            .map_err(|e| ApiError::Config(format!("{e:#}")))?;
        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(bearer)
            .json(body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let ra = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok());
        Ok((status, ra, resp.text().await.map_err(|e| ApiError::Network(e.to_string()))?))
    }
}

impl Transport for OssTransport {
    fn provider(&self) -> Provider {
        Provider::Foundry
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            let body = to_openai(&req.body);
            let mut attempt = 0;
            loop {
                let last = attempt >= self.max_retries;
                match self.attempt(&body).await? {
                    (200, _, text) => {
                        let v: Value = serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("bad JSON: {e}")))?;
                        return from_openai(&v, req.body["model"].as_str().unwrap_or(PREFIX));
                    }
                    (status, ra, _) if retryable(status) && !last => tokio::time::sleep(backoff(attempt, ra)).await,
                    (status, _, body) => return Err(ApiError::Http { status, body }),
                }
                attempt += 1;
            }
        })
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|b| match b["type"].as_str() {
                Some("text") => b["text"].as_str().unwrap_or("").to_string(),
                Some("image") | Some("document") => "[image omitted: this model reads text only]".to_string(),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Messages-API request body -> Chat Completions request body.
pub fn to_openai(body: &Value) -> Value {
    let mut msgs = vec![];
    let system = text_of(&body["system"]);
    if !system.is_empty() {
        msgs.push(json!({"role": "system", "content": system}));
    }
    for m in body["messages"].as_array().into_iter().flatten() {
        let blocks: Vec<Value> = match &m["content"] {
            Value::String(s) => vec![json!({"type": "text", "text": s})],
            Value::Array(a) => a.clone(),
            _ => vec![],
        };
        if m["role"] == "assistant" {
            let text = text_of(&Value::Array(blocks.iter().filter(|b| b["type"] == "text").cloned().collect()));
            let calls: Vec<Value> = blocks
                .iter()
                .filter(|b| b["type"] == "tool_use")
                .map(|b| json!({"id": b["id"], "type": "function", "function": {"name": b["name"], "arguments": b["input"].to_string()}}))
                .collect();
            let mut a = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) }});
            if !calls.is_empty() {
                a["tool_calls"] = Value::Array(calls);
            }
            msgs.push(a);
        } else {
            for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                let mut c = text_of(&b["content"]);
                if b["is_error"] == true {
                    c = format!("ERROR: {c}");
                }
                msgs.push(json!({"role": "tool", "tool_call_id": b["tool_use_id"], "content": c}));
            }
            let rest = text_of(&Value::Array(blocks.iter().filter(|b| b["type"] != "tool_result").cloned().collect()));
            if !rest.is_empty() {
                msgs.push(json!({"role": "user", "content": rest}));
            }
        }
    }
    let tools: Vec<Value> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t.get("input_schema").is_some())
        .map(|t| json!({"type": "function", "function": {"name": t["name"], "description": t["description"], "parameters": t["input_schema"]}}))
        .collect();
    let effort = match body["output_config"]["effort"].as_str() {
        Some("low") => "low",
        Some("high" | "xhigh" | "max") => "high",
        _ => "medium",
    };
    let mut out = json!({
        "model": body["model"],
        "messages": msgs,
        "max_tokens": body["max_tokens"],
        "reasoning_effort": effort,
    });
    if !tools.is_empty() {
        out["tools"] = Value::Array(tools);
        out["tool_choice"] = json!("auto");
    }
    out
}

/// Chat Completions response -> Messages-API response, `model` as requested
/// (so the ledger prices it by `crate::price::rate`).
pub fn from_openai(v: &Value, model: &str) -> Result<Value, ApiError> {
    let choice = &v["choices"][0];
    if choice.is_null() {
        return Err(ApiError::Network(format!("gpt-oss response has no choices: {}", v.to_string().chars().take(300).collect::<String>())));
    }
    let msg = &choice["message"];
    let mut content = vec![];
    if let Some(t) = msg["content"].as_str().filter(|t| !t.trim().is_empty()) {
        content.push(json!({"type": "text", "text": t}));
    }
    for c in msg["tool_calls"].as_array().into_iter().flatten() {
        let args = c["function"]["arguments"].as_str().unwrap_or("{}");
        let input: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({"_unparsed_arguments": args}));
        // tool_use ids must match ^[a-zA-Z0-9_-]+$ if the conversation later goes to Claude
        let id: String = c["id"].as_str().unwrap_or("call").chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' { ch } else { '_' }).collect();
        content.push(json!({"type": "tool_use", "id": id, "name": c["function"]["name"], "input": input}));
    }
    let stop = match choice["finish_reason"].as_str() {
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ if content.iter().any(|b| b["type"] == "tool_use") => "tool_use",
        _ => "end_turn",
    };
    if content.is_empty() {
        content.push(json!({"type": "text", "text": ""}));
    }
    let u = &v["usage"];
    let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
    Ok(json!({
        "type": "message", "role": "assistant", "model": model, "content": content, "stop_reason": stop,
        "usage": {"input_tokens": u["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(cached), "cache_read_input_tokens": cached, "output_tokens": u["completion_tokens"].as_u64().unwrap_or(0)},
    }))
}

/// Primary transport for Claude models, [`OssTransport`] for `gpt-oss-*`.
pub struct Split {
    pub primary: Arc<dyn Transport>,
    pub oss: Arc<dyn Transport>,
    /// The Claude model a gpt-oss request falls back to when Foundry fails.
    pub fallback: String,
    warned: AtomicBool,
}

impl Split {
    pub fn new(primary: Arc<dyn Transport>, oss: Arc<dyn Transport>, fallback: &str) -> Self {
        Split { primary, oss, fallback: fallback.to_string(), warned: AtomicBool::new(false) }
    }
}

impl Transport for Split {
    fn provider(&self) -> Provider {
        self.primary.provider()
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            if !is_oss(req.body["model"].as_str().unwrap_or("")) {
                return self.primary.send(req).await;
            }
            match self.oss.send(req).await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if !self.warned.swap(true, Ordering::Relaxed) {
                        eprintln!("gpt-oss on Foundry failed ({e}); falling back to {} for these turns", self.fallback);
                    }
                    let mut body = req.body.clone();
                    body["model"] = json!(self.fallback);
                    self.primary.send(&Request { body, betas: req.betas.clone() }).await
                }
            }
        })
    }
    fn tracer(&self) -> Option<Arc<crate::trace::Tracer>> {
        self.primary.tracer()
    }
    fn export_sessions(&self) -> Value {
        self.primary.export_sessions()
    }
    fn import_sessions(&self, sessions: &Value) {
        self.primary.import_sessions(sessions)
    }
}

/// Wrap `primary` in a [`Split`] when `WRIGHT_OSS_ENDPOINT` is set.
/// `WRIGHT_OSS_FALLBACK` names the Claude model for failed gpt-oss turns
/// (default Opus 5.5, the specialist's standard model).
pub fn wrap_from_env(primary: Arc<dyn Transport>) -> Arc<dyn Transport> {
    match OssTransport::from_env() {
        Some(oss) => {
            let fallback = std::env::var("WRIGHT_OSS_FALLBACK").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "claude-opus-5-5".into());
            Arc::new(Split::new(primary, Arc::new(oss), &fallback))
        }
        None => primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn messages_body_becomes_chat_completions() {
        let body = json!({
            "model": "gpt-oss-120b", "max_tokens": 4000, "output_config": {"effort": "xhigh"},
            "system": [{"type": "text", "text": "You size wings."}],
            "tools": [{"name": "design", "description": "set params", "input_schema": {"type": "object"}}, {"type": "web_search_20250305", "name": "web_search"}],
            "messages": [
                {"role": "user", "content": "Size it."},
                {"role": "assistant", "content": [{"type": "text", "text": "Trying."}, {"type": "tool_use", "id": "toolu_1", "name": "design", "input": {"span": 1.2}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "ok"}, {"type": "image", "source": {}}], "is_error": true}, {"type": "text", "text": "Now check."}]},
            ],
        });
        let o = to_openai(&body);
        assert_eq!(o["reasoning_effort"], "high");
        assert_eq!(o["tools"].as_array().unwrap().len(), 1);
        assert_eq!(o["tools"][0]["function"]["parameters"], json!({"type": "object"}));
        let m = o["messages"].as_array().unwrap();
        assert_eq!(m.iter().map(|x| x["role"].as_str().unwrap()).collect::<Vec<_>>(), ["system", "user", "assistant", "tool", "user"]);
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{\"span\":1.2}");
        assert!(m[3]["content"].as_str().unwrap().starts_with("ERROR: ok\n[image omitted"));
        assert_eq!(m[4]["content"], "Now check.");
    }

    #[test]
    fn chat_completion_becomes_messages_response() {
        let v = json!({"choices": [{"finish_reason": "tool_calls", "message": {"content": null, "tool_calls": [{"id": "call.9", "type": "function", "function": {"name": "design", "arguments": "{\"span\":1.4}"}}]}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 20, "prompt_tokens_details": {"cached_tokens": 40}}});
        let r = from_openai(&v, "gpt-oss-120b").unwrap();
        assert_eq!(r["stop_reason"], "tool_use");
        assert_eq!(r["content"][0]["id"], "call_9");
        assert_eq!(r["content"][0]["input"]["span"], 1.4);
        assert_eq!((r["usage"]["input_tokens"].as_u64(), r["usage"]["cache_read_input_tokens"].as_u64()), (Some(60), Some(40)));
        let r = from_openai(&json!({"choices": [{"finish_reason": "length", "message": {"content": "partial"}}]}), "gpt-oss-120b").unwrap();
        assert_eq!((r["stop_reason"].as_str(), r["content"][0]["text"].as_str()), (Some("max_tokens"), Some("partial")));
        assert!(from_openai(&json!({"error": "x"}), "gpt-oss-120b").is_err());
    }

    struct Rec {
        seen: Mutex<Vec<String>>,
        fail: bool,
    }
    impl Transport for Rec {
        fn provider(&self) -> Provider {
            Provider::Fake
        }
        fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(req.body["model"].as_str().unwrap_or("").to_string());
                if self.fail {
                    Err(ApiError::Http { status: 403, body: "PermissionDenied".into() })
                } else {
                    Ok(json!({"ok": true}))
                }
            })
        }
    }

    #[tokio::test]
    async fn split_routes_by_model_and_falls_back() {
        let primary = Arc::new(Rec { seen: Mutex::new(vec![]), fail: false });
        let oss = Arc::new(Rec { seen: Mutex::new(vec![]), fail: true });
        let s = Split::new(primary.clone(), oss.clone(), "claude-opus-5-5");
        let req = |m: &str| Request { body: json!({"model": m}), betas: vec![] };
        s.send(&req("claude-opus-5-5")).await.unwrap();
        s.send(&req("gpt-oss-120b")).await.unwrap();
        assert_eq!(*oss.seen.lock().unwrap(), ["gpt-oss-120b"]);
        assert_eq!(*primary.seen.lock().unwrap(), ["claude-opus-5-5", "claude-opus-5-5"]);
        assert_eq!(s.provider(), Provider::Fake);
    }
}
