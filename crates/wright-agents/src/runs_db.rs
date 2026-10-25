//! The runs database: every finished Wright effort (a `wright run` dir, a
//! recorded demo, the lessons and regression files) as one searchable
//! record, so a new run starts from the nearest prior designs and the
//! failures they already hit instead of re-deriving them.
//!
//! Store shape follows MLflow's tracking store (mlflow/mlflow
//! `mlflow/store/tracking/abstract_store.py`, `AbstractStore`): one
//! backend-agnostic interface the front end talks to (`get_run`,
//! `create_run` + `log_batch` folded into [`RunStore::log_run`]), with
//! search split the way MLflow splits it: the concrete `search_runs`
//! calls the backend's abstract `_search_runs` for the candidate runs and
//! returns them ranked; here [`RunStore::list_runs`] is the backend's part
//! (filter by domain, MLflow's experiment) and [`RunStore::search_runs`]
//! is the shared ranking, so every backend ranks the same way. Backend
//! choice by URI scheme follows MLflow's `TrackingStoreRegistry`
//! (`mlflow/tracking/_tracking_service/registry.py`, `get_store` ->
//! `get_store_builder(scheme)`; `utils.py` `_register_tracking_stores`:
//! `""` and `"file"` -> the local `FileStore`, other schemes -> remote
//! stores): `WRIGHT_RUNS_URI` unset or a path / `file:` -> [`JsonlStore`],
//! `cosmos://` -> the Azure Cosmos DB store (cargo feature `azure`),
//! `off` -> no store ([`from_env`]).
//!
//! Deviations, with reasons: the local default is a JSON-lines file, not
//! SQLite, because CLAUDE.md keeps storage as files until after Cal Hacks
//! and the shared cargo lock has no SQLite crate; it is append-only (the
//! last line for an id wins on read) so parallel sessions never lose a
//! write, and `wright runs import` compacts it. Ranking is BM25 over
//! request + spec + failures (the same Lucene formula as
//! [`crate::memory::retrieve`]), with an exact spec match ranked first.
//! Records are advisory evidence, like lessons: nothing here passes a
//! check.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The local store's file name in the data dir.
pub const DB_FILE: &str = "runs-db.jsonl";
/// Prior runs listed in the prompt block.
pub const PRIOR_RUNS: usize = 4;
/// Known failures listed in the prompt block.
pub const PRIOR_FAILURES: usize = 8;
/// Longest best-prior-design JSON in the prompt block, in characters.
pub const DESIGN_CHARS: usize = 4000;
/// Longest "Prior efforts" block, in characters.
pub const SECTION_CHARS: usize = 7000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// A finished (or stopped) run with a state: request, spec, design, checks.
    #[default]
    Run,
    /// A lesson from `lessons-<domain>.jsonl` (`crate::memory`).
    Lesson,
    /// A confirmed regression case from `regressions-<domain>.jsonl`.
    Regression,
}

/// One check's result in a record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRow {
    pub name: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<f64>,
    #[serde(default)]
    pub detail: String,
}

/// One prior effort (MLflow's `Run` = `RunInfo` + `RunData`, flattened).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    #[serde(default)]
    pub kind: RecordKind,
    pub domain: String,
    #[serde(default)]
    pub request: String,
    #[serde(default)]
    pub spec: Value,
    /// [`spec_key`] of `spec`: equal keys = the same spec exactly.
    #[serde(default)]
    pub spec_key: String,
    /// The chosen design (the verified incumbent when the state has one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<Value>,
    #[serde(default)]
    pub checks: Vec<CheckRow>,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub failing: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<f64>,
    #[serde(default)]
    pub objective_label: String,
    /// Checks passed / checks total (0 with no checks).
    #[serde(default)]
    pub score: f64,
    /// Paths on disk (STL/STEP stay there); remote copies in `artifact_urls`.
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_urls: Vec<String>,
    /// Why it failed: failing checks with their detail, confirmed attacks,
    /// open feedback, lesson text.
    #[serde(default)]
    pub failures: Vec<String>,
    /// Where it was imported from (a run dir or a jsonl file).
    #[serde(default)]
    pub source: String,
    /// YYYY-MM-DD.
    #[serde(default)]
    pub date: String,
}

impl RunRecord {
    fn document(&self) -> String {
        let mut s = format!("{} {}", self.request, self.failures.join(" "));
        for c in &self.checks {
            s.push(' ');
            s.push_str(&c.name);
        }
        crate::memory::query_text(&s, &self.spec)
    }
    /// Ranking among prior designs: passed, then fewer failing, then a
    /// higher objective (`crate::state::better`).
    fn better_than(&self, other: &RunRecord) -> bool {
        // a run with no check report ranks below any judged one
        let failing = |r: &RunRecord| if r.checks.is_empty() { usize::MAX } else { r.failing };
        (self.passed && !other.passed) || (self.passed == other.passed && crate::state::better((failing(self), self.objective), (failing(other), other.objective)))
    }
    /// "passed", "N failing" or "no checks recorded".
    pub fn verdict(&self) -> String {
        match (self.passed, self.checks.is_empty() && self.kind == RecordKind::Run) {
            (true, _) => "passed".into(),
            (false, true) => "no checks recorded".into(),
            (false, false) => format!("{} failing", self.failing),
        }
    }
}

/// What to look for.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub domain: Option<String>,
    pub text: String,
    pub spec: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub record: RunRecord,
    pub score: f64,
    pub exact_spec: bool,
}

/// A runs database backend (MLflow `AbstractStore`).
pub trait RunStore: Send + Sync + std::fmt::Debug {
    /// Where it lives, for messages.
    fn describe(&self) -> String;
    /// Insert or replace the record with this id.
    fn log_run(&self, rec: &RunRecord) -> anyhow::Result<()>;
    /// Every record, newest write last, optionally of one domain (MLflow
    /// `_search_runs` scoped to experiment ids).
    fn list_runs(&self, domain: Option<&str>) -> anyhow::Result<Vec<RunRecord>>;
    fn get_run(&self, id: &str) -> anyhow::Result<Option<RunRecord>> {
        Ok(self.list_runs(None)?.into_iter().rev().find(|r| r.id == id))
    }
    /// MLflow `log_batch`: several records; returns how many were written.
    fn log_batch(&self, recs: &[RunRecord]) -> anyhow::Result<usize> {
        for r in recs {
            self.log_run(r)?;
        }
        Ok(recs.len())
    }
    /// The ranked matches (MLflow `search_runs` over `_search_runs`).
    fn search_runs(&self, q: &Query, max_results: usize) -> anyhow::Result<Vec<Hit>> {
        Ok(rank(self.list_runs(q.domain.as_deref())?, q, max_results))
    }
}

// ------------------------------------------------------------ ranking

/// Canonical spec key: FNV-1a 64 of the spec's JSON with sorted keys
/// (serde_json's default map is ordered). Empty for a null or empty spec,
/// which never counts as an exact match.
pub fn spec_key(spec: &Value) -> String {
    let empty = match spec {
        Value::Null => true,
        Value::Object(m) => m.is_empty(),
        _ => false,
    };
    if empty {
        return String::new();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in spec.to_string().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Rank `records` for `q`: exact spec matches first, then BM25 (Lucene
/// `BM25Similarity`, k1 1.2, b 0.75, as `crate::memory::retrieve`) over
/// [`RunRecord::document`], newer first on a tie. Records with neither a
/// shared term nor an exact spec are dropped. One per id (the last
/// written wins).
pub fn rank(records: Vec<RunRecord>, q: &Query, max_results: usize) -> Vec<Hit> {
    let mut by_id: BTreeMap<String, (usize, RunRecord)> = BTreeMap::new();
    for (i, r) in records.into_iter().enumerate() {
        by_id.insert(r.id.clone(), (i, r));
    }
    let recs: Vec<(usize, RunRecord)> = by_id.into_values().filter(|(_, r)| q.domain.as_deref().is_none_or(|d| r.domain == d)).collect();
    let docs: Vec<Vec<String>> = recs.iter().map(|(_, r)| crate::memory::tokens(&r.document())).collect();
    let query = crate::memory::query_text(&q.text, q.spec.as_ref().unwrap_or(&Value::Null));
    let scores = bm25(&docs, &query);
    let key = q.spec.as_ref().map(spec_key).unwrap_or_default();
    let mut hits: Vec<(usize, Hit)> = recs
        .into_iter()
        .zip(scores)
        .map(|((i, r), s)| {
            let exact = !key.is_empty() && r.spec_key == key;
            (i, Hit { record: r, score: s, exact_spec: exact })
        })
        .filter(|(_, h)| h.exact_spec || h.score > 0.0)
        .collect();
    hits.sort_by(|a, b| b.1.exact_spec.cmp(&a.1.exact_spec).then(b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal)).then(b.0.cmp(&a.0)));
    hits.into_iter().take(max_results).map(|(_, h)| h).collect()
}

/// BM25 score of every token list in `docs` for `query`.
pub fn bm25(docs: &[Vec<String>], query: &str) -> Vec<f64> {
    let n = docs.len() as f64;
    let avgdl = docs.iter().map(|t| t.len() as f64).sum::<f64>() / n.max(1.0);
    let mut df: HashMap<&str, usize> = HashMap::new();
    for t in docs {
        for term in t.iter().collect::<HashSet<_>>() {
            *df.entry(term).or_default() += 1;
        }
    }
    let q: HashSet<String> = crate::memory::tokens(query).into_iter().collect();
    let (k1, b) = (1.2, 0.75);
    docs.iter()
        .map(|t| {
            let dl = t.len() as f64;
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for term in t {
                *tf.entry(term).or_default() += 1.0;
            }
            q.iter()
                .filter_map(|term| {
                    let f = *tf.get(term.as_str())?;
                    let d = *df.get(term.as_str()).unwrap_or(&0) as f64;
                    let idf = (1.0 + (n - d + 0.5) / (d + 0.5)).ln();
                    Some(idf * f / (f + k1 * (1.0 - b + b * dl / avgdl.max(1e-9))))
                })
                .sum()
        })
        .collect()
}

// ------------------------------------------------------------ local store

/// The default backend: one JSON record per line, append-only, the last
/// line for an id wins (MLflow's `FileStore` is its local default too).
#[derive(Debug, Clone)]
pub struct JsonlStore {
    pub path: PathBuf,
}

impl JsonlStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        JsonlStore { path: path.into() }
    }

    /// Rewrite the file with one line per id (temp file + rename, as
    /// `crate::memory::append` compacts). Returns the records kept.
    pub fn compact(&self) -> anyhow::Result<usize> {
        let all = self.list_runs(None)?;
        let mut seen = HashSet::new();
        let mut keep: Vec<&RunRecord> = all.iter().rev().filter(|r| seen.insert(r.id.clone())).collect();
        keep.reverse();
        let tmp = self.path.with_extension("jsonl.tmp");
        let mut s = String::new();
        for r in &keep {
            s.push_str(&serde_json::to_string(r)?);
            s.push('\n');
        }
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(keep.len())
    }
}

impl RunStore for JsonlStore {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }
    fn log_run(&self, rec: &RunRecord) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let line = format!("{}\n", serde_json::to_string(rec)?);
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        f.write_all(line.as_bytes())?;
        Ok(())
    }
    fn list_runs(&self, domain: Option<&str>) -> anyhow::Result<Vec<RunRecord>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(anyhow::Error::new(e).context(format!("reading {}", self.path.display()))),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<RunRecord>(l).ok())
            .filter(|r| domain.is_none_or(|d| r.domain == d))
            .collect())
    }
}

/// The store `WRIGHT_RUNS_URI` names (MLflow `get_store` by scheme):
/// unset -> `<data_dir>/runs-db.jsonl`; `off` / `none` -> no store; a path
/// or `file:PATH` -> that JSONL file; `cosmos://ACCOUNT/DATABASE/CONTAINER`
/// -> Azure Cosmos DB for NoSQL (feature `azure`, see docs/azure-db.md).
pub fn from_env(data_dir: &Path) -> anyhow::Result<Option<Arc<dyn RunStore>>> {
    from_uri(std::env::var("WRIGHT_RUNS_URI").ok().as_deref(), data_dir)
}

pub fn from_uri(uri: Option<&str>, data_dir: &Path) -> anyhow::Result<Option<Arc<dyn RunStore>>> {
    let uri = uri.map(str::trim).filter(|u| !u.is_empty());
    match uri {
        None => Ok(Some(Arc::new(JsonlStore::new(data_dir.join(DB_FILE))))),
        Some("off" | "none" | "0") => Ok(None),
        Some(u) if u.starts_with("cosmos://") => cosmos_store(u, data_dir),
        Some(u) => Ok(Some(Arc::new(JsonlStore::new(u.strip_prefix("file://").or_else(|| u.strip_prefix("file:")).unwrap_or(u))))),
    }
}

/// Cosmos when it can be opened (signed-in `az`, or WRIGHT_COSMOS_TOKEN);
/// otherwise a warning and the local JSONL, so a run offline or signed out
/// still has its prior efforts.
#[cfg(feature = "azure")]
fn cosmos_store(uri: &str, data_dir: &Path) -> anyhow::Result<Option<Arc<dyn RunStore>>> {
    match azure::CosmosStore::from_uri(uri) {
        Ok(s) => Ok(Some(Arc::new(s))),
        Err(e) => {
            eprintln!("runs database: {uri} unavailable ({e:#}); using {}", data_dir.join(DB_FILE).display());
            Ok(Some(Arc::new(JsonlStore::new(data_dir.join(DB_FILE)))))
        }
    }
}

#[cfg(not(feature = "azure"))]
fn cosmos_store(uri: &str, _data_dir: &Path) -> anyhow::Result<Option<Arc<dyn RunStore>>> {
    anyhow::bail!("WRIGHT_RUNS_URI={uri} needs the Azure backend: build with `--features azure` (docs/azure-db.md)")
}

// ------------------------------------------------------------ import

/// The record for a run directory holding `state.json` (a `DesignState`),
/// with run id, passed and objective from `events.jsonl` when present.
/// `None` when the dir has no readable state with a domain.
pub fn record_from_dir(dir: &Path) -> Option<RunRecord> {
    let state: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).ok()?).ok()?;
    let mut finished: Option<Value> = None;
    let mut run_id: Option<String> = None;
    let mut artifacts: Vec<String> = vec![];
    if let Ok(ev) = std::fs::read_to_string(dir.join("events.jsonl")) {
        for line in ev.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            match v["type"].as_str() {
                Some("run_started") if run_id.is_none() => run_id = v["run"].as_str().map(String::from),
                Some("run_finished") => {
                    run_id = v["run"].as_str().map(String::from).or(run_id);
                    finished = Some(v);
                }
                Some("artifact") => {
                    if let Some(p) = v["path"].as_str() {
                        artifacts.push(p.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let id = run_id.unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let mut rec = record_from_state(&state, &id, Some(dir))?;
    if let Some(f) = finished {
        rec.objective = f["objective"].as_f64().or(rec.objective);
        if let Some(l) = f["objective_label"].as_str() {
            rec.objective_label = l.to_string();
        }
    }
    for a in artifacts {
        if !rec.artifacts.contains(&a) {
            rec.artifacts.push(a);
        }
    }
    rec.date = modified_date(&dir.join("state.json")).unwrap_or_else(crate::memory::today);
    Some(rec)
}

/// The record for a serialized `DesignState`. `dir`, when given, is
/// scanned for build artifacts (meshes, CAD, G-code, BOM, sweeps).
pub fn record_from_state(state: &Value, id: &str, dir: Option<&Path>) -> Option<RunRecord> {
    let domain = state["domain"].as_str()?.to_string();
    let spec = state["spec"].clone();
    let design = state["best"]["design"].as_object().map(|_| state["best"]["design"].clone()).or_else(|| state["design"].as_object().map(|_| state["design"].clone()));
    let outcomes: Vec<wright_core::Outcome> = serde_json::from_value::<wright_core::Finished>(state["report"].clone()).map(|f| f.outcomes).unwrap_or_default();
    let checks: Vec<CheckRow> = outcomes.iter().map(|o| CheckRow { name: o.name.clone(), passed: o.passed(), margin: o.margin(), detail: clip(&o.detail, 240) }).collect();
    let failing = checks.iter().filter(|c| !c.passed).count();
    let passed = !checks.is_empty() && failing == 0;
    let mut failures: Vec<String> = checks.iter().filter(|c| !c.passed).map(|c| clip(&format!("{} failed: {}", c.name, c.detail), 300)).collect();
    for r in state["regressions"].as_array().into_iter().flatten() {
        if let (Some(check), Some(claim)) = (r["check"].as_str(), r["claim"].as_str()) {
            failures.push(clip(&format!("confirmed attack on {check}: {claim} (spec patch {})", r["spec_patch"]), 300));
        }
    }
    for f in state["feedback"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        failures.push(clip(f, 300));
    }
    failures.truncate(24);
    let score = if checks.is_empty() { 0.0 } else { (checks.len() - failing) as f64 / checks.len() as f64 };
    let objective = state["best"]["objective"].as_f64();
    Some(RunRecord {
        id: id.to_string(),
        kind: RecordKind::Run,
        domain,
        request: state["request"].as_str().unwrap_or("").to_string(),
        spec_key: spec_key(&spec),
        spec,
        design,
        checks,
        passed,
        failing,
        objective,
        objective_label: String::new(),
        score,
        artifacts: dir.map(scan_artifacts).unwrap_or_default(),
        artifact_urls: vec![],
        failures,
        source: dir.map(|d| d.display().to_string()).unwrap_or_default(),
        date: crate::memory::today(),
    })
}

const ARTIFACT_EXT: &[&str] = &["stl", "step", "stp", "glb", "gcode", "3mf", "csv", "param", "svg", "md", "fcstd", "brep", "obj", "gtl", "gbl", "gts", "gbs", "gto", "gbo", "gm1", "gml", "kicad_sch", "kicad_pcb", "kicad_pro", "gbr", "gerber", "drl", "pos", "pdf", "png", "zip"];

/// Build files under `dir` (two levels deep), capped at 200, sorted.
pub fn scan_artifacts(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = e.file_name().to_string_lossy().into_owned();
                if depth < 2 && !matches!(name.as_str(), "transcripts" | "control" | "target" | "node_modules") {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| ARTIFACT_EXT.contains(&x.to_ascii_lowercase().as_str())) {
                out.push(p.display().to_string());
            }
        }
    }
    out.sort();
    out.truncate(200);
    out
}

fn modified_date(p: &Path) -> Option<String> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(crate::memory::civil_date(secs))
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

/// Records for every run dir under `root` (the root itself, and dirs up
/// to three levels down that hold a `state.json`), sorted by path.
pub fn import_tree(root: &Path) -> Vec<RunRecord> {
    let mut dirs = vec![];
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        if d.join("state.json").is_file() {
            dirs.push(d.clone());
        }
        if depth >= 3 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !name.starts_with('.') && !matches!(name.as_str(), "transcripts" | "control" | "cad" | "target" | "node_modules" | "sweeps") {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    dirs.sort();
    dirs.iter().filter_map(|d| record_from_dir(d)).collect()
}

/// Records for the lessons and regression files in `data_dir`
/// (`lessons-<domain>.jsonl`, `regressions-<domain>.jsonl`; the rejected
/// file is skipped: those cases no longer apply).
pub fn import_data_dir(data_dir: &Path) -> anyhow::Result<Vec<RunRecord>> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(data_dir) else { return Ok(out) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let date = modified_date(&f).unwrap_or_else(crate::memory::today);
        if let Some(domain) = name.strip_prefix("lessons-").and_then(|n| n.strip_suffix(".jsonl")) {
            for l in crate::memory::load(&f, domain)? {
                let failure = match &l.fix {
                    Some(fix) => format!("{} -- fix: {fix}", l.text),
                    None => l.text.clone(),
                };
                out.push(RunRecord { id: format!("lesson:{}", l.id), kind: RecordKind::Lesson, domain: l.domain.clone(), request: l.request.clone(), failures: vec![clip(&failure, 600)], source: f.display().to_string(), date: l.date.clone(), ..blank(domain) });
            }
        } else if let Some(domain) = name.strip_prefix("regressions-").and_then(|n| n.strip_suffix(".jsonl")).filter(|d| *d != "rejected") {
            for r in crate::state::load_regressions(&f, domain)? {
                out.push(RunRecord {
                    id: format!("regression:{domain}:{}", r.id),
                    kind: RecordKind::Regression,
                    request: r.claim.clone(),
                    spec: r.spec_patch.clone(),
                    failures: vec![clip(&format!("confirmed attack on {}: {} (spec patch {}, margin {:?} when confirmed)", r.check, r.claim, r.spec_patch, r.margin_at_confirmation), 600)],
                    source: f.display().to_string(),
                    date: date.clone(),
                    ..blank(domain)
                });
            }
        }
    }
    Ok(out)
}

fn blank(domain: &str) -> RunRecord {
    RunRecord {
        id: String::new(),
        kind: RecordKind::Run,
        domain: domain.into(),
        request: String::new(),
        spec: Value::Null,
        spec_key: String::new(),
        design: None,
        checks: vec![],
        passed: false,
        failing: 0,
        objective: None,
        objective_label: String::new(),
        score: 0.0,
        artifacts: vec![],
        artifact_urls: vec![],
        failures: vec![],
        source: String::new(),
        date: String::new(),
    }
}

/// Record a finished harness run (called by `harness::run` after the
/// judge wrote its outputs) and say where, or why not, on the event bus.
pub fn log_summary(store: &dyn RunStore, run_id: &str, out_dir: Option<&Path>, summary: &crate::RunSummary, events: &tokio::sync::broadcast::Sender<wright_core::Event>) {
    let say = |text: String| {
        let _ = events.send(wright_core::Event::AgentMessage { run: run_id.into(), agent: "harness".into(), text });
    };
    let Ok(state) = serde_json::to_value(&summary.state) else { return };
    let Some(mut rec) = record_from_state(&state, run_id, out_dir) else { return };
    if let Some((o, label)) = &summary.objective {
        rec.objective = Some(*o);
        rec.objective_label = label.clone();
    }
    if let crate::StopReason::Error(e) | crate::StopReason::Budget(e) = &summary.stop {
        rec.failures.insert(0, clip(&format!("run stopped: {e}"), 300));
    }
    match store.log_run(&rec) {
        Ok(()) => say(format!("recorded this run in the runs database {}", store.describe())),
        Err(e) => say(format!("could not record this run in the runs database {}: {e:#}", store.describe())),
    }
}

// ------------------------------------------------------------ prompt block

/// The "Prior efforts" block for a new run: the nearest prior runs of the
/// domain (exact spec match first, then BM25 on request + spec), the best
/// prior design among them, and the known failures (from those runs and
/// the lessons / regression records). Empty when nothing matches, so an
/// empty store changes no prompt.
pub fn prior_section(store: &dyn RunStore, domain: &str, request: &str, spec: &Value) -> anyhow::Result<String> {
    let all = store.list_runs(Some(domain))?;
    if all.is_empty() {
        return Ok(String::new());
    }
    let q = Query { domain: Some(domain.into()), text: request.into(), spec: Some(spec.clone()).filter(|s| !s.is_null()) };
    let hits = rank(all, &q, 64);
    let runs: Vec<&Hit> = hits.iter().filter(|h| h.record.kind == RecordKind::Run).take(PRIOR_RUNS).collect();
    let others: Vec<&Hit> = hits.iter().filter(|h| h.record.kind != RecordKind::Run).collect();
    if runs.is_empty() && others.is_empty() {
        return Ok(String::new());
    }
    let mut s = format!(
        "\n\nPrior efforts (from the runs database {}; the nearest earlier requests of this domain, an exact spec match first). They are evidence from other runs, not results for this one: start from the best prior design when it fits this request, re-run check on it, and do not repeat the known failures.\n",
        store.describe()
    );
    for h in &runs {
        let r = &h.record;
        let verdict = r.verdict();
        let obj = r.objective.map(|o| format!(", objective {o:.3} {}", r.objective_label)).unwrap_or_default();
        let exact = if h.exact_spec { " [exact spec match]" } else { "" };
        let fails: Vec<&str> = r.checks.iter().filter(|c| !c.passed).map(|c| c.name.as_str()).collect();
        let fails = if fails.is_empty() { String::new() } else { format!("; failing: {}", fails.join(", ")) };
        s.push_str(&format!("- run {} ({}){exact}: {verdict}{obj}{fails}; request: \"{}\"\n", r.id, r.date, clip(&r.request, 220)));
    }
    let best = runs.iter().map(|h| &h.record).filter(|r| r.design.is_some()).fold(None::<&RunRecord>, |acc, r| match acc {
        Some(b) if !r.better_than(b) => Some(b),
        _ => Some(r),
    });
    if let Some(b) = best {
        let json = serde_json::to_string(b.design.as_ref().unwrap_or(&Value::Null)).unwrap_or_default();
        let verdict = if b.passed { "passed every check".to_string() } else { b.verdict() };
        s.push_str(&format!("Best prior design (run {}, {verdict}; seed it with edit_design if it fits, then check):\n{}\n", b.id, clip(&json, DESIGN_CHARS)));
    }
    let mut seen = HashSet::new();
    let failures: Vec<String> = runs
        .iter()
        .flat_map(|h| h.record.failures.iter().map(move |f| (f, &h.record.id)))
        .chain(others.iter().flat_map(|h| h.record.failures.iter().map(move |f| (f, &h.record.id))))
        .filter(|(f, _)| seen.insert(f.to_string()))
        .take(PRIOR_FAILURES)
        .map(|(f, id)| format!("- {} ({id})\n", clip(f, 300)))
        .collect();
    if !failures.is_empty() {
        s.push_str("Known failures from earlier efforts:\n");
        for f in failures {
            if s.len() + f.len() > SECTION_CHARS {
                break;
            }
            s.push_str(&f);
        }
    }
    Ok(s)
}

// ------------------------------------------------------------ Azure

/// Azure Cosmos DB for NoSQL over its REST API, with Microsoft Entra ID
/// (AAD) tokens, and optional Azure Blob upload of STL/STEP artifacts
/// through a container SAS URL. Chosen over Azure Database for PostgreSQL
/// because the REST path needs only `reqwest` + rustls, already in the
/// shared lock; tokio-postgres would add the client and a TLS adapter
/// (Azure PostgreSQL requires TLS). Entra tokens instead of the account
/// key: the key needs HMAC-SHA256 signing (no crate for it in the lock),
/// and Microsoft recommends disabling key auth. Env:
/// the token comes from the signed-in Azure CLI (`Token`), or
/// `WRIGHT_COSMOS_TOKEN` when set; optional `WRIGHT_BLOB_SAS_URL`
/// (`https://ACCOUNT.blob.core.windows.net/CONTAINER?sv=...`). REST
/// reference: learn.microsoft.com/rest/api/cosmos-db (Create a Document
/// with `x-ms-documentdb-is-upsert`, Query Documents with
/// `application/query+json`, Access control with `type=aad&ver=1.0`).
#[cfg(feature = "azure")]
pub mod azure {
    use super::{RunRecord, RunStore};
    use serde_json::{json, Value};

    #[derive(Debug, Clone)]
    pub struct CosmosStore {
        pub endpoint: String,
        pub database: String,
        pub container: String,
        token: Token,
        blob: Option<crate::azure_blob::BlobStore>,
    }

    use crate::azure_cli::Token;

    impl CosmosStore {
        /// `cosmos://ACCOUNT/DATABASE/CONTAINER` (ACCOUNT may be a full host).
        pub fn from_uri(uri: &str) -> anyhow::Result<Self> {
            let rest = uri.strip_prefix("cosmos://").unwrap_or(uri);
            let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
            let [account, database, container] = parts.as_slice() else {
                anyhow::bail!("WRIGHT_RUNS_URI must be cosmos://ACCOUNT/DATABASE/CONTAINER, got {uri}");
            };
            let host = if account.contains('.') { account.to_string() } else { format!("{account}.documents.azure.com") };
            let token = Token::new(std::env::var("WRIGHT_COSMOS_TOKEN").ok(), &format!("https://{host}/.default"));
            let store = CosmosStore { endpoint: format!("https://{host}"), database: database.to_string(), container: container.to_string(), token, blob: crate::azure_blob::BlobStore::from_env()? };
            store.token.get()?;
            Ok(store)
        }

        fn docs_url(&self) -> String {
            format!("{}/dbs/{}/colls/{}/docs", self.endpoint, self.database, self.container)
        }

        fn auth(&self) -> anyhow::Result<String> {
            Ok(url_encode(&format!("type=aad&ver=1.0&sig={}", self.token.get()?)))
        }

        fn query(&self, sql: &str, params: Value, partition: Option<&str>) -> anyhow::Result<Vec<RunRecord>> {
            let url = self.docs_url();
            let body = json!({"query": sql, "parameters": params});
            let auth = self.auth()?;
            let partition = partition.map(|p| json!([p]).to_string());
            block(async move {
                let client = reqwest::Client::new();
                let mut out = vec![];
                let mut continuation: Option<String> = None;
                loop {
                    let mut req = client
                        .post(&url)
                        .header("Authorization", &auth)
                        .header("x-ms-version", "2018-12-31")
                        .header("x-ms-date", http_date())
                        .header("x-ms-documentdb-isquery", "True")
                        .header("Content-Type", "application/query+json")
                        .body(body.to_string());
                    req = match &partition {
                        Some(p) => req.header("x-ms-documentdb-partitionkey", p),
                        None => req.header("x-ms-documentdb-query-enablecrosspartition", "True"),
                    };
                    if let Some(c) = &continuation {
                        req = req.header("x-ms-continuation", c);
                    }
                    let resp = req.send().await?;
                    let status = resp.status();
                    let next = resp.headers().get("x-ms-continuation").and_then(|v| v.to_str().ok()).map(String::from);
                    let v: Value = resp.json().await?;
                    if !status.is_success() {
                        anyhow::bail!("Cosmos query failed ({status}): {}", v["message"].as_str().unwrap_or(&v.to_string()));
                    }
                    out.extend(v["Documents"].as_array().into_iter().flatten().filter_map(|d| {
                        // the record's own id is kept as run_id (Cosmos ids are sanitized)
                        let mut r = serde_json::from_value::<RunRecord>(d.clone()).ok()?;
                        if let Some(orig) = d["run_id"].as_str() {
                            r.id = orig.to_string();
                        }
                        Some(r)
                    }));
                    match next {
                        Some(n) if !n.is_empty() => continuation = Some(n),
                        _ => break,
                    }
                }
                Ok(out)
            })
        }

        /// Upload the explicit artifact manifest before saving its private URLs.
        fn upload(&self, rec: &RunRecord) -> anyhow::Result<Vec<String>> {
            let Some(blob) = &self.blob else { return Ok(vec![]) };
            block(blob.upload(rec))
        }

    }

    impl RunStore for CosmosStore {
        fn describe(&self) -> String {
            format!("{}/dbs/{}/colls/{}", self.endpoint, self.database, self.container)
        }
        fn log_run(&self, rec: &RunRecord) -> anyhow::Result<()> {
            let mut rec = rec.clone();
            for url in self.upload(&rec)? {
                if !rec.artifact_urls.contains(&url) { rec.artifact_urls.push(url); }
            }
            let mut doc = serde_json::to_value(&rec)?;
            // Cosmos ids may not hold / \ ? #; the record keeps its own id field too
            doc["id"] = Value::String(safe_id(&rec.id));
            doc["run_id"] = Value::String(rec.id.clone());
            let (url, auth, pk) = (self.docs_url(), self.auth()?, json!([rec.domain]).to_string());
            block(async move {
                let resp = reqwest::Client::new()
                    .post(&url)
                    .header("Authorization", &auth)
                    .header("x-ms-version", "2018-12-31")
                    .header("x-ms-date", http_date())
                    .header("x-ms-documentdb-is-upsert", "True")
                    .header("x-ms-documentdb-partitionkey", pk)
                    .header("Content-Type", "application/json")
                    .body(doc.to_string())
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    anyhow::bail!("Cosmos upsert failed ({status}): {}", resp.text().await.unwrap_or_default());
                }
                Ok(())
            })
        }
        fn list_runs(&self, domain: Option<&str>) -> anyhow::Result<Vec<RunRecord>> {
            let mut recs = match domain {
                Some(d) => self.query("SELECT * FROM c WHERE c.domain = @d", json!([{"name": "@d", "value": d}]), Some(d))?,
                None => self.query("SELECT * FROM c", json!([]), None)?,
            };
            // oldest first, as the JSONL store returns them
            recs.sort_by(|a, b| a.date.cmp(&b.date));
            Ok(recs)
        }
    }

    /// `/ \ ? #` and whitespace replaced by `_`.
    pub fn safe_id(id: &str) -> String {
        id.chars().map(|c| if matches!(c, '/' | '\\' | '?' | '#') || c.is_whitespace() { '_' } else { c }).collect()
    }

    fn url_encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// RFC 7231 IMF-fixdate, as `x-ms-date` wants.
    pub fn http_date() -> String {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        fmt_http_date(secs)
    }

    pub fn fmt_http_date(secs: i64) -> String {
        const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let days = secs.div_euclid(86_400);
        let ymd = crate::memory::civil_date(secs);
        let (y, m, d) = (&ymd[0..4], ymd[5..7].parse::<usize>().unwrap_or(1), &ymd[8..10]);
        let t = secs.rem_euclid(86_400);
        format!("{}, {d} {} {y} {:02}:{:02}:{:02} GMT", DAYS[days.rem_euclid(7) as usize], MONTHS[m - 1], t / 3600, t / 60 % 60, t % 60)
    }

    /// Run `f` to completion on a fresh current-thread runtime in a scoped
    /// thread: the store is synchronous and is called from inside the
    /// harness's runtime, where `block_on` would panic.
    fn block<T: Send, F: std::future::Future<Output = anyhow::Result<T>> + Send>(f: F) -> anyhow::Result<T> {
        std::thread::scope(|s| s.spawn(|| tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(f)).join().map_err(|_| anyhow::anyhow!("Cosmos request thread panicked"))?)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn dates_ids_and_uris() {
            assert_eq!(fmt_http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
            assert_eq!(safe_id("lesson:a/b c"), "lesson:a_b_c");
            std::env::set_var("WRIGHT_COSMOS_TOKEN", "t");
            let s = CosmosStore::from_uri("cosmos://wright-runs/wright/runs").unwrap();
            assert_eq!(s.docs_url(), "https://wright-runs.documents.azure.com/dbs/wright/colls/runs/docs");
            assert!(CosmosStore::from_uri("cosmos://only-account").is_err());
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spec_key_ignores_key_order_and_empty_specs_never_match() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":2,"x":3}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":3,"y":2},"b":1}"#).unwrap();
        assert_eq!(spec_key(&a), spec_key(&b));
        assert_ne!(spec_key(&a), spec_key(&json!({"b": 2})));
        assert_eq!(spec_key(&Value::Null), "");
        assert_eq!(spec_key(&json!({})), "");
    }

    #[test]
    fn uri_schemes_pick_the_backend() {
        let d = Path::new("/tmp/x");
        assert_eq!(from_uri(None, d).unwrap().unwrap().describe(), "/tmp/x/runs-db.jsonl");
        assert!(from_uri(Some("off"), d).unwrap().is_none());
        assert_eq!(from_uri(Some("file:/tmp/y.jsonl"), d).unwrap().unwrap().describe(), "/tmp/y.jsonl");
        #[cfg(not(feature = "azure"))]
        assert!(from_uri(Some("cosmos://a/b/c"), d).unwrap_err().to_string().contains("--features azure"));
    }
}
