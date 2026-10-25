//! Cross-run lessons memory: what earlier runs of a domain learned, kept
//! beside the regressions file and read back into the prompts of the next
//! run.
//!
//! Shape from Voyager's skill library (MineDojo/Voyager
//! `voyager/agents/skill.py`, `SkillManager`): every finished task is
//! turned into a short description, stored under a key (a repeat of the
//! key is a rewrite, not a second copy), persisted next to the run's
//! checkpoints, and `retrieve_skills(query)` returns the top-k by
//! similarity to the next task's text for the prompt. Voyager embeds with
//! OpenAI and queries Chroma; here retrieval is BM25 over tokens
//! ([`retrieve`]) because a run must work with no network beyond the model
//! call. The BM25 terms follow Lucene's `BM25Similarity`
//! (apache/lucene `lucene/core/src/java/org/apache/lucene/search/
//! similarities/BM25Similarity.java`): `idf = ln(1 + (N - n + 0.5) /
//! (n + 0.5))`, `tf / (tf + k1 * (1 - b + b * dl / avgdl))`, k1 = 1.2,
//! b = 0.75. Deduplication by a canonical key per lesson follows
//! FunSearch's programs database (google-deepmind/funsearch
//! `implementation/programs_database.py`, `Island.register_program`: a
//! program with a signature already seen joins that cluster rather than
//! opening a new one), and the retrieved lessons are laid out weakest to
//! strongest as its `Island.get_prompt` does (`np.argsort(scores)`), so
//! the most relevant lesson sits nearest the task text.
//!
//! Stored per domain in `lessons-<domain>.jsonl` next to
//! `regressions-<domain>.jsonl`, append-only; when a domain's lessons pass
//! [`MAX_STORED`] the file is compacted to the newest ones (written to a
//! temporary file and renamed, as `state::move_rejected` does). Secrets
//! are scrubbed from every stored string ([`redact`]).

use crate::state::DesignState;
use crate::tools::DomainTools;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use wright_core::Outcome;

/// Lessons kept per domain before the file is compacted to the newest.
pub const MAX_STORED: usize = 400;
/// Lessons retrieved into a prompt.
pub const RETRIEVE: usize = 8;
/// Longest lesson text (and fix) stored, in characters.
pub const LESSON_CHARS: usize = 400;
/// Longest "Lessons from earlier runs" section, in characters.
pub const SECTION_CHARS: usize = 3500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A reviewer attack the host confirmed, and what fixed it (if anything).
    Attack,
    /// A part the final design used that nobody sells at a stated price,
    /// or that the catalog marks out of stock.
    Unorderable,
    /// A check that failed in every round of the run.
    NeverPassed,
    /// What the person said (a note on a render).
    Feedback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub domain: String,
    /// Provenance: the run that learned it and the day (YYYY-MM-DD).
    pub run: String,
    pub date: String,
    pub kind: Kind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Canonical key for deduplication (FunSearch's signature).
    pub key: String,
    /// The request the run was for: retrieval context.
    #[serde(default)]
    pub request: String,
}

impl Lesson {
    /// What retrieval indexes.
    fn document(&self) -> String {
        format!("{} {} {}", self.text, self.fix.as_deref().unwrap_or(""), self.request)
    }
    fn provenance(&self) -> String {
        format!("(run {}, {})", self.run, self.date)
    }
}

/// `lessons-<domain>.jsonl` beside `regressions-<domain>.jsonl`.
pub fn lessons_path(regressions: &Path) -> PathBuf {
    let name = regressions.file_name().and_then(|n| n.to_str()).unwrap_or("regressions.jsonl");
    let lessons = if name.contains("regressions") { name.replacen("regressions", "lessons", 1) } else { "lessons.jsonl".into() };
    regressions.with_file_name(lessons)
}

/// Every lesson for `domain` in the file (missing file: none). A line that
/// does not parse is skipped on its own, as `state::load_regressions` does.
pub fn load(path: &Path, domain: &str) -> anyhow::Result<Vec<Lesson>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(anyhow::Error::new(e).context(format!("reading {}", path.display()))),
    };
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Lesson>(l).ok())
        .filter(|l| l.domain == domain)
        .collect())
}

/// Append `new` lessons that the file does not already hold: one whose key
/// is stored is skipped unless it brings a fix the stored one lacks. Then,
/// past [`MAX_STORED`] for the domain, the file is compacted to the newest
/// (other domains' lines are kept as they are). Returns how many were
/// written.
pub fn append(path: &Path, new: &[Lesson]) -> anyhow::Result<usize> {
    if new.is_empty() {
        return Ok(0);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let domain = &new[0].domain;
    let existing = load(path, domain)?;
    let mut fixed: HashSet<String> = existing.iter().filter(|l| l.fix.is_some()).map(|l| l.key.clone()).collect();
    let mut seen: HashSet<String> = existing.iter().map(|l| l.key.clone()).collect();
    let mut written = 0;
    {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        for l in new {
            let richer = l.fix.is_some() && !fixed.contains(&l.key);
            if seen.contains(&l.key) && !richer {
                continue;
            }
            let mut l = l.clone();
            l.text = redact(&truncate(&l.text, LESSON_CHARS));
            l.fix = l.fix.map(|f| redact(&truncate(&f, LESSON_CHARS)));
            l.request = redact(&truncate(&l.request, LESSON_CHARS));
            writeln!(f, "{}", serde_json::to_string(&l)?)?;
            seen.insert(l.key.clone());
            if l.fix.is_some() {
                fixed.insert(l.key.clone());
            }
            written += 1;
        }
    }
    if existing.len() + written > MAX_STORED {
        compact(path, domain)?;
    }
    Ok(written)
}

/// Rewrite the file keeping the newest [`MAX_STORED`] lessons of `domain`
/// and every other line untouched.
fn compact(path: &Path, domain: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let ours: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| serde_json::from_str::<Lesson>(l).is_ok_and(|x| x.domain == domain))
        .map(|(i, _)| i)
        .collect();
    let drop: HashSet<usize> = ours.iter().copied().take(ours.len().saturating_sub(MAX_STORED)).collect();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if !drop.contains(&i) && !l.trim().is_empty() {
            out.push_str(l);
            out.push('\n');
        }
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ------------------------------------------------------------ retrieval

const STOP: &[&str] = &["the", "and", "for", "with", "that", "this", "under", "from", "into", "than", "then", "its", "are", "was", "not", "but", "has", "have", "run", "design", "spec", "check", "fails", "fail", "failing", "margin"];

/// Lowercase alphanumeric tokens of two or more characters, stop words
/// dropped; a number keeps its decimal point ("14.2").
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let keep_dot = c == '.' && i > 0 && i + 1 < chars.len() && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit();
        if c.is_alphanumeric() || keep_dot {
            cur.extend(c.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|t| t.chars().count() >= 2 && !STOP.contains(&t.as_str()));
    out
}

/// The retrieval query for a run: the request plus every key and leaf of
/// the spec.
pub fn query_text(request: &str, spec: &Value) -> String {
    let mut s = request.to_string();
    flatten(spec, &mut s);
    s
}

fn flatten(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                out.push(' ');
                out.push_str(k);
                flatten(v, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|v| flatten(v, out)),
        Value::Null => {}
        other => {
            out.push(' ');
            out.push_str(&other.to_string().replace('"', ""));
        }
    }
}

/// The top `k` lessons for `query` by BM25 (Lucene's `BM25Similarity`),
/// ties broken by recency (later in the file = newer). When fewer than `k`
/// lessons share a term with the query, the newest of the rest fill in,
/// as Voyager's `retrieve_skills` always returns k. One lesson per key:
/// the newest with a fix wins. Output is weakest to strongest (FunSearch
/// `Island.get_prompt`).
pub fn retrieve<'a>(lessons: &'a [Lesson], query: &str, k: usize) -> Vec<&'a Lesson> {
    if lessons.is_empty() || k == 0 {
        return vec![];
    }
    // one per key, preferring the newest, and among those one with a fix
    let mut by_key: BTreeMap<&str, &Lesson> = BTreeMap::new();
    for l in lessons {
        let replace = by_key.get(l.key.as_str()).is_none_or(|old| l.fix.is_some() || old.fix.is_none());
        if replace {
            by_key.insert(&l.key, l);
        }
    }
    let docs: Vec<(&Lesson, Vec<String>)> = by_key.values().map(|l| (*l, tokens(&l.document()))).collect();
    let n = docs.len() as f64;
    let avgdl = docs.iter().map(|(_, t)| t.len() as f64).sum::<f64>() / n.max(1.0);
    let mut df: HashMap<&str, usize> = HashMap::new();
    for (_, t) in &docs {
        for term in t.iter().collect::<HashSet<_>>() {
            *df.entry(term).or_default() += 1;
        }
    }
    let q: HashSet<String> = tokens(query).into_iter().collect();
    let (k1, b) = (1.2, 0.75);
    let order = |l: &Lesson| lessons.iter().position(|x| std::ptr::eq(x, l)).unwrap_or(0);
    let mut scored: Vec<(f64, usize, &Lesson)> = docs
        .iter()
        .map(|(l, t)| {
            let dl = t.len() as f64;
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for term in t {
                *tf.entry(term).or_default() += 1.0;
            }
            let score: f64 = q
                .iter()
                .filter_map(|term| {
                    let f = *tf.get(term.as_str())?;
                    let dfreq = *df.get(term.as_str()).unwrap_or(&0) as f64;
                    let idf = (1.0 + (n - dfreq + 0.5) / (dfreq + 0.5)).ln();
                    Some(idf * f / (f + k1 * (1.0 - b + b * dl / avgdl.max(1e-9))))
                })
                .sum();
            (score, order(l), *l)
        })
        .collect();
    // strongest first, newer first on a tie
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.cmp(&a.1)));
    let mut top: Vec<(f64, usize, &Lesson)> = scored.iter().filter(|s| s.0 > 0.0).take(k).copied().collect();
    if top.len() < k {
        let mut rest: Vec<(f64, usize, &Lesson)> = scored.iter().filter(|s| s.0 <= 0.0).copied().collect();
        rest.sort_by_key(|s| std::cmp::Reverse(s.1));
        top.extend(rest.into_iter().take(k - top.len()));
    }
    top.reverse();
    top.into_iter().map(|s| s.2).collect()
}

/// The prompt section for `lessons` (empty when there are none), capped at
/// [`SECTION_CHARS`]: the weakest lessons are dropped first.
pub fn section(lessons: &[&Lesson], source: &Path) -> String {
    if lessons.is_empty() {
        return String::new();
    }
    let head = format!(
        "\n\nLessons from earlier runs (from {}, chosen by relevance to this request; each ends with the run it came from and the date). They are evidence from other designs, not numbers for this one: verify anything you rely on with check.\n",
        source.display()
    );
    let lines: Vec<String> = lessons
        .iter()
        .map(|l| {
            let kind = match l.kind {
                Kind::Attack => "reviewer attack",
                Kind::Unorderable => "unorderable part",
                Kind::NeverPassed => "never passed",
                Kind::Feedback => "person's feedback",
            };
            let fix = l.fix.as_deref().map(|f| format!(" -- fix: {f}")).unwrap_or_default();
            format!("- [{kind}] {}{fix} {}\n", l.text, l.provenance())
        })
        .collect();
    let mut keep = lines.len();
    while keep > 1 && head.len() + lines[lines.len() - keep..].iter().map(String::len).sum::<usize>() > SECTION_CHARS {
        keep -= 1;
    }
    let mut s = head;
    for l in &lines[lines.len() - keep..] {
        s.push_str(l);
    }
    s
}

/// Load the domain's lessons beside `regressions_path`, retrieve the most
/// relevant for this run and format the prompt section ("" when none).
pub fn section_for(regressions_path: &Path, domain: &str, request: &str, spec: &Value) -> anyhow::Result<String> {
    let path = lessons_path(regressions_path);
    let all = load(&path, domain)?;
    let top = retrieve(&all, &query_text(request, spec), RETRIEVE);
    Ok(section(&top, &path))
}

// ------------------------------------------------------------ harvesting

/// What a run leaves behind for the harvest: the orchestrator's and
/// specialists' edits per round, the failing checks after each round's
/// review, and what the person said.
#[derive(Debug, Default, Clone)]
pub struct RunTrace {
    pub edits: Vec<(u32, Vec<String>)>,
    pub failing: Vec<(u32, Vec<Outcome>)>,
    pub person: Vec<String>,
}

impl RunTrace {
    /// Remember the person's notes in `feedback` (lines the harness marks
    /// `person, ...`); call it before the harness overwrites the list.
    pub fn note_feedback(&mut self, feedback: &[String]) {
        for f in feedback.iter().filter(|f| f.starts_with("person")) {
            if !self.person.contains(f) {
                self.person.push(f.clone());
            }
        }
    }

    /// Close a round: its edits and the failing checks of its report.
    pub fn round(&mut self, round: u32, edits: Vec<String>, state: &DesignState) {
        self.edits.push((round, edits));
        let failing = state.report.as_ref().map(|r| r.failures().cloned().collect()).unwrap_or_default();
        self.failing.push((round, failing));
        self.note_feedback(&state.feedback);
    }

    /// The lessons of a finished run.
    pub fn harvest(&self, domain: &dyn DomainTools, state: &DesignState, run: &str, date: &str) -> Vec<Lesson> {
        let mut out = vec![];
        let mut n = 0;
        let mut lesson = |kind: Kind, key: String, text: String, fix: Option<String>| {
            n += 1;
            out.push(Lesson {
                id: format!("{run}-lesson-{n}"),
                domain: state.domain.clone(),
                run: run.into(),
                date: date.into(),
                kind,
                text: redact(&truncate(&text, LESSON_CHARS)),
                fix: fix.map(|f| redact(&truncate(&f, LESSON_CHARS))),
                key,
                request: redact(&truncate(&state.request, LESSON_CHARS)),
            });
        };

        // confirmed attacks of this run, with the edits that followed
        let prefix = format!("{run}-r");
        for r in state.regressions.iter().filter(|r| r.id.starts_with(&prefix)) {
            let round: u32 = r.id[prefix.len()..].split('-').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let later: Vec<&str> = self.edits.iter().filter(|(rd, _)| *rd > round).flat_map(|(_, e)| e.iter().map(String::as_str)).collect();
            let passes = state.report.as_ref().and_then(|rep| rep.get(&format!("{}@{}", r.check, r.id))).is_some_and(Outcome::passed);
            let margin = r.margin_at_confirmation.map(|m| format!(", margin {m:.3} when confirmed")).unwrap_or_default();
            let text = format!("{} ({} failed under spec patch {}{margin})", r.claim.trim_end_matches('.'), r.check, r.spec_patch);
            let fix = match (passes, later.is_empty()) {
                (true, false) => Some(later.join("; ")),
                (true, true) => Some("passed without further edits".into()),
                (false, _) => Some("NOT fixed by the end of the run".into()),
            };
            let mut keys: Vec<String> = vec![];
            patch_keys(&r.spec_patch, String::new(), &mut keys);
            lesson(Kind::Attack, format!("attack:{}:{}", r.check, keys.join(",")), text, fix);
        }

        // parts of the final design nobody sells, or out of stock
        if let Some(design) = &state.design {
            if let Ok(bom) = domain.bom(design) {
                for l in bom.unpriced() {
                    lesson(Kind::Unorderable, format!("unorderable:{}", l.id), format!("part `{}` ({}, {}) had no vendor price: it cannot be ordered; pick another from catalog", l.id, l.name, l.category), None);
                }
            }
            for (path, _) in domain.catalog_paths() {
                let Some(id) = design_at(design, path).and_then(|v| v.as_str().map(str::to_owned)) else { continue };
                let Ok(opts) = domain.options(design, path, None) else { continue };
                if let Some(o) = opts.iter().find(|o| o.id == id) {
                    if o.in_stock == Some(false) {
                        lesson(Kind::Unorderable, format!("unorderable:{id}"), format!("part `{id}` ({}) at {path} was OUT OF STOCK at the end of the run; pick an in-stock option from catalog", o.label), None);
                    }
                }
            }
        }

        // checks that failed after every round
        if !self.failing.is_empty() {
            let mut counts: BTreeMap<&str, (usize, &Outcome)> = BTreeMap::new();
            for (_, f) in &self.failing {
                for o in f {
                    let e = counts.entry(&o.name).or_insert((0, o));
                    e.0 += 1;
                    e.1 = o;
                }
            }
            let rounds = self.failing.len();
            for (name, (c, last)) in counts {
                if c == rounds && !name.contains('@') {
                    let assumptions = if state.assumptions.is_empty() { String::new() } else { format!("; spec assumptions: {}", state.assumptions.join(", ")) };
                    lesson(Kind::NeverPassed, format!("never:{name}"), format!("{name} failed in every one of {rounds} round(s); last: {last}{assumptions}"), None);
                }
            }
        }

        for p in &self.person {
            let key = format!("feedback:{}", tokens(p).join(" "));
            lesson(Kind::Feedback, key, p.clone(), None);
        }
        out
    }
}

fn patch_keys(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, v) in m {
                let p = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                patch_keys(v, p, out);
            }
        }
        _ => out.push(at),
    }
}

fn design_at<'a>(design: &'a Value, path: &str) -> Option<&'a Value> {
    let mut v = design;
    for k in path.split('.').filter(|k| !k.is_empty()) {
        v = v.get(k)?;
    }
    Some(v)
}

// -------------------------------------------------------------- hygiene

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(3)).collect();
    format!("{cut}...")
}

/// Scrub anything that looks like a credential: known key prefixes
/// (`sk-`, `sk-ant-`, `AKIA`, `ghp_`, `xox`), `Bearer` tokens, `key=` /
/// `token=` / `password=` values, and bare tokens of 32+ characters that
/// are hex or base64-like. Replaced with `[redacted]`.
pub fn redact(text: &str) -> String {
    const PREFIXES: &[&str] = &["sk-", "sk-ant-", "akia", "ghp_", "gho_", "xoxb-", "xoxp-", "aiza", "sk_live_", "sk_test_"];
    const ASSIGN: &[&str] = &["key=", "token=", "password=", "secret=", "apikey=", "api_key=", "key:", "token:", "password:", "secret:"];
    let mut out: Vec<String> = vec![];
    let mut after_bearer = false;
    for word in text.split(' ') {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '=');
        let lower = trimmed.to_lowercase();
        let secretish = after_bearer
            || PREFIXES.iter().any(|p| lower.starts_with(p) && trimmed.len() > p.len() + 4)
            || ASSIGN.iter().any(|a| lower.contains(a))
            || (trimmed.len() >= 32 && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_' || c == '-') && trimmed.chars().any(|c| c.is_ascii_digit()) && trimmed.chars().any(|c| c.is_ascii_alphabetic()));
        after_bearer = lower == "bearer";
        out.push(if secretish { "[redacted]".into() } else { word.to_string() });
    }
    out.join(" ")
}

/// Today's date as YYYY-MM-DD (UTC), from the system clock. Civil-date
/// arithmetic is Howard Hinnant's `civil_from_days`
/// (howardhinnant.github.io/date_algorithms.html), as libc++'s
/// `chrono::year_month_day` uses.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    civil_date(secs)
}

pub fn civil_date(unix_secs: i64) -> String {
    let z = unix_secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(run: &str, kind: Kind, key: &str, text: &str, fix: Option<&str>) -> Lesson {
        Lesson { id: format!("{run}-x"), domain: "toy".into(), run: run.into(), date: "2026-10-24".into(), kind, text: text.into(), fix: fix.map(Into::into), key: key.into(), request: String::new() }
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wright-memory-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn lessons_file_sits_beside_the_regressions_file() {
        assert_eq!(lessons_path(Path::new("data/regressions-aircraft.jsonl")), PathBuf::from("data/lessons-aircraft.jsonl"));
        assert_eq!(lessons_path(Path::new("x/other.jsonl")), PathBuf::from("x/lessons.jsonl"));
    }

    #[test]
    fn bm25_ranks_the_lesson_sharing_rare_terms_first_and_lays_out_weakest_first() {
        let ls = vec![
            lesson("a", Kind::Attack, "k1", "stall speed rose at a 1000 m field; the wing area was too small", Some("wing area 0.4 -> 0.5 m2")),
            lesson("b", Kind::Unorderable, "k2", "motor T-Motor AT2814 was out of stock", None),
            lesson("c", Kind::Feedback, "k3", "person: fit the payload bay to a 105 mm shelf", None),
        ];
        let top = retrieve(&ls, "a 1.5 kg aircraft flying from a field at 1000 m altitude with a 14 m/s stall speed", 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top.last().unwrap().key, "k1", "strongest last");
        // zero-overlap lessons fill in by recency when k allows
        let all = retrieve(&ls, "a 1.5 kg aircraft at a 1000 m field", 3);
        assert_eq!(all.len(), 3);
        assert_eq!(all.last().unwrap().key, "k1");
    }

    #[test]
    fn retrieval_keeps_one_lesson_per_key_preferring_the_one_with_a_fix() {
        let ls = vec![lesson("a", Kind::Attack, "k", "cold day shrinks the span", None), lesson("b", Kind::Attack, "k", "cold day shrinks the span", Some("span 105 -> 110"))];
        let top = retrieve(&ls, "span for a cold day", 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].run, "b");
    }

    #[test]
    fn append_dedupes_by_key_unless_the_new_lesson_brings_a_fix_and_compacts_past_the_cap() {
        let p = tmp("lessons.jsonl");
        let a = lesson("a", Kind::Attack, "k", "cold day", None);
        assert_eq!(append(&p, std::slice::from_ref(&a)).unwrap(), 1);
        assert_eq!(append(&p, std::slice::from_ref(&a)).unwrap(), 0, "same key, no fix: skipped");
        let b = lesson("b", Kind::Attack, "k", "cold day", Some("span 110"));
        assert_eq!(append(&p, std::slice::from_ref(&b)).unwrap(), 1, "a fix is new information");
        assert_eq!(append(&p, &[b]).unwrap(), 0);
        // another domain's line survives compaction
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"{\"id\":\"o\",\"domain\":\"other\",\"run\":\"o\",\"date\":\"d\",\"kind\":\"feedback\",\"text\":\"t\",\"key\":\"ok\"}\nnot json\n").unwrap();
        let many: Vec<Lesson> = (0..MAX_STORED + 5).map(|i| lesson("m", Kind::Feedback, &format!("f{i}"), &format!("note {i}"), None)).collect();
        append(&p, &many).unwrap();
        let ours = load(&p, "toy").unwrap();
        assert_eq!(ours.len(), MAX_STORED);
        assert_eq!(ours.last().unwrap().key, format!("f{}", MAX_STORED + 4), "newest kept");
        assert!(!ours.iter().any(|l| l.key == "k"), "oldest dropped");
        assert_eq!(load(&p, "other").unwrap().len(), 1);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn section_carries_kind_fix_and_provenance_and_is_capped() {
        let l = lesson("aircraft-1792860707-r1", Kind::Attack, "k", "stall speed fails at a 1000 m field", Some("wing area 0.4 -> 0.5 m2"));
        let s = section(&[&l], Path::new("data/lessons-aircraft.jsonl"));
        assert!(s.contains("Lessons from earlier runs (from data/lessons-aircraft.jsonl"));
        assert!(s.contains("- [reviewer attack] stall speed fails at a 1000 m field -- fix: wing area 0.4 -> 0.5 m2 (run aircraft-1792860707-r1, 2026-10-24)"));
        assert_eq!(section(&[], Path::new("x")), "");
        let big: Vec<Lesson> = (0..40).map(|i| lesson("r", Kind::Feedback, &format!("{i}"), &"x".repeat(300), None)).collect();
        let refs: Vec<&Lesson> = big.iter().collect();
        let s = section(&refs, Path::new("x"));
        assert!(s.len() <= SECTION_CHARS + 20, "{}", s.len());
        assert!(s.contains("(run r, 2026-10-24)"));
    }

    #[test]
    fn secrets_never_reach_the_file() {
        let r = redact("set ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnop then Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9x ok; margin -0.677 m/s at 1000 m");
        assert!(!r.contains("sk-ant"), "{r}");
        assert!(!r.contains("eyJ"), "{r}");
        assert!(r.contains("margin -0.677 m/s at 1000 m"));
        assert_eq!(redact("stall_speed fails under {\"altitude_m\":1000}"), "stall_speed fails under {\"altitude_m\":1000}");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(1_792_860_707), "2026-10-24");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
    }
}
