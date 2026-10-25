//! Parallel specialists and the merge of their design edits.
//!
//! Today `delegate` runs one specialist at a time on the shared
//! [`DesignState`]. When the orchestrator delegates several *independent*
//! roles in one turn (structures and electronics, say), this module runs
//! them concurrently, each on its own fork of the state, and merges what
//! they changed:
//!
//! 1. **fan out / join** -- every branch is started, then every branch is
//!    waited for, in order, before anything is read: Temporal's branch
//!    sample (temporalio/samples-go `branch/workflow.go`,
//!    `SampleBranchWorkflow`: `ExecuteActivity` in a loop into `futures`,
//!    then `future.Get` over the same slice), which is also Dagster's
//!    `DynamicOut` contract (`map` clones the downstream op per output,
//!    `collect` gathers the results into one list;
//!    `python_modules/dagster/dagster/_core/definitions/output.py`).
//! 2. **each result is a JSON Patch (RFC 6902)** against the base design,
//!    computed by diffing the fork's design against the base, so a
//!    specialist's edits are a list of `add`/`remove`/`replace` ops on JSON
//!    Pointers, not a whole new document. The op types, the atomic
//!    `apply` with an undo stack, and `diff` follow idubrov/json-patch
//!    3.0.1 (`src/lib.rs`: `PatchOperation`, `add`/`remove`/`replace`/`mov`/
//!    `copy`/`test`, `patch` + `undo_patches`; `src/diff.rs`: `diff_impl`,
//!    `diff_array`, `diff_object`). Ported rather than depended on: the
//!    crate is not in the shared `Cargo.lock`, and six other sessions build
//!    this tree at once; the port is ~150 lines. RFC 6902 §5: if one op
//!    fails the whole patch is not applied.
//! 3. **conflict detection**: two patches conflict when an op of one touches
//!    the same pointer as an op of the other, or an ancestor of it (`/a`
//!    vs `/a/b`), or when both touch the same array with an index-shifting
//!    op (`add`/`remove`/`move` of an element).
//! 4. **merge**: with no conflict every patch is applied to the base
//!    together and the deterministic checks re-run on the result; if that is
//!    worse than the best single fork (a *check regression*: two edits that
//!    each pass but fail together), or if there was a conflict, fall back to
//!    sequential application ordered by [`better`]: the best-scoring fork
//!    is applied whole, each later fork contributes only the ops that do not
//!    conflict with what is already applied, and a fork whose ops would make
//!    the checks worse is refused. Every refused op is named in the tool
//!    result so the lead can re-delegate it.
//! 5. **versions**: the design carries a counter ([`DesignState::version`]);
//!    a fork records the version it was made from and a result against an
//!    older version is refused as stale -- Kubernetes' optimistic
//!    concurrency on `metadata.resourceVersion` (the update precondition in
//!    `k8s.io/apiserver/pkg/registry/rest/update.go`), not a lock.
//!
//! Which roles may run together is [`depends_on`]: roles the table does
//! not relate are independent. `cad` depends on everything (it reconciles
//! CAD with the resolved design) and never runs in parallel.

use crate::agent::{run_agent, AgentEnd, AgentError, Conversation, ModelCfg, ToolOutput};
use crate::client::{CallError, Client};
use crate::harness::{emit_checks, Specialist, Workbench};
use crate::prompts;
use crate::state::{better, DesignState};
use crate::tools::DomainTools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use wright_core::Event;

// ------------------------------------------------------------ RFC 6902

/// One JSON Patch operation (RFC 6902 §4), serialised as the RFC's object
/// (`{"op": "replace", "path": "/a/b", "value": 1}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Add { path: String, value: Value },
    Remove { path: String },
    Replace { path: String, value: Value },
    Move { from: String, path: String },
    Copy { from: String, path: String },
    Test { path: String, value: Value },
}

impl Op {
    pub fn path(&self) -> &str {
        match self {
            Op::Add { path, .. } | Op::Remove { path } | Op::Replace { path, .. } | Op::Move { path, .. } | Op::Copy { path, .. } | Op::Test { path, .. } => path,
        }
    }
    fn from(&self) -> Option<&str> {
        match self {
            Op::Move { from, .. } | Op::Copy { from, .. } => Some(from),
            _ => None,
        }
    }
    fn name(&self) -> &'static str {
        match self {
            Op::Add { .. } => "add",
            Op::Remove { .. } => "remove",
            Op::Replace { .. } => "replace",
            Op::Move { .. } => "move",
            Op::Copy { .. } => "copy",
            Op::Test { .. } => "test",
        }
    }
    /// Every pointer this op reads or writes.
    fn footprint(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path()).chain(self.from())
    }
    /// An op that shifts the indices of its siblings in an array.
    fn shifts(&self) -> bool {
        let last_is_index = |p: &str| p.rsplit('/').next().is_some_and(|s| s == "-" || s.parse::<usize>().is_ok());
        match self {
            Op::Add { path, .. } | Op::Remove { path } => last_is_index(path),
            Op::Move { from, path } => last_is_index(from) || last_is_index(path),
            _ => false,
        }
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap_or_default())
    }
}

/// A JSON Patch document: an array of operations applied in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Patch(pub Vec<Op>);

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PatchErrorKind {
    #[error("value did not match")]
    TestFailed,
    #[error("\"from\" path is invalid")]
    InvalidFromPointer,
    #[error("path is invalid")]
    InvalidPointer,
    #[error("cannot move the value inside itself")]
    CannotMoveInsideItself,
}

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("operation {operation} ({op}) failed at path '{path}': {kind}")]
pub struct PatchError {
    pub operation: usize,
    pub op: &'static str,
    pub path: String,
    pub kind: PatchErrorKind,
}

fn unescape(s: &str) -> Cow<'_, str> {
    if s.contains('~') {
        Cow::Owned(s.replace("~1", "/").replace("~0", "~"))
    } else {
        Cow::Borrowed(s)
    }
}

fn parse_index(s: &str, len: usize) -> Result<usize, PatchErrorKind> {
    if (s.starts_with('0') && s.len() != 1) || s.starts_with('+') {
        return Err(PatchErrorKind::InvalidPointer);
    }
    match s.parse::<usize>() {
        Ok(i) if i < len => Ok(i),
        _ => Err(PatchErrorKind::InvalidPointer),
    }
}

fn split_pointer(p: &str) -> Result<(&str, &str), PatchErrorKind> {
    p.rfind('/').ok_or(PatchErrorKind::InvalidPointer).map(|i| (&p[..i], &p[i + 1..]))
}

fn add(doc: &mut Value, path: &str, value: Value) -> Result<Option<Value>, PatchErrorKind> {
    if path.is_empty() {
        return Ok(Some(std::mem::replace(doc, value)));
    }
    let (parent, last) = split_pointer(path)?;
    let parent = doc.pointer_mut(parent).ok_or(PatchErrorKind::InvalidPointer)?;
    match parent {
        Value::Object(o) => Ok(o.insert(unescape(last).into_owned(), value)),
        Value::Array(a) if last == "-" => {
            a.push(value);
            Ok(None)
        }
        Value::Array(a) => {
            let i = parse_index(last, a.len() + 1)?;
            a.insert(i, value);
            Ok(None)
        }
        _ => Err(PatchErrorKind::InvalidPointer),
    }
}

fn remove(doc: &mut Value, path: &str, allow_last: bool) -> Result<Value, PatchErrorKind> {
    let (parent, last) = split_pointer(path)?;
    let parent = doc.pointer_mut(parent).ok_or(PatchErrorKind::InvalidPointer)?;
    match parent {
        Value::Object(o) => o.remove(unescape(last).as_ref()).ok_or(PatchErrorKind::InvalidPointer),
        Value::Array(a) if allow_last && last == "-" => a.pop().ok_or(PatchErrorKind::InvalidPointer),
        Value::Array(a) => {
            let i = parse_index(last, a.len())?;
            Ok(a.remove(i))
        }
        _ => Err(PatchErrorKind::InvalidPointer),
    }
}

fn replace(doc: &mut Value, path: &str, value: Value) -> Result<Value, PatchErrorKind> {
    let t = doc.pointer_mut(path).ok_or(PatchErrorKind::InvalidPointer)?;
    Ok(std::mem::replace(t, value))
}

fn mov(doc: &mut Value, from: &str, path: &str, allow_last: bool) -> Result<Option<Value>, PatchErrorKind> {
    if path.starts_with(from) && path[from.len()..].starts_with('/') {
        return Err(PatchErrorKind::CannotMoveInsideItself);
    }
    let v = remove(doc, from, allow_last).map_err(|e| match e {
        PatchErrorKind::InvalidPointer => PatchErrorKind::InvalidFromPointer,
        e => e,
    })?;
    add(doc, path, v)
}

fn copy(doc: &mut Value, from: &str, path: &str) -> Result<Option<Value>, PatchErrorKind> {
    let v = doc.pointer(from).ok_or(PatchErrorKind::InvalidFromPointer)?.clone();
    add(doc, path, v)
}

fn test(doc: &Value, path: &str, expected: &Value) -> Result<(), PatchErrorKind> {
    let t = doc.pointer(path).ok_or(PatchErrorKind::InvalidPointer)?;
    if t == expected {
        Ok(())
    } else {
        Err(PatchErrorKind::TestFailed)
    }
}

/// Apply `ops` to `doc` in order. Atomic (RFC 6902 §5): if any op fails
/// the ones already applied are undone and `doc` is as it was.
pub fn apply(doc: &mut Value, ops: &[Op]) -> Result<(), PatchError> {
    let mut undo = Vec::with_capacity(ops.len());
    if let Err(e) = apply_ops(doc, ops, Some(&mut undo)) {
        for op in undo.iter().rev() {
            // inverse ops of ops that applied: cannot fail
            let _ = apply_ops(doc, std::slice::from_ref(op), None);
        }
        return Err(e);
    }
    Ok(())
}

fn apply_ops(doc: &mut Value, ops: &[Op], mut undo: Option<&mut Vec<Op>>) -> Result<(), PatchError> {
    for (i, op) in ops.iter().enumerate() {
        let err = |kind: PatchErrorKind| PatchError { operation: i, op: op.name(), path: op.path().to_string(), kind };
        match op {
            Op::Add { path, value } => {
                let prev = add(doc, path, value.clone()).map_err(err)?;
                if let Some(u) = undo.as_deref_mut() {
                    u.push(match prev {
                        None => Op::Remove { path: path.clone() },
                        Some(v) => Op::Add { path: path.clone(), value: v },
                    });
                }
            }
            Op::Remove { path } => {
                let prev = remove(doc, path, false).map_err(err)?;
                if let Some(u) = undo.as_deref_mut() {
                    u.push(Op::Add { path: path.clone(), value: prev });
                }
            }
            Op::Replace { path, value } => {
                let prev = replace(doc, path, value.clone()).map_err(err)?;
                if let Some(u) = undo.as_deref_mut() {
                    u.push(Op::Replace { path: path.clone(), value: prev });
                }
            }
            Op::Move { from, path } => {
                let prev = mov(doc, from, path, false).map_err(err)?;
                if let Some(u) = undo.as_deref_mut() {
                    // undone in reverse: move back first, then restore what was overwritten
                    if let Some(v) = prev {
                        u.push(Op::Add { path: path.clone(), value: v });
                    }
                    u.push(Op::Move { from: path.clone(), path: from.clone() });
                }
            }
            Op::Copy { from, path } => {
                let prev = copy(doc, from, path).map_err(err)?;
                if let Some(u) = undo.as_deref_mut() {
                    u.push(match prev {
                        None => Op::Remove { path: path.clone() },
                        Some(v) => Op::Add { path: path.clone(), value: v },
                    });
                }
            }
            Op::Test { path, value } => test(doc, path, value).map_err(err)?,
        }
    }
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// The JSON Patch that turns `left` into `right` (json-patch `diff.rs`):
/// objects recurse and add/remove keys, arrays recurse by index with
/// trailing elements added or removed, anything else that differs is
/// replaced.
pub fn diff(left: &Value, right: &Value) -> Patch {
    let mut p = Patch::default();
    diff_impl(left, right, &mut String::new(), &mut p);
    p
}

fn diff_impl(left: &Value, right: &Value, at: &mut String, p: &mut Patch) {
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => diff_object(l, r, at, p),
        (Value::Array(l), Value::Array(r)) => diff_array(l, r, at, p),
        _ if left == right => {}
        _ => p.0.push(Op::Replace { path: at.clone(), value: right.clone() }),
    }
}

fn diff_array(left: &[Value], right: &[Value], at: &mut String, p: &mut Patch) {
    let len = left.len().max(right.len());
    let mut shift = 0usize;
    for i in 0..len {
        let keep = at.len();
        at.push_str(&format!("/{}", i - shift));
        match (left.get(i), right.get(i)) {
            (Some(l), Some(r)) => diff_impl(l, r, at, p),
            (Some(_), None) => {
                shift += 1;
                p.0.push(Op::Remove { path: at.clone() });
            }
            (None, Some(r)) => p.0.push(Op::Add { path: at.clone(), value: r.clone() }),
            (None, None) => unreachable!(),
        }
        at.truncate(keep);
    }
}

fn diff_object(left: &Map<String, Value>, right: &Map<String, Value>, at: &mut String, p: &mut Patch) {
    for (k, rv) in right {
        let keep = at.len();
        at.push('/');
        at.push_str(&escape(k));
        match left.get(k) {
            Some(lv) => diff_impl(lv, rv, at, p),
            None => p.0.push(Op::Add { path: at.clone(), value: rv.clone() }),
        }
        at.truncate(keep);
    }
    for k in left.keys() {
        if !right.contains_key(k) {
            p.0.push(Op::Remove { path: format!("{at}/{}", escape(k)) });
        }
    }
}

// ------------------------------------------------------------ conflicts

/// `a` and `b` are the same pointer or one is an ancestor of the other.
pub fn touches(a: &str, b: &str) -> bool {
    let under = |outer: &str, inner: &str| inner.starts_with(outer) && (outer.is_empty() || inner[outer.len()..].starts_with('/'));
    a == b || under(a, b) || under(b, a)
}

fn parent(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

/// Two ops conflict when their footprints touch, or when one shifts the
/// indices of an array the other addresses an element of.
pub fn conflicts(a: &Op, b: &Op) -> bool {
    if a.footprint().any(|x| b.footprint().any(|y| touches(x, y))) {
        return true;
    }
    let same_array = |shifter: &Op, other: &Op| {
        shifter.shifts() && shifter.footprint().any(|s| other.footprint().any(|o| parent(s) == parent(o) || touches(parent(s), o)))
    };
    same_array(a, b) || same_array(b, a)
}

/// The first pair of conflicting ops between two patches.
pub fn first_conflict<'a>(a: &'a [Op], b: &'a [Op]) -> Option<(&'a Op, &'a Op)> {
    a.iter().find_map(|x| b.iter().find(|y| conflicts(x, y)).map(|y| (x, y)))
}

// ---------------------------------------------------------------- merge

pub type Score = (usize, Option<f64>);

/// One specialist's result: its edits as a patch against design version
/// `base_version`, and how its own design scored under the checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fork {
    pub role: String,
    pub base_version: u64,
    pub patch: Patch,
    /// `(failing, objective)` of the fork's design, `None` if it could not
    /// be checked.
    pub score: Option<Score>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Resolution {
    /// Nothing to apply.
    Empty,
    /// Every patch applied together, checks no worse than the best fork.
    Together,
    /// Sequential fallback, with why.
    Sequential(String),
}

/// What the merge did, for the tool results and the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReport {
    pub resolution: Resolution,
    pub version: u64,
    /// Roles whose patch (or part of it) is in the result, in the order applied.
    pub applied: Vec<String>,
    /// `(role, why)` for forks refused whole.
    pub refused: Vec<(String, String)>,
    /// `(role, op, why)` for single ops dropped in the sequential fallback.
    pub dropped: Vec<(String, Op, String)>,
    pub score: Option<Score>,
}

impl MergeReport {
    pub fn refused_role(&self, role: &str) -> Option<&str> {
        self.refused.iter().find(|(r, _)| r == role).map(|(_, w)| w.as_str())
    }
    /// One paragraph for a tool result.
    pub fn describe(&self) -> String {
        let fmt = |s: Option<Score>| match s {
            Some((f, o)) => format!("{f} failing, objective {}", o.map_or("n/a".into(), |v| format!("{v:.3}"))),
            None => "unchecked".into(),
        };
        let mut out = match &self.resolution {
            Resolution::Empty => "merge: no design edits to merge".to_string(),
            Resolution::Together => format!("merge: edits from {} applied together (no conflict; {}); design version {}", self.applied.join(" and "), fmt(self.score), self.version),
            Resolution::Sequential(why) => format!(
                "merge: {why}; applied sequentially in order of the better checks: {} ({}); design version {}",
                self.applied.join(", then "),
                fmt(self.score),
                self.version
            ),
        };
        for (role, op, why) in &self.dropped {
            out.push_str(&format!("\n  dropped {role}'s {op}: {why}; re-delegate it if it is still needed"));
        }
        for (role, why) in &self.refused {
            out.push_str(&format!("\n  refused all of {role}'s edits: {why}"));
        }
        out
    }
}

fn fmt_score(s: Option<Score>) -> String {
    match s {
        Some((f, o)) => format!("{f} failing, objective {}", o.map_or("n/a".into(), |v| format!("{v:.3}"))),
        None => "unchecked".into(),
    }
}

/// Rank: the strictly better fork first; unscored forks last; ties keep
/// input order.
fn ranked(forks: &[Fork]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..forks.len()).collect();
    idx.sort_by(|&a, &b| {
        use std::cmp::Ordering::*;
        match (forks[a].score, forks[b].score) {
            (Some(x), Some(y)) => {
                if better(x, y) {
                    Less
                } else if better(y, x) {
                    Greater
                } else {
                    Equal
                }
            }
            (Some(_), None) => Less,
            (None, Some(_)) => Greater,
            (None, None) => Equal,
        }
    });
    idx
}

/// Merge `forks` into `base` (at `base_version`); `score` is the
/// deterministic check of a candidate design (`None`: it does not even
/// validate). Returns the merged design and the report; the design is
/// `base` unchanged when nothing could be applied.
pub fn merge(base: &Value, base_version: u64, forks: &[Fork], score: &dyn Fn(&Value) -> Option<Score>) -> (Value, MergeReport) {
    let mut report = MergeReport { resolution: Resolution::Empty, version: base_version, applied: vec![], refused: vec![], dropped: vec![], score: None };
    // stale forks are refused before anything else is looked at
    let live: Vec<&Fork> = forks
        .iter()
        .filter(|f| {
            if f.base_version != base_version {
                report.refused.push((f.role.clone(), format!("stale: made against design version {}, the design is at version {base_version}", f.base_version)));
                return false;
            }
            if f.patch.0.is_empty() {
                return false;
            }
            true
        })
        .collect();
    if live.is_empty() {
        report.score = score(base);
        return (base.clone(), report);
    }
    let owned: Vec<Fork> = live.iter().map(|f| (*f).clone()).collect();
    let order = ranked(&owned);
    let ordered: Vec<&Fork> = order.iter().map(|&i| &owned[i]).collect();

    // conflicts between any pair
    let mut conflict = None;
    'outer: for (i, a) in ordered.iter().enumerate() {
        for b in &ordered[i + 1..] {
            if let Some((x, y)) = first_conflict(&a.patch.0, &b.patch.0) {
                conflict = Some(format!("conflict: {} {} and {} {} touch the same part of the design", a.role, x, b.role, y));
                break 'outer;
            }
        }
    }

    if conflict.is_none() {
        let mut cand = base.clone();
        let mut ok = true;
        for f in &ordered {
            if let Err(e) = apply(&mut cand, &f.patch.0) {
                conflict = Some(format!("{}'s patch did not apply on top of the others ({e})", f.role));
                ok = false;
                break;
            }
        }
        if ok {
            let merged = score(&cand);
            let best = ordered[0].score;
            let regressed = match (merged, best) {
                (None, _) => true,
                (Some(m), Some(b)) => better(b, m),
                (Some(_), None) => false,
            };
            if !regressed {
                report.resolution = Resolution::Together;
                report.applied = ordered.iter().map(|f| f.role.clone()).collect();
                report.version = base_version + 1;
                report.score = merged;
                return (cand, report);
            }
            conflict = Some(format!(
                "check regression: the edits applied together score {} but {} alone scores {}",
                fmt_score(merged),
                ordered[0].role,
                fmt_score(best)
            ));
        }
    }

    // sequential fallback
    let why = conflict.unwrap_or_default();
    let mut cur = base.clone();
    let mut cur_score = score(base);
    let mut applied_ops: Vec<Op> = vec![];
    for f in &ordered {
        let mut ops = vec![];
        for op in &f.patch.0 {
            match applied_ops.iter().find(|a| conflicts(a, op)) {
                Some(a) => report.dropped.push((f.role.clone(), op.clone(), format!("conflicts with the already-applied {a}"))),
                None => ops.push(op.clone()),
            }
        }
        if ops.is_empty() {
            report.refused.push((f.role.clone(), "every op conflicted with edits already applied".into()));
            continue;
        }
        let mut cand = cur.clone();
        if let Err(e) = apply(&mut cand, &ops) {
            report.refused.push((f.role.clone(), format!("its patch no longer applies: {e}")));
            continue;
        }
        let sc = score(&cand);
        let worse = match (cur_score, sc) {
            (_, None) => true,
            (Some(c), Some(s)) => better(c, s),
            (None, Some(_)) => false,
        };
        if worse {
            report.refused.push((f.role.clone(), format!("it would make the checks worse ({} -> {})", fmt_score(cur_score), fmt_score(sc))));
            continue;
        }
        cur = cand;
        cur_score = sc;
        applied_ops.extend(ops);
        report.applied.push(f.role.clone());
    }
    report.resolution = Resolution::Sequential(why);
    report.score = cur_score;
    if !report.applied.is_empty() {
        report.version = base_version + 1;
    }
    (cur, report)
}

// ----------------------------------------------------------------- roles

/// Roles whose work depends on another role's result, per domain, keyed by
/// the names in [`prompts::roles`]. Pairs the table does not relate are
/// independent and may run concurrently. `cad` is downstream of everything
/// (it reconciles CAD with the resolved design); `sourcing` prices parts
/// others chose; `print` orients parts structures and CAD defined.
pub fn depends_on(domain: &str, role: &str) -> &'static [&'static str] {
    match (domain, role) {
        ("aircraft", "cad") => &["aero", "structures", "electronics", "sourcing", "print"],
        ("aircraft", "structures") => &["aero"],
        ("aircraft", "sourcing") => &["aero", "structures", "electronics", "print"],
        ("aircraft", "print") => &["structures"],
        ("rocket", "cad") => &["propulsion", "engine", "structures", "recovery"],
        ("rocket", "engine") => &["propulsion"],
        ("rocket", "structures") => &["propulsion", "engine"],
        ("rocket", "recovery") => &["structures"],
        ("balloon", "cad") => &["envelope", "payload", "tracking"],
        ("balloon", "envelope") => &["payload"],
        ("balloon", "tracking") => &["payload"],
        ("electronics", "layout") => &["circuit"],
        ("electronics", "verification") => &["circuit", "layout"],
        ("electronics", "sourcing") => &["circuit"],
        _ => &[],
    }
}

/// Neither role depends on the other.
pub fn independent(domain: &str, a: &str, b: &str) -> bool {
    a != b && !depends_on(domain, a).contains(&b) && !depends_on(domain, b).contains(&a)
}

/// Why `roles` cannot run together, or `None`: a role twice, `cad`
/// (downstream of everything, and it holds the one CAD session), or a
/// dependent pair.
pub fn cannot_run_together(domain: &str, roles: &[&str]) -> Option<String> {
    for (i, a) in roles.iter().enumerate() {
        if *a == "cad" {
            return Some("cad reconciles the others' work and runs on its own".into());
        }
        for b in &roles[i + 1..] {
            if a == b {
                return Some(format!("{a} is delegated twice in one turn"));
            }
            if !independent(domain, a, b) {
                return Some(format!("{a} and {b} depend on each other's work"));
            }
        }
    }
    None
}

// ------------------------------------------------------------- fan out

/// One `delegate` call of the batch.
#[derive(Debug, Clone)]
pub struct Branch {
    pub role: String,
    pub task: String,
}

/// What the fan-out needs from the orchestrator.
pub struct FanOut<'s> {
    pub domain: Arc<dyn DomainTools>,
    pub client: Client,
    pub model: ModelCfg,
    pub turns: u32,
    pub state: &'s mut DesignState,
    /// Where throwaway CAD sessions for the forks live (unused by non-CAD
    /// roles; the session starts no worker until an action is called).
    pub cad_dir: PathBuf,
    /// Appended to every specialist system prompt (`crate::memory`).
    pub lessons: String,
    /// The run directory, for the workbench.
    pub out_dir: Option<PathBuf>,
}

/// The join's result: one tool output per branch, in the branches' order,
/// and a transport error that must end the run if a branch hit one.
pub struct Joined {
    pub outputs: Vec<ToolOutput>,
    pub fatal: Option<CallError>,
    pub report: MergeReport,
    /// Every branch's edits, role-prefixed, for the lead's edit log.
    pub edits: Vec<String>,
}

struct BranchEnd {
    role: String,
    end: Result<AgentEnd, AgentError>,
    report: Option<String>,
    edits: Vec<String>,
    state: DesignState,
}

/// Poll every future until all are done; results in input order (the
/// `futures` loop then the `Get` loop of Temporal's branch sample, as one
/// future). Hand-rolled: `futures-util` is not a direct dependency.
struct JoinAll<'a, T> {
    futs: Vec<Option<Pin<Box<dyn Future<Output = T> + Send + 'a>>>>,
    done: Vec<Option<T>>,
}

impl<T: Unpin> Future for JoinAll<'_, T> {
    type Output = Vec<T>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Vec<T>> {
        let this = &mut *self;
        let mut pending = false;
        for (i, slot) in this.futs.iter_mut().enumerate() {
            if let Some(f) = slot {
                match f.as_mut().poll(cx) {
                    Poll::Ready(v) => {
                        this.done[i] = Some(v);
                        *slot = None;
                    }
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(this.done.iter_mut().map(|d| d.take().expect("every branch finished")).collect())
        }
    }
}

/// Run `branches` concurrently on forks of the current state and merge
/// their design edits back. The caller has checked [`cannot_run_together`]
/// and that a design exists.
pub async fn fan_out(ctx: FanOut<'_>, branches: &[Branch]) -> Joined {
    let FanOut { domain, client, model, turns, state, cad_dir, lessons, out_dir } = ctx;
    let base_version = state.version;
    let base_design = state.design.clone().unwrap_or(Value::Null);
    let base_notes = state.notes.len();
    let roles: Vec<&str> = branches.iter().map(|b| b.role.as_str()).collect();
    let _ = client.events.send(Event::AgentMessage {
        run: client.run.clone(),
        agent: "harness".into(),
        text: format!("running {} in parallel on design version {base_version}; their edits are merged when all have reported", roles.join(" and ")),
    });

    let mut futs: Vec<Pin<Box<dyn Future<Output = BranchEnd> + Send + '_>>> = vec![];
    for b in branches {
        let others: Vec<&str> = roles.iter().copied().filter(|r| *r != b.role).collect();
        let text = format!(
            "Task from the lead: {}\n\nYou are working at the same time as the {} specialist(s) on the same design; change only the fields your role owns, because edits to the same field conflict and the harness keeps the better-checked one.\n\n{}",
            b.task,
            others.join(", "),
            crate::slim::specialist_prompt(state)
        );
        let fork = state.clone();
        let client = client.clone();
        let domain = domain.clone();
        let model = model.clone();
        let role = b.role.clone();
        let dir = cad_dir.join(format!("fork-{role}"));
        let system = format!("{}{lessons}", prompts::specialist_system(domain.as_ref(), &role));
        let out_dir = out_dir.clone();
        futs.push(Box::pin(async move {
            let mut conv = Conversation::new(format!("specialist:{role}"), model, system, prompts::specialist_tools_for(domain.name(), &role));
            let mut st = fork;
            let mut cad = wright_cad::Session::new(dir);
            let mut sp = Specialist {
                wb: Workbench { cad: &mut cad, domain, state: &mut st, client: client.clone(), edits: vec![], may_edit_spec: false, out_dir },
                report: None,
                visuals: None,
            };
            let end = run_agent(&client, &mut conv, vec![json!({"type": "text", "text": text})], &mut sp, turns).await;
            let edits = std::mem::take(&mut sp.wb.edits);
            let report = sp.report.take();
            drop(sp);
            BranchEnd { role, end, report, edits, state: st }
        }));
    }
    let n = futs.len();
    let ends = JoinAll { futs: futs.into_iter().map(Some).collect(), done: (0..n).map(|_| None).collect() }.await;

    // each branch's summary and patch
    let mut ends = ends;
    let mut fatal: Option<CallError> = None;
    let mut stopped = vec![false; ends.len()];
    let mut summaries = vec![];
    let mut forks = vec![];
    for (i, e) in ends.iter_mut().enumerate() {
        let ok = e.end.is_ok();
        let design = e.state.design.clone().unwrap_or(Value::Null);
        forks.push(Fork {
            role: e.role.clone(),
            base_version,
            patch: diff(&base_design, &design),
            score: if ok { state.score_of(domain.as_ref(), &design) } else { None },
        });
        // a transport error is moved out whole (CallError is not Clone) so
        // the run can stop for it
        let summary = match std::mem::replace(&mut e.end, Ok(AgentEnd::TurnLimit)) {
            Ok(AgentEnd::Done { text }) | Ok(AgentEnd::Stopped { text }) => Ok(e.report.clone().unwrap_or(text)),
            Ok(AgentEnd::TurnLimit) => Ok(e.report.clone().unwrap_or_else(|| format!("{} hit its {turns}-turn limit without reporting", e.role))),
            Err(AgentError::Call(c)) => {
                stopped[i] = true;
                let msg = format!("{} stopped: {c}", e.role);
                if fatal.is_none() {
                    fatal = Some(c);
                }
                Err(msg)
            }
            Err(err) => Err(format!("{} failed: {err}", e.role)),
        };
        summaries.push(summary);
    }

    let (merged, report) = merge(&base_design, base_version, &forks, &|d| state.score_of(domain.as_ref(), d));

    // every fork's verified design is offered as the incumbent too
    for (f, e) in forks.iter().zip(&ends) {
        if f.score.is_some() {
            if let Some(d) = &e.state.design {
                if let Ok(fin) = state.full_check_of(domain.as_ref(), d) {
                    let saved = state.design.replace(d.clone());
                    state.offer(domain.as_ref(), &fin);
                    state.design = saved;
                }
            }
        }
    }
    if report.version != base_version {
        state.design = Some(merged);
        state.version = report.version;
    }
    for e in &ends {
        state.notes.extend(e.state.notes.iter().skip(base_notes).cloned());
    }
    for (e, s) in ends.iter().zip(&summaries) {
        let s = match s {
            Ok(s) | Err(s) => s,
        };
        state.notes.push(format!("{}: {}", e.role, crate::harness::truncate(s.trim(), crate::harness::SUMMARY_CHARS)));
    }
    let _ = client.events.send(Event::AgentMessage { run: client.run.clone(), agent: "harness".into(), text: report.describe() });

    let status = match state.full_check(domain.as_ref()) {
        Ok(fin) => {
            emit_checks(&client, domain.as_ref(), state, &fin);
            state.offer(domain.as_ref(), &fin);
            let failing: Vec<&str> = fin.failures().map(|o| o.name.as_str()).collect();
            let s = format!("checks now: {} of {} failing {failing:?}", failing.len(), fin.outcomes.len());
            state.report = Some(fin);
            s
        }
        Err(e) => format!("checks could not run on the merged design: {e:#}"),
    };

    let edits: Vec<String> = ends.iter().flat_map(|e| e.edits.iter().map(move |x| format!("{}: {x}", e.role))).collect();
    let outputs = ends
        .iter()
        .zip(&summaries)
        .zip(&stopped)
        .map(|((e, s), stop)| {
            let edits = &e.edits;
            let mut text = match s {
                Ok(s) => format!("{} summary: {}\nedits: {edits:?}\n", e.role, crate::harness::truncate(s.trim(), crate::harness::SUMMARY_CHARS)),
                Err(s) => format!("{s}\nedits: {edits:?}\n"),
            };
            text.push_str(&report.describe());
            text.push('\n');
            text.push_str(&status);
            let refused = report.refused_role(&e.role).is_some() || report.dropped.iter().any(|(r, _, _)| *r == e.role);
            let mut out = if s.is_err() || refused { ToolOutput::err(text) } else { ToolOutput::ok(text) };
            if *stop {
                out = out.stop();
            }
            out
        })
        .collect();
    Joined { outputs, fatal, report, edits }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ApiError, BoxFut, Provider, Request, Transport};
    use crate::fake::{reply, tool_use, ScriptedTransport};
    use crate::harness::{run, RunConfig};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wright_core::{Artifacts, Bom, CheckId, Domain, Finished, Report, Source};

    fn ops(v: Value) -> Vec<Op> {
        serde_json::from_value::<Patch>(v).unwrap().0
    }

    // RFC 6902 appendix A examples, through the json-patch port
    #[test]
    fn rfc6902_appendix_a() {
        let mut d = json!({"foo": "bar"});
        apply(&mut d, &ops(json!([{"op": "add", "path": "/baz", "value": "qux"}]))).unwrap();
        assert_eq!(d, json!({"baz": "qux", "foo": "bar"}));
        let mut d = json!({"foo": ["bar", "baz"]});
        apply(&mut d, &ops(json!([{"op": "add", "path": "/foo/1", "value": "qux"}]))).unwrap();
        assert_eq!(d, json!({"foo": ["bar", "qux", "baz"]}));
        let mut d = json!({"baz": "qux", "foo": "bar"});
        apply(&mut d, &ops(json!([{"op": "remove", "path": "/baz"}]))).unwrap();
        assert_eq!(d, json!({"foo": "bar"}));
        let mut d = json!({"foo": {"bar": "baz", "waldo": "fred"}, "qux": {"corge": "grault"}});
        apply(&mut d, &ops(json!([{"op": "move", "from": "/foo/waldo", "path": "/qux/thud"}]))).unwrap();
        assert_eq!(d, json!({"foo": {"bar": "baz"}, "qux": {"corge": "grault", "thud": "fred"}}));
        let mut d = json!({"foo": ["all", "grass", "cows", "eat"]});
        apply(&mut d, &ops(json!([{"op": "move", "from": "/foo/1", "path": "/foo/3"}]))).unwrap();
        assert_eq!(d, json!({"foo": ["all", "cows", "eat", "grass"]}));
        let mut d = json!({"/": 9, "~1": 10});
        apply(&mut d, &ops(json!([{"op": "test", "path": "/~01", "value": 10}]))).unwrap();
        let mut d = json!({"foo": "bar"});
        apply(&mut d, &ops(json!([{"op": "add", "path": "/child", "value": {"grandchild": {}}}]))).unwrap();
        assert_eq!(d["child"]["grandchild"], json!({}));
    }

    // RFC 6902 section 5: one failing op, nothing applied
    #[test]
    fn a_failed_op_leaves_the_document_unchanged() {
        let before = json!({"a": 1, "b": [1, 2]});
        let mut d = before.clone();
        let e = apply(&mut d, &ops(json!([{"op": "replace", "path": "/a", "value": 2}, {"op": "add", "path": "/b/0", "value": 0}, {"op": "test", "path": "/a", "value": 1}]))).unwrap_err();
        assert_eq!(e.kind, PatchErrorKind::TestFailed);
        assert_eq!(e.operation, 2);
        assert_eq!(d, before);
        let e = apply(&mut d, &ops(json!([{"op": "move", "from": "/b", "path": "/b/0"}]))).unwrap_err();
        assert_eq!(e.kind, PatchErrorKind::CannotMoveInsideItself);
        assert_eq!(d, before);
    }

    #[test]
    fn diff_matches_json_patch_example_and_round_trips() {
        let left = json!({"title": "Goodbye!", "author": {"givenName": "John", "familyName": "Doe"}, "tags": ["example", "sample"], "content": "This will be unchanged"});
        let right = json!({"title": "Hello!", "author": {"givenName": "John"}, "tags": ["example"], "content": "This will be unchanged", "phoneNumber": "+01-123-456-7890"});
        let p = diff(&left, &right);
        // the same four ops as json-patch's doc example; serde_json's Map is
        // sorted here, so keys come out alphabetically rather than in
        // insertion order
        let expected: Patch = serde_json::from_value(json!([
            {"op": "replace", "path": "/title", "value": "Hello!"},
            {"op": "remove", "path": "/author/familyName"},
            {"op": "remove", "path": "/tags/1"},
            {"op": "add", "path": "/phoneNumber", "value": "+01-123-456-7890"},
        ]))
        .unwrap();
        assert_eq!(p.0.len(), 4);
        for op in &expected.0 {
            assert!(p.0.contains(op), "{p:?} lacks {op}");
        }
        let mut d = left.clone();
        apply(&mut d, &p.0).unwrap();
        assert_eq!(d, right);
        let l = json!({"a": [1, 2, 3, 4], "k/ey": 1});
        let r = json!({"a": [1, 9], "k/ey": 2});
        let mut d = l.clone();
        apply(&mut d, &diff(&l, &r).0).unwrap();
        assert_eq!(d, r);
        assert!(diff(&r, &r).0.is_empty());
    }

    #[test]
    fn conflicts_are_same_pointer_ancestor_or_shifted_array() {
        let rep = |p: &str| Op::Replace { path: p.into(), value: json!(1) };
        assert!(conflicts(&rep("/a/b"), &rep("/a/b")));
        assert!(conflicts(&rep("/a"), &rep("/a/b")));
        assert!(conflicts(&rep("/a/b/c"), &rep("/a")));
        assert!(!conflicts(&rep("/a/b"), &rep("/a/bc")));
        assert!(!conflicts(&rep("/spar_mm"), &rep("/pack_mah")));
        assert!(!conflicts(&rep("/parts/0/x"), &rep("/parts/1/x")));
        assert!(conflicts(&Op::Remove { path: "/parts/0".into() }, &rep("/parts/1/x")));
        assert!(conflicts(&Op::Add { path: "/parts/-".into(), value: json!(1) }, &rep("/parts/1")));
        assert!(!conflicts(&Op::Add { path: "/parts/-".into(), value: json!(1) }, &rep("/other/1")));
        assert!(conflicts(&Op::Move { from: "/a".into(), path: "/z".into() }, &rep("/a/x")));
        assert!(conflicts(&rep(""), &rep("/anything")));
    }

    // the check: a + b <= 10, objective a + b
    fn sum_score(d: &Value) -> Option<Score> {
        let a = d["a"].as_f64()?;
        let b = d["b"].as_f64()?;
        Some((usize::from(a + b > 10.0), Some(a + b)))
    }
    fn fork(role: &str, v: u64, base: &Value, to: Value) -> Fork {
        Fork { role: role.into(), base_version: v, patch: diff(base, &to), score: sum_score(&to) }
    }

    #[test]
    fn independent_patches_merge_together() {
        let base = json!({"a": 1, "b": 1, "c": 0});
        let forks = [fork("structures", 3, &base, json!({"a": 4, "b": 1, "c": 0})), fork("electronics", 3, &base, json!({"a": 1, "b": 3, "c": 0}))];
        let (d, r) = merge(&base, 3, &forks, &sum_score);
        assert_eq!(d, json!({"a": 4, "b": 3, "c": 0}));
        assert_eq!(r.resolution, Resolution::Together);
        assert_eq!(r.version, 4);
        assert_eq!(r.applied, ["structures", "electronics"]);
        assert!(r.refused.is_empty() && r.dropped.is_empty());
    }

    #[test]
    fn a_check_regression_falls_back_to_sequential_by_better() {
        let base = json!({"a": 1, "b": 1});
        // each alone passes (7, 8); together 6+7 = 13 fails
        let forks = [fork("structures", 0, &base, json!({"a": 6, "b": 1})), fork("electronics", 0, &base, json!({"a": 1, "b": 7}))];
        let (d, r) = merge(&base, 0, &forks, &sum_score);
        assert!(matches!(&r.resolution, Resolution::Sequential(w) if w.starts_with("check regression")), "{r:?}");
        // electronics (objective 8) ranks first by better(); structures would then fail the check
        assert_eq!(d, json!({"a": 1, "b": 7}));
        assert_eq!(r.applied, ["electronics"]);
        assert_eq!(r.refused.len(), 1);
        assert!(r.refused[0].1.contains("worse"), "{r:?}");
        assert_eq!(r.version, 1);
    }

    #[test]
    fn a_conflict_keeps_the_better_fork_and_the_others_non_conflicting_ops() {
        let base = json!({"a": 1, "b": 1, "c": 1});
        let forks = [
            fork("electronics", 5, &base, json!({"a": 2, "b": 5, "c": 1})), // objective 7
            fork("structures", 5, &base, json!({"a": 3, "b": 1, "c": 9})),  // objective 4, also edits c
        ];
        let (d, r) = merge(&base, 5, &forks, &sum_score);
        assert!(matches!(&r.resolution, Resolution::Sequential(w) if w.starts_with("conflict: electronics")), "{r:?}");
        assert_eq!(d, json!({"a": 2, "b": 5, "c": 9}));
        assert_eq!(r.applied, ["electronics", "structures"]);
        assert_eq!(r.dropped.len(), 1);
        assert_eq!(r.dropped[0].0, "structures");
        assert_eq!(r.dropped[0].1.path(), "/a");
        assert_eq!(r.version, 6);
    }

    #[test]
    fn a_stale_fork_is_refused() {
        let base = json!({"a": 1, "b": 1});
        let forks = [fork("structures", 1, &base, json!({"a": 2, "b": 1})), fork("electronics", 2, &base, json!({"a": 1, "b": 2}))];
        let (d, r) = merge(&base, 2, &forks, &sum_score);
        assert_eq!(d, json!({"a": 1, "b": 2}));
        assert_eq!(r.refused.len(), 1);
        assert_eq!(r.refused[0].0, "structures");
        assert!(r.refused[0].1.starts_with("stale"));
        assert_eq!(r.version, 3);
        let (d, r) = merge(&base, 9, &forks, &sum_score);
        assert_eq!(d, base);
        assert_eq!(r.version, 9);
        assert_eq!(r.resolution, Resolution::Empty);
    }

    #[test]
    fn dependency_table_names_real_roles() {
        for domain in ["aircraft", "rocket", "balloon", "electronics"] {
            let names: Vec<&str> = prompts::roles(domain).iter().map(|r| r.0).collect();
            for r in &names {
                for d in depends_on(domain, r) {
                    assert!(names.contains(d), "{domain}: {r} depends on unknown role {d}");
                    assert!(!depends_on(domain, d).contains(r), "{domain}: {r} <-> {d} cycle");
                }
            }
        }
        assert!(cannot_run_together("aircraft", &["structures", "electronics"]).is_none());
        assert!(cannot_run_together("aircraft", &["aero", "electronics", "print"]).is_none());
        assert!(cannot_run_together("aircraft", &["aero", "structures"]).is_some());
        assert!(cannot_run_together("aircraft", &["cad", "electronics"]).is_some());
        assert!(cannot_run_together("rocket", &["propulsion", "recovery"]).is_none());
        assert!(cannot_run_together("balloon", &["envelope", "tracking"]).is_none());
        assert!(cannot_run_together("balloon", &["payload", "payload"]).is_some());
        assert!(cannot_run_together("electronics", &["layout", "sourcing"]).is_none());
        assert!(cannot_run_together("electronics", &["circuit", "layout"]).is_some());
    }

    // ------------------------------------------------ scripted end to end

    struct Fixture;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum C {
        Spar,
        Pack,
        Mass,
    }
    impl CheckId for C {
        const ALL: &'static [Self] = &[C::Spar, C::Pack, C::Mass];
        fn name(self) -> &'static str {
            match self {
                C::Spar => "spar",
                C::Pack => "pack",
                C::Mass => "mass",
            }
        }
        fn describe(self) -> &'static str {
            "fixture"
        }
    }
    impl Domain for Fixture {
        const NAME: &'static str = "aircraft";
        type Spec = Value;
        type Design = Value;
        type Check = C;
        fn spec_schema() -> Value {
            json!({"type": "object"})
        }
        fn design(_: &Value) -> anyhow::Result<Value> {
            Ok(json!({"spar_mm": 8, "pack_mah": 1500, "mass_g": 1000}))
        }
        fn check(_: &Value, d: &Value) -> Finished {
            let mut r = Report::<C>::new();
            r.margin(C::Spar, d["spar_mm"].as_f64().unwrap_or(f64::NAN) - 10.0, "mm", Source::Physics, "");
            r.margin(C::Pack, d["pack_mah"].as_f64().unwrap_or(f64::NAN) - 2000.0, "mAh", Source::Rating, "");
            r.margin(C::Mass, 1500.0 - d["mass_g"].as_f64().unwrap_or(f64::NAN), "g", Source::Requirement, "");
            r.finish()
        }
        fn bom(_: &Value) -> Bom {
            Bom::default()
        }
        fn build(_: &Value, _: &std::path::Path) -> anyhow::Result<Artifacts> {
            Ok(Artifacts::default())
        }
        fn objective(d: &Value) -> (f64, &'static str) {
            (2000.0 - d["mass_g"].as_f64().unwrap_or(f64::NAN), "lightness")
        }
    }

    /// The scripted model with a fixed latency per call, counting how many
    /// calls are in flight at once.
    struct Slow {
        inner: ScriptedTransport,
        latency: std::time::Duration,
        inflight: AtomicUsize,
        max_inflight: AtomicUsize,
    }
    impl Transport for Slow {
        fn provider(&self) -> Provider {
            Provider::Fake
        }
        fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
            Box::pin(async move {
                let n = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_inflight.fetch_max(n, Ordering::SeqCst);
                tokio::time::sleep(self.latency).await;
                self.inflight.fetch_sub(1, Ordering::SeqCst);
                self.inner.send(req).await
            })
        }
    }

    /// Orchestrator: design, then delegate structures + electronics in one
    /// turn, then submit. Each specialist: one edit_design, then report.
    fn script(structures_patch: Value, electronics_patch: Value) -> ScriptedTransport {
        ScriptedTransport::from_fn(move |body| {
            let system = body["system"][0]["text"].as_str().unwrap_or("");
            let n = body["messages"].as_array().map_or(0, Vec::len);
            let content = if system.starts_with("You lead") {
                match n {
                    1 => vec![tool_use("t1", "design", json!({}))],
                    3 => vec![
                        tool_use("t2", "delegate", json!({"role": "structures", "task": "fix spar"})),
                        tool_use("t3", "delegate", json!({"role": "electronics", "task": "fix pack"})),
                    ],
                    _ => vec![tool_use("t4", "submit", json!({"summary": "done"}))],
                }
            } else {
                let patch = if system.contains("the structures specialist") { &structures_patch } else { &electronics_patch };
                match n {
                    1 => vec![tool_use("s1", "edit_design", json!({"patch_json": patch.to_string(), "why": "task"}))],
                    _ => vec![tool_use("s2", "report", json!({"summary": format!("applied {patch}")}))],
                }
            };
            Some(reply("claude-opus-5-5", content, 10, 10))
        })
    }

    async fn scripted_run(parallel: bool, structures: Value, electronics: Value, latency_ms: u64) -> (crate::harness::RunSummary, Arc<Slow>, std::time::Duration) {
        let transport = Arc::new(Slow {
            inner: script(structures, electronics),
            latency: std::time::Duration::from_millis(latency_ms),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
        });
        let (tx, _rx) = crate::channel();
        let client = Client::new(transport.clone(), crate::Budget::new(5., 45., None), tx, "merge-test");
        let mut cfg = RunConfig::new("merge-test", "a light plane", json!({}));
        cfg.max_rounds = 1;
        cfg.orchestrator_turns = 4;
        cfg.specialist_turns = 3;
        cfg.reviewer_turns = 0;
        cfg.parallel_specialists = parallel;
        let t0 = std::time::Instant::now();
        let summary = run(client, crate::tools::tools::<Fixture>(), cfg).await;
        (summary, transport, t0.elapsed())
    }

    #[tokio::test]
    async fn parallel_specialists_merge_and_save_wall_clock() {
        let s = json!({"spar_mm": 12});
        let e = json!({"pack_mah": 2200});
        let (seq, seq_t, seq_elapsed) = scripted_run(false, s.clone(), e.clone(), 40).await;
        let (par, par_t, par_elapsed) = scripted_run(true, s, e, 40).await;
        for r in [&seq, &par] {
            let d = r.state.design.as_ref().unwrap();
            assert_eq!(d["spar_mm"], 12, "{d}");
            assert_eq!(d["pack_mah"], 2200, "{d}");
            assert!(r.state.report.as_ref().unwrap().passed(), "{}", r.state.report.as_ref().unwrap());
        }
        // 7 model calls either way; the two specialists' 2+2 calls overlap in parallel
        assert_eq!(seq_t.inner.calls(), 7);
        assert_eq!(par_t.inner.calls(), 7);
        assert_eq!(seq_t.max_inflight.load(Ordering::SeqCst), 1);
        assert_eq!(par_t.max_inflight.load(Ordering::SeqCst), 2);
        // design: version 1; the merge: version 2
        assert_eq!(par.state.version, 2);
        assert!(par.state.notes.iter().any(|n| n.starts_with("structures:")) && par.state.notes.iter().any(|n| n.starts_with("electronics:")));
        eprintln!("wall clock at 40 ms/call: sequential {seq_elapsed:?}, parallel {par_elapsed:?}");
        assert!(par_elapsed < seq_elapsed, "parallel {par_elapsed:?} not faster than sequential {seq_elapsed:?}");
    }

    #[tokio::test]
    async fn a_forced_conflict_is_resolved_by_better_and_reported() {
        // both edit mass_g: structures to a lighter, passing 900; electronics
        // to 1600, which fails the mass check, plus a pack edit nobody contests
        let (par, transport, _) = scripted_run(true, json!({"mass_g": 900}), json!({"mass_g": 1600, "pack_mah": 2500}), 1).await;
        let d = par.state.design.as_ref().unwrap();
        assert_eq!(d["mass_g"], 900, "{d}");
        assert_eq!(d["pack_mah"], 2500, "{d}");
        assert_eq!(transport.max_inflight.load(Ordering::SeqCst), 2);
        // the lead's tool results name the conflict and the dropped op
        let reqs = transport.inner.requests.lock().unwrap();
        let last = reqs.last().unwrap().body["messages"].as_array().unwrap().last().unwrap().clone();
        let text = last.to_string();
        assert!(text.contains("conflict: structures"), "{text}");
        assert!(text.contains("dropped electronics's"), "{text}");
        assert!(text.contains("/mass_g"), "{text}");
        let electronics = last["content"].as_array().unwrap().iter().find(|b| b["tool_use_id"] == "t3").unwrap();
        assert_eq!(electronics["is_error"], true);
    }

    #[tokio::test]
    async fn dependent_roles_stay_sequential() {
        // aero <- structures: delegated together they run one after the other
        let transport = Arc::new(Slow {
            inner: ScriptedTransport::from_fn(|body| {
                let system = body["system"][0]["text"].as_str().unwrap_or("");
                let n = body["messages"].as_array().map_or(0, Vec::len);
                let content = if system.starts_with("You lead") {
                    match n {
                        1 => vec![tool_use("t1", "design", json!({}))],
                        3 => vec![tool_use("t2", "delegate", json!({"role": "aero", "task": "x"})), tool_use("t3", "delegate", json!({"role": "structures", "task": "y"}))],
                        _ => vec![tool_use("t4", "submit", json!({"summary": "done"}))],
                    }
                } else {
                    vec![tool_use("s", "report", json!({"summary": "nothing"}))]
                };
                Some(reply("claude-opus-5-5", content, 10, 10))
            }),
            latency: std::time::Duration::from_millis(5),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
        });
        let (tx, _rx) = crate::channel();
        let client = Client::new(transport.clone(), crate::Budget::new(5., 45., None), tx, "seq-test");
        let mut cfg = RunConfig::new("seq-test", "a plane", json!({}));
        cfg.max_rounds = 1;
        cfg.orchestrator_turns = 4;
        cfg.specialist_turns = 2;
        cfg.reviewer_turns = 0;
        let _ = run(client, crate::tools::tools::<Fixture>(), cfg).await;
        assert_eq!(transport.max_inflight.load(Ordering::SeqCst), 1);
        assert_eq!(transport.inner.calls(), 5);
    }
}
