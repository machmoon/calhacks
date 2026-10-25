//! Post-design stages: CFD, FEA, fabrication films, editable CAD, USD,
//! flight sims. Each runs on a design that passed and was built, and writes
//! `<out_dir>/<name>/manifest.json` ([`Manifest`]).
//!
//! Prior art:
//! * A stage is a Dagster asset, not a task: it names what it produces
//!   (its `name`, which is also its manifest's `kind` and its directory)
//!   and what it needs (`deps` in Dagster's `@asset`; here [`Stage::needs`]),
//!   and returns a result object carrying its outputs' metadata and check
//!   results (dagster `python_modules/dagster/dagster/_core/definitions/result.py`,
//!   `MaterializeResult { metadata, check_results, data_version, .. }`;
//!   here [`Manifest`] with `items[].metrics` and `checks`).
//! * Whether a stage re-runs follows Snakemake's needrun reasons
//!   (snakemake `src/snakemake/dag.py`, `DAG.update_needrun`): it runs if
//!   its output is missing (`reason.missing_output`), if its input changed
//!   by checksum (`updated_input` is gated on `not is_same_checksum`), if
//!   its code changed (`code_changed`; here [`Stage::version`]), or if
//!   forced (`forced`). A run that failed left no complete output and runs
//!   again (Snakemake's incomplete-file handling). The checksum here is
//!   [`input_hash`]: design, spec, built files, the stage's version and
//!   every upstream stage's hash.
//!
//! A stage that fails never fails the design run: [`Stages::run`] turns an
//! error or a panic into a manifest with `error` set and goes on.

use crate::Artifacts;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The kind a stage may need that no stage produces: the domain's build
/// files ([`StageCtx::artifacts`]).
pub const BUILD: &str = "build";

/// One produced file: a picture, a film, a CAD file, or numbers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub label: String,
    /// Relative to the manifest's directory (`<out_dir>/<kind>/`).
    pub path: String,
    /// The file's extension: "mp4" | "png" | "json" | "csv" | "step" | "usd" ...
    pub media: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub metrics: BTreeMap<String, f64>,
}

/// A stage's own verdict on the design (Dagster's `AssetCheckResult`):
/// "FEA: boom root von Mises 180 MPa vs 160 MPa allowable" fails. Agents
/// read these as evidence and may respond with a design edit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageCheck {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub detail: String,
}

/// `<out_dir>/<kind>/manifest.json`:
/// `{ "kind", "title", "items": [...], "checks"?, "error"?, "input_hash"? }`.
/// The first three are the contract wright-cfd already writes; the rest
/// are optional, so a manifest without them still reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<StageCheck>,
    /// Why the stage failed; `None` when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// [`input_hash`] of what it ran on; set by [`Stages::run`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_hash: Option<String>,
}

impl Manifest {
    pub fn new(kind: impl Into<String>, title: impl Into<String>) -> Self {
        Manifest { kind: kind.into(), title: title.into(), items: vec![], checks: vec![], error: None, input_hash: None }
    }

    /// A manifest that says the stage failed and why.
    pub fn failed(kind: impl Into<String>, why: impl Into<String>) -> Self {
        let kind = kind.into();
        Manifest { title: kind.clone(), error: Some(why.into()), ..Manifest::new(kind, "") }
    }

    /// Add a file under `dir` (the stage's directory), stored relative to it.
    pub fn add(&mut self, dir: &Path, file: &Path, label: &str, summary: &str, metrics: &[(&str, f64)]) {
        let rel = file.strip_prefix(dir).unwrap_or(file).to_string_lossy().replace('\\', "/");
        let media = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        self.items.push(Item {
            label: label.into(),
            path: rel,
            media,
            summary: summary.into(),
            metrics: metrics.iter().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k.to_string(), *v)).collect(),
        });
    }

    pub fn check(&mut self, name: &str, passed: bool, detail: &str) {
        self.checks.push(StageCheck { name: name.into(), passed, detail: detail.into() });
    }

    pub fn ok(&self) -> bool {
        self.error.is_none()
    }

    pub fn read(dir: &Path) -> Option<Manifest> {
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()
    }

    pub fn write(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("manifest.json");
        let tmp = dir.join("manifest.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// A few lines for an agent: title, error or checks, items with metrics.
    pub fn brief(&self) -> String {
        let mut s = format!("{} ({})", self.title, self.kind);
        if let Some(e) = &self.error {
            s.push_str(&format!(": FAILED to run: {e}"));
            return s;
        }
        for c in &self.checks {
            s.push_str(&format!("\n- check {}: {} {}", c.name, if c.passed { "PASS" } else { "FAIL" }, c.detail));
        }
        for it in &self.items {
            s.push_str(&format!("\n- {} [{}]", it.label, it.path));
            if !it.summary.is_empty() {
                s.push_str(&format!(": {}", it.summary));
            }
            if !it.metrics.is_empty() {
                let m: Vec<String> = it.metrics.iter().map(|(k, v)| format!("{k}={v:.4}")).collect();
                s.push_str(&format!(" ({})", m.join(", ")));
            }
        }
        s
    }
}

/// What a stage runs on.
#[derive(Debug, Clone)]
pub struct StageCtx {
    /// The domain's name ("aircraft", "rocket", "balloon").
    pub domain: String,
    /// The run's output directory; the stage writes under [`Self::dir`].
    pub out_dir: PathBuf,
    /// This stage's directory: `<out_dir>/<name>`.
    pub dir: PathBuf,
    pub design: Value,
    pub spec: Value,
    /// The domain's build files (paths as the build returned them).
    pub artifacts: Artifacts,
    /// Manifests of the stages this one needs, by name.
    pub upstream: BTreeMap<String, Manifest>,
}

/// One post-design stage. Implement it in the stage's crate and register it
/// in `crates/wright-cli/src/stages.rs`.
pub trait Stage: Send + Sync {
    /// Its manifest's `kind` and its directory under the run: "cfd", "fea",
    /// "fab", "cad", "usd", "sim".
    fn name(&self) -> &'static str;
    /// The kinds it needs first: other stages' names, or [`BUILD`] for the
    /// domain's build files. A stage whose need failed is not run.
    fn needs(&self) -> &'static [&'static str] {
        &[BUILD]
    }
    /// Whether it has anything to do for this domain (CFD's AVL case is
    /// aircraft only).
    fn applies_to(&self, domain: &str) -> bool {
        let _ = domain;
        true
    }
    /// Bump when the stage's code changes what it writes: outputs made by
    /// another version are stale (Snakemake's `code_changed`).
    fn version(&self) -> &'static str {
        "1"
    }
    /// Do the work, write files under `ctx.dir`, and return the manifest.
    /// The runner writes `manifest.json` from what this returns (setting
    /// `kind` and `input_hash`), so a stage need not write it itself.
    fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest>;
}

/// What one stage did in [`Stages::run`].
#[derive(Debug, Clone)]
pub struct StageRun {
    pub name: String,
    pub manifest: Manifest,
    /// Up to date: not run again, the manifest on disk was kept.
    pub fresh: bool,
}

/// The registered stages.
#[derive(Clone, Default)]
pub struct Stages {
    stages: Vec<Arc<dyn Stage>>,
}

impl std::fmt::Debug for Stages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.stages.iter().map(|s| s.name())).finish()
    }
}

impl Stages {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a stage. A later stage with the same name replaces the earlier
    /// for every domain the later one applies to; stages with one name for
    /// different domains (each domain's "sim") live side by side.
    pub fn register(&mut self, stage: impl Stage + 'static) -> &mut Self {
        self.register_arc(Arc::new(stage))
    }

    pub fn register_arc(&mut self, stage: Arc<dyn Stage>) -> &mut Self {
        self.stages.push(stage);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// The stages that apply to `domain`, in registration order.
    pub fn names(&self, domain: &str) -> Vec<&'static str> {
        self.for_domain(domain).iter().map(|s| s.name()).collect()
    }

    /// The stage named `name` for `domain`: the latest registered one that
    /// applies to it.
    pub fn get(&self, name: &str, domain: &str) -> Option<Arc<dyn Stage>> {
        self.stages.iter().rev().find(|s| s.name() == name && s.applies_to(domain)).cloned()
    }

    /// The stages that apply to `domain`, one per name (the latest
    /// registered wins), in the order each name was first registered.
    fn for_domain(&self, domain: &str) -> Vec<Arc<dyn Stage>> {
        let mut out: Vec<Arc<dyn Stage>> = vec![];
        for s in self.stages.iter().filter(|s| s.applies_to(domain)) {
            match out.iter().position(|o| o.name() == s.name()) {
                Some(i) => out[i] = s.clone(),
                None => out.push(s.clone()),
            }
        }
        out
    }

    /// Run `only` (with the stages it needs), or every stage that applies to
    /// `base.domain`, needs first. `base` is the context without the
    /// per-stage `dir` and `upstream`. A fresh stage is kept unless `force`.
    /// `each` sees every stage's result as it finishes (to emit events).
    /// Never fails: a stage's error lands in its manifest.
    pub fn run(&self, base: &StageCtx, only: Option<&str>, force: bool, mut each: impl FnMut(&StageRun)) -> Vec<StageRun> {
        let wanted: Vec<Arc<dyn Stage>> = match only {
            Some(n) => match self.get(n, &base.domain) {
                Some(s) => self.closure(&s, &base.domain),
                None => {
                    let r = StageRun { name: n.into(), manifest: Manifest::failed(n, format!("no stage named `{n}` (registered: {})", self.names(&base.domain).join(", "))), fresh: false };
                    each(&r);
                    return vec![r];
                }
            },
            None => self.for_domain(&base.domain),
        };
        let mut done: BTreeMap<String, Manifest> = BTreeMap::new();
        let mut out = vec![];
        let mut left = wanted;
        while !left.is_empty() {
            // the first stage whose stage-needs are all settled; if none is
            // (a cycle or an unregistered need) the first left runs and
            // reports what it lacks
            let i = left
                .iter()
                .position(|s| s.needs().iter().all(|n| *n == BUILD || done.contains_key(*n) || !left.iter().any(|l| l.name() == *n)))
                .unwrap_or(0);
            let s = left.remove(i);
            let r = self.run_one(s.as_ref(), base, &done, force && only.is_none_or(|o| o == s.name()));
            each(&r);
            done.insert(r.name.clone(), r.manifest.clone());
            out.push(r);
        }
        out
    }

    /// Several stages at once: the union of their closures, in waves (every
    /// stage whose needs are settled runs concurrently, at most `max_par`
    /// at a time, each on its own thread; each stage still runs once, so
    /// two requested stages that need `cad` share one `cad` run). Results
    /// come back in a fixed order (wave, then the order the stages were
    /// registered), so the event log is the same whatever finishes first.
    /// Same contract as [`Stages::run`]: never fails, errors land in
    /// manifests. Precedent: Bazel/Ninja run every ready action of the
    /// build graph concurrently up to `-j` (ninja `src/build.cc`,
    /// `Builder::Build`: `while (plan_.more_to_do())` starts every ready
    /// edge while `command_runner_->CanRunMore()` reports capacity).
    pub fn run_many(&self, base: &StageCtx, names: &[&str], force: bool, max_par: usize) -> Vec<StageRun> {
        let mut wanted: Vec<Arc<dyn Stage>> = vec![];
        let mut out = vec![];
        for n in names {
            match self.get(n, &base.domain) {
                Some(s) => {
                    for x in self.closure(&s, &base.domain) {
                        if !wanted.iter().any(|w| w.name() == x.name()) {
                            wanted.push(x);
                        }
                    }
                }
                None => out.push(StageRun { name: n.to_string(), manifest: Manifest::failed(*n, format!("no stage named `{n}` (registered: {})", self.names(&base.domain).join(", "))), fresh: false }),
            }
        }
        // registration order, for a deterministic result order
        let order = |n: &str| self.stages.iter().position(|s| s.name() == n).unwrap_or(usize::MAX);
        wanted.sort_by_key(|s| order(s.name()));
        let forced = |n: &str| force && names.contains(&n);
        let mut done: BTreeMap<String, Manifest> = BTreeMap::new();
        let mut left = wanted;
        while !left.is_empty() {
            let ready: Vec<Arc<dyn Stage>> = {
                let settled = |s: &Arc<dyn Stage>| s.needs().iter().all(|n| *n == BUILD || done.contains_key(*n) || !left.iter().any(|l| l.name() == *n));
                let r: Vec<Arc<dyn Stage>> = left.iter().filter(|s| settled(s)).cloned().collect();
                // a cycle or an unregistered need: the first left runs and says what it lacks
                if r.is_empty() { vec![left[0].clone()] } else { r }
            };
            left.retain(|s| !ready.iter().any(|r| r.name() == s.name()));
            for chunk in ready.chunks(max_par.max(1)) {
                let runs: Vec<StageRun> = std::thread::scope(|sc| {
                    let hs: Vec<_> = chunk.iter().map(|s| { let done = &done; sc.spawn(move || self.run_one(s.as_ref(), base, done, forced(s.name()))) }).collect();
                    hs.into_iter().zip(chunk).map(|(h, s)| h.join().unwrap_or_else(|_| StageRun { name: s.name().into(), manifest: Manifest::failed(s.name(), "the stage thread panicked"), fresh: false })).collect()
                });
                for r in runs {
                    done.insert(r.name.clone(), r.manifest.clone());
                    out.push(r);
                }
            }
        }
        out
    }

    /// `s` and every registered stage it needs, transitively.
    fn closure(&self, s: &Arc<dyn Stage>, domain: &str) -> Vec<Arc<dyn Stage>> {
        let mut out: Vec<Arc<dyn Stage>> = vec![];
        let mut stack = vec![s.clone()];
        while let Some(x) = stack.pop() {
            if out.iter().any(|o| o.name() == x.name()) {
                continue;
            }
            for n in x.needs() {
                if let Some(d) = self.get(n, domain) {
                    stack.push(d);
                }
            }
            out.push(x);
        }
        out.reverse();
        out
    }

    fn run_one(&self, s: &dyn Stage, base: &StageCtx, done: &BTreeMap<String, Manifest>, force: bool) -> StageRun {
        let name = s.name().to_string();
        let dir = base.out_dir.join(&name);
        let fail = |why: String| {
            let m = Manifest::failed(name.clone(), why);
            let _ = m.write(&dir);
            StageRun { name: name.clone(), manifest: m, fresh: false }
        };
        let mut upstream = BTreeMap::new();
        for n in s.needs() {
            if *n == BUILD {
                if base.artifacts.files.is_empty() {
                    return fail("needs the build files, and the design has none (only a passing design is built)".into());
                }
                continue;
            }
            match done.get(*n) {
                Some(m) if m.ok() => {
                    upstream.insert(n.to_string(), m.clone());
                }
                Some(m) => return fail(format!("needs `{n}`, which failed: {}", m.error.as_deref().unwrap_or(""))),
                None => return fail(format!("needs `{n}`, which is not a registered stage for {}", base.domain)),
            }
        }
        let hash = input_hash(s, base, &upstream);
        if !force {
            if let Some(old) = Manifest::read(&dir) {
                let complete = old.ok() && old.items.iter().all(|it| dir.join(&it.path).exists());
                if complete && old.input_hash.as_deref() == Some(hash.as_str()) {
                    return StageRun { name, manifest: old, fresh: true };
                }
            }
        }
        let ctx = StageCtx { dir: dir.clone(), upstream, ..base.clone() };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return fail(format!("cannot create {}: {e}", dir.display()));
        }
        let result = catch_unwind(AssertUnwindSafe(|| s.run(&ctx)));
        let mut m = match result {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => Manifest::failed(name.clone(), format!("{e:#}")),
            Err(p) => {
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "unknown panic".into());
                Manifest::failed(name.clone(), format!("panicked: {msg}"))
            }
        };
        m.kind = name.clone();
        if m.title.is_empty() {
            m.title = name.clone();
        }
        // only a complete run is reusable (a failed one runs again)
        m.input_hash = m.ok().then_some(hash);
        if let Some(missing) = m.items.iter().find(|it| !dir.join(&it.path).exists()) {
            m.error = Some(format!("listed `{}`, which it did not write", missing.path));
            m.input_hash = None;
        }
        if let Err(e) = m.write(&dir) {
            m.error.get_or_insert(format!("could not write its manifest: {e:#}"));
        }
        StageRun { name, manifest: m, fresh: false }
    }
}

/// FNV-1a over the stage's name and version, the design, the spec, the
/// built files (path, size, modified time) and each upstream manifest's
/// own hash: what decides whether the last output is stale.
pub fn input_hash(s: &dyn Stage, base: &StageCtx, upstream: &BTreeMap<String, Manifest>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    eat(s.name().as_bytes());
    eat(s.version().as_bytes());
    eat(base.domain.as_bytes());
    eat(base.design.to_string().as_bytes());
    eat(base.spec.to_string().as_bytes());
    if s.needs().contains(&BUILD) {
        for (label, p) in &base.artifacts.files {
            eat(label.as_bytes());
            eat(p.to_string_lossy().as_bytes());
            if let Ok(md) = std::fs::metadata(p) {
                eat(&md.len().to_le_bytes());
                if let Ok(t) = md.modified().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)) {
                    eat(&t.as_nanos().to_le_bytes());
                }
            }
        }
    }
    for (n, m) in upstream {
        eat(n.as_bytes());
        eat(m.input_hash.as_deref().unwrap_or("").as_bytes());
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static RUNS: AtomicUsize = AtomicUsize::new(0);

    struct Cad;
    impl Stage for Cad {
        fn name(&self) -> &'static str {
            "cad"
        }
        fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
            RUNS.fetch_add(1, Ordering::SeqCst);
            let f = ctx.dir.join("part.step");
            std::fs::write(&f, "ISO-10303-21;")?;
            let mut m = Manifest::new("cad", "Editable CAD");
            m.add(&ctx.dir, &f, "STEP", "one solid", &[("solids", 1.0)]);
            Ok(m)
        }
    }
    struct Fea;
    impl Stage for Fea {
        fn name(&self) -> &'static str {
            "fea"
        }
        fn needs(&self) -> &'static [&'static str] {
            &["cad"]
        }
        fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
            anyhow::ensure!(ctx.upstream.contains_key("cad"), "no cad");
            let mut m = Manifest::new("fea", "FEA");
            m.check("boom_root_stress", false, "180 MPa vs 160 MPa allowable");
            Ok(m)
        }
    }
    struct Boom;
    impl Stage for Boom {
        fn name(&self) -> &'static str {
            "sim"
        }
        fn run(&self, _: &StageCtx) -> anyhow::Result<Manifest> {
            panic!("solver crashed")
        }
    }

    /// Each domain's "sim" lives side by side; a later stage replaces an
    /// earlier one of the same name only for the domains it applies to.
    #[test]
    fn same_name_stages_for_different_domains_do_not_replace_each_other() {
        struct Sim(&'static str);
        impl Stage for Sim {
            fn name(&self) -> &'static str {
                "sim"
            }
            fn applies_to(&self, domain: &str) -> bool {
                domain == self.0
            }
            fn run(&self, _: &StageCtx) -> anyhow::Result<Manifest> {
                Ok(Manifest::new("sim", self.0))
            }
        }
        let mut st = Stages::new();
        st.register(Sim("aircraft")).register(Sim("rocket")).register(Sim("balloon"));
        for d in ["aircraft", "rocket", "balloon"] {
            assert_eq!(st.names(d), vec!["sim"]);
            let out = std::env::temp_dir().join(format!("wright-stage-same-{d}-{}", std::process::id()));
            let mut b = base(&out);
            b.domain = d.into();
            let runs = st.run(&b, Some("sim"), true, |_| {});
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].manifest.title, d);
            let _ = std::fs::remove_dir_all(out);
        }
        assert!(st.names("boat").is_empty());
        // a later all-domain "sim" replaces all three
        st.register(Boom);
        assert!(std::sync::Arc::ptr_eq(&st.get("sim", "rocket").unwrap(), &st.get("sim", "balloon").unwrap()));
    }

    /// run_many: two slow stages that both need one shared stage run at
    /// the same time, the shared one once, results in registration order.
    #[test]
    fn run_many_runs_ready_stages_together_and_shared_needs_once() {
        static SHARED: AtomicUsize = AtomicUsize::new(0);
        struct Shared;
        impl Stage for Shared {
            fn name(&self) -> &'static str { "shared" }
            fn run(&self, _: &StageCtx) -> anyhow::Result<Manifest> {
                SHARED.fetch_add(1, Ordering::SeqCst);
                Ok(Manifest::new("shared", "shared"))
            }
        }
        struct Slow(&'static str);
        impl Stage for Slow {
            fn name(&self) -> &'static str { self.0 }
            fn needs(&self) -> &'static [&'static str] { &["shared"] }
            fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
                anyhow::ensure!(ctx.upstream.contains_key("shared"), "no shared");
                std::thread::sleep(std::time::Duration::from_millis(400));
                Ok(Manifest::new(self.0, self.0))
            }
        }
        let mut st = Stages::new();
        st.register(Shared).register(Slow("sim")).register(Slow("fab")).register(Slow("fea"));
        let out = std::env::temp_dir().join(format!("wright-stage-many-{}", std::process::id()));
        let b = base(&out);
        let t = std::time::Instant::now();
        let runs = st.run_many(&b, &["fea", "sim", "fab"], true, 8);
        let took = t.elapsed();
        assert!(took < std::time::Duration::from_millis(1000), "ran one at a time: {took:?}");
        assert_eq!(SHARED.load(Ordering::SeqCst), 1);
        assert_eq!(runs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["shared", "sim", "fab", "fea"]);
        assert!(runs.iter().all(|r| r.manifest.ok()), "{runs:?}");
        // max_par 1 is the serial schedule
        let t = std::time::Instant::now();
        st.run_many(&b, &["sim", "fab"], true, 1);
        assert!(t.elapsed() >= std::time::Duration::from_millis(800));
        let _ = std::fs::remove_dir_all(out);
    }

    fn base(out: &Path) -> StageCtx {
        let built = out.join("built.stl");
        std::fs::create_dir_all(out).unwrap();
        std::fs::write(&built, "solid").unwrap();
        let mut a = Artifacts::default();
        a.files.push(("part".into(), built));
        StageCtx { domain: "aircraft".into(), out_dir: out.into(), dir: out.into(), design: serde_json::json!({"span": 1}), spec: Value::Null, artifacts: a, upstream: BTreeMap::new() }
    }

    #[test]
    fn stages_run_needs_first_reuse_fresh_output_and_contain_failures() {
        let out = std::env::temp_dir().join(format!("wright-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let mut st = Stages::new();
        st.register(Fea).register(Boom).register(Cad);
        let b = base(&out);
        let r = st.run(&b, None, false, |_| {});
        let names: Vec<&str> = r.iter().map(|x| x.name.as_str()).collect();
        // registration order, except that fea waits for cad, which it needs
        assert_eq!(names, ["sim", "cad", "fea"]);
        let r: Vec<StageRun> = ["cad", "fea", "sim"].iter().map(|n| r.iter().find(|x| x.name == *n).unwrap().clone()).collect();
        assert!(r[0].manifest.ok() && r[1].manifest.ok());
        assert!(!r[1].manifest.checks[0].passed);
        assert!(r[2].manifest.error.as_deref().unwrap().contains("solver crashed"));
        assert!(Manifest::read(&out.join("sim")).unwrap().error.is_some(), "a failure is written down");
        assert_eq!(RUNS.load(Ordering::SeqCst), 1);

        // same inputs: cad is fresh and not run again; the failed sim runs again
        let r = st.run(&b, None, false, |_| {});
        let fresh: Vec<(&str, bool)> = r.iter().map(|x| (x.name.as_str(), x.fresh)).collect();
        assert_eq!(fresh, [("sim", false), ("cad", true), ("fea", true)]);
        assert_eq!(RUNS.load(Ordering::SeqCst), 1);
        // a changed design makes it stale; force re-runs a fresh one
        let mut b2 = b.clone();
        b2.design = serde_json::json!({"span": 2});
        assert!(!st.run(&b2, Some("cad"), false, |_| {})[0].fresh);
        assert_eq!(RUNS.load(Ordering::SeqCst), 2);
        assert!(!st.run(&b2, Some("cad"), true, |_| {})[0].fresh);
        assert_eq!(RUNS.load(Ordering::SeqCst), 3);
        // asking for fea alone brings cad with it
        let r = st.run(&b2, Some("fea"), false, |_| {});
        assert_eq!(r.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["cad", "fea"]);
        // no build files: a stage that needs the build says so
        let mut nb = b.clone();
        nb.artifacts = Artifacts::default();
        let r = st.run(&nb, Some("cad"), true, |_| {});
        assert!(r[0].manifest.error.as_deref().unwrap().contains("needs the build files"));
        let r = st.run(&b, Some("nope"), false, |_| {});
        assert!(r[0].manifest.error.as_deref().unwrap().contains("no stage named"));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn a_cfd_manifest_without_the_new_fields_still_reads() {
        let m: Manifest = serde_json::from_str(r#"{"kind":"cfd","title":"AVL","items":[{"label":"Cp","path":"cp.png","media":"png","summary":"s","metrics":{"cl":0.5}}]}"#).unwrap();
        assert!(m.ok() && m.checks.is_empty() && m.input_hash.is_none());
    }
}
