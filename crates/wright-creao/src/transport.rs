//! HTTP behind a trait so every client path is tested offline with
//! [`Fake`]; [`Reqwest`] is the one real implementation.

use crate::config::ApiKey;
use crate::error::CreaoError;
use std::future::Future;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
}

pub trait Transport: Send + Sync {
    /// Send `req` with `Authorization: Bearer <key>`.
    fn send(&self, req: Request, key: &ApiKey) -> impl Future<Output = Result<Response, CreaoError>> + Send;
}

/// reqwest with rustls. Its default redirect policy drops `Authorization`
/// when a redirect changes host, which is what the artifact download
/// (302 to a temporary storage URL) requires per docs.creao.ai/pro/api-trigger.
pub struct Reqwest {
    http: reqwest::Client,
}

impl Reqwest {
    pub fn new() -> Result<Self, CreaoError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent(concat!("wright-creao/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| CreaoError::Transport(e.to_string()))?;
        Ok(Reqwest { http })
    }
}

impl Transport for Reqwest {
    async fn send(&self, req: Request, key: &ApiKey) -> Result<Response, CreaoError> {
        let mut rb = match req.method {
            Method::Get => self.http.get(&req.url),
            Method::Post => self.http.post(&req.url),
        }
        .header(reqwest::header::AUTHORIZATION, key.bearer());
        if let Some(b) = &req.body {
            rb = rb.json(b);
        }
        // reqwest errors can carry the URL but never headers, so the key cannot leak here.
        let resp = rb.send().await.map_err(|e| CreaoError::Transport(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse().ok());
        let body = resp.bytes().await.map_err(|e| CreaoError::Transport(e.without_url().to_string()))?.to_vec();
        Ok(Response { status, body, retry_after })
    }
}

/// Scripted transport for tests: answers in order, records every request.
#[derive(Default)]
pub struct Fake {
    pub replies: Mutex<std::collections::VecDeque<Response>>,
    pub seen: Mutex<Vec<Request>>,
}

impl Fake {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn reply(self, status: u16, body: serde_json::Value) -> Self {
        self.replies.lock().unwrap().push_back(Response { status, body: body.to_string().into_bytes(), retry_after: None });
        self
    }
    pub fn reply_bytes(self, status: u16, body: &[u8]) -> Self {
        self.replies.lock().unwrap().push_back(Response { status, body: body.to_vec(), retry_after: None });
        self
    }
    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
}

impl Transport for Fake {
    async fn send(&self, req: Request, key: &ApiKey) -> Result<Response, CreaoError> {
        assert!(key.bearer().starts_with("Bearer "));
        self.seen.lock().unwrap().push(req);
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| CreaoError::Transport("fake transport: no scripted reply left".into()))
    }
}
