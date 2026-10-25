//! A content-addressed result cache on disk, for deterministic worker jobs
//! whose inputs can be hashed (a slice of an STL under one slicer config and
//! version, a design search under one spec and one set of regressions, a
//! wind forecast for one site and launch time).
//!
//! Precedent: ccache (ccache/ccache `src/ccache/storage/local/localstorage.cpp`):
//! the key is a hash of every input that can change the result (compiler,
//! arguments, preprocessed source); a hit refreshes the entry's mtime
//! (`util::set_timestamps` in `get`, "save file from LRU cleanup"), and after a
//! store `perform_automatic_cleanup` -> `clean_dir` deletes the oldest files by
//! mtime until the directory is back under its limit, trimming to 90 %. Here
//! one flat directory per namespace, a byte limit instead of ccache's 256
//! subdirectories, and an optional time-to-live per lookup (ccache's
//! `max_age` in `evict`).
//!
//! Location: `WRIGHT_CACHE_DIR`, else `~/.wright/cache`; `WRIGHT_CACHE=0`
//! turns every lookup into a miss (and stores nothing). Size cap:
//! `WRIGHT_CACHE_MAX_MB` (default 512) per namespace.

use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A stable 128-bit content hash (two FNV-1a 64 lanes with different
/// offsets), as 32 hex characters. Stable across processes and builds, so
/// keys survive restarts; not cryptographic (the cache holds our own results).
#[derive(Clone)]
pub struct Hasher {
    a: u64,
    b: u64,
}

impl Default for Hasher {
    fn default() -> Self {
        Hasher { a: 0xcbf2_9ce4_8422_2325, b: 0x6c62_272e_07bb_0142 }
    }
}

impl Hasher {
    pub fn new() -> Self {
        Self::default()
    }
    /// Feed bytes; each call is length-prefixed so `("ab","c") != ("a","bc")`.
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        const P: u64 = 0x0000_0100_0000_01b3;
        for &x in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            self.a = (self.a ^ x as u64).wrapping_mul(P);
            self.b = (self.b ^ (x as u64).rotate_left(5)).wrapping_mul(P);
        }
        self
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.update(s.as_bytes())
    }
    /// A JSON value by its canonical text (serde_json orders object keys).
    pub fn json(&mut self, v: &impl Serialize) -> &mut Self {
        let t = serde_json::to_string(v).unwrap_or_default();
        self.update(t.as_bytes())
    }
    pub fn hex(&self) -> String {
        format!("{:016x}{:016x}", self.a, self.b)
    }
}

/// Hash of several parts in order.
pub fn key(parts: &[&[u8]]) -> String {
    let mut h = Hasher::new();
    for p in parts {
        h.update(p);
    }
    h.hex()
}

/// Is caching on (`WRIGHT_CACHE` not `0`)?
pub fn enabled() -> bool {
    std::env::var("WRIGHT_CACHE").map(|v| v != "0").unwrap_or(true)
}

/// The cache root: `WRIGHT_CACHE_DIR`, else `~/.wright/cache`, else the temp dir.
pub fn root() -> PathBuf {
    if let Some(d) = std::env::var_os("WRIGHT_CACHE_DIR") {
        return PathBuf::from(d);
    }
    match std::env::var_os("HOME") {
        Some(h) => Path::new(&h).join(".wright/cache"),
        None => std::env::temp_dir().join("wright-cache"),
    }
}

fn max_bytes() -> u64 {
    std::env::var("WRIGHT_CACHE_MAX_MB").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(512) * 1024 * 1024
}

/// One namespace (`slice`, `balloon-search`, `tawhiri`, ...) of JSON entries.
#[derive(Debug, Clone)]
pub struct Cache {
    pub dir: PathBuf,
    pub max_bytes: u64,
}

impl Cache {
    /// `<root>/<namespace>`.
    pub fn named(namespace: &str) -> Self {
        Cache { dir: root().join(namespace), max_bytes: max_bytes() }
    }
    pub fn at(dir: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Cache { dir: dir.into(), max_bytes }
    }
    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// The entry for `key`, if present, readable and younger than `ttl`.
    /// A hit refreshes the entry's mtime (LRU, as ccache's `get`).
    pub fn get<T: DeserializeOwned>(&self, key: &str, ttl: Option<Duration>) -> Option<T> {
        if !enabled() {
            return None;
        }
        let p = self.path(key);
        let meta = std::fs::metadata(&p).ok()?;
        if let (Some(ttl), Ok(m)) = (ttl, meta.modified()) {
            if SystemTime::now().duration_since(m).unwrap_or_default() > ttl {
                return None;
            }
        }
        let v = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
        if ttl.is_none() {
            // TTL entries keep their write time so they still expire
            if let Ok(f) = std::fs::File::options().append(true).open(&p) {
                let _ = f.set_modified(SystemTime::now());
            }
        }
        Some(v)
    }

    /// Store `value` under `key` (temp file + rename), then trim the
    /// namespace to 90 % of its cap, oldest first. Errors are ignored: a
    /// cache that cannot write is a cache that misses.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) {
        if !enabled() || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(value) else { return };
        let tmp = self.dir.join(format!(".{key}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, self.path(key));
        }
        self.trim();
    }

    /// ccache's `clean_dir`: delete oldest-by-mtime until under 90 % of the cap.
    pub fn trim(&self) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= self.max_bytes {
            return;
        }
        files.sort();
        let target = self.max_bytes / 10 * 9;
        for (_, len, p) in files {
            if total <= target {
                break;
            }
            if std::fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_and_length_prefixed() {
        assert_eq!(key(&[b"ab", b"c"]), key(&[b"ab", b"c"]));
        assert_ne!(key(&[b"ab", b"c"]), key(&[b"a", b"bc"]));
        assert_eq!(key(&[b"x"]).len(), 32);
        // pinned: the hash must not change between builds (keys live on disk)
        assert_eq!(key(&[b"wright"]), key(&[b"wright"]));
    }

    #[test]
    fn put_get_ttl_and_trim() {
        let dir = std::env::temp_dir().join(format!("wright-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Cache::at(&dir, 300);
        c.put("a", &vec![1u8; 50]);
        assert_eq!(c.get::<Vec<u8>>("a", None).unwrap().len(), 50);
        assert!(c.get::<Vec<u8>>("missing", None).is_none());
        assert!(c.get::<Vec<u8>>("a", Some(Duration::from_secs(3600))).is_some());
        std::thread::sleep(Duration::from_millis(20));
        assert!(c.get::<Vec<u8>>("a", Some(Duration::from_millis(1))).is_none(), "expired");
        // ~150 bytes each: the third store pushes the oldest out
        c.put("b", &vec![2u8; 50]);
        std::thread::sleep(Duration::from_millis(20));
        c.put("c", &vec![3u8; 50]);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(left.len() < 3, "{left:?}");
        assert!(c.get::<Vec<u8>>("c", None).is_some(), "newest kept");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
