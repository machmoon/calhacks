//! Microsoft Entra tokens from the signed-in Azure CLI, the way
//! azure-sdk-for-rust's `AzureCliCredential` gets them
//! (sdk/identity/azure_identity/src/azure_cli_credential.rs:
//! `az account get-access-token -o json --scope RESOURCE/.default`, reading
//! `accessToken` + `expires_on`), cached until 5 minutes before expiry so a
//! long run never holds an expired token. Used by the Cosmos runs database
//! (`runs_db::azure`) and the Foundry gpt-oss transport (`oss`).

use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum Token {
    /// A token given in the environment (it is never refreshed).
    Fixed(String),
    Cli { scope: String, cached: Arc<Mutex<Option<(String, i64)>>> },
}

impl Token {
    /// `fixed` when non-empty, else the Azure CLI for `scope`.
    pub fn new(fixed: Option<String>, scope: &str) -> Self {
        match fixed.filter(|t| !t.is_empty()) {
            Some(t) => Token::Fixed(t),
            None => Token::Cli { scope: scope.to_string(), cached: Default::default() },
        }
    }

    pub fn get(&self) -> anyhow::Result<String> {
        let (scope, cached) = match self {
            Token::Fixed(t) => return Ok(t.clone()),
            Token::Cli { scope, cached } => (scope, cached),
        };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let mut slot = cached.lock().map_err(|_| anyhow::anyhow!("Azure token cache poisoned"))?;
        if let Some((t, exp)) = slot.as_ref() {
            if *exp - 300 > now {
                return Ok(t.clone());
            }
        }
        let out = std::process::Command::new("az")
            .args(["account", "get-access-token", "-o", "json", "--scope", scope])
            .output()
            .map_err(|e| anyhow::anyhow!("Azure CLI not found ({e}); install it and run `az login`"))?;
        if !out.status.success() {
            anyhow::bail!("az account get-access-token failed (run `az login`): {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let (t, exp) = parse(&out.stdout)?;
        *slot = Some((t.clone(), exp));
        Ok(t)
    }
}

/// `accessToken` and `expires_on` (unix seconds, Azure CLI 2.54+) from
/// `az account get-access-token -o json`.
pub fn parse(stdout: &[u8]) -> anyhow::Result<(String, i64)> {
    let v: Value = serde_json::from_slice(stdout).map_err(|e| anyhow::anyhow!("az token JSON: {e}"))?;
    let t = v["accessToken"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| anyhow::anyhow!("az token has no accessToken"))?;
    let exp = v["expires_on"]
        .as_i64()
        .or_else(|| v["expires_on"].as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("az token has no expires_on (Azure CLI 2.54+ needed)"))?;
    Ok((t.to_string(), exp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_token_json() {
        let out = br#"{"accessToken":"abc","expiresOn":"2030-01-02 03:04:05.000000","expires_on":1893553445,"tokenType":"Bearer"}"#;
        assert_eq!(parse(out).unwrap(), ("abc".to_string(), 1_893_553_445));
        assert!(parse(br#"{"accessToken":"abc","expiresOn":"x"}"#).is_err());
        assert!(parse(br#"{"accessToken":"","expires_on":1}"#).is_err());
        assert_eq!(Token::new(Some("t".into()), "s").get().unwrap(), "t");
    }
}
