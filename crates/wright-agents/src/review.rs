//! The hostile reviewer made systematic: an attack taxonomy it must walk,
//! evidence every attack must cite, a proposer-vs-reviewer rebuttal round
//! before a confirmed attack becomes a regression, and per-run metrics.
//!
//! Precedent, read in source (not from memory):
//!
//! * **MetaGPT** `metagpt/actions/write_code_review.py` (geekan/MetaGPT
//!   @main): the reviewer answers an *ordered checklist* item by item
//!   ("Code Review: Ordered List ... 1. Is the code implemented as per the
//!   requirements? ... 6. Are methods from other files being reused
//!   correctly?"; "If any answer is no, explain how to fix it"), ends with a
//!   binary `LGTM/LBTM`, and `WriteCodeReview.run` loops review -> rewrite
//!   -> re-review `code_validate_k_times` rounds, stopping at the first
//!   LGTM. Here: [`Category::ALL`] is the checklist the reviewer walks, the
//!   verdict's `walked_clear` is its "Yes." per item, and
//!   [`rebuttal_round`] is the review -> answer -> re-review step with the
//!   proposer answering instead of rewriting.
//! * **inspect_ai** `src/inspect_ai/scorer/_model.py`
//!   (UKGovernmentBEIS/inspect_ai @main, `model_graded_qa`): a grader
//!   verdict that is off-menu or missing is "a protocol deviation, not
//!   evidence about the submission", returned as
//!   `Score.unscored(reason="grader_failed")` with the grading prompt and
//!   the grader's whole message kept in `metadata`. Here: an attack the
//!   host cannot confirm is [`AttackOutcome::Rejected`] with a
//!   [`RejectKind`] and the reason text, never a block and never silently
//!   dropped, and every [`Attempt`] keeps the evidence it cited.
//!
//! The confirmation method is the one the harness already has: a stored
//! [`wright_core::Condition`] (the spec with the attack's patch) under which
//! the named check fails on the current design; see `Reviewer::attack` in
//! `harness.rs`. This module classifies and records what that path decides.

use crate::agent::{run_agent, Conversation, Handler, ToolOutput};
use crate::client::{BoxFut, Client};
use crate::state::{DesignState, Regression};
use crate::tools::DomainTools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use wright_core::Event;

/// The attack taxonomy, derived from what reviews 3, 4 and 5 actually
/// found (docs/TODO.md). The reviewer walks it in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// A load, part or effect no check measures (review 3: BOM missing
    /// igniter, plumbing, GN2; line rating unsourced; dead trackers get credit).
    Omission,
    /// A unit, sign or reference-frame error (review 3: stall on
    /// uncorrected CLmax, flutter overstated by sqrt 2, Re on radius vs
    /// diameter; review 4: stall limit must be EAS).
    UnitsSign,
    /// Two copies of one quantity disagree (review 3: CAD OD 156.7 vs
    /// 152.4 mm tube; review 4: design.requirements vs spec; review 5 F1).
    CopyConsistency,
    /// Stock, price or vendor reality (review 5 F4: four out-of-stock
    /// parts; review 3: BOM harness/adapters, microSD, regulator).
    StockSourcing,
    /// Environment the builder will meet: altitude, temperature, wind,
    /// gust, as-built mass (review 4: 1000 m field; the 3500 m + ISA+45 K
    /// attacks that were *outside* the envelope and got set aside).
    Environment,
    /// Geometry: interference, fit, clearance, bed fit (review 3: liner
    /// stock 130.4 vs 127 mm; CAD clashes in docs/cad-progress.md).
    Interference,
    /// A model used outside its validity range (review 3: eta_cstar > 1
    /// accepted, N2O MEOP below vapour pressure, circular current checks).
    ModelValidity,
    /// Operator and regulatory limits (review 3: LoRa 15.247, GPS altitude
    /// limits, flight-train rules, ArduPilot AIRSPEED_MIN from stall).
    OperatorRegulatory,
}

impl Category {
    pub const ALL: &'static [Category] = &[
        Category::Omission,
        Category::UnitsSign,
        Category::CopyConsistency,
        Category::StockSourcing,
        Category::Environment,
        Category::Interference,
        Category::ModelValidity,
        Category::OperatorRegulatory,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Omission => "omission",
            Category::UnitsSign => "units_sign",
            Category::CopyConsistency => "copy_consistency",
            Category::StockSourcing => "stock_sourcing",
            Category::Environment => "environment",
            Category::Interference => "interference",
            Category::ModelValidity => "model_validity",
            Category::OperatorRegulatory => "operator_regulatory",
        }
    }

    /// What to look for, and how it is confirmed here.
    pub fn describe(self) -> &'static str {
        match self {
            Category::Omission => "a load, part, loss or effect the checks never measure (a missing BOM line, an unsourced rating, a tracker counted as working with no GPS). Confirm by a condition that makes a declared check expose it; if no check can, it is a verdict finding, not an attack",
            Category::UnitsSign => "a unit, sign or reference-frame slip (TAS vs EAS, radius vs diameter, a sqrt(2) factor, a margin computed with the wrong sign). Cite the check and the design field whose number is off",
            Category::CopyConsistency => "two copies of one quantity that disagree (CAD vs sized dimension, design.requirements vs spec, an id in the design vs the catalog row). Cite both fields; the host refuses attacks that predict a copy-consistency check, so predict a check of what the vehicle does",
            Category::StockSourcing => "stock, price, vendor and lead-time reality (a part out of stock, a price rise, a vendor minimum). Cite the catalog row or BOM line; perturb the sourcing fields of the spec",
            Category::Environment => "the environment the builder will meet inside the declared envelope: field altitude, hot or cold day, wind and gust, as-built mass growth, pack voltage sag. Cite the check whose margin is thinnest",
            Category::Interference => "fit and geometry: interference, clearance, bed fit, a tube that does not fit a liner, a cable keepout. Cite the parts (design fields) that touch",
            Category::ModelValidity => "a model used outside its range (efficiency above 1, pressure below vapour pressure, a Reynolds number the airfoil polar does not cover, a circular check that compares a value with itself). Cite the check and the value outside the range",
            Category::OperatorRegulatory => "operator and regulatory limits (a radio band or power limit, a GPS altitude limit, a flight-train or impulse-class rule, a flight controller parameter derived from a wrong number). Cite the rule and the design field",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Category::ALL.iter().copied().find(|c| c.name() == s.trim())
    }

    /// A category for an attack that named none (older scripts, legacy
    /// callers), from the check name, the patched fields and the claim.
    pub fn infer(check: &str, patch: &Value, claim: &str) -> Category {
        let mut text = format!("{check} {claim} ").to_lowercase();
        if let Some(o) = patch.as_object() {
            for k in o.keys() {
                text.push_str(&k.to_lowercase());
                text.push(' ');
            }
        }
        let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
        if has(&["stock", "price", "vendor", "budget", "cost", "lead_time", "orderable", "sourc"]) {
            Category::StockSourcing
        } else if has(&["interfer", "clearance", "fit", "clash", "keepout", "bed"]) {
            Category::Interference
        } else if has(&["unit", "sign", "eas", "tas", "radius", "diameter", "sqrt"]) {
            Category::UnitsSign
        } else if has(&["copy", "consisten", "matches", "made_for", "requirements"]) {
            Category::CopyConsistency
        } else if has(&["fcc", "faa", "legal", "regulat", "band", "limit_dbm", "ardupilot", "rule"]) {
            Category::OperatorRegulatory
        } else if has(&["valid", "range", "eta", "efficiency", "vapour", "vapor", "reynolds", "circular"]) {
            Category::ModelValidity
        } else if has(&["missing", "omit", "unsourced", "no check", "not measured", "ignored"]) {
            Category::Omission
        } else {
            // altitude, temperature, wind, gust, mass, shrink, voltage: the
            // default because most spec perturbations are environment
            Category::Environment
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Why the host could not confirm an attack (inspect_ai's
/// `grader_failed` made specific).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectKind {
    /// The predicted check is not declared by the domain.
    NoSuchCheck,
    /// The predicted check is a copy-consistency check (review 5 F1).
    ConsistencyCheck,
    /// The patch rewrites a value locked as the person's request.
    LockedRequirement,
    /// A value no builder will meet (schema or unit bounds).
    Implausible,
    /// The patched spec fails the domain's validation.
    OutsideEnvelope,
    /// The cited evidence names nothing in the report or the design.
    BadEvidence,
    /// The attack named a category that is not in the taxonomy.
    BadCategory,
    /// No design yet, or the check could not run.
    CouldNotRun,
    /// A malformed call (patch did not parse, schema mismatch).
    Malformed,
}

impl RejectKind {
    pub fn name(self) -> &'static str {
        match self {
            RejectKind::NoSuchCheck => "no_such_check",
            RejectKind::ConsistencyCheck => "consistency_check",
            RejectKind::LockedRequirement => "locked_requirement",
            RejectKind::Implausible => "implausible",
            RejectKind::OutsideEnvelope => "outside_envelope",
            RejectKind::BadEvidence => "bad_evidence",
            RejectKind::BadCategory => "bad_category",
            RejectKind::CouldNotRun => "could_not_run",
            RejectKind::Malformed => "malformed",
        }
    }

    /// The kind behind a rejection message from `Reviewer::attack`
    /// (`harness.rs`); the messages are the contract, matched by their
    /// fixed prefixes.
    pub fn classify(message: &str) -> RejectKind {
        let m = message;
        if m.starts_with("no declared check") {
            RejectKind::NoSuchCheck
        } else if m.contains("checks that the design matches the spec") {
            RejectKind::ConsistencyCheck
        } else if m.contains("is the person's own requirement") {
            RejectKind::LockedRequirement
        } else if m.starts_with("rejected as implausible") {
            RejectKind::Implausible
        } else if m.contains("outside the domain's envelope") {
            RejectKind::OutsideEnvelope
        } else if m.starts_with("rejected: evidence") {
            RejectKind::BadEvidence
        } else if m.starts_with("rejected: category") {
            RejectKind::BadCategory
        } else if m.contains("could not run") || m.contains("no design to attack") {
            RejectKind::CouldNotRun
        } else {
            RejectKind::Malformed
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum AttackOutcome {
    /// The host ran the condition and the check failed: a regression case.
    Confirmed { id: String, margin: Option<f64> },
    /// The host ran the condition and the check passed.
    Refuted { detail: String },
    /// The host could not run it; `reason` is the message the reviewer saw.
    Rejected { kind: RejectKind, reason: String },
    /// Confirmed, then withdrawn by the reviewer after the proposer's
    /// rebuttal ([`rebuttal_round`]).
    Withdrawn { id: String, rebuttal: String, ruling: String },
}

/// One attack the reviewer raised, with everything needed to audit it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub round: u32,
    pub category: Category,
    /// Whether the reviewer named the category itself or the host inferred it.
    pub category_inferred: bool,
    pub claim: String,
    /// What the reviewer cited: a check name, a design field, a catalog row.
    pub evidence: String,
    pub check: String,
    pub spec_patch: Value,
    pub outcome: AttackOutcome,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategoryCount {
    pub raised: usize,
    pub confirmed: usize,
    pub refuted: usize,
    pub rejected: usize,
    pub withdrawn: usize,
}

/// Per-run review metrics (in `RunSummary.review` and the
/// `review metrics` harness message each round).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewMetrics {
    pub raised: usize,
    pub confirmed: usize,
    pub refuted: usize,
    pub rejected: usize,
    pub withdrawn: usize,
    /// Failing checks the reviewer cited (confirmed by the report).
    pub cited: usize,
    pub by_category: BTreeMap<String, CategoryCount>,
    pub rejected_by_kind: BTreeMap<String, usize>,
    /// Categories neither attacked nor declared clear in any verdict.
    pub unwalked: Vec<String>,
    /// Rounds in which a proposer rebuttal was heard.
    pub rebuttal_rounds: usize,
    /// Confirmed attacks the rebuttal gate did not send to a rebuttal
    /// ([`gate_rebuttals`]), by reason.
    #[serde(default)]
    pub rebuttals_skipped: usize,
    #[serde(default)]
    pub rebuttals_skipped_by_reason: BTreeMap<String, usize>,
}

/// Everything the reviewer did in a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewLog {
    pub attempts: Vec<Attempt>,
    /// `(round, check)` of each confirmed citation.
    pub cited: Vec<(u32, String)>,
    /// Categories each verdict declared walked and clear.
    pub walked_clear: BTreeSet<Category>,
    pub rebuttal_rounds: usize,
    /// `(attack id, reason)` of each confirmed attack the gate kept out of
    /// the rebuttal round.
    #[serde(default)]
    pub rebuttals_skipped: Vec<(String, String)>,
}

impl ReviewLog {
    /// Record one attack from the reviewer's input and the host's answer
    /// (`Reviewer::attack` in `harness.rs`): an ok answer confirmed it, a
    /// "not confirmed" answer refuted it, anything else rejected it.
    pub fn record(&mut self, round: u32, raw: &Value, patch: &Value, out: &ToolOutput, confirmed: Option<&Regression>) {
        let claim = raw["claim"].as_str().unwrap_or("").to_string();
        let check = raw["expect_failing_check"].as_str().unwrap_or("").to_string();
        let evidence = raw["evidence"].as_str().unwrap_or("").to_string();
        let named = raw["category"].as_str().and_then(Category::parse);
        let category = named.unwrap_or_else(|| Category::infer(&check, patch, &claim));
        let outcome = if !out.is_error {
            AttackOutcome::Confirmed {
                id: confirmed.map(|r| r.id.clone()).unwrap_or_else(|| "?".into()),
                margin: confirmed.and_then(|r| r.margin_at_confirmation),
            }
        } else if out.content.starts_with("not confirmed") {
            AttackOutcome::Refuted { detail: out.content.clone() }
        } else {
            AttackOutcome::Rejected { kind: RejectKind::classify(&out.content), reason: out.content.clone() }
        };
        self.attempts.push(Attempt { round, category, category_inferred: named.is_none(), claim, evidence, check, spec_patch: patch.clone(), outcome });
    }

    pub fn cite(&mut self, round: u32, check: &str) {
        self.cited.push((round, check.to_string()));
    }

    /// A verdict's `walked_clear` list; unknown names are ignored.
    pub fn walked(&mut self, names: &[String]) {
        for n in names {
            if let Some(c) = Category::parse(n) {
                self.walked_clear.insert(c);
            }
        }
    }

    /// Mark a confirmed attack withdrawn after the rebuttal round.
    pub fn withdraw(&mut self, id: &str, rebuttal: &str, ruling: &str) -> bool {
        for a in &mut self.attempts {
            if matches!(&a.outcome, AttackOutcome::Confirmed { id: i, .. } if i == id) {
                a.outcome = AttackOutcome::Withdrawn { id: id.into(), rebuttal: rebuttal.into(), ruling: ruling.into() };
                return true;
            }
        }
        false
    }

    pub fn metrics(&self) -> ReviewMetrics {
        let mut m = ReviewMetrics { cited: self.cited.len(), rebuttal_rounds: self.rebuttal_rounds, rebuttals_skipped: self.rebuttals_skipped.len(), ..Default::default() };
        for (_, why) in &self.rebuttals_skipped {
            *m.rebuttals_skipped_by_reason.entry(why.clone()).or_default() += 1;
        }
        let mut touched: BTreeSet<Category> = self.walked_clear.clone();
        for a in &self.attempts {
            touched.insert(a.category);
            let c = m.by_category.entry(a.category.name().into()).or_default();
            m.raised += 1;
            c.raised += 1;
            match &a.outcome {
                AttackOutcome::Confirmed { .. } => {
                    m.confirmed += 1;
                    c.confirmed += 1;
                }
                AttackOutcome::Refuted { .. } => {
                    m.refuted += 1;
                    c.refuted += 1;
                }
                AttackOutcome::Rejected { kind, .. } => {
                    m.rejected += 1;
                    c.rejected += 1;
                    *m.rejected_by_kind.entry(kind.name().into()).or_default() += 1;
                }
                AttackOutcome::Withdrawn { .. } => {
                    m.withdrawn += 1;
                    c.withdrawn += 1;
                }
            }
        }
        m.unwalked = Category::ALL.iter().filter(|c| !touched.contains(c)).map(|c| c.name().to_string()).collect();
        m
    }
}

// ------------------------------------------------------------- vetting

/// Check an attack's category and evidence before the host runs it.
/// `Ok(evidence)` is the evidence to record (derived from the report when
/// the reviewer gave none: the predicted check *is* evidence); `Err` is the
/// rejection message, with the prefixes [`RejectKind::classify`] knows.
pub fn vet(state: &DesignState, domain: &dyn DomainTools, raw: &Value) -> Result<String, String> {
    if let Some(c) = raw["category"].as_str() {
        if !c.is_empty() && Category::parse(c).is_none() {
            return Err(format!(
                "rejected: category `{c}` is not in the taxonomy; use one of {}",
                Category::ALL.iter().map(|c| c.name()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let check = raw["expect_failing_check"].as_str().unwrap_or("");
    let evidence = raw["evidence"].as_str().unwrap_or("").trim();
    if evidence.is_empty() {
        let today = state.report.as_ref().and_then(|r| r.get(check)).map(|o| o.to_string()).unwrap_or_else(|| "no report".into());
        return Ok(format!("(derived by host) check {check} today: {today}"));
    }
    let checks: Vec<&str> = domain.checks().iter().map(|(n, _)| *n).collect();
    let tokens: Vec<&str> = evidence.split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-' | '@' | '/'))).filter(|t| !t.is_empty()).collect();
    let names_check = tokens.iter().any(|t| checks.contains(t) || state.report.as_ref().is_some_and(|r| r.get(t).is_some()));
    let design = state.design.as_ref().unwrap_or(&Value::Null);
    let names_field = tokens.iter().any(|t| at_path(design, t).is_some() || at_path(&state.spec, t).is_some());
    let names_row = tokens.iter().any(|t| t.len() >= 3 && string_leaves(design).any(|s| s == *t));
    if names_check || names_field || names_row {
        Ok(evidence.to_string())
    } else {
        Err(format!(
            "rejected: evidence `{}` names no check in the report, no field of the design or spec, and no part id in the design; cite what you looked at",
            truncate(evidence, 120)
        ))
    }
}

fn at_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut v = root;
    for k in path.split('.') {
        v = match v {
            Value::Object(o) => o.get(k)?,
            Value::Array(a) => a.get(k.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(v)
}

fn string_leaves(v: &Value) -> Box<dyn Iterator<Item = &str> + '_> {
    match v {
        Value::String(s) => Box::new(std::iter::once(s.as_str())),
        Value::Array(a) => Box::new(a.iter().flat_map(string_leaves)),
        Value::Object(o) => Box::new(o.values().flat_map(string_leaves)),
        _ => Box::new(std::iter::empty()),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.into()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

// ------------------------------------------------------------- prompts

/// The taxonomy the reviewer must walk, appended to its system prompt
/// (MetaGPT's ordered checklist, `EXAMPLE_AND_INSTRUCTION`).
pub fn taxonomy_block() -> String {
    let mut s = String::from(
        "\n\nAttack taxonomy. Walk every category below in order, like a checklist, before your verdict. For each one either make an attack in that category or decide it is clear for this design. \
Every attack names its `category` and its `evidence`: the check name, design field (dotted path) or catalog/part id you looked at; the host rejects evidence that names none of these. \
The host confirms an attack only by running the named check under your spec patch as a stored condition; an attack it cannot run is rejected with the reason, and a rejection is not a finding. \
In `verdict`, list under `walked_clear` every category you walked and found nothing attackable; categories you neither attack nor clear count as unwalked in the run's metrics.\n",
    );
    for c in Category::ALL {
        s.push_str(&format!("- {}: {}\n", c.name(), c.describe()));
    }
    s
}

/// Property schema for the `attack` tool: the three original fields plus
/// `category` (enumerated) and `evidence`.
pub fn attack_properties() -> Value {
    let cats: Vec<&str> = Category::ALL.iter().map(|c| c.name()).collect();
    json!({
        "claim": {"type": "string", "description": "one sentence: the condition and the predicted failure"},
        "category": {"type": "string", "enum": cats, "description": "the taxonomy category this attack walks"},
        "evidence": {"type": "string", "description": "what you looked at: a check name from the report, a design field (dotted path), or a part/catalog id in the design, with the number that worried you"},
        "spec_patch_json": {"type": "string", "description": "RFC 7396 merge patch to the spec, as a JSON object string"},
        "expect_failing_check": {"type": "string", "description": "the check name you predict fails"}
    })
}

/// Property schema for the `verdict` tool: the summary plus `walked_clear`.
pub fn verdict_properties() -> Value {
    let cats: Vec<&str> = Category::ALL.iter().map(|c| c.name()).collect();
    json!({
        "summary": {"type": "string", "description": "your overall assessment, including non-blocking concerns and any FINDING about a locked value"},
        "walked_clear": {"type": "array", "items": {"type": "string", "enum": cats}, "description": "categories you walked and found nothing attackable in"}
    })
}

fn tool(name: &str, description: &str, props: Value) -> Value {
    let required: Vec<String> = props.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    json!({"name": name, "description": description, "strict": true,
           "input_schema": {"type": "object", "properties": props, "required": required, "additionalProperties": false}})
}

/// The proposer's tools in the rebuttal round.
pub fn proposer_tools() -> Vec<Value> {
    vec![
        tool(
            "rebut",
            "Answer one confirmed attack. `accept` if the condition is one the builder will plausibly face and the failure is real; `contest` with an argument the reviewer can act on (the condition contradicts the request text, cannot occur together with another spec value, or the number cited is not what the design holds). You cannot contest the arithmetic: the host ran the check.",
            json!({
                "attack_id": {"type": "string", "description": "the regression id as given"},
                "stance": {"type": "string", "enum": ["accept", "contest"]},
                "argument": {"type": "string", "description": "why, in two sentences at most; empty when accepting"}
            }),
        ),
        tool("done", "Finish the rebuttal once every attack has an answer.", json!({})),
    ]
}

/// The reviewer's tools when it adjudicates rebuttals.
pub fn adjudicate_tools() -> Vec<Value> {
    vec![
        tool(
            "rule",
            "Rule on one contested attack: `uphold` keeps it as a regression case; `withdraw` drops it. Withdraw only if the proposer showed the condition is not one this builder faces (it contradicts the request text or the rules of the event); the host's measurement itself is not in dispute.",
            json!({
                "attack_id": {"type": "string"},
                "ruling": {"type": "string", "enum": ["uphold", "withdraw"]},
                "reason": {"type": "string", "description": "one or two sentences"}
            }),
        ),
        tool("verdict", "Finish adjudication. Attacks without a ruling stand.", json!({"summary": {"type": "string"}})),
    ]
}

pub fn proposer_system(domain: &dyn DomainTools) -> String {
    format!(
        "You are the lead of a team designing a {}. The hostile reviewer's attacks below were each CONFIRMED by the host: it ran the named check under the attack's spec patch and the check failed on your current design. \
You get one short rebuttal round before each becomes a permanent regression case. Accept an attack whose condition a builder will plausibly face. Contest only with an argument the reviewer can act on: the condition contradicts what the person asked for (quote the request), cannot occur together with another value in the spec, or the evidence the reviewer cited is not what the design holds. Do not argue with the measurement. Call rebut once per attack, then done.",
        domain.name()
    )
}

// ------------------------------------------------------------- rebuttal

#[derive(Deserialize)]
struct Rebut {
    attack_id: String,
    stance: String,
    #[serde(default)]
    argument: String,
}

#[derive(Deserialize)]
struct Rule {
    attack_id: String,
    ruling: String,
    #[serde(default)]
    reason: String,
}

struct Proposer {
    ids: Vec<String>,
    /// contested attacks: id -> argument
    contested: BTreeMap<String, String>,
    accepted: BTreeSet<String>,
}

impl Handler for Proposer {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            match name {
                "rebut" => {
                    let r: Rebut = match serde_json::from_value(input.clone()) {
                        Ok(r) => r,
                        Err(e) => return ToolOutput::malformed(format!("input does not match the tool schema: {e}")),
                    };
                    if !self.ids.contains(&r.attack_id) {
                        return ToolOutput::err(format!("no confirmed attack `{}`; the ids are {}", r.attack_id, self.ids.join(", ")));
                    }
                    match r.stance.as_str() {
                        "accept" => {
                            self.contested.remove(&r.attack_id);
                            self.accepted.insert(r.attack_id);
                            ToolOutput::ok("accepted")
                        }
                        "contest" if r.argument.trim().is_empty() => ToolOutput::err("a contest needs an argument"),
                        "contest" => {
                            self.contested.insert(r.attack_id, r.argument);
                            ToolOutput::ok("noted; the reviewer will rule")
                        }
                        other => ToolOutput::err(format!("stance must be accept or contest, not `{other}`")),
                    }
                }
                "done" => ToolOutput::ok("rebuttal closed").stop(),
                other => ToolOutput::malformed(format!("unknown tool '{other}'")),
            }
        })
    }
}

struct Adjudicator {
    contested: BTreeSet<String>,
    /// id -> reason, for attacks withdrawn
    withdrawn: BTreeMap<String, String>,
}

impl Handler for Adjudicator {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            match name {
                "rule" => {
                    let r: Rule = match serde_json::from_value(input.clone()) {
                        Ok(r) => r,
                        Err(e) => return ToolOutput::malformed(format!("input does not match the tool schema: {e}")),
                    };
                    if !self.contested.contains(&r.attack_id) {
                        return ToolOutput::err(format!("`{}` was not contested; only contested attacks are ruled on", r.attack_id));
                    }
                    match r.ruling.as_str() {
                        "uphold" => {
                            self.withdrawn.remove(&r.attack_id);
                            ToolOutput::ok("upheld")
                        }
                        "withdraw" => {
                            self.withdrawn.insert(r.attack_id, r.reason);
                            ToolOutput::ok("withdrawn")
                        }
                        other => ToolOutput::err(format!("ruling must be uphold or withdraw, not `{other}`")),
                    }
                }
                "verdict" => ToolOutput::ok("adjudication recorded").stop(),
                other => ToolOutput::malformed(format!("unknown tool '{other}'")),
            }
        })
    }
}

/// What the rebuttal round decided.
#[derive(Debug, Default)]
pub struct Rebuttal {
    /// Attacks the reviewer withdrew: `(id, proposer's argument, ruling)`.
    pub withdrawn: Vec<(String, String, String)>,
    /// Ids the proposer accepted outright.
    pub accepted: Vec<String>,
}

/// One proposer-vs-reviewer round over this round's confirmed attacks
/// (MetaGPT `WriteCodeReview.run`: review, answer, re-review; stops when
/// nothing is contested). The proposer speaks with [`crate::harness::Models::proposer`]
/// (default: the specialist's model, `crate::roles`), the adjudicator with the reviewer's, both in fresh short
/// conversations. Any model or script error leaves every attack standing:
/// a host-confirmed failure needs no one's permission to count.
pub async fn rebuttal_round(
    client: &Client,
    models: &crate::harness::Models,
    domain: Arc<dyn DomainTools>,
    state: &DesignState,
    confirmed: &[Regression],
    log: &mut ReviewLog,
) -> Rebuttal {
    let mut out = Rebuttal::default();
    if confirmed.is_empty() {
        return out;
    }
    log.rebuttal_rounds += 1;
    let listing: Vec<Value> = confirmed
        .iter()
        .map(|r| {
            let a = log.attempts.iter().find(|a| matches!(&a.outcome, AttackOutcome::Confirmed { id, .. } if *id == r.id));
            json!({"id": r.id, "claim": r.claim, "spec_patch": r.spec_patch, "check": r.check, "margin": r.margin_at_confirmation,
                   "category": a.map(|a| a.category.name()), "evidence": a.map(|a| a.evidence.clone())})
        })
        .collect();
    let ids: Vec<String> = confirmed.iter().map(|r| r.id.clone()).collect();

    let mut conv = Conversation::new("proposer", models.proposer(), proposer_system(domain.as_ref()), proposer_tools());
    let mut p = Proposer { ids: ids.clone(), contested: BTreeMap::new(), accepted: BTreeSet::new() };
    if let Some(t) = crate::trace::Tracer::of(client) {
        t.set_current("proposer", &format!("proposer-r{}", state.round));
    }
    let task = format!(
        "Confirmed attacks this round:\n{}\n\n{}",
        serde_json::to_string_pretty(&listing).unwrap_or_default(),
        state.for_prompt()
    );
    let r = run_agent(client, &mut conv, vec![json!({"type": "text", "text": task})], &mut p, 3).await;
    out.accepted = p.accepted.into_iter().collect();
    if r.is_err() || p.contested.is_empty() {
        return out;
    }

    let contested = p.contested;
    let system = crate::prompts::reviewer_system(domain.as_ref()) + "\n\nYou are now adjudicating the proposer's rebuttals to attacks the host already confirmed. Rule on each contested one, then call verdict.";
    let mut conv = Conversation::new("reviewer", models.reviewer.clone(), system, adjudicate_tools());
    let mut adj = Adjudicator { contested: contested.keys().cloned().collect(), withdrawn: BTreeMap::new() };
    if let Some(t) = crate::trace::Tracer::of(client) {
        t.set_current("adjudicator", &format!("adjudicator-r{}", state.round));
    }
    let rebuttals: Vec<Value> = listing
        .iter()
        .filter(|l| contested.contains_key(l["id"].as_str().unwrap_or("")))
        .map(|l| {
            let mut l = l.clone();
            l["proposer_argument"] = json!(contested[l["id"].as_str().unwrap_or("")]);
            l
        })
        .collect();
    let task = format!("Contested attacks and the proposer's arguments:\n{}", serde_json::to_string_pretty(&rebuttals).unwrap_or_default());
    let _ = run_agent(client, &mut conv, vec![json!({"type": "text", "text": task})], &mut adj, 3).await;
    for (id, ruling) in adj.withdrawn {
        let argument = contested.get(&id).cloned().unwrap_or_default();
        log.withdraw(&id, &argument, &ruling);
        let _ = client.events.send(Event::AgentMessage {
            run: client.run.clone(),
            agent: "harness".into(),
            text: format!("attack {id} withdrawn after rebuttal: {ruling}"),
        });
        out.withdrawn.push((id, argument, ruling));
    }
    out
}

// ------------------------------------------------------- rebuttal gate

/// Categories whose confirmation is a measurement: the host ran a
/// deterministic check under the patch and it failed, and the proposer's
/// only allowed arguments (the condition contradicts the request, cannot
/// co-occur, or the cited number is not the design's) do not apply to a
/// unit slip, a copy mismatch, a clash or a model out of range.
pub const MEASURED: &[Category] = &[Category::UnitsSign, Category::CopyConsistency, Category::Interference, Category::ModelValidity];

/// Why a confirmed attack skipped the rebuttal (the trace's reason names).
pub const SKIP_MEASURED: &str = "measured_category";
pub const SKIP_ALREADY_FAILING: &str = "already_failing";
pub const SKIP_NOT_AN_ASSUMPTION: &str = "not_an_agent_choice";

/// Dotted paths of every leaf an RFC 7396 patch (or an assumption's JSON
/// head) sets; keys that already hold dots stay as they are.
pub fn patch_paths(v: &Value) -> Vec<String> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(o) if !o.is_empty() => {
                for (k, x) in o {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    walk(&p, x, out);
                }
            }
            _ if !prefix.is_empty() => out.push(prefix.to_string()),
            _ => {}
        }
    }
    let mut out = vec![];
    walk("", v, &mut out);
    out
}

/// The spec paths the agents chose themselves: the JSON object that opens
/// each `DesignState::assumptions` line (`{"launch.name": ...} (why)`).
pub fn assumption_paths(state: &DesignState) -> Vec<String> {
    state
        .assumptions
        .iter()
        .filter_map(|a| serde_json::Deserializer::from_str(a.trim_start()).into_iter::<Value>().next()?.ok())
        .flat_map(|v| patch_paths(&v))
        .collect()
}

fn overlaps(a: &str, b: &str) -> bool {
    let under = |x: &str, y: &str| x == y || x.starts_with(&format!("{y}."));
    under(a, b) || under(b, a)
}

/// Which confirmed attacks deserve a rebuttal (10/24 night: the balloon
/// demo's rebuttals withdrew nothing). Heard: an attack whose patch moves
/// a value the agents chose (`assumptions_added_by_agents`), or a
/// stock/sourcing or operator/regulatory attack (a catalog choice or a
/// rule's reading the proposer can argue from the request). Skipped,
/// with the reason recorded in `log`: a measured category ([`MEASURED`]);
/// a check that already fails on the design without the patch (the
/// deterministic report has decided it); and an environment or omission
/// perturbation of a value nobody chose (the declared envelope bounds it,
/// and the host already ran it).
pub fn gate_rebuttals(state: &DesignState, confirmed: &[Regression], log: &mut ReviewLog) -> Vec<Regression> {
    let assumed = assumption_paths(state);
    let mut heard = vec![];
    for r in confirmed {
        let cat = log
            .attempts
            .iter()
            .find(|a| matches!(&a.outcome, AttackOutcome::Confirmed { id, .. } if *id == r.id))
            .map(|a| a.category)
            .unwrap_or_else(|| Category::infer(&r.check, &r.spec_patch, &r.claim));
        let already = state.report.as_ref().and_then(|f| f.get(&r.check)).is_some_and(|o| !o.passed());
        let touches_assumption = patch_paths(&r.spec_patch).iter().any(|p| assumed.iter().any(|a| overlaps(p, a)));
        let why = if MEASURED.contains(&cat) {
            Some(SKIP_MEASURED)
        } else if already {
            Some(SKIP_ALREADY_FAILING)
        } else if touches_assumption || matches!(cat, Category::StockSourcing | Category::OperatorRegulatory) {
            None
        } else {
            Some(SKIP_NOT_AN_ASSUMPTION)
        };
        match why {
            Some(w) => log.rebuttals_skipped.push((r.id.clone(), w.to_string())),
            None => heard.push(r.clone()),
        }
    }
    heard
}

/// One INTERNAL span `wright.rebuttal_gate` per round with confirmed
/// attacks: how many were heard and how many skipped, by reason.
pub fn trace_gate(tracer: &crate::trace::Tracer, round: u32, heard: usize, skipped: &[(String, String)]) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut attributes = BTreeMap::new();
    attributes.insert("gen_ai.operation.name".to_string(), json!("wright.rebuttal_gate"));
    attributes.insert("wright.round".to_string(), json!(round));
    attributes.insert("wright.rebuttal.heard".to_string(), json!(heard));
    attributes.insert("wright.rebuttal.skipped".to_string(), json!(skipped.len()));
    for (id, why) in skipped {
        attributes.insert(format!("wright.rebuttal.skipped.{id}"), json!(why));
    }
    tracer.record(crate::trace::Span {
        trace_id: tracer.trace_id.clone(),
        span_id: tracer.new_span_id(),
        parent_span_id: None,
        name: "wright.rebuttal_gate".into(),
        kind: crate::trace::KIND_INTERNAL,
        start_unix_nanos: now,
        end_unix_nanos: now,
        attributes,
        status: crate::trace::STATUS_OK,
        status_message: String::new(),
    });
}

/// The per-round metrics line the harness emits as an `AgentMessage`
/// from `harness` (`review metrics round N: {json}`), so a page can parse
/// it without a new event variant.
pub fn metrics_message(round: u32, m: &ReviewMetrics) -> String {
    format!("review metrics round {round}: {}", serde_json::to_string(m).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_picks_the_category_from_check_patch_and_claim() {
        assert_eq!(Category::infer("budget", &json!({"price_usd": 12}), "a price rise"), Category::StockSourcing);
        assert_eq!(Category::infer("stall", &json!({"field_altitude_m": 1000}), "a 1000 m field"), Category::Environment);
        assert_eq!(Category::infer("tube_fit", &json!({}), "liner clearance"), Category::Interference);
        assert_eq!(Category::infer("stall", &json!({}), "EAS vs TAS"), Category::UnitsSign);
        assert_eq!(Category::infer("radio_power", &json!({}), "FCC band limit"), Category::OperatorRegulatory);
    }

    #[test]
    fn classify_maps_the_host_messages() {
        assert_eq!(RejectKind::classify("no declared check 'x'"), RejectKind::NoSuchCheck);
        assert_eq!(RejectKind::classify("rejected: `c` checks that the design matches the spec it was made from"), RejectKind::ConsistencyCheck);
        assert_eq!(RejectKind::classify("rejected: target_mm is the person's own requirement; ..."), RejectKind::LockedRequirement);
        assert_eq!(RejectKind::classify("rejected as implausible: ..."), RejectKind::Implausible);
        assert_eq!(RejectKind::classify("rejected: outside the domain's envelope (...)"), RejectKind::OutsideEnvelope);
        assert_eq!(RejectKind::classify("rejected: evidence `x` names no check"), RejectKind::BadEvidence);
        assert_eq!(RejectKind::classify("attack could not run: boom"), RejectKind::CouldNotRun);
    }

    #[test]
    fn metrics_count_by_category_and_unwalked() {
        let mut log = ReviewLog::default();
        let ok = ToolOutput::ok("confirmed (r1), recorded as a regression case: FAIL span -3.000 mm");
        let reg = Regression { id: "r1".into(), domain: "t".into(), claim: "c".into(), spec_patch: json!({"shrink_mm": 8}), check: "span".into(), margin_at_confirmation: Some(-3.0) };
        log.record(1, &json!({"claim": "c", "expect_failing_check": "span", "category": "environment", "evidence": "span"}), &json!({"shrink_mm": 8}), &ok, Some(&reg));
        assert_eq!(log.attempts[0].outcome, AttackOutcome::Confirmed { id: "r1".into(), margin: Some(-3.0) });
        let bad = ToolOutput::err("rejected: evidence `moon` names no check in the report");
        log.record(1, &json!({"claim": "c", "expect_failing_check": "span", "category": "omission", "evidence": "moon"}), &json!({}), &bad, None);
        log.walked(&["interference".into(), "nonsense".into()]);
        let m = log.metrics();
        assert_eq!((m.raised, m.confirmed, m.rejected), (2, 1, 1));
        assert_eq!(m.by_category["environment"].confirmed, 1);
        assert_eq!(m.rejected_by_kind["bad_evidence"], 1);
        assert_eq!(m.unwalked.len(), 5);
        assert!(!m.unwalked.contains(&"interference".to_string()));
        assert!(log.withdraw("r1", "arg", "ruling"));
        assert_eq!(log.metrics().withdrawn, 1);
    }

    #[test]
    fn the_gate_hears_attacks_on_agent_choices_only() {
        let mut st = DesignState::new("balloon", "r", json!({}));
        st.assumptions = vec![
            r#"{"recovery_window_h": 4.0} (schema default: 4 h)"#.into(),
            r#"{"launch": {"elevation_m":75.0,"lat_deg":36.33}} (unattended default)"#.into(),
            "free text with no json".into(),
        ];
        assert_eq!(assumption_paths(&st), vec!["recovery_window_h", "launch.elevation_m", "launch.lat_deg"]);
        let reg = |id: &str, patch: Value, check: &str| Regression { id: id.into(), domain: "balloon".into(), claim: "c".into(), spec_patch: patch, check: check.into(), margin_at_confirmation: Some(-1.0) };
        let regs = vec![
            reg("a", json!({"recovery_window_h": 48}), "power_margin"),
            reg("b", json!({"launch": {"elevation_m": 3000}}), "landing_speed"),
            reg("c", json!({"ground_temperature_c": -5}), "part_temperature"),
        ];
        let mut log = ReviewLog::default();
        let heard = gate_rebuttals(&st, &regs, &mut log);
        assert_eq!(heard.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(log.rebuttals_skipped, vec![("c".to_string(), SKIP_NOT_AN_ASSUMPTION.to_string())]);
        assert_eq!(log.metrics().rebuttals_skipped, 1);
    }
}
