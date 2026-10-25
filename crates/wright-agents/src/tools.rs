//! Every domain, behind one object-safe interface over JSON.
//!
//! `wright_core::Domain` has associated types and consts, so it cannot be a
//! trait object; [`DomainTools`] is its JSON-in, JSON-out shadow and
//! [`tools`] makes one for any `D: Domain`. The harness holds an
//! `Arc<dyn DomainTools>` and never names a domain, which is what lets one
//! harness design aircraft, rockets and balloons.
//!
//! Every value crossing in is validated by deserializing into the domain's
//! own `Spec` / `Design` type, so an agent cannot hand a domain a field it
//! does not have. A panic inside a domain becomes an error the agent sees
//! (OpenHands returns tool failures to the model rather than ending the
//! run: software-agent-sdk @acd96e6, `agent/agent.py:1495-1557`).

use serde_json::Value;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;
use wright_core::{Artifacts, Bom, CheckId, Domain, Finished};

pub trait DomainTools: Send + Sync {
    fn name(&self) -> &'static str;
    fn spec_schema(&self) -> Value;
    /// `(name, describe)` for every declared check: the agents' repair
    /// vocabulary.
    fn checks(&self) -> Vec<(&'static str, &'static str)>;
    /// [`Domain::consistency_checks`].
    fn consistency_checks(&self) -> &'static [&'static str];
    /// Round-trip through `Spec`: errors name the bad field.
    fn validate_spec(&self, spec: &Value) -> anyhow::Result<Value>;
    fn validate_design(&self, design: &Value) -> anyhow::Result<Value>;
    fn design(&self, spec: &Value) -> anyhow::Result<Value>;
    /// [`Domain::design_under`]: design from `spec` so that the design
    /// also passes under each condition (its `spec` as JSON).
    fn design_under(&self, spec: &Value, conditions: &[wright_core::Condition<Value>]) -> anyhow::Result<Value>;
    fn check(&self, spec: &Value, design: &Value) -> anyhow::Result<Finished>;
    fn bom(&self, design: &Value) -> anyhow::Result<Bom>;
    fn build(&self, design: &Value, out: &Path) -> anyhow::Result<Artifacts>;
    fn objective(&self, design: &Value) -> anyhow::Result<(f64, &'static str)>;
    fn headlines(&self, spec: &Value, design: &Value) -> anyhow::Result<Vec<wright_core::Headline>>;
    /// `(path, what it chooses)` for every catalog-backed design path.
    fn catalog_paths(&self) -> Vec<(&'static str, &'static str)>;
    /// Read-only catalog rows for the choice at `path` of `design`.
    fn options(&self, design: &Value, path: &str, query: Option<&str>) -> anyhow::Result<Vec<wright_core::CatalogOption>>;
    /// The same rows before any design exists (`Domain::options_before_design`);
    /// `None` when the domain's catalog depends on the design.
    fn options_before_design(&self, _path: &str, _query: Option<&str>) -> Option<Vec<wright_core::CatalogOption>> {
        None
    }
}

pub struct Tools<D>(PhantomData<fn() -> D>);

pub fn tools<D: Domain + 'static>() -> Arc<dyn DomainTools> {
    Arc::new(Tools::<D>(PhantomData))
}

fn guard<T>(what: &str, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".into());
            anyhow::bail!("{what} panicked: {msg}")
        }
    }
}

fn spec<D: Domain>(v: &Value) -> anyhow::Result<D::Spec> {
    serde_json::from_value(v.clone()).map_err(|e| anyhow::anyhow!("spec does not fit the {} schema: {e}", D::NAME))
}
/// Dotted paths of every `null` in `v` (at most 12).
fn null_paths(v: &Value, at: String) -> Vec<String> {
    let mut out = vec![];
    match v {
        Value::Null => out.push(at),
        Value::Object(m) => {
            for (k, x) in m {
                out.extend(null_paths(x, if at.is_empty() { k.clone() } else { format!("{at}.{k}") }));
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                out.extend(null_paths(x, format!("{at}[{i}]")));
            }
        }
        _ => {}
    }
    out.truncate(12);
    out
}

fn design<D: Domain>(v: &Value) -> anyhow::Result<D::Design> {
    serde_json::from_value(v.clone()).map_err(|e| anyhow::anyhow!("design is not a valid {} design: {e}", D::NAME))
}

/// Turn on the memoized design search (the CLI does, for `wright run`;
/// tests and the bench never do, so scripted fixtures are never cached).
static DESIGN_CACHE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn enable_design_cache(on: bool) {
    DESIGN_CACHE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The identity of the running binary (path, size, mtime): any rebuild of
/// a domain's search is a different key.
fn build_identity() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let m = std::fs::metadata(&exe).ok();
    let mtime = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}|{}|{mtime}", exe.display(), m.map(|m| m.len()).unwrap_or(0))
}

/// `None` unless [`enable_design_cache`] and `wright_core::cache::enabled()`.
fn design_key(domain: &str, spec: &Value, conditions: &[wright_core::Condition<Value>]) -> Option<String> {
    if !DESIGN_CACHE.load(std::sync::atomic::Ordering::Relaxed) || !wright_core::cache::enabled() {
        return None;
    }
    let mut h = wright_core::cache::Hasher::new();
    h.str(domain).json(spec).str(&build_identity()).str(&std::env::var("WRIGHT_CACHE_SALT").unwrap_or_default());
    for c in conditions {
        h.str(&c.id).str(&c.check).json(&c.spec);
    }
    Some(h.hex())
}

impl<D: Domain + 'static> Tools<D> {
    fn design_under_uncached(&self, v: &Value, conditions: &[wright_core::Condition<Value>]) -> anyhow::Result<Value> {
        let s = spec::<D>(v)?;
        let conds = conditions
            .iter()
            .map(|c| {
                Ok(wright_core::Condition {
                    id: c.id.clone(),
                    check: c.check.clone(),
                    claim: c.claim.clone(),
                    spec: spec::<D>(&c.spec).map_err(|e| anyhow::anyhow!("regression {}: {e}", c.id))?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let out = guard("design", || Ok(serde_json::to_value(D::design_under(&s, &conds)?)?))?;
        // A design that cannot be read back is a domain bug (a NaN in a
        // plain f64 serialises as null): say so here, loudly, instead of
        // letting agents hand-patch nulls (seen in a live run, 2026-10-24).
        if let Err(e) = design::<D>(&out) {
            let nulls = null_paths(&out, String::new());
            anyhow::bail!(
                "the {} domain produced a design it cannot read back ({e}); null fields: {}. \
                 This is a bug in the domain, not something to patch by hand: report it in your summary.",
                D::NAME,
                if nulls.is_empty() { "none".into() } else { nulls.join(", ") }
            );
        }
        Ok(out)
    }
}

impl<D: Domain + 'static> DomainTools for Tools<D> {
    fn name(&self) -> &'static str {
        D::NAME
    }
    fn spec_schema(&self) -> Value {
        D::spec_schema()
    }
    fn checks(&self) -> Vec<(&'static str, &'static str)> {
        D::Check::ALL.iter().map(|c| (c.name(), c.describe())).collect()
    }
    fn consistency_checks(&self) -> &'static [&'static str] {
        D::consistency_checks()
    }
    fn validate_spec(&self, v: &Value) -> anyhow::Result<Value> {
        let s = spec::<D>(v)?;
        guard("validate_spec", || D::validate_spec(&s)).map_err(|e| anyhow::anyhow!("spec is outside the {} envelope: {e:#}", D::NAME))?;
        Ok(serde_json::to_value(s)?)
    }
    fn validate_design(&self, v: &Value) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(design::<D>(v)?)?)
    }
    fn design(&self, v: &Value) -> anyhow::Result<Value> {
        self.design_under(v, &[])
    }
    fn design_under(&self, v: &Value, conditions: &[wright_core::Condition<Value>]) -> anyhow::Result<Value> {
        // the search across rounds and runs, memoized by (domain, spec,
        // regression cases, this binary) when the CLI turned it on
        let key = design_key(D::NAME, v, conditions);
        let cache = key.as_ref().map(|_| wright_core::cache::Cache::named(&format!("design-{}", D::NAME)));
        if let (Some(k), Some(c)) = (&key, &cache) {
            if let Some(hit) = c.get::<Result<Value, String>>(k, None) {
                return hit.map_err(|e| anyhow::anyhow!("{e}"));
            }
        }
        let out = self.design_under_uncached(v, conditions);
        if let (Some(k), Some(c)) = (&key, &cache) {
            c.put(k, &out.as_ref().map(Value::clone).map_err(|e| format!("{e:#}")));
        }
        out
    }
    fn headlines(&self, s: &Value, d: &Value) -> anyhow::Result<Vec<wright_core::Headline>> {
        let (s, d) = (spec::<D>(s)?, design::<D>(d)?);
        guard("headlines", || Ok(D::headlines(&s, &d)))
    }
    fn catalog_paths(&self) -> Vec<(&'static str, &'static str)> {
        D::catalog_paths()
    }
    fn options(&self, d: &Value, path: &str, query: Option<&str>) -> anyhow::Result<Vec<wright_core::CatalogOption>> {
        let d = design::<D>(d)?;
        guard("options", || Ok(D::options(&d, path, query)))
    }
    fn options_before_design(&self, path: &str, query: Option<&str>) -> Option<Vec<wright_core::CatalogOption>> {
        D::options_before_design(path, query)
    }
    fn check(&self, s: &Value, d: &Value) -> anyhow::Result<Finished> {
        let (s, d) = (spec::<D>(s)?, design::<D>(d)?);
        guard("check", || Ok(D::check(&s, &d)))
    }
    fn bom(&self, d: &Value) -> anyhow::Result<Bom> {
        let d = design::<D>(d)?;
        guard("bom", || Ok(D::bom(&d)))
    }
    fn build(&self, d: &Value, out: &Path) -> anyhow::Result<Artifacts> {
        let d = design::<D>(d)?;
        guard("build", || D::build(&d, out))
    }
    fn objective(&self, d: &Value) -> anyhow::Result<(f64, &'static str)> {
        let d = design::<D>(d)?;
        guard("objective", || Ok(D::objective(&d)))
    }
}

/// RFC 7396 JSON Merge Patch: objects merge, `null` deletes, anything else
/// replaces. Agents edit a spec or design by patch, never by rewriting it.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = Value::Object(Default::default());
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        other => *target = other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patch_rfc7396_examples() {
        let mut t = json!({"a": "b", "c": {"d": "e", "f": "g"}});
        merge_patch(&mut t, &json!({"a": "z", "c": {"f": null}}));
        assert_eq!(t, json!({"a": "z", "c": {"d": "e"}}));
        let mut t = json!([1, 2]);
        merge_patch(&mut t, &json!({"a": 1}));
        assert_eq!(t, json!({"a": 1}));
    }
}
