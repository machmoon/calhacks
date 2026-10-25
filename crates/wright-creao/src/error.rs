//! CREAO failures, keyed on the documented stable codes
//! (docs.creao.ai/developer-api/errors), never on English messages.

#[derive(Debug, thiserror::Error)]
pub enum CreaoError {
    #[error("config: {0}")]
    Config(String),
    /// 401 `AUTHENTICATION_REQUIRED`: missing, malformed, revoked or unknown key.
    #[error("CREAO rejected the API key (AUTHENTICATION_REQUIRED)")]
    Auth,
    /// 403 `API_KEY_SCOPE_RESTRICTED`: a capi_ key used outside its one agent.
    #[error("CREAO key is scoped to a different agent or route ({0})")]
    Scope(String),
    /// 402 `INSUFFICIENT_CREDITS`.
    #[error("CREAO account is out of credits (available {available:?}, required {required:?})")]
    Credits { available: Option<f64>, required: Option<f64> },
    /// 429 `RATE_LIMITED`; `Retry-After` seconds when sent.
    #[error("CREAO rate limited (retry after {0:?} s)")]
    RateLimited(Option<u64>),
    #[error("CREAO HTTP {status}: {code}")]
    Http { status: u16, code: String },
    #[error("transport: {0}")]
    Transport(String),
    #[error("decoding CREAO response: {0}")]
    Decode(String),
    /// Terminal `failed` / `cancelled` run.
    #[error("CREAO run {run} ended {status} ({code})")]
    RunFailed { run: String, status: String, code: String },
    #[error("CREAO run {0} still not terminal after the wait budget")]
    Timeout(String),
    /// Run input over the documented 256 KB serialized limit.
    #[error("run input is {0} bytes; CREAO accepts at most 256 KB")]
    InputTooLarge(usize),
    /// An artifact URL that is not on a CREAO origin: the key is never sent there.
    #[error("refusing to send the CREAO key to a non-CREAO URL: {0}")]
    ForeignUrl(String),
}

impl CreaoError {
    /// Map a non-2xx response to a typed error from its `{ "error": CODE }` envelope.
    pub fn from_status(status: u16, body: &[u8], retry_after: Option<u64>) -> Self {
        let v: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
        let code = v.get("error").and_then(|e| e.as_str().map(str::to_string).or_else(|| e.get("code").and_then(|c| c.as_str()).map(str::to_string)));
        let code = code.unwrap_or_else(|| "UNKNOWN".into());
        match (status, code.as_str()) {
            (401, _) => CreaoError::Auth,
            (402, _) | (_, "INSUFFICIENT_CREDITS") => CreaoError::Credits {
                available: v.get("available").and_then(|x| x.as_f64()),
                required: v.get("required").and_then(|x| x.as_f64()),
            },
            (403, "API_KEY_SCOPE_RESTRICTED") => CreaoError::Scope(code),
            (429, _) => CreaoError::RateLimited(retry_after),
            _ => CreaoError::Http { status, code },
        }
    }
}
