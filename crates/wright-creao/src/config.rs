//! Where the CREAO credentials come from, and how they are kept out of
//! logs, URLs and files.
//!
//! The key lives only in the repo's gitignored `.env` (or the process
//! environment). [`ApiKey`] has no `Display` and a redacting `Debug`, and
//! the only place its text is read is the `Authorization` header in
//! [`crate::transport::Reqwest`].

use crate::error::CreaoError;
use std::path::Path;

/// A CREAO API key. Never printed: `Debug` shows only which key family it is.
#[derive(Clone)]
pub struct ApiKey(String);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApiKey({:?}, <redacted>)", self.family())
    }
}

/// The two documented key families (docs.creao.ai/developer-api/api-paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFamily {
    /// `capi_`: app-scoped API Trigger key, runs exactly one agent at
    /// `agent.creao.ai/api/v1/apps/{appId}/runs`, body field `inputs`.
    AppTrigger,
    /// `cr_sk_`: CREAO CLI Account key, `developer.creao.ai/v1/*`, body
    /// fields `agent_id` + `input`.
    Account,
}

impl ApiKey {
    pub fn new(raw: impl Into<String>) -> Result<Self, CreaoError> {
        let raw = raw.into().trim().trim_matches(|c| c == '"' || c == '\'').to_string();
        let k = ApiKey(raw);
        if k.0.is_empty() || k.0.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(CreaoError::Config("CREAO_API_KEY is empty or malformed".into()));
        }
        if !k.0.starts_with("capi_") && !k.0.starts_with("cr_sk_") {
            return Err(CreaoError::Config(
                "CREAO_API_KEY is neither a capi_ (API Trigger) nor a cr_sk_ (Account) key".into(),
            ));
        }
        Ok(k)
    }
    pub fn family(&self) -> KeyFamily {
        if self.0.starts_with("cr_sk_") {
            KeyFamily::Account
        } else {
            KeyFamily::AppTrigger
        }
    }
    /// The bearer header value. Crate-private: only the transport reads it.
    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

/// Everything needed to talk to one CREAO agent.
#[derive(Debug, Clone)]
pub struct Config {
    pub key: ApiKey,
    /// The agent's id (shown on its page in agent.creao.ai). Not secret.
    pub agent_id: String,
    /// `https://agent.creao.ai` for capi_ keys, `https://developer.creao.ai` for cr_sk_.
    pub base_url: String,
}

pub const APP_TRIGGER_BASE: &str = "https://agent.creao.ai";
pub const ACCOUNT_BASE: &str = "https://developer.creao.ai";

impl Config {
    pub fn new(key: ApiKey, agent_id: impl Into<String>) -> Result<Self, CreaoError> {
        let agent_id = agent_id.into().trim().to_string();
        if !is_uuid(&agent_id) {
            return Err(CreaoError::Config("CREAO_APP_ID is not a UUID".into()));
        }
        let base_url = match key.family() {
            KeyFamily::AppTrigger => APP_TRIGGER_BASE,
            KeyFamily::Account => ACCOUNT_BASE,
        }
        .to_string();
        Ok(Config { key, agent_id, base_url })
    }

    /// Read `CREAO_API_KEY` and `CREAO_APP_ID` from the process environment,
    /// falling back to a dotenv-style file (the repo's `.env`). The process
    /// environment wins.
    pub fn load(dotenv: &Path) -> Result<Self, CreaoError> {
        let file = std::fs::read_to_string(dotenv).unwrap_or_default();
        let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| dotenv_get(&file, name));
        let key = get("CREAO_API_KEY").ok_or_else(|| CreaoError::Config(format!("CREAO_API_KEY not set in env or {}", dotenv.display())))?;
        let app = get("CREAO_APP_ID").ok_or_else(|| {
            CreaoError::Config(format!(
                "CREAO_APP_ID not set in env or {} (the agent id from its agent.creao.ai page)",
                dotenv.display()
            ))
        })?;
        Config::new(ApiKey::new(key)?, app)
    }
}

/// `NAME=value` lookup in dotenv text: skips comments, allows `export `,
/// strips matching quotes. Last assignment wins, as in a shell.
pub fn dotenv_get(text: &str, name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == name {
                let v = v.trim();
                let v = v
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                    .unwrap_or(v);
                found = Some(v.to_string());
            }
        }
    }
    found.filter(|v| !v.is_empty())
}

/// 8-4-4-4-12 hex. Ids go into URL paths, so nothing else is allowed in.
pub fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_key() {
        let k = ApiKey::new("capi_supersecretvalue123").unwrap();
        let shown = format!("{k:?}");
        assert!(!shown.contains("supersecret"), "{shown}");
        let c = Config::new(k, "00000000-0000-4000-8000-000000000000").unwrap();
        assert!(!format!("{c:?}").contains("supersecret"));
    }

    #[test]
    fn families_and_bases() {
        let a = Config::new(ApiKey::new("capi_x").unwrap(), "00000000-0000-4000-8000-000000000000").unwrap();
        assert_eq!(a.base_url, APP_TRIGGER_BASE);
        let b = Config::new(ApiKey::new("cr_sk_x").unwrap(), "00000000-0000-4000-8000-000000000000").unwrap();
        assert_eq!(b.base_url, ACCOUNT_BASE);
        assert!(ApiKey::new("sk-ant-x").is_err());
        assert!(ApiKey::new("capi_ x").is_err());
    }

    #[test]
    fn agent_id_must_be_a_uuid() {
        let k = ApiKey::new("capi_x").unwrap();
        assert!(Config::new(k.clone(), "../../v1/secrets").is_err());
        assert!(Config::new(k, "00000000-0000-4000-8000-00000000000g").is_err());
    }

    #[test]
    fn dotenv_parsing() {
        let t = "# c\nOTHER=1\nexport CREAO_APP_ID=\"abc\"\nCREAO_API_KEY='capi_q'\n";
        assert_eq!(dotenv_get(t, "CREAO_APP_ID").as_deref(), Some("abc"));
        assert_eq!(dotenv_get(t, "CREAO_API_KEY").as_deref(), Some("capi_q"));
        assert_eq!(dotenv_get(t, "MISSING"), None);
        assert_eq!(dotenv_get("CREAO_APP_ID=\n", "CREAO_APP_ID"), None);
    }
}
