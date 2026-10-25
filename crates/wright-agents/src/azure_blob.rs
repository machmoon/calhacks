//! Private run artifacts using Azure Blob REST (Put Blob / Get Blob).
//! Follows learn.microsoft.com/rest/api/storageservices/put-blob and the
//! Azure CLI credential cache in runs_db. No storage account keys required.
use crate::runs_db::{azure::http_date, RunRecord};
use crate::azure_cli::Token;
use anyhow::{Context, Result};
use std::path::Path;

const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct BlobStore {
    base: String,
    query: Option<String>,
    token: Option<Token>,
}
impl std::fmt::Debug for BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobStore").field("container", &self.base).finish_non_exhaustive()
    }
}
impl BlobStore {
    pub fn from_env() -> Result<Option<Self>> {
        let url = std::env::var("WRIGHT_BLOB_URL").ok().filter(|v| !v.is_empty())
            .or_else(|| std::env::var("WRIGHT_BLOB_SAS_URL").ok().filter(|v| !v.is_empty()));
        url.filter(|v| v != "off").map(|v| Self::new(&v)).transpose()
    }
    /// HTTPS Azure container URL. A legacy SAS query is also supported, but
    /// never appears in Debug, errors, or persisted artifact URLs.
    pub fn new(url: &str) -> Result<Self> {
        let mut parsed = reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!("invalid Blob container URL"))?;
        anyhow::ensure!(parsed.scheme() == "https" && parsed.host_str().is_some_and(|h| h.ends_with(".blob.core.windows.net"))
            && parsed.username().is_empty() && parsed.password().is_none() && parsed.port().is_none()
            && parsed.fragment().is_none(), "Blob storage requires an HTTPS Azure Blob endpoint");
        let container = parsed.path().trim_matches('/');
        anyhow::ensure!(!container.is_empty() && !container.contains('/') && container.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'), "Blob URL must name one container");
        let query = parsed.query().filter(|s| !s.is_empty()).map(String::from);
        parsed.set_query(None);
        Ok(Self { base: parsed.as_str().trim_end_matches('/').into(),
            token: query.is_none().then(|| Token::Cli { scope: "https://storage.azure.com/.default".into(), cached: Default::default() }), query })
    }
    fn client() -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder().timeout(std::time::Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none()).build()?)
    }
    fn request(&self, client: &reqwest::Client, method: reqwest::Method, url: &str) -> Result<reqwest::RequestBuilder> {
        let mut target = reqwest::Url::parse(url)?;
        target.set_query(self.query.as_deref());
        let mut req = client.request(method, target).header("x-ms-version", "2021-08-06").header("x-ms-date", http_date());
        if let Some(token) = &self.token { req = req.bearer_auth(token.get()?); }
        Ok(req)
    }
    /// Upload only the run's explicit artifact manifest, preserving separate
    /// directories and run IDs without lossy filename sanitization.
    pub async fn upload(&self, rec: &RunRecord) -> Result<Vec<String>> {
        let client = Self::client()?;
        let mut urls = Vec::new();
        for name in &rec.artifacts {
            let path = Path::new(name);
            let metadata = std::fs::symlink_metadata(path).with_context(|| format!("missing artifact {name}"))?;
            anyhow::ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "artifact must be a regular file: {name}");
            anyhow::ensure!(metadata.len() <= MAX_BYTES, "artifact exceeds 128 MiB upload limit: {name}");
            let bytes = std::fs::read(path).with_context(|| format!("read artifact {name}"))?;
            anyhow::ensure!(bytes.len() as u64 <= MAX_BYTES, "artifact exceeds upload limit: {name}");
            let url = format!("{}/{}", self.base, object_key(&rec.id, name));
            let resp = self.request(&client, reqwest::Method::PUT, &url)?
                .header("x-ms-blob-type", "BlockBlob").header("Content-Type", "application/octet-stream")
                .body(bytes).send().await.map_err(|e| e.without_url())?;
            anyhow::ensure!(resp.status().is_success(), "Blob upload failed for {name} ({})", resp.status());
            urls.push(url);
        }
        Ok(urls)
    }
    /// Read back a private artifact from this container using the same login.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let parsed = reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!("invalid artifact URL"))?;
        anyhow::ensure!(parsed.as_str().starts_with(&format!("{}/", self.base)) && parsed.query().is_none() && parsed.fragment().is_none(), "artifact URL is outside the configured container");
        let mut resp = self.request(&Self::client()?, reqwest::Method::GET, url)?.send().await.map_err(|e| e.without_url())?;
        anyhow::ensure!(resp.status().is_success(), "Blob download failed ({})", resp.status());
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.without_url())? {
            anyhow::ensure!(bytes.len() + chunk.len() <= MAX_BYTES as usize, "download exceeds 128 MiB limit");
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
fn object_key(run: &str, path: &str) -> String {
    // Encoding the complete original path as one segment avoids basename
    // collisions and path traversal. Stable keys make retries idempotent.
    fn encode(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02x}")).collect()
    }
    format!("{}/{}", encode(run), encode(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_keys_and_secrets() {
        for bad in ["http://x.blob.core.windows.net/runs", "https://evil.example/runs", "https://x.blob.core.windows.net/", "https://x.blob.core.windows.net/a/b"] {
            assert!(BlobStore::new(bad).is_err());
        }
        let store = BlobStore::new("https://x.blob.core.windows.net/runs?sig=secret").unwrap();
        assert!(!format!("{store:?}").contains("secret"));
        assert_ne!(object_key("a/b", "pcb/top.step"), object_key("a_b", "pcb/top.step"));
        assert_ne!(object_key("a", "pcb/top.step"), object_key("a", "cad/top.step"));
    }
    #[test]
    fn discovers_pcb_and_cad_but_not_credentials() {
        let dir = std::env::temp_dir().join(format!("wright-blob-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["board.kicad_pcb", "circuit.kicad_sch", "copper.gbr", "holes.drl", "case.step", "view.glb", "assembly.FCStd", ".env", "token.json"] {
            std::fs::write(dir.join(name), "test").unwrap();
        }
        let paths = crate::runs_db::scan_artifacts(&dir);
        assert_eq!(paths.len(), 7);
        assert!(!paths.iter().any(|p| p.ends_with(".env") || p.ends_with("token.json")));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn authenticated_roundtrip_and_failure() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/runs", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (method, status, body) in [("PUT", "201 Created", ""), ("GET", "200 OK", "pcb-test"), ("GET", "403 Forbidden", "")] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut buf = Vec::new();
                let mut byte = [0];
                while !buf.ends_with(b"\r\n\r\n") { socket.read_exact(&mut byte).unwrap(); buf.push(byte[0]); }
                let header = String::from_utf8(buf).unwrap().to_ascii_lowercase();
                assert!(header.starts_with(&method.to_ascii_lowercase()));
                assert!(header.contains("authorization: bearer test-token"));
                if method == "PUT" {
                    assert!(header.contains("x-ms-blob-type: blockblob"));
                    let mut body = [0; 8]; socket.read_exact(&mut body).unwrap(); assert_eq!(&body, b"pcb-test");
                }
                write!(socket, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let store = BlobStore { base, query: None, token: Some(Token::Fixed("test-token".into())) };
        let path = std::env::temp_dir().join(format!("wright-blob-{}.kicad_pcb", std::process::id()));
        std::fs::write(&path, b"pcb-test").unwrap();
        let mut rec = crate::runs_db::record_from_state(&serde_json::json!({"domain":"electronics", "spec":{}, "design":{}}), "test", None).unwrap();
        rec.artifacts = vec![path.display().to_string()];
        let urls = store.upload(&rec).await.unwrap();
        assert_eq!(store.download(&urls[0]).await.unwrap(), b"pcb-test");
        assert!(store.download(&urls[0]).await.unwrap_err().to_string().contains("403"));
        assert!(store.download("https://evil.example/file").await.is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(store.upload(&rec).await.unwrap_err().to_string().contains("missing artifact"));
        server.join().unwrap();
    }
}
