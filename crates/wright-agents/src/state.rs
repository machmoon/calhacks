//! The typed state agents share instead of sharing conversation history.
//!
//! Specialists run in their own conversations and return only a condensed
//! result (OpenHands delegate: software-agent-sdk @acd96e6,
//! `tools/delegate/impl.py:287-380`; Anthropic's multi-agent research
//! system); what they change lands here, so the orchestrator reads one
//! current state rather than every specialist's transcript.

use crate::tools::{merge_patch, DomainTools};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::Path;
use wright_core::{Evidence, Finished, Outcome, Source};

/// A confirmed reviewer attack: a spec perturbation under which `check`
/// failed on some design. From then on every design must pass `check`
/// under it too (`docs/ARCHITECTURE.md`: "every confirmed bug becomes a
/// permanent check").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Regression {
    pub id: String,
    pub domain: String,
    pub claim: String,
    pub spec_patch: Value,
    pub check: String,
    /// The margin the attack measured when it was confirmed.
    pub margin_at_confirmation: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignState {
    pub domain: String,
    pub request: String,
    pub spec: Value,
    pub design: Option<Value>,
    /// The last full report (declared checks + regression cases).
    pub report: Option<Finished>,
    /// Open feedback: confirmed reviewer issues and a person's notes.
    pub feedback: Vec<String>,
    pub regressions: Vec<Regression>,
    /// Specialists' summaries, oldest first.
    pub notes: Vec<String>,
    pub round: u32,
    /// The orchestrator's engineering plan ([`crate::plan`]).
    #[serde(default)]
    pub plan: crate::plan::Plan,
    /// The spec as the person gave it: the reviewer's attacks may vary the
    /// world (wind, temperature, a gust) but never rewrite what the person
    /// asked for, so every leaf here is locked against an attack's patch.
    #[serde(default)]
    pub stated: Value,
    /// Spec changes made after the first write: assumptions the person did
    /// not state (a rail length, a temperature). Shown to the reviewer, who
    /// must challenge any that make a check easier to pass.
    #[serde(default)]
    pub assumptions: Vec<String>,
    /// The best design verified so far (see [`Incumbent`]).
    #[serde(default)]
    pub best: Option<Incumbent>,
    /// Stored regression cases set aside as outside the domain's envelope,
    /// with why, waiting to be moved to the rejected file at the harness's
    /// next write ([`move_rejected`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_aside: Vec<(Regression, String)>,
    /// The last design built for `run_stage`, and its build files.
    #[serde(skip)]
    pub wip_build: Option<(Value, wright_core::Artifacts)>,
    /// Designs a post-design stage judged, with how many of its checks
    /// they failed: counted like failing checks when ranking the
    /// incumbent, so an FEA hot spot the team fixed is not reverted to
    /// for a better objective.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_failures: Vec<(Value, usize)>,
    /// Design version: bumped every time `design` is replaced (a design
    /// edit, a redesign, a revert, a merge). A parallel specialist's patch
    /// is made against one version and refused if the design has moved on
    /// (`crate::merge`).
    #[serde(default)]
    pub version: u64,
}

/// The best verified design so far, ranked by fewest failing checks
/// (regression cases included), then by the domain's objective (higher is
/// better). Shape from FunSearch's programs database
/// (google-deepmind/funsearch `implementation/programs_database.py`,
/// `_register_program_in_island`): every evaluated program is offered, and
/// it replaces the best only on a strictly better score (`>`), so a tie
/// keeps the earlier one. Ranking feasibility before the objective follows
/// Optuna's `Study._get_best_trial` (optuna `optuna/study/study.py`): a
/// best trial that violates a constraint gives way to the best feasible
/// one. Unlike FunSearch the score is not frozen: when a regression case
/// is added or the spec changes, the incumbent is re-checked
/// ([`DesignState::rescore_best`]) so it is ranked on today's checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incumbent {
    pub design: Value,
    pub failing: usize,
    pub objective: Option<f64>,
    /// The round it was first verified in.
    pub round: u32,
}

/// `a` strictly better than `b`: fewer failing, then a higher objective
/// (a missing or NaN objective ranks below any number).
pub fn better(a: (usize, Option<f64>), b: (usize, Option<f64>)) -> bool {
    let obj = |o: Option<f64>| o.filter(|v| v.is_finite()).unwrap_or(f64::NEG_INFINITY);
    a.0 < b.0 || (a.0 == b.0 && obj(a.1) > obj(b.1))
}

impl DesignState {
    pub fn new(domain: &str, request: &str, spec: Value) -> Self {
        DesignState {
            domain: domain.into(),
            request: request.into(),
            stated: spec.clone(),
            assumptions: vec![],
            spec,
            best: None,
            set_aside: vec![],
            wip_build: None,
            stage_failures: vec![],
            version: 0,
            design: None,
            report: None,
            feedback: vec![],
            regressions: vec![],
            notes: vec![],
            round: 0,
            plan: Default::default(),
        }
    }

    pub fn passed(&self) -> bool {
        self.report.as_ref().is_some_and(Finished::passed)
    }

    /// What agents are shown: the state as JSON with the report as the
    /// same aligned text a person reads (`Finished`'s `Display`), which is
    /// shorter than the outcome structs and carries every number.
    pub fn for_prompt(&self) -> String {
        let report = match &self.report {
            Some(r) => r.to_string(),
            None => "no check report yet".into(),
        };
        let v = json!({
            "domain": self.domain,
            "request": self.request,
            "round": self.round,
            "spec": self.spec,
            "design": self.design,
            "open_feedback": self.feedback,
            // what the first spec write locked as the person's request:
            // attacks on it are refused; a value here that contradicts the
            // request text is a finding for the verdict
            "locked_from_request": self.stated,
            "assumptions_added_by_agents": self.assumptions,
            "regression_cases": self.regressions.iter().map(|r| json!({"id": r.id, "claim": r.claim, "spec_patch": r.spec_patch, "check": r.check})).collect::<Vec<_>>(),
            "specialist_notes": self.notes.iter().rev().take(6).rev().collect::<Vec<_>>(),
            "plan": {"goal": self.plan.goal, "tasks": self.plan.lines()},
        });
        format!("DesignState:\n{}\n\nLast check report:\n{report}", serde_json::to_string_pretty(&v).unwrap_or_default())
    }

    /// The value at a dotted path of the stated spec, for messages.
    pub fn stated_at(&self, path: &str) -> String {
        let mut v = &self.stated;
        for k in path.split('.') {
            v = &v[k];
        }
        v.to_string()
    }

    /// The spec the checks judge: the working spec with the person's stated
    /// fields laid back over it, so no edit can quietly lower what they
    /// asked for (the edit tools also refuse to; this is the backstop).
    pub fn effective_spec(&self) -> Value {
        let mut spec = self.spec.clone();
        if !self.stated.is_null() && spec.is_object() {
            merge_patch(&mut spec, &self.stated);
        }
        spec
    }

    /// Every declared check on the effective spec, then each regression
    /// case's check under its perturbed spec, named `check@id`.
    pub fn full_check(&self, domain: &dyn DomainTools) -> anyhow::Result<Finished> {
        let design = self.design.as_ref().ok_or_else(|| anyhow::anyhow!("no design yet: call design first"))?;
        self.full_check_of(domain, design)
    }

    /// The regression cases that apply to this run, as conditions for
    /// [`DomainTools::design_under`]: the effective spec with each case's
    /// patch applied (cases that rewrite the person's request are skipped,
    /// as in [`Self::full_check`]).
    pub fn conditions(&self, domain: &dyn DomainTools) -> anyhow::Result<Vec<wright_core::Condition<Value>>> {
        let base = domain.validate_spec(&self.effective_spec())?;
        Ok(self
            .regressions
            .iter()
            .filter(|r| crate::harness::touches_stated(&self.stated, &r.spec_patch, String::new()).is_none())
            .filter(|r| !domain.consistency_checks().contains(&r.check.as_str()))
            .map(|r| {
                let mut spec = base.clone();
                merge_patch(&mut spec, &r.spec_patch);
                wright_core::Condition { id: r.id.clone(), check: r.check.clone(), claim: r.claim.clone(), spec }
            })
            .collect())
    }

    /// `(failing, objective)` of `design` under today's checks.
    pub fn score_of(&self, domain: &dyn DomainTools, design: &Value) -> Option<(usize, Option<f64>)> {
        let fin = self.full_check_of(domain, design).ok()?;
        Some((fin.failures().count() + self.stage_failing(design), domain.objective(design).ok().map(|o| o.0)))
    }

    /// A person sent feedback (a sketch, a note): what they asked for may
    /// cost objective, so no earlier design may be swapped back in over the
    /// team's response to it (review 5, F2: a "fit my 105 mm shelf" edit
    /// was reverted to the longer, higher-objective design). The incumbent
    /// starts again from the next verified design.
    pub fn person_spoke(&mut self) {
        self.best = None;
    }

    /// Failing post-design stage checks recorded for `design`.
    pub fn stage_failing(&self, design: &Value) -> usize {
        self.stage_failures.iter().filter(|(d, _)| d == design).map(|(_, n)| *n).sum()
    }

    /// Record a stage's verdict on `design` (replacing an earlier one for
    /// the same design and stage is not tracked: the latest count adds).
    pub fn record_stage(&mut self, domain: &dyn DomainTools, design: &Value, failing: usize) {
        if failing > 0 {
            self.stage_failures.push((design.clone(), failing));
            self.rescore_best(domain);
        }
    }

    /// Offer the current design, whose full report is `fin`: it becomes
    /// the incumbent if strictly better.
    pub fn offer(&mut self, domain: &dyn DomainTools, fin: &Finished) {
        let Some(design) = &self.design else { return };
        let score = (fin.failures().count() + self.stage_failing(design), domain.objective(design).ok().map(|o| o.0));
        if self.best.as_ref().is_none_or(|b| better(score, (b.failing, b.objective))) {
            self.best = Some(Incumbent { design: design.clone(), failing: score.0, objective: score.1, round: self.round });
        }
    }

    /// Re-check the incumbent under today's spec and regression cases.
    /// One that no longer even checks (the spec changed shape) is dropped.
    pub fn rescore_best(&mut self, domain: &dyn DomainTools) {
        let Some(b) = self.best.take() else { return };
        if let Some((failing, objective)) = self.score_of(domain, &b.design) {
            self.best = Some(Incumbent { failing, objective, ..b });
        }
    }

    /// If the incumbent is strictly better than the current design, make
    /// it current and say why (for an agent_message); `None` otherwise.
    pub fn prefer_best(&mut self, domain: &dyn DomainTools) -> Option<String> {
        self.rescore_best(domain);
        let b = self.best.clone()?;
        let current = self.design.as_ref().and_then(|d| self.score_of(domain, d));
        let take = match current {
            None => true,
            Some(c) => better((b.failing, b.objective), c),
        };
        if !take {
            return None;
        }
        let fmt = |o: Option<f64>| o.map_or("n/a".into(), |v| format!("{v:.3}"));
        let msg = match current {
            Some((cf, co)) => format!(
                "the current design is worse than the best verified one ({cf} failing, objective {} vs {} failing, objective {}, from round {}): continuing from the best one",
                fmt(co),
                b.failing,
                fmt(b.objective),
                b.round
            ),
            None => format!("the current design cannot be checked: continuing from the best verified one ({} failing, from round {})", b.failing, b.round),
        };
        self.design = Some(b.design);
        self.version += 1;
        Some(msg)
    }

    /// Why stored case `r` is outside the domain's envelope, or `None`:
    /// a patched field outside the schema's bounds (needs no spec), or,
    /// once this run has a spec that validates, a patched spec the
    /// domain's validation refuses. A case that rewrites this person's
    /// request is not judged here: it is ignored for this run and may
    /// hold for another.
    pub fn envelope_violation(&self, domain: &dyn DomainTools, r: &Regression) -> Option<String> {
        if domain.consistency_checks().contains(&r.check.as_str()) {
            return Some(format!(
                "`{}` is a copy-consistency check: under a changed spec it fails for every design, so the case can never pass",
                r.check
            ));
        }
        if crate::harness::touches_stated(&self.stated, &r.spec_patch, String::new()).is_some() {
            return None;
        }
        if let Some(why) = crate::harness::outside_envelope(&domain.spec_schema(), &r.spec_patch) {
            return Some(why);
        }
        let base = domain.validate_spec(&self.effective_spec()).ok()?;
        let mut spec = base;
        merge_patch(&mut spec, &r.spec_patch);
        domain.validate_spec(&spec).err().map(|e| format!("the patched spec fails the domain's validation: {e:#}"))
    }

    /// Set aside every stored case outside the envelope (review 5: an
    /// unbounded schema let 3500 m + ISA+45 K attacks be confirmed and
    /// stored). Returns one message per case set aside; the cases wait in
    /// [`Self::set_aside`] to be moved out of the regressions file.
    pub fn screen_regressions(&mut self, domain: &dyn DomainTools) -> Vec<String> {
        let mut msgs = vec![];
        let mut keep = vec![];
        for r in std::mem::take(&mut self.regressions) {
            match self.envelope_violation(domain, &r) {
                Some(why) => {
                    msgs.push(format!("set aside regression {} ({} under {}): {why}", r.id, r.check, r.spec_patch));
                    self.set_aside.push((r, why));
                }
                None => keep.push(r),
            }
        }
        self.regressions = keep;
        if !msgs.is_empty() {
            self.rescore_best(domain);
        }
        msgs
    }

    /// [`Self::full_check`] of any design.
    pub fn full_check_of(&self, domain: &dyn DomainTools, design: &Value) -> anyhow::Result<Finished> {
        let base = domain.validate_spec(&self.effective_spec())?;
        let mut fin = domain.check(&base, design)?;
        for r in &self.regressions {
            // a stored attack that rewrites this person's request is not a
            // regression for this run (it was confirmed under another one)
            if crate::harness::touches_stated(&self.stated, &r.spec_patch, String::new()).is_some()
                || domain.consistency_checks().contains(&r.check.as_str())
            {
                continue;
            }
            let mut spec = base.clone();
            merge_patch(&mut spec, &r.spec_patch);
            let name = format!("{}@{}", r.check, r.id);
            let outcome = match domain.validate_spec(&spec).and_then(|s| domain.check(&s, design)) {
                Ok(f) => match f.get(&r.check) {
                    Some(o) => Outcome { name, detail: format!("regression {}: {} | {}", r.id, r.claim, o.detail), ..o.clone() },
                    None => missing(name, format!("check {} no longer exists", r.check)),
                },
                Err(e) => missing(name, format!("regression {} could not run: {e}", r.id)),
            };
            fin.outcomes.push(outcome);
        }
        Ok(fin)
    }
}

fn missing(name: String, why: String) -> Outcome {
    Outcome { name, evidence: Evidence::Missing { why }, detail: String::new(), source: Source::Requirement }
}

/// Confirmed regressions for `domain` from a JSON-lines file (missing file:
/// none yet). Bad lines are skipped; see [`load_regressions_reporting`].
pub fn load_regressions(path: &Path, domain: &str) -> anyhow::Result<Vec<Regression>> {
    load_regressions_reporting(path, domain).map(|(r, _)| r)
}

/// Like [`load_regressions`], plus one line per skipped entry saying why.
/// A line that does not parse (a half-written line from a killed run) is
/// skipped on its own, never taking the rest with it; a file that exists
/// but cannot be read is an error (the caller stops the run), so a
/// permission problem never quietly drops every regression (review 4,
/// finding 5).
pub fn load_regressions_reporting(path: &Path, domain: &str) -> anyhow::Result<(Vec<Regression>, Vec<String>)> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], vec![])),
        Err(e) => return Err(anyhow::Error::new(e).context(format!("reading {}", path.display()))),
    };
    let (mut out, mut skipped) = (vec![], vec![]);
    let mut seen = std::collections::HashSet::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Regression>(line) {
            Ok(mut r) if r.domain == domain => {
                // files written before ids carried the run id repeat
                // `r1-1`; keep every case, made distinct by its line
                if !seen.insert(r.id.clone()) {
                    r.id = format!("{}-L{}", r.id, i + 1);
                    seen.insert(r.id.clone());
                }
                out.push(r);
            }
            Ok(_) => {}
            Err(e) => skipped.push(format!("line {}: {e}", i + 1)),
        }
    }
    Ok((out, skipped))
}

pub fn append_regression(path: &Path, r: &Regression) -> anyhow::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(r)?)?;
    Ok(())
}

/// Where cases set aside as out of envelope go: `regressions-rejected.jsonl`
/// next to the regressions file, one [`Regression`] per line plus a
/// `rejected` reason (the shape the file already had).
pub fn rejected_path(regressions: &Path) -> std::path::PathBuf {
    regressions.with_file_name("regressions-rejected.jsonl")
}

/// Move `cases` out of the regressions file at `path` into
/// [`rejected_path`]: each is appended there with its reason, then the
/// regressions file is rewritten without it (to a temporary file, then
/// renamed, so a crash leaves the old file whole). Every other line,
/// including other domains' and lines that do not parse, is kept as it
/// was. A case is matched by domain, check and patch, since loading may
/// have renamed a duplicate id.
pub fn move_rejected(path: &Path, cases: &[(Regression, String)]) -> anyhow::Result<()> {
    if cases.is_empty() {
        return Ok(());
    }
    let rej = rejected_path(path);
    {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&rej)?;
        for (r, why) in cases {
            let mut v = serde_json::to_value(r)?;
            v["rejected"] = Value::String(why.clone());
            writeln!(f, "{}", serde_json::to_string(&v)?)?;
        }
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut left: Vec<&Regression> = cases.iter().map(|(r, _)| r).collect();
    let mut out = String::new();
    for line in text.lines() {
        if let Ok(r) = serde_json::from_str::<Regression>(line) {
            if let Some(i) = left.iter().position(|c| c.domain == r.domain && c.check == r.check && c.spec_patch == r.spec_patch) {
                left.remove(i);
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
