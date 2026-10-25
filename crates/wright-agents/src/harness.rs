//! The run: an evaluator-optimizer loop around orchestrator-workers.
//!
//! Pattern from Anthropic's "Building effective agents" (orchestrator-
//! workers; evaluator-optimizer "when there are clear evaluation criteria"
//! -- here the checks) and the multi-agent research system (sub-agents
//! return condensed findings; judge the end state). Concretely, per round:
//!
//! 1. the **orchestrator** (Opus 5.5, Fable 5.1 in demo mode, effort medium) works the shared
//!    [`DesignState`] with the engineering tools and `delegate`s focused
//!    tasks to **specialists** (Opus 5.5, effort medium; roles per domain in
//!    [`crate::prompts::roles`]), each a fresh conversation of at most 8
//!    turns returning a <=300-token summary -- the Claude Agent SDK's
//!    `AgentDefinition` sub-agents whose output returns as one tool result
//!    (claude-agent-sdk-python @0e89b29, `types.py:2405-2415`);
//! 2. the harness re-runs every check (free and deterministic);
//! 3. a **hostile reviewer** (Opus 5.5, effort high) sees
//!    only the state and the report, and can block only with a failing check
//!    or an attack the harness confirms by running it; a confirmed attack
//!    becomes a permanent [`Regression`];
//! 4. stop when every check passes and nothing confirmed blocks, or on the
//!    round or budget limit; the final state is judged by the checks, not by
//!    any agent's say-so.
//!
//! Every step goes out as a `wright_core::Event` on the run's broadcast
//! channel; an `Event::Feedback` (a person's sketch on a render plus a note)
//! sent on the same channel reaches the orchestrator as an image block and
//! text in its next user message.

use crate::agent::{run_agent, AgentEnd, AgentError, Conversation, Handler, ModelCfg, ToolOutput};
use crate::client::{BoxFut, CallError, Client};
use crate::prompts;
use crate::state::{append_regression, load_regressions_reporting, move_rejected, rejected_path, DesignState, Regression};
use crate::tools::{merge_patch, DomainTools};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use wright_core::{Event, Evidence};

/// ~4 characters per token: the <=300-token cap on a specialist summary.
pub(crate) const SUMMARY_CHARS: usize = 1200;

#[derive(Debug, Clone)]
pub struct Models {
    pub orchestrator: ModelCfg,
    pub specialist: ModelCfg,
    pub reviewer: ModelCfg,
    /// The rebuttal proposer; `None` = the specialist's model
    /// ([`Models::proposer`], `crate::roles`).
    pub proposer: Option<ModelCfg>,
    /// Spec elicitation (`crate::elicit`).
    pub elicit: ModelCfg,
}

impl Models {
    /// The role table (`crate::roles`). Pat (10/24 22:10): Opus 5.5 leads
    /// by default; `demo` (`--demo` / `WRIGHT_DEMO=1`) puts Fable 5.1 on
    /// the orchestrator. The CAD role selects subscription-backed Astra
    /// separately in `delegate`.
    pub fn standard(demo: bool) -> Self {
        let lead = if demo { "claude-fable-5-1" } else { "claude-opus-5-5" };
        Models {
            orchestrator: ModelCfg::new(lead, "medium", 8000),
            specialist: ModelCfg::new("claude-opus-5-5", "medium", 4000),
            reviewer: ModelCfg::new("claude-opus-5-5", "high", 8000),
            proposer: None,
            elicit: ModelCfg::new("claude-sonnet-5-5", "low", 8000),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub run_id: String,
    /// What the person asked for, in their words.
    pub request: String,
    /// Initial user reference text/image blocks, forwarded to the lead and CAD specialist.
    pub reference_blocks: Vec<Value>,
    /// The starting spec (requirements only is fine; `null` lets the
    /// orchestrator write it with `edit_spec`).
    pub spec: Value,
    pub models: Models,
    pub max_rounds: u32,
    pub orchestrator_turns: u32,
    pub specialist_turns: u32,
    pub reviewer_turns: u32,
    /// JSON-lines file of confirmed regressions, loaded at start and
    /// appended to.
    pub regressions_path: Option<PathBuf>,
    /// Where to write state, BOM and build artifacts at the end.
    pub out_dir: Option<PathBuf>,
    /// Post-design stages (CFD, FEA, fab, CAD, USD, sims) the orchestrator
    /// may run on the current design with `run_stage`; empty: no tool.
    pub stages: wright_core::Stages,
    /// Hear a proposer-vs-reviewer rebuttal round before each confirmed
    /// attack is stored (`crate::review::rebuttal_round`); two extra
    /// short model calls per round with confirmed attacks. Off by default
    /// so scripted tests keep their call counts; the CLI turns it on.
    pub rebuttal: bool,
    /// Continue from this checkpoint (`wright run --resume RUN_DIR`;
    /// `crate::checkpoint`): state, orchestrator history, round, spend.
    pub resume: Option<Box<crate::checkpoint::Checkpoint>>,
    /// The in-process pause / approve / stop channel the desktop holds;
    /// the `<out_dir>/control/` mailbox works with or without it.
    pub control: Option<crate::checkpoint::Control>,
    /// Elicit the spec from the request before the team starts
    /// (`crate::elicit`: sourced assumptions, unit normalisation, at most
    /// three clarifying questions, a deterministic feasibility probe).
    /// Runs only when `spec` is `null` and the run is not resumed; one
    /// extra model call. Off by default so scripted tests keep their call
    /// counts; the CLI turns it on.
    pub elicit: Option<crate::elicit::ElicitConfig>,
    /// Run independent specialists delegated in one turn concurrently and
    /// merge their edits (`crate::merge`; roles with a declared dependency
    /// on each other, and `cad`, still run one at a time). Default on.
    pub parallel_specialists: bool,
    /// The runs database (`crate::runs_db`): queried at the start for the
    /// "Prior efforts" block (nearest prior requests, best prior design,
    /// known failures; nothing when the store has no match) and given this
    /// run's record at the end. `None` (default): neither.
    pub runs_db: Option<std::sync::Arc<dyn crate::runs_db::RunStore>>,
}

impl RunConfig {
    pub fn new(run_id: &str, request: &str, spec: Value) -> Self {
        RunConfig {
            run_id: run_id.into(),
            request: request.into(),
            reference_blocks: vec![],
            spec,
            models: Models::standard(false),
            max_rounds: 4,
            orchestrator_turns: 16,
            specialist_turns: 8,
            reviewer_turns: 6,
            regressions_path: None,
            out_dir: None,
            stages: wright_core::Stages::new(),
            rebuttal: false,
            resume: None,
            control: None,
            elicit: None,
            parallel_specialists: true,
            runs_db: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    Passed,
    RoundLimit,
    Budget(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub passed: bool,
    pub stop: StopReason,
    pub rounds: u32,
    pub spent_usd: f64,
    pub objective: Option<(f64, String)>,
    pub state: DesignState,
    /// What `Domain::build` wrote for a passing design (CAD, prints,
    /// params), with the kinds it declared: the build guide is made from
    /// these, never from a listing of the out dir (review 4, finding 6).
    pub artifacts: Option<wright_core::Artifacts>,
    /// What each post-design stage did ([`RunConfig::stages`]), for a
    /// design that passed and was built; a failed stage is here with its
    /// error, never a failed run.
    pub stages: Vec<wright_core::StageRun>,
    /// What the hostile reviewer did: attacks raised / confirmed / refuted
    /// / rejected / withdrawn, by taxonomy category (`crate::review`).
    pub review: crate::review::ReviewMetrics,
}

// ---------------------------------------------------------------- inputs

fn arg<T: DeserializeOwned>(input: &Value) -> Result<T, ToolOutput> {
    serde_json::from_value(input.clone()).map_err(|e| ToolOutput::malformed(format!("input does not match the tool schema: {e}")))
}

fn patch(json_text: &str) -> Result<Value, ToolOutput> {
    match serde_json::from_str::<Value>(json_text) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err(ToolOutput::malformed("patch_json must be a JSON object")),
        Err(e) => Err(ToolOutput::malformed(format!("patch_json is not valid JSON: {e}"))),
    }
}

#[derive(Deserialize)]
struct PatchArgs {
    patch_json: String,
    why: String,
    /// Dotted spec paths the orchestrator chose itself (the person did not
    /// state them). On the first spec write of a run with no spec, every
    /// other value is locked as the person's request (review 4, finding 3:
    /// locking only what the orchestrator volunteered left everything open
    /// by default). Later writes are all assumptions.
    #[serde(default)]
    assumptions: Vec<String>,
    /// The earlier, inverted field (paths the person stated). Still
    /// accepted so older scripts parse; those paths are locked, and since
    /// locking is now the default it changes nothing else.
    #[serde(default)]
    #[allow(dead_code)]
    from_request: Vec<String>,
}
#[derive(Deserialize)]
struct Empty {}
#[derive(Deserialize)]
struct CatalogArgs {
    path: String,
    #[serde(default)]
    query: String,
}
/// At most this many catalog rows per lookup (the coordinator's ask; a
/// tool result stays a few hundred tokens).
const CATALOG_MAX: usize = 20;
#[derive(Deserialize)]
struct Delegate {
    role: String,
    task: String,
    /// The plan task this delegation does ([`crate::plan`]); "" before a plan exists.
    #[serde(default)]
    plan_task: String,
}
#[derive(Deserialize)]
struct StageArgs {
    stage: String,
}
#[derive(Deserialize)]
struct Summary {
    summary: String,
    /// Reviewer's verdict only: taxonomy categories walked and clear.
    #[serde(default)]
    walked_clear: Vec<String>,
}
#[derive(Deserialize)]
struct Cite {
    check: String,
    argument: String,
}
#[derive(Deserialize)]
struct Attack {
    claim: String,
    spec_patch_json: String,
    expect_failing_check: String,
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}... [truncated]", s.chars().take(n).collect::<String>())
    }
}

// ------------------------------------------------------------- workbench

/// The engineering tools over the shared state, used by the orchestrator
/// and every specialist.
pub(crate) struct Workbench<'s> {
    pub(crate) cad: &'s mut wright_cad::Session,
    pub(crate) domain: Arc<dyn DomainTools>,
    pub(crate) state: &'s mut DesignState,
    pub(crate) client: Client,
    pub(crate) edits: Vec<String>,
    /// Only the orchestrator may change the spec; a specialist changes the
    /// design (a specialist once "passed" rail exit by setting the launch
    /// rail to 12 m -- a live run, 2026-10-24).
    pub(crate) may_edit_spec: bool,
    /// The run directory (`sweep` writes under `<out_dir>/sweeps/`).
    pub(crate) out_dir: Option<PathBuf>,
}

impl Workbench<'_> {
    fn recheck(&mut self) -> anyhow::Result<()> {
        let fin = self.state.full_check(self.domain.as_ref())?;
        emit_checks(&self.client, self.domain.as_ref(), self.state, &fin);
        self.state.offer(self.domain.as_ref(), &fin);
        self.state.report = Some(fin);
        // a failing check a done plan task vouched for makes it and its dependents stale
        crate::plan::after_check(self.state, &self.client);
        Ok(())
    }

    fn report_output(&self, prefix: &str) -> ToolOutput {
        let r = self.state.report.as_ref().expect("recheck sets report");
        let plan = self.state.plan.notice.as_deref().map(|n| format!("\n{n}")).unwrap_or_default();
        let text = format!("{prefix}{r}{plan}");
        if r.passed() {
            ToolOutput::ok(text)
        } else {
            ToolOutput::err(text)
        }
    }

    /// `None`: not a workbench tool.
    fn call(&mut self, name: &str, input: &Value) -> Option<ToolOutput> {
        Some(match name {
            "cad_action" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct CadArgs { action: String, args_json: String }
                let a: CadArgs = match arg(input) { Ok(a) => a, Err(e) => return Some(e) };
                let params: Value = match serde_json::from_str(&a.args_json) {
                    Ok(v) => v,
                    Err(e) => return Some(ToolOutput::malformed(format!("invalid CAD args_json: {e}"))),
                };
                match self.cad.action(&a.action, params) {
                    Ok(result) => {
                        if ["save","save_preview"].contains(&a.action.as_str()) {
                            for (key, label) in [("native", "Editable FreeCAD document"), ("step", "CAD STEP assembly"), ("glb", "CAD assembly preview")] {
                                if let Some(path) = result.get(key).and_then(Value::as_str) {
                                    let _ = self.client.events.send(Event::Artifact { run: self.client.run.clone(), label: label.into(), path: path.into() });
                                }
                            }
                        }
                        if a.action == "analyze_wing" {
                            for artifact in result.get("artifacts").and_then(Value::as_array).into_iter().flatten() {
                                if let (Some(label),Some(path))=(artifact["label"].as_str(),artifact["path"].as_str()) {
                                    let _=self.client.events.send(Event::Artifact{run:self.client.run.clone(),label:label.into(),path:path.into()});
                                }
                            }
                        }
                        if !["inspect", "check", "check_containment", "analyze_wing"].contains(&a.action.as_str()) {
                            let note = format!("CAD {} completed. CAD edits are separate from the resolved domain design; reconcile dimensions before claiming full validation.", a.action);
                            self.edits.push(note.clone());
                            self.state.notes.push(note);
                        }
                        let mut text_result=result.clone();
                        if let Some(fields)=text_result.as_object_mut(){fields.remove("content_blocks");}
                        let text = serde_json::to_string_pretty(&text_result).unwrap_or_default();
                        if result.get("passed") == Some(&Value::Bool(false)) { ToolOutput::err(text) }
                        else { ToolOutput::ok(text).with_blocks(result.get("content_blocks").and_then(Value::as_array).cloned().unwrap_or_default()) }
                    }
                    Err(e) => ToolOutput::err(format!("CAD action failed: {e:#}")),
                }
            }
            "edit_spec" if !self.may_edit_spec => ToolOutput::err(
                "specialists change the design, not the spec: the spec is what the builder faces. \
                 If the spec looks wrong, say so in your report to the lead",
            ),
            "edit_spec" => self.edit(input, true),
            "edit_design" => self.edit(input, false),
            "design" => {
                if let Err(o) = arg::<Empty>(input) {
                    return Some(o);
                }
                // the search sees every confirmed regression case, so a
                // re-design after an attack is screened against it
                let conditions = match self.state.conditions(self.domain.as_ref()) {
                    Ok(c) => c,
                    Err(e) => return Some(ToolOutput::err(format!("design failed: the spec does not validate: {e:#}"))),
                };
                let mut screened = if conditions.is_empty() {
                    String::new()
                } else {
                    format!(" (searched with {} regression case(s): {})", conditions.len(), conditions.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", "))
                };
                // a search that finds nothing passing every stored case must
                // not leave the team with no design at all (review 5, F6):
                // design from the spec alone and say which cases still fail
                let designed = match self.domain.design_under(&self.state.spec, &conditions) {
                    Err(e) if !conditions.is_empty() => match self.domain.design_under(&self.state.spec, &[]) {
                        Ok(d) => {
                            screened = format!(
                                " WITHOUT the regression cases: the search found no design that passes all {} of them ({e:#}); this is the design from the spec alone, and the report below shows which cases still fail",
                                conditions.len()
                            );
                            Ok(d)
                        }
                        Err(_) => Err(e),
                    },
                    other => other,
                };
                match designed {
                    Err(e) => ToolOutput::err(format!("design failed: {e:#}")),
                    Ok(d) => {
                        self.state.design = Some(d);
                        self.state.version += 1;
                        self.edits.push(format!("re-designed from the spec{screened}"));
                        match self.recheck() {
                            Err(e) => ToolOutput::err(format!("designed, but check could not run: {e:#}")),
                            Ok(()) => {
                                let obj = self
                                    .domain
                                    .objective(self.state.design.as_ref().unwrap())
                                    .map(|(v, l)| format!("objective {l}: {v:.3}\n"))
                                    .unwrap_or_default();
                                self.report_output(&format!("designed{screened}.\n{obj}"))
                            }
                        }
                    }
                }
            }
            "check" => {
                if let Err(o) = arg::<Empty>(input) {
                    return Some(o);
                }
                match self.recheck() {
                    Err(e) => ToolOutput::err(format!("check could not run: {e:#}")),
                    Ok(()) => self.report_output(""),
                }
            }
            "catalog" => self.catalog(input),
            // lane 8 (src/sourcing.rs): ranked search, alternates, BOM diff
            "catalog_search" | "alternates" | "bom_diff" => match crate::sourcing::workbench_call(self.domain.as_ref(), self.state, name, input) {
                Some(Ok(text)) => ToolOutput::ok(text),
                Some(Err(crate::sourcing::Fail::Malformed(m))) => ToolOutput::malformed(m),
                Some(Err(crate::sourcing::Fail::Err(m))) => ToolOutput::err(m),
                None => return None,
            },
            "sweep" => crate::sweep::tool_call(self.domain.as_ref(), self.state, &self.client.events, &self.client.run, self.out_dir.as_deref(), input),
            "revert_best" => {
                if let Err(o) = arg::<Empty>(input) {
                    return Some(o);
                }
                self.state.rescore_best(self.domain.as_ref());
                let Some(b) = self.state.best.clone() else {
                    return Some(ToolOutput::err("no design has been verified yet: call design, then check"));
                };
                self.state.design = Some(b.design);
                self.state.version += 1;
                self.edits.push(format!("reverted to the best verified design (round {})", b.round));
                match self.recheck() {
                    Err(e) => ToolOutput::err(format!("reverted, but check could not run: {e:#}")),
                    Ok(()) => self.report_output(&format!("reverted to the best verified design (from round {}).\n", b.round)),
                }
            }
            "bom" => {
                if let Err(o) = arg::<Empty>(input) {
                    return Some(o);
                }
                let Some(d) = &self.state.design else {
                    return Some(ToolOutput::err("no design yet: call design first"));
                };
                match self.domain.bom(d) {
                    Err(e) => ToolOutput::err(format!("bom failed: {e:#}")),
                    Ok(b) => {
                        let unpriced = b.unpriced().len();
                        let text = format!("{}total ${:.2}; {unpriced} unpriced lines", b.csv(), b.total().as_usd());
                        if unpriced > 0 {
                            ToolOutput::err(text)
                        } else {
                            ToolOutput::ok(text)
                        }
                    }
                }
            }
            _ => return None,
        })
    }

    /// The read-only lookup beside the edit tools, after SWE-agent's
    /// `search_dir` (SWE-agent/SWE-agent `tools/search/bin/search_dir`):
    /// a capped list with a total, "narrow your search" past the cap, and a
    /// plain "no matches" that names what was searched.
    fn catalog(&self, input: &Value) -> ToolOutput {
        catalog_lookup(self.domain.as_ref(), self.state.design.as_ref(), input)
    }
}

/// Read-only catalog lookup, shared by the workbench and the hostile
/// reviewer (lane 4: stock/sourcing attacks cite catalog rows as evidence).
pub(crate) fn catalog_lookup(domain: &dyn DomainTools, design: Option<&Value>, input: &Value) -> ToolOutput {
        let a: CatalogArgs = match arg(input) {
            Ok(a) => a,
            Err(o) => return o,
        };
        let paths = domain.catalog_paths();
        let listing = || -> String {
            if paths.is_empty() {
                format!("the {} domain exposes no catalog lookups yet", domain.name())
            } else {
                paths.iter().map(|(p, d)| format!("- {p}: {d}")).collect::<Vec<_>>().join("\n")
            }
        };
        let path = a.path.trim();
        if path.is_empty() {
            return ToolOutput::ok(format!("catalog-backed design paths:\n{}", listing()));
        }
        if !paths.iter().any(|(p, _)| *p == path) {
            return ToolOutput::err(format!("`{path}` is not a catalog-backed path. Catalog-backed paths:\n{}", listing()));
        }
        let query = Some(a.query.trim()).filter(|q| !q.is_empty());
        let opts = match design {
            Some(design) => match domain.options(design, path, query) {
                Ok(o) => o,
                Err(e) => return ToolOutput::err(format!("catalog lookup failed: {e:#}")),
            },
            // no design yet: domains whose catalog does not depend on the
            // design still answer, so a specialist can look up ids for the spec
            None => match domain.options_before_design(path, query) {
                Some(o) => o,
                None => return ToolOutput::err("no design yet: call design first (this domain's catalog rows depend on the design)"),
            },
        };
        let what = match query {
            Some(q) => format!("\"{q}\" at {path}"),
            None => path.to_string(),
        };
        if opts.is_empty() {
            return ToolOutput::ok(format!("No options found for {what}. Try a shorter query, or \"\" for all."));
        }
        let mut out = format!("Found {} options for {what}", opts.len());
        if opts.len() > CATALOG_MAX {
            out.push_str(&format!(" (showing the first {CATALOG_MAX}; narrow the query to see others)"));
        }
        out.push_str(":\n");
        for o in opts.iter().take(CATALOG_MAX) {
            let specs: Vec<String> = o.specs.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let price = o.price_usd.map_or("unpriced".into(), |p| format!("${p:.2}"));
            let stock = match o.in_stock {
                Some(true) => "in stock",
                Some(false) => "OUT OF STOCK",
                None => "stock unknown",
            };
            out.push_str(&format!("- {} | {} | {} | {price} | {stock} | {}\n", o.id, o.label, specs.join(", "), o.url));
        }
        ToolOutput::ok(out)
}

impl Workbench<'_> {

    fn edit(&mut self, input: &Value, spec: bool) -> ToolOutput {
        let a: PatchArgs = match arg(input) {
            Ok(a) => a,
            Err(o) => return o,
        };
        let p = match patch(&a.patch_json) {
            Ok(p) => p,
            Err(o) => return o,
        };
        if spec {
            if let Some(path) = touches_stated(&self.state.stated, &p, String::new()) {
                return ToolOutput::err(format!(
                    "edit rejected: {path} is the person's own requirement ({}); design to it, or say \
                     in your summary that it cannot be met",
                    self.state.stated_at(&path)
                ));
            }
        } else if let Some(key) = shares_spec_key(&self.state.spec, &self.state.stated, &p) {
            // A design may carry its own copy of the requirements (wright-air
            // judges design.requirements): editing it would move what the
            // checks judge without touching the locked spec (review 4,
            // finding 1).
            return ToolOutput::err(format!(
                "edit rejected: `{key}` is part of the spec, not the design; the design's copy follows the \
                 spec and is not edited by hand. Change the design's own fields, or (lead only) the spec \
                 with edit_spec if it is an assumption"
            ));
        }
        let mut next = if spec {
            if self.state.spec.is_null() {
                json!({})
            } else {
                self.state.spec.clone()
            }
        } else {
            match &self.state.design {
                Some(d) => d.clone(),
                None => return ToolOutput::err("no design yet: call design first"),
            }
        };
        merge_patch(&mut next, &p);
        // a made-up part id is rejected here with the nearest real ids
        // (review 5, F9; lane 8, src/sourcing.rs)
        if !spec {
            if let Some(msg) = self.state.design.as_ref().and_then(|d| crate::sourcing::gate_patch(self.domain.as_ref(), d, &p)) {
                return ToolOutput::err(msg);
            }
        }
        let checked = if spec { self.domain.validate_spec(&next) } else { self.domain.validate_design(&next) };
        match checked {
            Err(e) => ToolOutput::err(format!("edit rejected, nothing changed: {e:#}")),
            Ok(v) => {
                let what = if spec { "spec" } else { "design" };
                self.edits.push(format!("{what} {} ({})", a.patch_json, a.why));
                if spec {
                    // A run that started with no spec: the first spec the
                    // orchestrator writes from the person's words is what they
                    // asked for, and every value in it is locked, except the
                    // paths it lists under `assumptions` (values it chose
                    // itself, which the reviewer must challenge). Locking by
                    // default, not by the orchestrator volunteering
                    // `from_request`, is review 4's finding 3; the
                    // `assumptions` escape is the live balloon run's lesson (a
                    // 0.95 recovery target it invented was locked and stalled
                    // four rounds). Later writes are all assumptions: they
                    // never add locks.
                    let first = self.state.stated.is_null();
                    let assumed = if first {
                        let (said, assumed) = split_paths(&p, &a.assumptions, String::new(), false);
                        self.state.stated = said;
                        assumed
                    } else {
                        p.clone()
                    };
                    if assumed.as_object().is_some_and(|m| !m.is_empty()) {
                        self.state.assumptions.push(format!("{assumed} ({})", a.why));
                    }
                    self.state.spec = v;
                    // a stored case may leave the envelope under the new
                    // spec (an altitude-dependent hot-day limit)
                    let aside = self.state.screen_regressions(self.domain.as_ref());
                    // the incumbent's ranking was under the old spec
                    self.state.rescore_best(self.domain.as_ref());
                    if !aside.is_empty() {
                        return ToolOutput::ok(format!(
                            "{what} updated. Run check to see the margins. Under this spec, stored regression cases left the domain's envelope and no longer apply:\n- {}",
                            aside.join("\n- ")
                        ));
                    }
                } else {
                    self.state.design = Some(v);
                    self.state.version += 1;
                }
                ToolOutput::ok(format!("{what} updated. Run check to see the margins."))
            }
        }
    }
}

pub(crate) fn emit_checks(client: &Client, domain: &dyn DomainTools, state: &DesignState, fin: &wright_core::Finished) {
    let failed = fin.failures().count();
    let _ = client.events.send(Event::Checks {
        run: client.run.clone(),
        passed: fin.outcomes.len() - failed,
        failed,
        outcomes: fin.outcomes.clone(),
    });
    if let Some(design) = &state.design {
        if let Ok(items) = domain.headlines(&state.spec, design) {
            if !items.is_empty() {
                let _ = client.events.send(Event::Headlines { run: client.run.clone(), items });
            }
        }
    }
}

// -------------------------------------------------------------- feedback

/// `Event::Feedback` for this run, read off the broadcast channel.
struct Inbox {
    rx: broadcast::Receiver<Event>,
    run: String,
    client: Client,
    /// Where the studio saves sketches (its artifacts dir = the run's out
    /// dir): a relative `sketch_png` is resolved against it, never the
    /// process's working directory.
    base: Option<std::path::PathBuf>,
}

impl Inbox {
    /// Content blocks for every feedback event waiting, plus their notes.
    fn drain(&mut self) -> (Vec<Value>, Vec<String>) {
        let (mut blocks, mut notes) = (vec![], vec![]);
        loop {
            match self.rx.try_recv() {
                // only this run's: an empty or other run id is not ours (review 4,
                // finding 14; the studio refuses those too)
                Ok(Event::Feedback { run, view, sketch_png, note }) if run == self.run => {
                    let path = match (&self.base, std::path::Path::new(&sketch_png)) {
                        (Some(b), p) if p.is_relative() && !sketch_png.starts_with("data:") && b.join(p).exists() => {
                            b.join(p).to_string_lossy().into_owned()
                        }
                        _ => sketch_png.clone(),
                    };
                    blocks.extend(feedback_blocks(&view, &path, &note));
                    notes.push(format!("person, on view '{view}': {note}"));
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    let _ = self.client.events.send(Event::AgentMessage {
                        run: self.run.clone(),
                        agent: "harness".into(),
                        text: format!("event channel lagged by {n}; feedback may have been dropped -- use a larger channel"),
                    });
                }
                Err(_) => break,
            }
        }
        (blocks, notes)
    }
}

/// A sketch as an image block: a `data:` URL, a bare base64 PNG, an
/// http(s) URL, or a file path. Followed by the person's words.
pub fn feedback_blocks(view: &str, sketch_png: &str, note: &str) -> Vec<Value> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let image = if let Some(data) = sketch_png.strip_prefix("data:image/png;base64,") {
        Some(json!({"type": "base64", "media_type": "image/png", "data": data}))
    } else if sketch_png.starts_with("http://") || sketch_png.starts_with("https://") {
        Some(json!({"type": "url", "url": sketch_png}))
    } else if let Ok(bytes) = std::fs::read(sketch_png) {
        Some(json!({"type": "base64", "media_type": "image/png", "data": b64.encode(bytes)}))
    } else if !sketch_png.is_empty() && b64.decode(sketch_png).is_ok() {
        Some(json!({"type": "base64", "media_type": "image/png", "data": sketch_png}))
    } else {
        None
    };
    let mut out = vec![];
    let lead = match image {
        Some(source) => {
            out.push(json!({"type": "image", "source": source}));
            format!("A person drew on the render '{view}' (the image above, their strokes over the render) and wrote: {note}")
        }
        None => format!("A person left feedback on the render '{view}' (the sketch could not be loaded from {sketch_png:?}): {note}"),
    };
    out.push(json!({"type": "text", "text": lead}));
    out
}

// ------------------------------------------------------------- handlers

/// Keep only the newest user reference batch and newest CAD render batch.
/// Images from other tools/roles are not retained; source order and provenance survive.
#[derive(Default)]
pub(crate) struct CadVisualContext {
    sequence:u64,
    user:Option<(u64,Result<Vec<Value>,String>)>,
    render:Option<(u64,Result<Vec<Value>,String>)>,
}
fn has_image(blocks:&[Value])->bool {
    blocks.iter().any(|b| matches!(b["type"].as_str(),Some("image"|"image_url")) || b["content"].as_array().is_some_and(|a|has_image(a)))
}
impl CadVisualContext {
    fn remember(&mut self, blocks:Vec<Value>, render:bool, provenance:&str) {
        if !has_image(&blocks){return;}
        self.sequence+=1;
        let record=crate::codex::validate_images(&json!({"messages":[{"role":"user","content":blocks}]}))
            .map(|_|{let mut out=vec![json!({"type":"text","text":format!("CAD visual context: {provenance}. Snapshot evidence only; image labels do not establish current geometry or safety. This is the latest batch from this source, replacing older batches.")})];out.extend(blocks);out})
            .map_err(|e|e.to_string());
        if render {self.render=Some((self.sequence,record));} else {self.user=Some((self.sequence,record));}
    }
    fn user_blocks(&mut self,blocks:&[Value]) {
        let direct:Vec<Value>=blocks.iter().filter(|b|matches!(b["type"].as_str(),Some("text"|"image"|"image_url"))).cloned().collect();
        self.remember(direct,false,"user reference / render markup, with original accompanying text");
    }
    fn tool_result(&mut self,name:&str,input:&Value,output:&ToolOutput) {
        if name=="cad_action" && !output.is_error {
            if !matches!(input["action"].as_str(),Some("component_catalog"|"analyze_wing"|"inspect"|"check"|"check_containment"|"check_translation"|"save"|"save_preview"|"render")) {self.render=None;}
            let mut blocks=vec![json!({"type":"text","text":output.content})];blocks.extend(output.blocks.clone());
            self.remember(blocks,true,&format!("native CAD tool result for action {}",input["action"]));
        }
    }
    fn blocks(&self)->Result<Vec<Value>,String> {
        let mut records:Vec<_>=self.user.iter().chain(self.render.iter()).collect();records.sort_by_key(|r|r.0);
        let mut blocks=vec![];for (_,record) in records {blocks.extend(record.as_ref().map_err(Clone::clone)?.clone());}
        crate::codex::validate_images(&json!({"messages":[{"role":"user","content":blocks}]})).map_err(|e|e.to_string())?;
        Ok(blocks)
    }
}

struct Orchestrator<'s> {
    wb: Workbench<'s>,
    cfg: RunConfig,
    /// "Lessons from earlier runs" section (`crate::memory`), appended to
    /// every specialist's system prompt as it is to the orchestrator's.
    lessons: String,
    inbox: &'s mut Inbox,
    fatal: Option<CallError>,
    visuals: &'s mut CadVisualContext,
    /// The person's gate (pause / approve / stop) and the checkpoint
    /// writer (`crate::checkpoint`); `turns_before` is what a resumed
    /// round had already used.
    keeper: &'s mut crate::checkpoint::Keeper,
    ckpt: &'s mut crate::checkpoint::Checkpointer,
    round: u32,
    turns_before: u32,
}

impl Handler for Orchestrator<'_> {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            // a paused run holds the call here until a person acts
            let gate = match self.keeper.before_tool(name).await {
                Ok(note) => note,
                Err(out) => return out,
            };
            let out = async {
            if let Some(out) = self.wb.call(name, input) {
                return out;
            }
            match name {
                "submit" => match arg::<Summary>(input) {
                    Ok(_) => ToolOutput::ok("submitted to the reviewer").stop(),
                    Err(o) => o,
                },
                "delegate" => {
                    let d: Delegate = match arg(input) {
                        Ok(d) => d,
                        Err(o) => return o,
                    };
                    let roles = prompts::roles(self.wb.domain.name());
                    if !roles.iter().any(|r| r.0 == d.role) {
                        let names: Vec<&str> = roles.iter().map(|r| r.0).collect();
                        return ToolOutput::malformed(format!("no specialist '{}'; roles are {names:?}", d.role));
                    }
                    // the plan names the task; the harness moves it running -> done | blocked
                    let title = match crate::plan::begin_delegation(self.wb.state, &self.wb.client, &d.role, &d.plan_task) {
                        Ok(t) => t,
                        Err(e) => return ToolOutput::err(e),
                    };
                    let task = match title {
                        Some(t) => format!("[plan task {}: {t}] {}", d.plan_task.trim(), d.task),
                        None => d.task.clone(),
                    };
                    let out = self.delegate(&d.role, &task).await;
                    match crate::plan::end_delegation(self.wb.state, &self.wb.client, &d.plan_task) {
                        Some(line) => ToolOutput { content: format!("{}\n{line}", out.content), ..out },
                        None => out,
                    }
                }
                "plan_set" | "plan_update" => {
                    crate::plan::call(self.wb.state, &self.wb.client, self.wb.domain.as_ref(), name, input).expect("plan tool")
                }
                "run_stage" => match arg::<StageArgs>(input) {
                    Ok(a) => self.run_stage(&a.stage),
                    Err(o) => o,
                },
                other => ToolOutput::malformed(format!("unknown tool '{other}'")),
            }
            }.await;
            crate::checkpoint::with_note(out, gate)
        })
    }
    fn after_turn(&mut self, conv: &Conversation, owed: &[Value], turns: u32) {
        let saved = crate::checkpoint::SavedConversation::of(conv, owed);
        let phase = crate::checkpoint::Phase::Orchestrating { turns_used: self.turns_before + turns };
        if let Some(Err(e)) = self.ckpt.save("step", &self.cfg, &self.wb.client, self.wb.state, self.round, phase, self.keeper.gate(), Some(saved)) {
            let _ = self.wb.client.events.send(Event::AgentMessage { run: self.cfg.run_id.clone(), agent: "harness".into(), text: format!("checkpoint not written: {e}") });
        }
    }
    /// Several `delegate` calls in one turn to independent roles run
    /// concurrently and are merged (`crate::merge`); anything else, or a
    /// dependent pair, falls back to the one-by-one `call`.
    fn call_many<'a>(&'a mut self, calls: &'a [(String, Value)]) -> BoxFut<'a, Option<Vec<ToolOutput>>> {
        Box::pin(async move {
            // several run_stage calls in one turn (with only read-only or
            // plan calls beside them) run together: the others first, in
            // order, then the stages as one batch (Orchestrator::run_stages)
            const BESIDE_STAGES: &[&str] = &["run_stage", "plan_update", "plan_set", "check", "catalog", "catalog_search", "alternates", "bom", "bom_diff"];
            if calls.iter().filter(|(n, _)| n == "run_stage").count() >= 2 && calls.iter().all(|(n, _)| BESIDE_STAGES.contains(&n.as_str())) {
                let mut outs: Vec<Option<ToolOutput>> = vec![None; calls.len()];
                let mut stages: Vec<(usize, String)> = vec![];
                for (i, (name, input)) in calls.iter().enumerate() {
                    if name == "run_stage" {
                        match arg::<StageArgs>(input) {
                            Ok(a) => stages.push((i, a.stage)),
                            Err(o) => outs[i] = Some(o),
                        }
                    } else {
                        outs[i] = Some(self.call(name, input).await);
                    }
                }
                if !stages.is_empty() {
                    let gate = match self.keeper.before_tool("run_stage").await {
                        Ok(note) => note,
                        Err(out) => {
                            for (i, _) in &stages {
                                outs[*i] = Some(out.clone());
                            }
                            return Some(outs.into_iter().map(Option::unwrap_or_default).collect());
                        }
                    };
                    let names: Vec<&str> = stages.iter().map(|(_, s)| s.as_str()).collect();
                    let results = self.run_stages(&names);
                    for ((i, _), out) in stages.iter().zip(results) {
                        outs[*i] = Some(crate::checkpoint::with_note(out, gate.clone()));
                    }
                }
                return Some(outs.into_iter().map(Option::unwrap_or_default).collect());
            }
            if !self.cfg.parallel_specialists || calls.iter().any(|(n, _)| n != "delegate") {
                return None;
            }
            let ds: Vec<Delegate> = calls.iter().map(|(_, i)| arg::<Delegate>(i).ok()).collect::<Option<_>>()?;
            let domain = self.wb.domain.clone();
            let roles: Vec<&str> = ds.iter().map(|d| d.role.as_str()).collect();
            if roles.iter().any(|r| !prompts::roles(domain.name()).iter().any(|k| k.0 == *r)) {
                return None;
            }
            let say = |text: String| {
                let _ = self.wb.client.events.send(Event::AgentMessage { run: self.wb.client.run.clone(), agent: "harness".into(), text });
            };
            if let Some(why) = crate::merge::cannot_run_together(domain.name(), &roles) {
                say(format!("delegating {} one at a time: {why}", roles.join(", ")));
                return None;
            }
            self.wb.state.design.as_ref()?;
            // one gate for the batch (a paused run holds here)
            let gate = match self.keeper.before_tool("delegate").await {
                Ok(note) => note,
                Err(out) => return Some(vec![out; calls.len()]),
            };
            let mut branches = vec![];
            for d in &ds {
                let title = match crate::plan::begin_delegation(self.wb.state, &self.wb.client, &d.role, &d.plan_task) {
                    Ok(t) => t,
                    Err(_) => return None,
                };
                let task = match title {
                    Some(t) => format!("[plan task {}: {t}] {}", d.plan_task.trim(), d.task),
                    None => d.task.clone(),
                };
                branches.push(crate::merge::Branch { role: d.role.clone(), task });
            }
            let cad_dir = self.cfg.out_dir.clone().unwrap_or_else(std::env::temp_dir).join(format!("cad-forks-{}", self.cfg.run_id));
            let ctx = crate::merge::FanOut {
                domain,
                client: self.wb.client.clone(),
                model: self.cfg.models.specialist.clone(),
                turns: self.cfg.specialist_turns,
                state: &mut *self.wb.state,
                cad_dir,
                lessons: self.lessons.clone(),
                out_dir: self.wb.out_dir.clone(),
            };
            let joined = crate::merge::fan_out(ctx, &branches).await;
            if let Some(e) = joined.fatal {
                self.fatal = Some(e);
            }
            self.wb.edits.extend(joined.edits);
            let outs = joined
                .outputs
                .into_iter()
                .zip(&ds)
                .map(|(out, d)| {
                    let out = match crate::plan::end_delegation(self.wb.state, &self.wb.client, &d.plan_task) {
                        Some(line) => ToolOutput { content: format!("{}\n{line}", out.content), ..out },
                        None => out,
                    };
                    crate::checkpoint::with_note(out, gate.clone())
                })
                .collect();
            Some(outs)
        })
    }
    fn observe_user_blocks(&mut self,blocks:&[Value]) { self.visuals.user_blocks(blocks); }
    fn observe_tool_result(&mut self,name:&str,input:&Value,output:&ToolOutput) { self.visuals.tool_result(name,input,output); }
    fn extra_blocks(&mut self) -> Vec<Value> {
        let (blocks, notes) = self.inbox.drain();
        if !blocks.is_empty() || !notes.is_empty() {
            self.wb.state.person_spoke();
        }
        self.wb.state.feedback.extend(notes);
        blocks
    }
}

impl Orchestrator<'_> {
    /// Build the current design into `<out>/wip` (once per design) and run
    /// one post-design stage on it. Its checks and numbers come back as
    /// evidence, and go into the shared notes so specialists and the
    /// reviewer see them too; its files are shown live as artifacts.
    fn run_stage(&mut self, stage: &str) -> ToolOutput {
        self.run_stages(&[stage]).pop().unwrap_or_else(|| ToolOutput::err("no stage ran"))
    }

    /// Run one or several post-design stages on the current design, built
    /// once into `<out>/wip`; one output per requested stage, in order.
    /// Several requested in one turn run together (`Stages::run_many`:
    /// every ready stage at once, capped at the CPU count; Pat 10/24
    /// 22:10, no lower cap), and their artifact events are sent after all
    /// finish, in request order, so the event log does not depend on which
    /// solver finished first.
    fn run_stages(&mut self, stages: &[&str]) -> Vec<ToolOutput> {
        let domain = self.wb.domain.clone();
        let names = self.cfg.stages.names(domain.name());
        let mut outs: Vec<Option<ToolOutput>> = stages
            .iter()
            .map(|s| (!names.contains(s)).then(|| ToolOutput::err(format!("no stage `{s}` for {}; stages: {}", domain.name(), names.join(", ")))))
            .collect();
        let valid: Vec<&str> = stages.iter().zip(&outs).filter(|(_, o)| o.is_none()).map(|(s, _)| *s).collect();
        if valid.is_empty() {
            return outs.into_iter().map(|o| o.unwrap_or_default()).collect();
        }
        let Some(design) = self.wb.state.design.clone() else {
            return outs.into_iter().map(|o| o.unwrap_or_else(|| ToolOutput::err("there is no design yet: call design first"))).collect();
        };
        let spec = domain.validate_spec(&self.wb.state.effective_spec()).unwrap_or_else(|_| self.wb.state.effective_spec());
        let wip = match &self.cfg.out_dir {
            Some(o) => o.join("wip"),
            None => std::env::temp_dir().join(format!("wright-stages-{}", self.cfg.run_id)),
        };
        let artifacts = match &self.wb.state.wip_build {
            Some((d, a)) if *d == design => a.clone(),
            _ => {
                let _ = std::fs::create_dir_all(&wip);
                match domain.build(&design, &wip) {
                    Ok(a) => {
                        self.wb.state.wip_build = Some((design.clone(), a.clone()));
                        a
                    }
                    // a stage that needs no build can still run
                    Err(e) => {
                        self.wb.state.notes.push(format!("stage {}: the current design could not be built for it: {e:#}", valid.join(", ")));
                        wright_core::Artifacts::default()
                    }
                }
            }
        };
        let base = wright_core::StageCtx {
            domain: domain.name().into(),
            out_dir: wip.clone(),
            dir: wip.clone(),
            design,
            spec,
            artifacts,
            upstream: Default::default(),
        };
        let client = self.wb.client.clone();
        let run = self.cfg.run_id.clone();
        let rel = |p: PathBuf| match &self.cfg.out_dir {
            Some(o) => p.strip_prefix(o).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string_lossy().into_owned()),
            None => p.to_string_lossy().into_owned(),
        };
        let all: Vec<wright_core::StageRun> = if valid.len() == 1 {
            self.cfg.stages.run(&base, Some(valid[0]), false, |_| {})
        } else {
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let _ = client.events.send(Event::AgentMessage { run: run.clone(), agent: "harness".into(), text: format!("running stages {} together", valid.join(", ")) });
            self.cfg.stages.run_many(&base, &valid, false, cores)
        };
        // each requested stage's runs: itself and the needs it pulled in
        // (Stages::run's closure), in the order they ran
        let closure = |stage: &str| -> Vec<&wright_core::StageRun> {
            let mut want = vec![stage.to_string()];
            let mut i = 0;
            while i < want.len() {
                if let Some(s) = self.cfg.stages.get(&want[i], domain.name()) {
                    for n in s.needs() {
                        if !want.iter().any(|w| w == n) {
                            want.push(n.to_string());
                        }
                    }
                }
                i += 1;
            }
            all.iter().filter(|r| want.contains(&r.name)).collect()
        };
        let mut sent = std::collections::BTreeSet::new();
        let mut failing_total = 0usize;
        for (slot, stage) in outs.iter_mut().zip(stages) {
            if slot.is_some() {
                continue;
            }
            let runs = closure(stage);
            for r in &runs {
                if sent.insert(r.name.clone()) {
                    for it in &r.manifest.items {
                        let path = rel(wip.join(&r.name).join(&it.path));
                        let _ = client.events.send(Event::Artifact { run: run.clone(), label: format!("{} (round {}): {}", r.manifest.title, self.wb.state.round, it.label), path });
                    }
                    failing_total += r.manifest.checks.iter().filter(|c| !c.passed).count();
                }
            }
            let mut text = String::new();
            for r in &runs {
                let fresh = if r.fresh { " [up to date: not re-run]" } else { "" };
                text.push_str(&format!("{}{fresh}\n", r.manifest.brief()));
            }
            let text = truncate(text.trim(), 3000);
            self.wb.state.notes.push(format!("stage {stage} (round {}): {}", self.wb.state.round, truncate(&text, SUMMARY_CHARS)));
            let failed = runs.iter().any(|r| !r.manifest.ok());
            *slot = Some(if failed { ToolOutput::err(text) } else { ToolOutput::ok(text) });
        }
        // a failing stage check counts against this design when the
        // incumbent is ranked
        if let Some(d) = self.wb.state.design.clone() {
            self.wb.state.record_stage(domain.as_ref(), &d, failing_total);
        }
        outs.into_iter().map(|o| o.unwrap_or_default()).collect()
    }

    async fn delegate(&mut self, role: &str, task: &str) -> ToolOutput {
        let domain = self.wb.domain.clone();
        let mut client = self.wb.client.clone();
        let mut model = self.cfg.models.specialist.clone();
        if role == "cad" {
            let choice = std::env::var("WRIGHT_CAD_TRANSPORT").unwrap_or_default();
            let enabled = match crate::codex::cad_enabled(client.transport.provider(), &choice) {
                Ok(enabled) => enabled,
                Err(error) => return ToolOutput::err(error.to_string()),
            };
            if enabled {
                // traced like every other model call (its spans were missing from trace.jsonl)
                let codex: Arc<dyn crate::client::Transport> = Arc::new(crate::codex::CodexTransport::from_env());
                client.transport = match crate::trace::Tracer::of(&self.wb.client) {
                    Some(t) => crate::trace::TracedTransport::wrap(codex, t),
                    None => codex,
                };
                model = ModelCfg::new(crate::codex::MODEL, "low", 4000);
                let _ = client.events.send(wright_core::Event::AgentMessage {
                    run: client.run.clone(), agent: "specialist:cad".into(),
                    text: "CAD specialist uses gpt-6-astra through the signed-in ChatGPT subscription. Wright executes typed actions; no model/API fallback. Run USD values are accounting estimates, not subscription charges.".into(),
                });
            }
        }
        let mut conv = Conversation::new(
            format!("specialist:{role}"),
            model,
            format!("{}{}", prompts::specialist_system(domain.as_ref(), role), self.lessons),
            prompts::specialist_tools_for(domain.name(), role),
        );
        // failing + nearly failing checks only; full_report has the rest (crate::slim)
        let text = format!("Task from the lead: {task}\n\n{}", crate::slim::specialist_prompt(self.wb.state));
        let mut input_blocks=vec![json!({"type":"text","text":text})];
        if role=="cad" {
            match self.visuals.blocks() {
                Ok(blocks)=>input_blocks.extend(blocks),
                Err(error)=>return ToolOutput::err(format!("CAD reference images rejected; no images were silently omitted: {error}")),
            }
        }
        let round = self.wb.state.round;
        let mut sp = Specialist {
            wb: Workbench { cad: &mut *self.wb.cad, domain, state: &mut *self.wb.state, client: client.clone(), edits: vec![], may_edit_spec: false, out_dir: self.wb.out_dir.clone() },
            report: None,
            visuals: if role=="cad" {Some(&mut *self.visuals)}else{None},
        };
        let agent_name = conv.agent.clone();
        let end = run_agent(&client, &mut conv, input_blocks, &mut crate::trace::Traced::new(crate::trace::Tracer::of(&client), &agent_name, &format!("specialist-{role}-r{round}"), &mut sp), self.cfg.specialist_turns).await;
        transcript(&self.cfg, &conv, &format!("specialist-{role}-r{round}"), true);
        let edits = std::mem::take(&mut sp.wb.edits);
        let report = sp.report.take();
        let summary = match end {
            Ok(AgentEnd::Done { text }) | Ok(AgentEnd::Stopped { text }) => report.unwrap_or(text),
            Ok(AgentEnd::TurnLimit) => report.unwrap_or_else(|| format!("{role} hit its {}-turn limit without reporting", self.cfg.specialist_turns)),
            Err(AgentError::Call(e)) => {
                let msg = e.to_string();
                self.fatal = Some(e);
                return ToolOutput::err(format!("{role} stopped: {msg}")).stop();
            }
            Err(e) => return ToolOutput::err(format!("{role} failed: {e}")),
        };
        let summary = truncate(summary.trim(), SUMMARY_CHARS);
        self.wb.state.notes.push(format!("{role}: {summary}"));
        self.wb.edits.extend(edits.iter().map(|e| format!("{role}: {e}")));
        let status = match &self.wb.state.report {
            Some(r) => {
                let failing: Vec<&str> = r.failures().map(|o| o.name.as_str()).collect();
                format!("checks now: {} of {} failing {failing:?}", failing.len(), r.outcomes.len())
            }
            None => "no check report yet".into(),
        };
        ToolOutput::ok(format!("{role} summary: {summary}\nedits: {edits:?}\n{status}"))
    }
}

pub(crate) struct Specialist<'s> {
    pub(crate) wb: Workbench<'s>,
    pub(crate) report: Option<String>,
    pub(crate) visuals: Option<&'s mut CadVisualContext>,
}

impl Handler for Specialist<'_> {
    fn observe_tool_result(&mut self,name:&str,input:&Value,output:&ToolOutput) {
        if let Some(context)=self.visuals.as_mut(){context.tool_result(name,input,output);}
    }
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            if let Some(out) = self.wb.call(name, input) {
                return out;
            }
            match name {
                "full_report" => ToolOutput::ok(self.wb.state.for_prompt()),
                "report" => match arg::<Summary>(input) {
                    Ok(s) => {
                        self.report = Some(s.summary);
                        ToolOutput::ok("reported").stop()
                    }
                    Err(o) => o,
                },
                other => ToolOutput::malformed(format!("unknown tool '{other}'")),
            }
        })
    }
}

/// A blocking issue the harness confirmed.
#[derive(Debug, Clone)]
pub struct Issue {
    pub text: String,
}

struct Reviewer<'s> {
    run: String,
    domain: Arc<dyn DomainTools>,
    state: &'s DesignState,
    issues: Vec<Issue>,
    regressions: Vec<Regression>,
    verdict: Option<String>,
    /// Every attack and citation, classified (`crate::review`).
    log: crate::review::ReviewLog,
}

impl Handler for Reviewer<'_> {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            match name {
                "cite_failing_check" => {
                    let c: Cite = match arg(input) {
                        Ok(c) => c,
                        Err(o) => return o,
                    };
                    match self.state.report.as_ref().and_then(|r| r.get(&c.check)) {
                        None => ToolOutput::err(format!("no check named '{}' in the report", c.check)),
                        Some(o) if o.passed() => ToolOutput::err(format!("rejected: {} passes ({o}); cite a failing check or make an attack", c.check)),
                        Some(o) => {
                            self.log.cite(self.state.round, &c.check);
                            self.issues.push(Issue { text: format!("failing check {}: {} -- {}", c.check, o, c.argument) });
                            ToolOutput::ok(format!("confirmed: {o}"))
                        }
                    }
                }
                "attack" => {
                    let a: Attack = match arg(input) {
                        Ok(a) => a,
                        Err(o) => return o,
                    };
                    let p = match patch(&a.spec_patch_json) {
                        Ok(p) => p,
                        Err(o) => return o,
                    };
                    // taxonomy category + cited evidence are vetted first;
                    // every outcome is logged for the run's review metrics
                    let out = match crate::review::vet(self.state, self.domain.as_ref(), input) {
                        Ok(_) => self.attack(a, p.clone()),
                        Err(why) => ToolOutput::err(why),
                    };
                    let reg = if out.is_error { None } else { self.regressions.last().cloned() };
                    self.log.record(self.state.round, input, &p, &out, reg.as_ref());
                    out
                }
                // read-only: a stock/sourcing attack cites the row it found
                "catalog" => catalog_lookup(self.domain.as_ref(), self.state.design.as_ref(), input),
                "verdict" => match arg::<Summary>(input) {
                    Ok(s) => {
                        self.log.walked(&s.walked_clear);
                        self.verdict = Some(s.summary);
                        ToolOutput::ok("verdict recorded").stop()
                    }
                    Err(o) => o,
                },
                other => ToolOutput::malformed(format!("unknown tool '{other}'")),
            }
        })
    }
}

impl Reviewer<'_> {
    fn attack(&mut self, a: Attack, p: Value) -> ToolOutput {
        let Some(design) = &self.state.design else {
            return ToolOutput::err("there is no design to attack");
        };
        if !self.domain.checks().iter().any(|(n, _)| *n == a.expect_failing_check) {
            return ToolOutput::err(format!("no declared check '{}'", a.expect_failing_check));
        }
        if self.domain.consistency_checks().contains(&a.expect_failing_check.as_str()) {
            return ToolOutput::err(format!(
                "rejected: `{}` checks that the design matches the spec it was made from; under any changed spec it fails for every design, so it cannot confirm an attack. Predict a check of what the vehicle does",
                a.expect_failing_check
            ));
        }
        if let Some(path) = touches_stated(&self.state.stated, &p, String::new()) {
            return ToolOutput::err(format!(
                "rejected: {path} is the person's own requirement; an attack may vary outside \
                 conditions, not rewrite what they asked for"
            ));
        }
        // judged against the spec the checks judge, after the domain's
        // defaults are filled in (an absent field reads as its default)
        let effective = self.state.effective_spec();
        let current = self.domain.validate_spec(&effective).unwrap_or_else(|_| effective.clone());
        let schema = self.domain.spec_schema();
        if let Some(why) = implausible(&schema, &schema, &current, &p, String::new()) {
            return ToolOutput::err(format!("rejected as implausible: {why}; attack with conditions a builder could meet"));
        }
        let mut spec = effective;
        merge_patch(&mut spec, &p);
        // the domain's own spec validation is part of the envelope (an
        // altitude-dependent hot-day limit the schema cannot state); a
        // patched spec it refuses is not a condition, so it is rejected
        // rather than reported as a failure to run
        let spec = match self.domain.validate_spec(&spec) {
            Ok(s) => s,
            Err(e) => {
                return ToolOutput::err(format!(
                    "rejected: outside the domain's envelope (the patched spec fails its validation: {e:#}); attack inside the declared envelope"
                ))
            }
        };
        let fin = match self.domain.check(&spec, design) {
            Ok(f) => f,
            Err(e) => return ToolOutput::err(format!("attack could not run: {e:#}")),
        };
        let o = fin.get(&a.expect_failing_check).expect("declared check is in a finished report");
        if o.passed() {
            return ToolOutput::err(format!("not confirmed: under that spec {o}"));
        }
        // unique across runs: the regressions file is shared by every run
        // of the domain (review 4, finding 16)
        let id = format!("{}-r{}-{}", self.run, self.state.round, self.regressions.len() + 1);
        let margin = match o.evidence {
            Evidence::Margin { value, .. } => Some(value),
            _ => None,
        };
        self.regressions.push(Regression {
            id: id.clone(),
            domain: self.domain.name().into(),
            claim: a.claim.clone(),
            spec_patch: p,
            check: a.expect_failing_check.clone(),
            margin_at_confirmation: margin,
        });
        self.issues.push(Issue { text: format!("confirmed attack {id}: {} -- {o}", a.claim) });
        ToolOutput::ok(format!("confirmed ({id}), recorded as a regression case: {o}"))
    }
}

/// Split a merge patch into (locked, assumed) leaves. With `listed_is_said`
/// the leaves at `paths` are the locked ones and the rest assumed;
/// without it (the default since review 4) the leaves at `paths` are the
/// assumptions and everything else is locked.
fn split_paths(patch: &Value, paths: &[String], at: String, listed_is_said: bool) -> (Value, Value) {
    match patch {
        Value::Object(m) => {
            let (mut said, mut assumed) = (serde_json::Map::new(), serde_json::Map::new());
            for (k, v) in m {
                let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                let listed = paths.iter().any(|p| p == &path);
                if listed {
                    if listed_is_said {
                        said.insert(k.clone(), v.clone());
                    } else {
                        assumed.insert(k.clone(), v.clone());
                    }
                } else if v.is_object() {
                    let (s, a) = split_paths(v, paths, path, listed_is_said);
                    if s.as_object().is_some_and(|o| !o.is_empty()) {
                        said.insert(k.clone(), s);
                    }
                    if a.as_object().is_some_and(|o| !o.is_empty()) {
                        assumed.insert(k.clone(), a);
                    }
                } else if listed_is_said {
                    assumed.insert(k.clone(), v.clone());
                } else {
                    said.insert(k.clone(), v.clone());
                }
            }
            (Value::Object(said), Value::Object(assumed))
        }
        other => (json!({}), other.clone()),
    }
}

/// The first top-level key of a design patch that the spec also has (the
/// working spec or the person's stated one), e.g. `requirements`.
fn shares_spec_key(spec: &Value, stated: &Value, patch: &Value) -> Option<String> {
    let p = patch.as_object()?;
    p.keys()
        .find(|k| spec.get(k.as_str()).is_some() || stated.get(k.as_str()).is_some())
        .cloned()
}

/// Why an attack value is not one a builder will meet, or `None`.
///
/// Bounds come from the domain's spec schema first (`minimum`/`maximum`,
/// `exclusiveMinimum`/`exclusiveMaximum`, `enum`): the schema is the one
/// statement of what a field can be. Where it gives no range (none of the
/// three domains declares one today) the fallback is the field's unit,
/// read from its name suffix ([`unit_bounds`]): the value must be finite,
/// inside the unit's physical range when it has one, and within three times
/// the current magnitude plus an absolute floor -- so a field at 0 still
/// has a bound (review 4, finding 4: a ratio test had none at 0 and
/// rejected a realistic -5 C against 15 C). A null or absent field is
/// attackable only when a range is known. Text, arrays and deletions are
/// not conditions; booleans and strings pass only if the schema enumerates
/// them.
fn implausible(root: &Value, schema: &Value, current: &Value, patch: &Value, at: String) -> Option<String> {
    let node = resolve(root, schema);
    match patch {
        Value::Object(p) => p.iter().find_map(|(k, pv)| {
            let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
            let sub = property(root, node, k).unwrap_or(&Value::Null);
            implausible(root, sub, current.get(k).unwrap_or(&Value::Null), pv, path)
        }),
        Value::Null => Some(format!("{at}: deleting a field is not a condition a builder meets")),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if !x.is_finite() {
                return Some(format!("{at} = {x} is not finite"));
            }
            if let Some((lo, hi)) = schema_range(root, node) {
                return (!(lo.is_none_or(|l| x >= l) && hi.is_none_or(|h| x <= h))).then(|| {
                    format!("{at} = {x} is outside the schema's range [{}, {}]", fmt_bound(lo), fmt_bound(hi))
                });
            }
            let leaf = at.rsplit('.').next().unwrap_or(&at);
            let (floor, range) = unit_bounds(leaf);
            if let Some((lo, hi)) = range {
                if !(lo..=hi).contains(&x) {
                    return Some(format!("{at} = {x} is outside the physical range [{lo}, {hi}] for its unit"));
                }
            }
            match current.as_f64() {
                Some(cur) if x.abs() > 3.0 * cur.abs() + floor => {
                    Some(format!("{at} = {x} against {cur} today (beyond 3x the current magnitude + {floor})"))
                }
                Some(_) => None,
                None if range.is_some() => None,
                None => Some(format!("{at} has no current value and neither the schema nor its unit gives a range")),
            }
        }
        Value::Bool(_) | Value::String(_) => {
            let enumerated = node.get("enum").and_then(Value::as_array).is_some_and(|e| e.contains(patch))
                || (patch.is_boolean() && schema_types(node).contains(&"boolean"));
            (!enumerated).then(|| format!("{at} = {patch}: only numbers, or values the schema enumerates, are attack conditions"))
        }
        Value::Array(_) => Some(format!("{at}: a list is not an attack condition")),
    }
}

/// The first numeric leaf of `patch` outside the envelope the spec schema
/// declares (`minimum`/`maximum`), or, for a field the schema gives no
/// range, outside its unit's physical range ([`unit_bounds`]); a value
/// the schema enumerates must be one of them. Unlike [`implausible`] this
/// needs no current spec, so stored regression cases are screened with it
/// when they are loaded.
pub(crate) fn outside_envelope(schema: &Value, patch: &Value) -> Option<String> {
    fn walk(root: &Value, node: &Value, patch: &Value, at: String) -> Option<String> {
        let node = resolve(root, node);
        match patch {
            Value::Object(p) => p.iter().find_map(|(k, pv)| {
                let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                walk(root, property(root, node, k).unwrap_or(&Value::Null), pv, path)
            }),
            Value::Number(n) => {
                let x = n.as_f64().unwrap_or(f64::NAN);
                if !x.is_finite() {
                    return Some(format!("{at} = {x} is not finite"));
                }
                if let Some((lo, hi)) = schema_range(root, node) {
                    return (!(lo.is_none_or(|l| x >= l) && hi.is_none_or(|h| x <= h)))
                        .then(|| format!("{at} = {x} is outside the schema's range [{}, {}]", fmt_bound(lo), fmt_bound(hi)));
                }
                let leaf = at.rsplit('.').next().unwrap_or(&at);
                let (_, range) = unit_bounds(leaf);
                range.and_then(|(lo, hi)| {
                    (!(lo..=hi).contains(&x)).then(|| format!("{at} = {x} is outside the physical range [{lo}, {hi}] for its unit"))
                })
            }
            Value::String(_) | Value::Bool(_) => {
                let e = node.get("enum").and_then(Value::as_array)?;
                (!e.contains(patch)).then(|| format!("{at} = {patch} is not one of the values the schema enumerates"))
            }
            _ => None,
        }
    }
    walk(schema, schema, patch, String::new())
}

/// Every numeric spec field the schema bounds, as `path: [min, max]`
/// lines, for the reviewer's prompt (attack inside the envelope).
pub(crate) fn envelope_lines(schema: &Value) -> Vec<String> {
    fn walk(root: &Value, node: &Value, at: &str, depth: usize, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        let node = resolve(root, node);
        if !at.is_empty() {
            if let Some((lo, hi)) = schema_range(root, node) {
                let what = variants(root, node).into_iter().find_map(|n| n.get("description").and_then(Value::as_str));
                let mut line = format!("{at}: [{}, {}]", fmt_bound(lo), fmt_bound(hi));
                if let Some(d) = what {
                    let d: String = d.split_whitespace().collect::<Vec<_>>().join(" ");
                    let d: String = d.chars().take(120).collect();
                    line.push_str(&format!(" ({d})"));
                }
                out.push(line);
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for n in variants(root, node) {
            if let Some(props) = n.get("properties").and_then(Value::as_object) {
                for (k, v) in props {
                    if seen.insert(k.clone()) {
                        let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                        walk(root, v, &path, depth + 1, out);
                    }
                }
            }
        }
    }
    let mut out = vec![];
    walk(schema, schema, "", 0, &mut out);
    out
}

fn fmt_bound(b: Option<f64>) -> String {
    b.map_or("-".into(), |v| v.to_string())
}

/// Follow a local `$ref` (`#/definitions/X`, `#/$defs/X`).
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let mut node = node;
    for _ in 0..8 {
        let Some(r) = node.get("$ref").and_then(Value::as_str) else { break };
        let Some(ptr) = r.strip_prefix('#') else { break };
        match root.pointer(ptr) {
            Some(n) => node = n,
            None => break,
        }
    }
    node
}

/// The variants of a schema node (`anyOf`/`oneOf`/`allOf`), or the node.
fn variants<'a>(root: &'a Value, node: &'a Value) -> Vec<&'a Value> {
    let mut out = vec![node];
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(a) = node.get(key).and_then(Value::as_array) {
            out.extend(a.iter().map(|v| resolve(root, v)));
        }
    }
    out
}

fn property<'a>(root: &'a Value, node: &'a Value, key: &str) -> Option<&'a Value> {
    variants(root, node).into_iter().find_map(|n| n.get("properties").and_then(|p| p.get(key))).map(|n| resolve(root, n))
}

fn schema_types(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    }
}

/// `(min, max)` from the schema, if it states either.
fn schema_range(root: &Value, node: &Value) -> Option<(Option<f64>, Option<f64>)> {
    for n in variants(root, node) {
        let lo = n.get("minimum").or_else(|| n.get("exclusiveMinimum")).and_then(Value::as_f64);
        let hi = n.get("maximum").or_else(|| n.get("exclusiveMaximum")).and_then(Value::as_f64);
        if lo.is_some() || hi.is_some() {
            return Some((lo, hi));
        }
    }
    None
}

/// `(absolute floor, physical range)` for a spec field, from its unit
/// suffix: the fallback when the schema states no range. The floors are
/// Wright's own allowances (unsourced: "a change a builder can plausibly
/// face from a zero baseline"), the ranges physical limits of Earth-surface
/// conditions (temperature -80..60 C spans the recorded surface extremes).
pub(crate) fn unit_bounds(field: &str) -> (f64, Option<(f64, f64)>) {
    let f = field.to_ascii_lowercase();
    let ends = |s: &str| f.ends_with(s);
    if ends("_c") || ends("_degc") || f.contains("temperature") {
        (30.0, Some((-80.0, 60.0)))
    } else if ends("_dk") || ends("_delta_k") {
        (30.0, Some((-60.0, 60.0)))
    } else if ends("_k") {
        (30.0, Some((190.0, 340.0)))
    } else if ends("_ms") || ends("_mps") || f.contains("wind") || f.contains("gust") {
        (15.0, Some((-60.0, 60.0)))
    } else if ends("_pct") || ends("_percent") {
        (15.0, Some((0.0, 100.0)))
    } else if ends("_deg") {
        (30.0, Some((-360.0, 360.0)))
    } else if ends("_mm") {
        (25.0, None)
    } else if ends("_m") {
        (1000.0, None)
    } else if ends("_ft") {
        (3000.0, None)
    } else if ends("_kg") {
        (1.0, None)
    } else if ends("_g") {
        (500.0, None)
    } else if ends("_usd") {
        (250.0, None)
    } else if ends("_s") {
        (60.0, None)
    } else if ends("_h") {
        (2.0, None)
    } else if ends("_kpa") {
        (20.0, None)
    } else if ends("_pa") {
        (20000.0, None)
    } else {
        (1.0, None)
    }
}

/// The first leaf of `patch` that would change a leaf of `stated` (the
/// person's own spec), as a dotted path.
pub(crate) fn touches_stated(stated: &Value, patch: &Value, at: String) -> Option<String> {
    match (stated, patch) {
        (Value::Object(s), Value::Object(p)) => p.iter().find_map(|(k, pv)| {
            let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
            s.get(k).and_then(|sv| touches_stated(sv, pv, path))
        }),
        (Value::Null, _) => None,
        (s, p) if s != p => Some(at),
        _ => None,
    }
}


// ------------------------------------------------------------------ run

fn stop_for(e: &CallError) -> StopReason {
    match e {
        CallError::Budget(b) => StopReason::Budget(b.to_string()),
        other => StopReason::Error(other.to_string()),
    }
}

fn stop_for_agent(e: AgentError) -> StopReason {
    match e {
        AgentError::Call(c) => stop_for(&c),
        other => StopReason::Error(other.to_string()),
    }
}

/// Design one vehicle. Never panics on model or domain failure; the summary
/// says why it stopped.
pub async fn run(client: Client, domain: Arc<dyn DomainTools>, cfg: RunConfig) -> RunSummary {
    let mut inbox = Inbox { rx: client.events.subscribe(), run: cfg.run_id.clone(), client: client.clone(), base: cfg.out_dir.clone() };
    // every model and tool call as an OTel GenAI span in <out_dir>/trace.jsonl (trace.rs)
    let client = crate::trace::trace_client(&client, crate::trace::Tracer::for_run(cfg.out_dir.as_deref(), &cfg.run_id));
    let mut state = match &cfg.resume {
        Some(ck) => ck.state.clone(),
        None => DesignState::new(domain.name(), &cfg.request, cfg.spec.clone()),
    };
    // checkpoints + the person's gate (crate::checkpoint); a resumed run's
    // spend counts against this run's budget as if it were one process
    let mut keeper = crate::checkpoint::Keeper::new(&cfg, &client);
    let mut ckpt = crate::checkpoint::Checkpointer::new(&cfg);
    if let Some(ck) = &cfg.resume {
        crate::checkpoint::restore_spend(&client, ck);
        client.transport.import_sessions(&ck.transport_sessions);
        let _ = client.events.send(Event::Control { run: cfg.run_id.clone(), action: "resume".into(), state: "running".into(), note: format!("resumed from checkpoint {} (step {}, round {})", ck.id, ck.step, ck.round) });
    }
    let cad_directory = cfg.out_dir.clone().unwrap_or_else(|| {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        std::env::temp_dir().join(format!("wright-cad-{}-{stamp}", std::process::id()))
    }).join("cad");
    let mut cad = wright_cad::Session::new(cad_directory);
    let say = |text: String| {
        let _ = client.events.send(Event::AgentMessage { run: cfg.run_id.clone(), agent: "harness".into(), text });
    };
    if let Some(p) = &cfg.regressions_path {
        // a bad line is skipped and named, one by one; a file that exists
        // but cannot be read stops the run before any model is called
        // (review 4, finding 5: one bad line used to drop them all)
        match load_regressions_reporting(p, domain.name()) {
            Ok((r, skipped)) => {
                for why in skipped {
                    say(format!("skipped a regression line in {}: {why}", p.display()));
                }
                state.regressions = r;
                screen(&client, &cfg, &mut state, domain.as_ref());
            }
            Err(e) => {
                let msg = format!("could not read regressions from {}: {e:#}", p.display());
                say(msg.clone());
                let _ = client.events.send(Event::RunStarted { run: cfg.run_id.clone(), domain: domain.name().into(), request: cfg.request.clone() });
                return judge(client, domain, cfg, state, StopReason::Error(msg), 0);
            }
        }
    }
    let _ = client.events.send(Event::RunStarted { run: cfg.run_id.clone(), domain: domain.name().into(), request: cfg.request.clone() });
    // the role -> model table (crate::roles), in the log and the trace
    say(format!("models: {}", cfg.models.describe()));
    if let Some(t) = crate::trace::Tracer::of(&client) {
        crate::roles::trace_mapping(&t, &cfg.models);
        t.set_current("elicit", "elicit");
    }

    // Spec elicitation (lane 7, crate::elicit): a run that starts from a
    // request alone gets its spec, locks, sourced assumptions and the
    // feasibility probe before the orchestrator's first turn; the probe's
    // verdict is in `state.notes`. A failure here is said and the
    // orchestrator writes the spec itself as before.
    if let (Some(ec), true, None) = (&cfg.elicit, cfg.spec.is_null(), &cfg.resume) {
        match crate::elicit::elicit(&client, domain.as_ref(), &cfg.request, &Value::Null, ec, &mut crate::elicit::NoHuman).await {
            Ok(e) => e.apply(&mut state),
            Err(e) => say(format!("spec elicitation failed ({e:#}); the orchestrator writes the spec itself")),
        }
    }

    // Lessons earlier runs of this domain left beside the regressions file
    // (`crate::memory`): the most relevant go into every system prompt.
    // An unreadable file is said and skipped, never a reason to stop.
    let lessons = match &cfg.regressions_path {
        Some(p) => crate::memory::section_for(p, domain.name(), &cfg.request, &cfg.spec).unwrap_or_else(|e| {
            say(format!("could not read lessons beside {}: {e:#}", p.display()));
            String::new()
        }),
        None => String::new(),
    };
    // Prior efforts from the runs database (`crate::runs_db`): appended
    // to the lessons so the orchestrator and every specialist see it; an
    // empty store or no match adds nothing.
    let prior = match &cfg.runs_db {
        Some(db) => crate::runs_db::prior_section(db.as_ref(), domain.name(), &cfg.request, &state.spec).unwrap_or_else(|e| {
            say(format!("could not read the runs database {}: {e:#}", db.describe()));
            String::new()
        }),
        None => String::new(),
    };
    if !prior.is_empty() {
        say(format!("seeded the team with prior efforts from {}", cfg.runs_db.as_ref().map(|d| d.describe()).unwrap_or_default()));
    }
    let lessons = format!("{lessons}{prior}");
    let mut trace = crate::memory::RunTrace::default();

    let mut orch = Conversation::new(
        "orchestrator",
        cfg.models.orchestrator.clone(),
        format!("{}{lessons}", prompts::orchestrator_system_with(domain.as_ref(), &cfg.stages.names(domain.name()))),
        prompts::orchestrator_tools_with(domain.as_ref(), &cfg.stages.names(domain.name())),
    );
    let (start_round, mut turns_before) = match &cfg.resume {
        Some(ck) => {
            if let Some(saved) = &ck.orchestrator {
                saved.restore(&mut orch);
            }
            (ck.resume_round(), ck.turns_used())
        }
        None => {
            ckpt.save("input", &cfg, &client, &state, 0, crate::checkpoint::Phase::Orchestrating { turns_used: 0 }, keeper.gate(), None);
            (1, 0)
        }
    };
    let mut stop = StopReason::RoundLimit;
    let mut rounds = 0;
    let mut carry: Vec<Value> = vec![]; // feedback that arrived during review
    let mut review_log = crate::review::ReviewLog::default();
    let mut visuals=CadVisualContext::default();

    for round in start_round..=cfg.max_rounds {
        rounds = round;
        state.round = round;
        // a round resumed mid-way keeps its history and feedback; the lead
        // is told so instead of being restarted
        let mid_round = round == start_round && cfg.resume.as_ref().is_some_and(|c| matches!(c.phase, crate::checkpoint::Phase::Orchestrating { .. }));
        let lead = if mid_round {
            cfg.resume.as_ref().map(|c| c.resume_note()).unwrap_or_default()
        } else if round == 1 {
            format!("A person asked: {}\n\nStart the design.", cfg.request)
        } else {
            format!("Round {round}. The reviewer's confirmed issues and any feedback are under open_feedback; fix them, check, and submit.")
        };
        let mut task = vec![json!({"type": "text", "text": format!("{lead}\n\n{}", state.for_prompt())})];
        if round==1 && !mid_round {task.extend(cfg.reference_blocks.clone());}
        task.append(&mut carry);
        if !mid_round {
            state.feedback.clear();
        }
        let turns_left = if mid_round { cfg.orchestrator_turns.saturating_sub(turns_before).max(1) } else { cfg.orchestrator_turns };

        let mut h = Orchestrator {
            wb: Workbench { cad: &mut cad, domain: domain.clone(), state: &mut state, client: client.clone(), edits: vec![], may_edit_spec: true, out_dir: cfg.out_dir.clone() },
            cfg: cfg.clone(),
            lessons: lessons.clone(),
            inbox: &mut inbox,
            fatal: None,
            visuals: &mut visuals,
            keeper: &mut keeper,
            ckpt: &mut ckpt,
            round,
            turns_before,
        };
        let r = run_agent(&client, &mut orch, task, &mut crate::trace::Traced::new(crate::trace::Tracer::of(&client), "orchestrator", "orchestrator", &mut h), turns_left).await;
        turns_before = 0;
        // for the lessons harvest: this round's edits (specialists' included)
        // and the person's notes before the review overwrites open_feedback
        let round_edits = std::mem::take(&mut h.wb.edits);
        trace.note_feedback(&h.wb.state.feedback);
        transcript(&cfg, &orch, "orchestrator", false);
        if let Some(e) = h.fatal.take() {
            stop = stop_for(&e);
            break;
        }
        if let Err(e) = r {
            stop = stop_for_agent(e);
            break;
        }
        // the person stopped the run, or a pause ran out: the checkpoint
        // from the held step stands; `--resume` continues from it
        if let Some(why) = keeper.stopped.take() {
            stop = StopReason::Error(why);
            break;
        }

        screen(&client, &cfg, &mut state, domain.as_ref());
        // The reviewer sees the best verified design, not a later worse one.
        let swapped = state.prefer_best(domain.as_ref()).map(|msg| format!("Before review: {msg}."));
        if let Some(text) = &swapped {
            let _ = client.events.send(Event::AgentMessage { run: cfg.run_id.clone(), agent: "harness".into(), text: text.clone() });
            // the lead must know the design it submitted is not the one reviewed
            state.feedback.push(format!("harness: {text}"));
        }
        // The checks, not the orchestrator, say where the design stands.
        match state.full_check(domain.as_ref()) {
            Ok(fin) => {
                emit_checks(&client, domain.as_ref(), &state, &fin);
                state.report = Some(fin);
            }
            Err(_) => state.report = None,
        }

        let mut rv = Reviewer { run: cfg.run_id.clone(), domain: domain.clone(), state: &state, issues: vec![], regressions: vec![], verdict: None, log: std::mem::take(&mut review_log) };
        let mut conv = Conversation::new("reviewer", cfg.models.reviewer.clone(), prompts::reviewer_system(domain.as_ref()), prompts::reviewer_tools());
        let text = format!("Review this design.\n\n{}", state.for_prompt());
        let r = run_agent(&client, &mut conv, vec![json!({"type": "text", "text": text})], &mut crate::trace::Traced::new(crate::trace::Tracer::of(&client), "reviewer", &format!("reviewer-r{round}"), &mut rv), cfg.reviewer_turns).await;
        transcript(&cfg, &conv, &format!("reviewer-r{round}"), true);
        let reviewed = rv.verdict.is_some();
        let verdict_text = rv.verdict.clone().unwrap_or_else(|| "no verdict".into()); // for the checkpoint's conversation list
        let (mut issues, mut regressions) = (rv.issues, rv.regressions);
        review_log = rv.log;
        if !reviewed {
            // a reviewer that never gave a verdict has not approved anything
            issues.push(Issue { text: "the reviewer did not finish its review (no verdict)".into() });
        }
        if let Err(e) = r {
            stop = stop_for_agent(e);
            break;
        }
        // proposer-vs-reviewer rebuttal before a confirmed attack is stored
        // (crate::review; opt-in by cfg.rebuttal)
        if cfg.rebuttal && !regressions.is_empty() {
            // only attacks on an agent's choice are argued (crate::review::gate_rebuttals); the rest are counted as skipped
            let skipped_before = review_log.rebuttals_skipped.len();
            let heard = crate::review::gate_rebuttals(&state, &regressions, &mut review_log);
            for (id, why) in &review_log.rebuttals_skipped[skipped_before..] {
                say(format!("rebuttal skipped for {id}: {why}"));
            }
            if let Some(t) = crate::trace::Tracer::of(&client) {
                crate::review::trace_gate(&t, round, heard.len(), &review_log.rebuttals_skipped[skipped_before..]);
            }
            let reb = crate::review::rebuttal_round(&client, &cfg.models, domain.clone(), &state, &heard, &mut review_log).await;
            for (id, _, _) in &reb.withdrawn {
                regressions.retain(|r| &r.id != id);
                issues.retain(|i| !i.text.starts_with(&format!("confirmed attack {id}:")));
            }
        }
        let _ = client.events.send(Event::AgentMessage {
            run: cfg.run_id.clone(),
            agent: "harness".into(),
            text: crate::review::metrics_message(round, &review_log.metrics()),
        });
        for reg in regressions {
            if let Some(p) = &cfg.regressions_path {
                if let Err(e) = append_regression(p, &reg) {
                    let _ = client.events.send(Event::AgentMessage {
                        run: cfg.run_id.clone(),
                        agent: "harness".into(),
                        text: format!("could not save regression {} to {}: {e:#} (it still applies to this run)", reg.id, p.display()),
                    });
                }
            }
            state.regressions.push(reg);
        }
        // a new regression case re-ranks the incumbent
        state.rescore_best(domain.as_ref());
        state.feedback = issues.iter().map(|i| format!("reviewer: {}", i.text)).collect();
        state.feedback.extend(swapped.map(|t| format!("harness: {t}")));
        if let Ok(fin) = state.full_check(domain.as_ref()) {
            emit_checks(&client, domain.as_ref(), &state, &fin);
            state.report = Some(fin);
            // a confirmed attack fails a check some plan task vouched for: stale, replan next round
            if let Some(n) = crate::plan::after_check(&mut state, &client) {
                state.feedback.push(format!("harness: {n}"));
            }
        }
        let (late, notes) = inbox.drain();
        if !late.is_empty() || !notes.is_empty() {
            state.person_spoke();
        }
        state.feedback.extend(notes);
        trace.round(round, round_edits, &state);
        let clean = state.passed() && issues.is_empty() && late.is_empty();
        carry = late;
        ckpt.note_conversation(&conv, round, &verdict_text);
        if !clean {
            ckpt.save("review", &cfg, &client, &state, round, crate::checkpoint::Phase::Reviewed, keeper.gate(), Some(crate::checkpoint::SavedConversation::of(&orch, &[])));
        }
        if clean {
            stop = StopReason::Passed;
            break;
        }
    }
    // a run that ended on its own terms is over: `--resume` refuses it. One
    // that crashed, hit the budget or was stopped keeps its last step
    // checkpoint, which is what `--resume` continues from
    if matches!(stop, StopReason::Passed | StopReason::RoundLimit) {
        ckpt.save("finish", &cfg, &client, &state, rounds, crate::checkpoint::Phase::Finished, keeper.gate(), Some(crate::checkpoint::SavedConversation::of(&orch, &orch.pending)));
    }
    keeper.finish();

    // What this run learned, for the next one (`crate::memory`): confirmed
    // attacks and their fix, unorderable parts, checks that never passed,
    // the person's feedback. Deduplicated and capped in the file.
    if let Some(p) = &cfg.regressions_path {
        let lessons = trace.harvest(domain.as_ref(), &state, &cfg.run_id, &crate::memory::today());
        let path = crate::memory::lessons_path(p);
        match crate::memory::append(&path, &lessons) {
            Ok(n) if n > 0 => say(format!("saved {n} lesson(s) for later runs to {}", path.display())),
            Ok(_) => {}
            Err(e) => say(format!("could not save lessons to {}: {e:#}", path.display())),
        }
    }

    let runs_db = cfg.runs_db.clone().map(|db| (db, cfg.run_id.clone(), cfg.out_dir.clone(), client.events.clone()));
    let mut summary = judge(client, domain, cfg, state, stop, rounds);
    summary.review = review_log.metrics();
    if let Some((db, run_id, out_dir, events)) = runs_db {
        crate::runs_db::log_summary(db.as_ref(), &run_id, out_dir.as_deref(), &summary, &events);
    }
    summary
}

/// Judge the final state by the checks alone, write outputs, announce.
/// Set aside stored regression cases outside the domain's envelope, say
/// so, and move every case set aside so far (here or by an `edit_spec`)
/// into the rejected file: the harness's next write.
fn screen(client: &Client, cfg: &RunConfig, state: &mut DesignState, domain: &dyn DomainTools) {
    let say = |text: String| {
        let _ = client.events.send(Event::AgentMessage { run: cfg.run_id.clone(), agent: "harness".into(), text });
    };
    for m in state.screen_regressions(domain) {
        say(format!("{m}; it is outside the domain's envelope and no longer applies"));
    }
    if state.set_aside.is_empty() {
        return;
    }
    if let Some(p) = &cfg.regressions_path {
        match move_rejected(p, &state.set_aside) {
            Ok(()) => {
                let ids: Vec<&str> = state.set_aside.iter().map(|(r, _)| r.id.as_str()).collect();
                say(format!("moved {} out-of-envelope regression case(s) to {}: {}", ids.len(), rejected_path(p).display(), ids.join(", ")));
            }
            Err(e) => say(format!("could not move out-of-envelope regression cases to {}: {e:#} (they are still skipped in this run)", rejected_path(p).display())),
        }
    }
    state.set_aside.clear();
}

/// Save `conv`'s full history under `<out_dir>/transcripts/<name>.json`
/// for post-training (`wright-train`, docs/post-training.md). The
/// orchestrator's one conversation is rewritten every round (append-only
/// history: each write is a superset); a `fresh` conversation (a
/// specialist, the reviewer) never overwrites an earlier one of the same
/// name. No `out_dir`, no transcript.
fn transcript(cfg: &RunConfig, conv: &Conversation, name: &str, fresh: bool) {
    let Some(out) = &cfg.out_dir else { return };
    let dir = out.join("transcripts");
    let mut file = name.to_string();
    if fresh {
        let mut n = 1;
        while dir.join(format!("{file}.json")).exists() {
            n += 1;
            file = format!("{name}-{n}");
        }
    }
    if let Err(e) = crate::agent::save_transcript(&dir, &file, conv) {
        eprintln!("transcript {file} not written: {e}");
    }
}

fn judge(client: Client, domain: Arc<dyn DomainTools>, cfg: RunConfig, mut state: DesignState, stop: StopReason, rounds: u32) -> RunSummary {
    screen(&client, &cfg, &mut state, domain.as_ref());
    // end on the best verified design if the last one is worse
    if let Some(msg) = state.prefer_best(domain.as_ref()) {
        let text = format!("Final design: {msg}.");
        let _ = client.events.send(Event::AgentMessage { run: cfg.run_id.clone(), agent: "harness".into(), text: text.clone() });
        state.feedback.push(format!("harness: {text}"));
    }
    match state.full_check(domain.as_ref()) {
        Ok(fin) => {
            emit_checks(&client, domain.as_ref(), &state, &fin);
            state.report = Some(fin);
        }
        Err(_) => state.report = None,
    }
    let passed = state.passed() && stop == StopReason::Passed;
    let objective = state.design.as_ref().and_then(|d| domain.objective(d).ok()).map(|(v, l)| (v, l.to_string()));

    let mut built = None;
    if let Some(out) = &cfg.out_dir {
        let mut written: Vec<(String, PathBuf)> = vec![];
        if std::fs::create_dir_all(out).is_ok() {
            let p = out.join("state.json");
            if std::fs::write(&p, serde_json::to_string_pretty(&state).unwrap_or_default()).is_ok() {
                written.push(("design state".into(), p));
            }
            if let Some(d) = &state.design {
                if let Ok(b) = domain.bom(d) {
                    let p = out.join("bom.csv");
                    if std::fs::write(&p, b.csv()).is_ok() {
                        written.push(("bill of materials".into(), p));
                    }
                }
                // Build files only for a design that passed: a builder must
                // never be handed CAD for a failing vehicle.
                if passed {
                    match domain.build(d, out) {
                        Ok(a) => {
                            written.extend(a.files.iter().cloned());
                            built = Some(a);
                        }
                        Err(e) => {
                            let _ = client.events.send(Event::AgentMessage { run: cfg.run_id.clone(), agent: "harness".into(), text: format!("build failed: {e:#}") });
                        }
                    }
                }
            }
        }
        for (label, path) in written {
            let _ = client.events.send(Event::Artifact { run: cfg.run_id.clone(), label, path: path.display().to_string() });
        }
    }

    // the post-design pipeline (CFD, FEA, fab, CAD, USD, sims) on what was
    // built; each stage's failure stays in its own manifest
    let mut stage_runs = vec![];
    if let (Some(arts), Some(out), Some(design)) = (&built, &cfg.out_dir, &state.design) {
        if !cfg.stages.is_empty() {
            let spec = domain.validate_spec(&state.effective_spec()).unwrap_or_else(|_| state.effective_spec());
            let base = wright_core::StageCtx {
                domain: domain.name().into(),
                out_dir: out.clone(),
                dir: out.clone(),
                design: design.clone(),
                spec,
                artifacts: arts.clone(),
                upstream: Default::default(),
            };
            stage_runs = run_stages(&client.events, &cfg.run_id, &cfg.stages, &base, None, false);
        }
    }

    let (value, label) = objective.clone().unwrap_or((0.0, "no design".into()));
    let _ = client.events.send(Event::RunFinished { run: cfg.run_id.clone(), passed, objective: value, objective_label: label });
    RunSummary { passed, stop, rounds, spent_usd: client.spent(), objective, state, artifacts: built, stages: stage_runs, review: Default::default() }
}

/// Run the registered post-design stages (or `only` and what it needs) on
/// `base`, announcing each: an `Artifact` event per item (its path relative
/// to `base.out_dir`, as the build's are) and a harness message per stage
/// (ran, up to date, or failed and why). Used at the end of a run and by
/// `wright stages RUN_DIR`.
pub fn run_stages(
    events: &broadcast::Sender<Event>,
    run_id: &str,
    stages: &wright_core::Stages,
    base: &wright_core::StageCtx,
    only: Option<&str>,
    force: bool,
) -> Vec<wright_core::StageRun> {
    let emit = |r: &wright_core::StageRun| {
        let m = &r.manifest;
        let text = match &m.error {
            Some(e) => format!("stage {}: failed: {e}", r.name),
            None => {
                let failing: Vec<&str> = m.checks.iter().filter(|c| !c.passed).map(|c| c.name.as_str()).collect();
                format!(
                    "stage {}{}: {} file(s), {} check(s){}",
                    r.name,
                    if r.fresh { " (up to date, not re-run)" } else { "" },
                    m.items.len(),
                    m.checks.len(),
                    if failing.is_empty() { String::new() } else { format!(", failing: {}", failing.join(", ")) }
                )
            }
        };
        let _ = events.send(Event::AgentMessage { run: run_id.into(), agent: "harness".into(), text });
        for it in &m.items {
            let _ = events.send(Event::Artifact { run: run_id.into(), label: format!("{}: {}", m.title, it.label), path: format!("{}/{}", r.name, it.path) });
        }
    };
    match only {
        Some(_) => stages.run(base, only, force, emit),
        // every stage: independent ones at once (Stages::run_many, capped at
        // the CPU count), events in the fixed result order afterwards
        None => {
            let names = stages.names(&base.domain);
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let runs = stages.run_many(base, &names, force, cores);
            runs.iter().for_each(emit);
            runs
        }
    }
}

/// Append every event on the channel to a JSON-lines file until the
/// channel closes: the run's audit trail (what each agent saw, called and
/// spent), replayable by the studio.
pub fn spawn_event_log(mut rx: broadcast::Receiver<Event>, path: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use std::io::Write as _;
        let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Ok(line) = serde_json::to_string(&ev) {
                        let _ = writeln!(f, "{line}");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

#[cfg(test)]
mod stated_tests {
    use super::touches_stated;
    use serde_json::json;

    #[test]
    fn an_attack_may_vary_conditions_but_not_requirements() {
        let stated = json!({"requirements": {"range_km": 200, "budget_usd": 2000}});
        assert_eq!(touches_stated(&stated, &json!({"requirements": {"budget_usd": 50}}), String::new()),
                   Some("requirements.budget_usd".into()));
        assert_eq!(touches_stated(&stated, &json!({"conditions": {"wind_ms": 12}}), String::new()), None);
        assert_eq!(touches_stated(&stated, &json!({"requirements": {"range_km": 200}}), String::new()), None);
        assert_eq!(touches_stated(&json!(null), &json!({"anything": 1}), String::new()), None);
    }
}

#[cfg(test)]
mod cad_visual_tests {
    use super::*;
    use crate::client::{ApiError,Provider,Request,Transport};
    use std::sync::{Mutex,atomic::{AtomicUsize,Ordering}};
    use wright_core::{Artifacts,Bom,CheckId,Domain,Finished,Report};
    const PIXEL:&str="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aP9sAAAAASUVORK5CYII=";
    fn image()->Value {json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":PIXEL}})}
    struct Fixture;
    #[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)] enum C { Valid }
    impl CheckId for C {
        const ALL:&'static [Self]=&[C::Valid];
        fn name(self)->&'static str{"fixture"} fn describe(self)->&'static str{"Harness fixture"}
    }
    impl Domain for Fixture {
        const NAME:&'static str="aircraft";
        type Spec=Value;type Design=Value;type Check=C;
        fn spec_schema()->Value{json!({"type":"object"})}
        fn design(_: &Value)->anyhow::Result<Value>{Ok(json!({}))}
        fn check(_: &Value,_: &Value)->Finished{Report::<C>::new().finish()}
        fn bom(_: &Value)->Bom{Bom::default()}
        fn build(_: &Value,_:&std::path::Path)->anyhow::Result<Artifacts>{Ok(Artifacts::default())}
        fn objective(_: &Value)->(f64,&'static str){(0.,"fixture")}
    }
    #[test]
    fn newest_sources_preserve_provenance_and_limits() {
        let mut context=CadVisualContext::default();
        context.user_blocks(&[json!({"type":"text","text":"old reference"}),image()]);
        let render=ToolOutput::ok("current CAD render").with_blocks(vec![image()]);
        context.tool_result("unrelated_tool",&json!({}),&render);
        context.tool_result("cad_action",&json!({"action":"render"}),&render);
        context.user_blocks(&[json!({"type":"text","text":"latest user reference"}),image()]);
        let blocks=context.blocks().unwrap();let text=json!(blocks).to_string();
        assert!(!text.contains("old reference"));assert!(text.contains("current CAD render"));
        assert!(text.find("current CAD render").unwrap()<text.find("latest user reference").unwrap());
        assert_eq!(blocks.iter().filter(|b|b["type"]=="image").count(),2);
        context.tool_result("cad_action",&json!({"action":"check_containment"}),&ToolOutput::ok("read-only containment evidence"));
        context.tool_result("cad_action",&json!({"action":"analyze_wing"}),&ToolOutput::ok("read-only wing analysis"));
        assert_eq!(context.blocks().unwrap().iter().filter(|b|b["type"]=="image").count(),2);
        context.tool_result("cad_action",&json!({"action":"set_parameter"}),&ToolOutput::ok("edited"));
        assert_eq!(context.blocks().unwrap().iter().filter(|b|b["type"]=="image").count(),1);
        context.tool_result("cad_action",&json!({"action":"render"}),&render);
        context.user_blocks(&vec![image();4]);
        assert!(context.blocks().unwrap_err().contains("four images"));
        context.user_blocks(&[json!({"type":"image","source":{"type":"url","url":"https://example.com/image.png"}})]);
        assert!(context.blocks().unwrap_err().contains("remote URLs"));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn actual_delegate_forwards_initial_image_through_codex_adapter() {
        use std::os::unix::fs::PermissionsExt;
        struct Routing {
            calls:AtomicUsize, requests:Mutex<Vec<Request>>, adapter:crate::codex::CodexTransport, events:broadcast::Sender<Event>,
        }
        impl Transport for Routing {
            fn provider(&self)->Provider{Provider::Fake}
            fn send<'a>(&'a self,req:&'a Request)->BoxFut<'a,Result<Value,ApiError>> {
                Box::pin(async move {
                    self.requests.lock().unwrap().push(req.clone());
                    if req.body["model"]==crate::codex::MODEL {return self.adapter.send(req).await;}
                    let content=match self.calls.fetch_add(1,Ordering::SeqCst) {
                        0|2=>vec![crate::fake::tool_use("delegate","delegate",json!({"role":"cad","task":"Inspect the supplied user reference"}))],
                        1=>{let _=self.events.send(Event::Feedback{run:"visual-delegate".into(),view:"iso".into(),sketch_png:format!("data:image/png;base64,{PIXEL}"),note:"fresh user markup".into()});vec![crate::fake::tool_use("design","design",json!({}))]},
                        _=>vec![crate::fake::text("done")],
                    };
                    Ok(crate::fake::reply("claude-opus-5-5",content,10,10))
                })
            }
        }
        let directory=std::env::temp_dir().join(format!("wright-delegate-images-{}",std::process::id()));std::fs::create_dir_all(&directory).unwrap();
        let binary=directory.join("fake-codex");let received=directory.join("received.png");
        let response=json!({"text":"reference received","tool_calls":[{"name":"report","input_json":"{\"summary\":\"reference inspected\"}"}]}).to_string();
        let events=format!("{}\n{}\n",json!({"type":"item.completed","item":{"type":"agent_message","text":response}}),json!({"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":10}}));
        let output_path=received.to_string_lossy().replace('\'',"'\\''");
        std::fs::write(&binary,format!("#!/bin/sh\nif [ \"$1\" = login ]; then echo 'Logged in using ChatGPT'; exit 0; fi\nfound=0\nwhile [ \"$#\" -gt 0 ]; do\n if [ \"$1\" = --image ]; then shift; cp \"$1\" '{output_path}' || exit 4; found=1; fi\n shift\ndone\n[ \"$found\" = 1 ] || exit 5\ncat >/dev/null\ncat <<'EVENTS'\n{events}EVENTS\n")).unwrap();
        std::fs::set_permissions(&binary,std::fs::Permissions::from_mode(0o700)).unwrap();
        let (tx,_)=crate::channel();
        let transport=Arc::new(Routing{calls:AtomicUsize::new(0),requests:Mutex::new(vec![]),events:tx.clone(),adapter:crate::codex::CodexTransport{binary:binary.to_string_lossy().into_owned(),timeout:std::time::Duration::from_secs(5)}});
        let client=Client::new(transport.clone(),crate::Budget::new(5.,45.,None),tx,"visual-delegate");
        let mut cfg=RunConfig::new("visual-delegate","Use my reference",json!({}));cfg.max_rounds=1;cfg.orchestrator_turns=5;cfg.reviewer_turns=0;cfg.models.specialist=ModelCfg::new(crate::codex::MODEL,"low",1000);
        cfg.reference_blocks=vec![json!({"type":"text","text":"initial user reference provenance"}),image()];
        let _=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let requests=transport.requests.lock().unwrap();let cad=requests.iter().find(|r|r.body["model"]==crate::codex::MODEL).expect("CAD delegate request");
        let text=cad.body.to_string();assert!(text.contains("initial user reference provenance"));assert!(text.contains(PIXEL));
        let cad_requests:Vec<_>=requests.iter().filter(|r|r.body["model"]==crate::codex::MODEL).collect();assert_eq!(cad_requests.len(),2);
        let latest=cad_requests[1].body.to_string();assert!(latest.contains("fresh user markup"));assert!(latest.contains(PIXEL));assert!(!latest.contains("initial user reference provenance"));
        assert_eq!(std::fs::read(&received).expect("adapter passed --image"),base64::engine::general_purpose::STANDARD.decode(PIXEL).unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn native_preview_action_emits_project_artifacts_without_step_claim() {
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source=root.join("out/native-reference-contract/mixed.step");
        if !source.exists(){eprintln!("SKIP run native reference fixture first");return;}
        let responses=vec![
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::tool_use("import","cad_action",json!({"action":"import_step","args_json":json!({"path":source,"name":"Mixed","preserve_reference_geometry":true}).to_string()}))],10,10),
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::tool_use("preview","cad_action",json!({"action":"save_preview","args_json":json!({"objects":["Mixed_Solid001"],"reference_objects":["Mixed_Reference001"]}).to_string()}))],10,10),
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::text("preview completed")],10,10),
        ];
        let(tx,mut events)=crate::channel();let transport=Arc::new(crate::fake::ScriptedTransport::new(responses));
        let client=Client::new(transport,crate::Budget::new(5.,45.,None),tx,"preview-artifacts");
        let out=std::env::temp_dir().join(format!("wright-preview-artifacts-{}",std::process::id()));
        let mut cfg=RunConfig::new("preview-artifacts","Show imported reference component",json!({}));cfg.out_dir=Some(out.clone());cfg.max_rounds=1;cfg.orchestrator_turns=3;cfg.reviewer_turns=0;
        let _=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let mut paths=vec![];
        while let Ok(event)=events.try_recv(){match event {
            Event::Artifact{path,..} if path.contains("/cad/")=>paths.push(path),
            Event::ToolResult{tool,ok,summary,..} if tool=="cad_action"=>assert!(ok,"{summary}"),
            _=>{}
        }}
        assert!(paths.iter().any(|p|p.ends_with("preview.FCStd")));
        assert!(paths.iter().any(|p|p.ends_with("preview.glb")));
        assert!(!paths.iter().any(|p|p.ends_with(".step")));
        for path in paths {assert!(std::path::Path::new(&path).exists());}
        std::fs::remove_dir_all(out).unwrap();
    }
    #[tokio::test]
    async fn wing_analysis_emits_json_plot_and_loads_project_artifacts() {
        let source=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out/freecad-aero-sections/fixture.FCStd");
        if !source.exists(){eprintln!("SKIP run native aerodynamic section fixture first");return;}
        let responses=vec![
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::tool_use("open","cad_action",json!({"action":"open_document","args_json":json!({"path":source}).to_string()}))],10,10),
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::tool_use("analysis","cad_action",json!({"action":"analyze_wing","args_json":json!({"object_name":"Wing","alphas_deg":[4],"samples_per_branch":33}).to_string()}))],10,10),
            crate::fake::reply("claude-opus-5-5",vec![crate::fake::text("analysis complete")],10,10),
        ];
        let(tx,mut events)=crate::channel();let transport=Arc::new(crate::fake::ScriptedTransport::new(responses));
        let client=Client::new(transport,crate::Budget::new(5.,45.,None),tx,"wing-artifacts");
        let out=std::env::temp_dir().join(format!("wright-wing-artifacts-{}",std::process::id()));
        let mut cfg=RunConfig::new("wing-artifacts","Analyze actual native wing",json!({}));cfg.out_dir=Some(out.clone());cfg.max_rounds=1;cfg.orchestrator_turns=3;cfg.reviewer_turns=0;
        let _=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let mut paths=vec![];
        while let Ok(event)=events.try_recv(){match event {
            Event::Artifact{path,..} if path.contains("wing-analysis-")=>paths.push(path),
            Event::ToolResult{tool,ok,summary,..} if tool=="cad_action"=>assert!(ok,"{summary}"),
            _=>{}
        }}
        for file in ["analysis.json","wing-analysis.png","panel-loads.csv"]{assert!(paths.iter().any(|p|p.ends_with(file)),"{paths:?}");}
        std::fs::remove_dir_all(out).unwrap();
    }
    #[tokio::test]
    #[ignore = "live subscription: real CAD delegate requests and receives a native rendered PNG"]
    async fn live_cad_delegate_requests_native_render() {
        struct LiveRoute { calls:AtomicUsize,requests:Mutex<Vec<Request>>,adapter:crate::codex::CodexTransport }
        impl Transport for LiveRoute {
            fn provider(&self)->Provider{Provider::Fake}
            fn send<'a>(&'a self,req:&'a Request)->BoxFut<'a,Result<Value,ApiError>> {
                Box::pin(async move {
                    self.requests.lock().unwrap().push(req.clone());
                    if req.body["model"]==crate::codex::MODEL{return self.adapter.send(req).await;}
                    let content=match self.calls.fetch_add(1,Ordering::SeqCst) {
                        0=>vec![crate::fake::tool_use("make","cad_action",json!({"action":"make_part","args_json":"{\"spec\":{\"name\":\"VisionBox\",\"kind\":\"box\",\"params\":{\"x\":30,\"y\":20,\"z\":10}}}"}))],
                        1=>vec![crate::fake::tool_use("delegate","delegate",json!({"role":"cad","task":"VisionBox exists. First call cad_action render with objects [VisionBox], width320,height240. Then inspect the returned image and call report describing its visible shape. Do not edit geometry. You must request the native render before reporting. The render is not fit or hidden-geometry proof."}))],
                        _=>vec![crate::fake::text("connection test finished")],
                    };
                    Ok(crate::fake::reply("claude-opus-5-5",content,10,10))
                })
            }
        }
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/astra-subscription");
        let output=root.join(format!("render-loop-{}",std::process::id()));std::fs::create_dir_all(&output).unwrap();
        let transport=Arc::new(LiveRoute{calls:AtomicUsize::new(0),requests:Mutex::new(vec![]),adapter:crate::codex::CodexTransport::from_env()});
        let(tx,mut events)=crate::channel();let client=Client::new(transport.clone(),crate::Budget::new(5.,45.,None),tx,"live-render-loop");
        let mut cfg=RunConfig::new("live-render-loop","Verify actual Astra CAD render loop",json!({}));cfg.out_dir=Some(output.clone());cfg.max_rounds=1;cfg.orchestrator_turns=3;cfg.reviewer_turns=0;cfg.specialist_turns=3;cfg.models.specialist=ModelCfg::new(crate::codex::MODEL,"low",1500);
        let _=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let requests=transport.requests.lock().unwrap();let cad:Vec<_>=requests.iter().filter(|r|r.body["model"]==crate::codex::MODEL).collect();
        assert!(cad.len()>=2,"Astra must request a render and receive a subsequent image turn");
        let blocks:Vec<_>=cad.last().unwrap().body["messages"].as_array().unwrap().iter().flat_map(|m|m["content"].as_array().into_iter().flatten()).filter(|b|b["type"]=="tool_result").flat_map(|b|b["content"].as_array().into_iter().flatten()).collect();
        let image=blocks.iter().find(|b|b["type"]=="image").expect("actual native PNG in CAD model request");
        let bytes=base64::engine::general_purpose::STANDARD.decode(image["source"]["data"].as_str().unwrap()).unwrap();assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        std::fs::write(output.join("native-render.png"),bytes).unwrap();
        let metadata=blocks.iter().filter(|b|b["type"]=="text").filter_map(|b|b["text"].as_str()).collect::<Vec<_>>().join("\n");assert!(metadata.contains("geometry_fingerprint"));assert!(!metadata.contains("base64"));
        let mut trace=vec![];while let Ok(event)=events.try_recv(){match event {Event::ToolCall{agent,tool,input,..}=>trace.push(json!({"agent":agent,"tool":tool,"input":input})),Event::AgentMessage{agent,text,..} if agent=="specialist:cad"=>trace.push(json!({"agent":agent,"text":text})),_=>{}}}
        assert!(trace.iter().any(|e|e["agent"]=="specialist:cad"&&e["tool"]=="report"),"{trace:?}");
        std::fs::write(root.join("native-render-loop.json"),serde_json::to_string_pretty(&json!({"passed":true,"model":crate::codex::MODEL,"cad_model_calls":cad.len(),"image":output.join("native-render.png"),"native_metadata":metadata,"trace":trace,"scope":"Transport and real native rendering smoke, not visual accuracy or complex CAD proficiency"})).unwrap()).unwrap();
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: read-only diagnosis of the actual VTOL right tail"]
    async fn live_astra_vtol_right_tail_diagnosis() {
        run_live_readonly_diagnosis("out/tsung-vtol-reference/assembly.FCStd", "astra-vtol-diagnosis",5,
            vec!["BoomRight","FinRight","TopStabilizer"],
            "Perform a read-only physical-fit diagnosis of the actual VTOL right-tail selection BoomRight, FinRight, TopStabilizer in the open native document. Use native render and native check on these three objects, and inspect parameters if useful. Report physical fit issues supported by the measurements, name affected pairs, include quantitative evidence, and distinguish visible appearance from checked geometry. Do not edit, move, replace or save geometry. Do not make flight, structural-strength or manufacturing approval claims. If evidence is insufficient, say so. Finish with report.").await;
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: held-out read-only mechanical assessment of actual v1 boom clamp"]
    async fn live_astra_vtol_clamp_v1_mechanical_assessment() {
        run_live_readonly_diagnosis("out/tsung-vtol-tail-joint/assembly.FCStd", "astra-vtol-clamp-v1-diagnosis",5,
            vec!["BoomRight","FinClampDrill1","LowerClampDrill1","TopStabilizer"],
            "Perform a read-only mechanical assessment of the actual v1 VTOL tail joint in the open native document. The physical selection is BoomRight, FinClampDrill1, LowerClampDrill1, TopStabilizer. Use inspect, native render, and native check on this selection. Evaluate whether the geometry provides functional boom clamping, rather than merely being collision-free. Derive your conclusion from native evidence, identify any limitations or missing evidence, and explain the relevant mechanical reasoning. Include quantitative measurements where available. Do not edit, move, replace or save geometry. Do not make flight or structural-strength approval claims. Finish with report.").await;
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: full-aircraft attachment-interface diagnosis"]
    async fn live_astra_vtol_full_assembly_diagnosis() {
        run_live_readonly_diagnosis("out/tsung-vtol-tail-assembly/assembly.FCStd", "astra-vtol-full-assembly-diagnosis",8,
            vec!["WingRight","BoomRight","LiftMotorFR","LiftMotorRR","WingLeft","BoomLeft","LiftMotorFL","LiftMotorRL","TopStabilizer","Pod","PusherMotor","FinNutPocket1","LowerDrillV2_1","Screw0","Screw1","Nut0","Nut1","FinClampLeft","LowerSaddleLeft","ScrewLeft0","ScrewLeft1","NutLeft0","NutLeft1"],
            "Perform a read-only attachment-interface diagnosis of the actual full VTOL assembly. Physical outputs: WingRight, BoomRight, LiftMotorFR, LiftMotorRR, WingLeft, BoomLeft, LiftMotorFL, LiftMotorRL, TopStabilizer, Pod, PusherMotor, FinNutPocket1, LowerDrillV2_1, Screw0, Screw1, Nut0, Nut1, FinClampLeft, LowerSaddleLeft, ScrewLeft0, ScrewLeft1, NutLeft0, NutLeft1. Discover the native tree with inspect summary:true and pagination; use inspect objects:[names] only for relevant geometry and dependencies, avoiding an unscoped full-document dump. Obtain native check evidence across the full physical selection, then examine attachment interfaces that warrant closer assessment using targeted inspections/checks and selected renders. Identify unfinished attachment interfaces, distinguishing geometric contact/connectivity from mechanical retention. Choose any required-contact pairs based on your own assessment. Include quantitative native evidence, clearly identify assumptions and scope, and say which interfaces remain unassessed. Do not treat appearance or zero overlap alone as a working attachment. Do not edit, move, replace or save anything. Do not claim manufacturing, structural-strength or flight approval. Finish with report within eight turns.").await;
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: compact full-aircraft attachment-interface diagnosis"]
    async fn live_astra_vtol_full_assembly_compact_diagnosis() {
        run_live_readonly_diagnosis("out/tsung-vtol-tail-assembly/assembly.FCStd", "astra-vtol-full-assembly-compact-diagnosis",8,
            vec!["WingRight","BoomRight","LiftMotorFR","LiftMotorRR","WingLeft","BoomLeft","LiftMotorFL","LiftMotorRL","TopStabilizer","Pod","PusherMotor","FinNutPocket1","LowerDrillV2_1","Screw0","Screw1","Nut0","Nut1","FinClampLeft","LowerSaddleLeft","ScrewLeft0","ScrewLeft1","NutLeft0","NutLeft1"],
            "Perform a read-only attachment-interface diagnosis of the actual full VTOL assembly. Physical outputs: WingRight, BoomRight, LiftMotorFR, LiftMotorRR, WingLeft, BoomLeft, LiftMotorFL, LiftMotorRL, TopStabilizer, Pod, PusherMotor, FinNutPocket1, LowerDrillV2_1, Screw0, Screw1, Nut0, Nut1, FinClampLeft, LowerSaddleLeft, ScrewLeft0, ScrewLeft1, NutLeft0, NutLeft1. Discover the native tree with inspect summary:true and pagination; use inspect objects:[names] only for relevant geometry and dependencies, avoiding an unscoped full-document dump. Use check summary:true for the full physical selection and targeted contact checks; compact responses still compute all pairs and include every clash and declared contact, counts and components, with omitted_pair_count explicit. Do not assume omitted pairs have measured exact distances. Obtain native check evidence across the full physical selection, then examine attachment interfaces that warrant closer assessment using targeted inspections/checks and selected renders. Identify unfinished attachment interfaces, distinguishing geometric contact/connectivity from mechanical retention. Choose any required-contact pairs based on your own assessment. Include quantitative native evidence, clearly identify assumptions and scope, and say which interfaces remain unassessed. Do not treat appearance or zero overlap alone as a working attachment. Do not edit, move, replace or save anything. Do not claim manufacturing, structural-strength or flight approval. Finish with report within eight turns.").await;
    }

    async fn run_live_readonly_diagnosis(source_relative:&str,case_id:&str,turn_limit:u32,selection:Vec<&str>,task:&str) {
        struct DiagnosisRoute { calls:AtomicUsize, trace:Mutex<Vec<Value>>, source:PathBuf, task:String, adapter:crate::codex::CodexTransport }
        impl Transport for DiagnosisRoute {
            fn provider(&self)->Provider{Provider::Fake}
            fn send<'a>(&'a self,req:&'a Request)->BoxFut<'a,Result<Value,ApiError>> {
                Box::pin(async move {
                    if req.body["model"]==crate::codex::MODEL {
                        let mut readonly=req.clone();
                        if let Some(tools)=readonly.body["tools"].as_array_mut(){
                            tools.retain(|t|t["name"]=="cad_action"||t["name"]=="report");
                            for tool in tools {if tool["name"]=="cad_action" {tool["input_schema"]["properties"]["action"]["enum"]=json!(["inspect","render","check"]);}}
                        }
                        let result=self.adapter.send(&readonly).await;
                        self.trace.lock().unwrap().push(json!({"route":"astra","request":readonly.body,"response":match &result {Ok(v)=>v.clone(),Err(e)=>json!({"error":e.to_string()})}}));
                        let response=result?;
                        // Enforce the evaluation's read-only boundary before any host execution.
                        for call in response["content"].as_array().into_iter().flatten().filter(|b|b["type"]=="tool_use") {
                            if call["name"]=="report" {continue;}
                            if call["name"]!="cad_action" || !matches!(call["input"]["action"].as_str(),Some("inspect"|"render"|"check")) {
                                return Err(ApiError::Config("read-only VTOL evaluation rejected a mutating/unoffered action".into()));
                            }
                        }
                        return Ok(response);
                    }
                    let content=match self.calls.fetch_add(1,Ordering::SeqCst) {
                        0=>vec![crate::fake::tool_use("open","cad_action",json!({"action":"open_document","args_json":json!({"path":self.source}).to_string()}))],
                        1=>vec![crate::fake::tool_use("diagnose","delegate",json!({"role":"cad","task":self.task}))],
                        _=>vec![crate::fake::text("read-only diagnosis concluded")],
                    };
                    let response=crate::fake::reply("claude-opus-5-5",content,10,10);
                    self.trace.lock().unwrap().push(json!({"route":"scripted_host","request":req.body,"response":response}));Ok(response)
                })
            }
        }
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source=std::fs::canonicalize(root.join(source_relative)).unwrap();
        let original=std::fs::read(&source).unwrap();
        let output=root.join(format!("out/{case_id}/run-{}",std::process::id()));std::fs::create_dir_all(&output).unwrap();
        let transport=Arc::new(DiagnosisRoute{calls:AtomicUsize::new(0),trace:Mutex::new(vec![]),source:source.clone(),task:task.into(),adapter:crate::codex::CodexTransport::from_env()});
        let(tx,mut events)=crate::channel();let client=Client::new(transport.clone(),crate::Budget::new(12.,45.,None),tx,case_id);
        let mut cfg=RunConfig::new(case_id,"Read-only actual VTOL mechanical diagnosis",json!({}));cfg.out_dir=Some(output.clone());cfg.max_rounds=1;cfg.orchestrator_turns=3;cfg.reviewer_turns=0;cfg.specialist_turns=turn_limit;cfg.models.specialist=ModelCfg::new(crate::codex::MODEL,"low",3500);
        let outcome=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let trace=transport.trace.lock().unwrap().clone();
        std::fs::write(output.join("full-request-response-trace.json"),serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let mut event_trace=vec![];let mut report=None;
        while let Ok(event)=events.try_recv(){
            if let Event::ToolCall{agent,tool,input,..}=&event {if agent=="specialist:cad"&&tool=="report" {report=input["summary"].as_str().map(str::to_owned);}}
            event_trace.push(serde_json::to_value(event).unwrap());
        }
        std::fs::write(output.join("events.json"),serde_json::to_string_pretty(&event_trace).unwrap()).unwrap();
        std::fs::write(output.join("astra-report.txt"),report.as_deref().unwrap_or("No report produced; inspect trace for failure.")).unwrap();
        // Fresh native process/document comparison; this oracle is never sent to Astra.
        let mut oracle=wright_cad::Session::new(output.join("independent-native"));
        oracle.action("open_document",json!({"path":source})).unwrap();
        let objects=json!(selection);
        let mut expected_names=selection.clone();expected_names.sort();
        let independent=oracle.action("check",json!({"objects":objects})).unwrap();
        let inspected=oracle.action("inspect",json!({"objects":selection})).unwrap();
        std::fs::write(output.join("independent-inspect.json"),serde_json::to_string_pretty(&inspected).unwrap()).unwrap();
        std::fs::write(output.join("independent-check.json"),serde_json::to_string_pretty(&independent).unwrap()).unwrap();
        let mut observed_checks=vec![];let mut image_count=0;
        for turn in trace.iter().filter(|t|t["route"]=="astra") {
            for block in turn["request"]["messages"].as_array().into_iter().flatten().flat_map(|m|m["content"].as_array().into_iter().flatten()).filter(|b|b["type"]=="tool_result") {
                let content=&block["content"];
                let texts:Vec<&str>=if let Some(s)=content.as_str(){vec![s]}else{content.as_array().into_iter().flatten().filter(|b|b["type"]=="text").filter_map(|b|b["text"].as_str()).collect()};
                for text in texts {if let Ok(value)=serde_json::from_str::<Value>(text){if value.get("pairs").is_some(){observed_checks.push(value);}}}
                for image in content.as_array().into_iter().flatten().filter(|b|b["type"]=="image") {
                    let bytes=base64::engine::general_purpose::STANDARD.decode(image["source"]["data"].as_str().unwrap()).unwrap();
                    image_count+=1;std::fs::write(output.join(format!("model-visible-render-{image_count}.png")),bytes).unwrap();
                }
            }
        }
        let matching=observed_checks.iter().any(|c| {
            let mut selected:Vec<_>=c["objects"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();selected.sort();
            if selected!=expected_names || c["overlap_count"]!=independent["overlap_count"] || c["pairs_total"]!=independent["pairs_total"] {return false;}
            if c["pair_details_scope"]=="all_clashes_and_declared_contacts" {
                // This initial oracle compares geometry only; the separate replay checker
                // verifies the exact model-declared contacts and all compact counts.
                let actual=c["pairs"].as_array().unwrap();
                if c["omitted_pair_count"].as_u64()!=Some(independent["pairs_total"].as_u64().unwrap()-actual.len() as u64) {return false;}
                if actual.iter().filter(|p|p["clash"]==true).count()!=independent["overlap_count"].as_u64().unwrap() as usize{return false;}
                return actual.iter().all(|actual|independent["pairs"].as_array().unwrap().iter().any(|expected| {
                    let same_pair=(actual["a"]==expected["a"]&&actual["b"]==expected["b"])||(actual["a"]==expected["b"]&&actual["b"]==expected["a"]);
                    let volume=expected["overlap_mm3"].as_f64().unwrap();
                    same_pair && actual["clash"]==expected["clash"] && actual["overlap_mm3"].as_f64().is_some_and(|v|(v-volume).abs()<=1e-5_f64.max(volume.abs()*1e-8))
                }));
            }
            independent["pairs"].as_array().unwrap().iter().all(|expected| {
                c["pairs"].as_array().into_iter().flatten().any(|actual| {
                    let same_pair=(actual["a"]==expected["a"]&&actual["b"]==expected["b"])||(actual["a"]==expected["b"]&&actual["b"]==expected["a"]);
                    let volume=expected["overlap_mm3"].as_f64().unwrap();
                    same_pair && actual["clash"]==expected["clash"] && actual["overlap_mm3"].as_f64().is_some_and(|v|(v-volume).abs()<=1e-5_f64.max(volume.abs()*1e-8))
                })
            })
        });
        let unchanged=std::fs::read(&source).unwrap()==original;
        let result=json!({"source":source,"source_bytes_unchanged":unchanged,"model":crate::codex::MODEL,"astra_turns":trace.iter().filter(|t|t["route"]=="astra").count(),"model_visible_images":image_count,"model_received_check_matches_independent_native":matching,"independent_overlap_count":independent["overlap_count"],"report_produced":report.is_some(),"report":report,"harness_stop":format!("{:?}",outcome.stop),"scope":"One read-only task-specific diagnosis, not broad CAD proficiency or a complete design assessment"});
        std::fs::write(output.join("comparison.json"),serde_json::to_string_pretty(&result).unwrap()).unwrap();
        eprintln!("VTOL diagnosis evidence: {}",output.display());
        assert!(unchanged,"source FCStd was modified");assert!(report.is_some(),"no diagnosis report: {result}");assert!(image_count>0,"no render reached Astra");assert!(matching,"native evidence mismatch: {result}");
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: bounded typed repair attempt on a copy of actual v1 clamp"]
    async fn live_astra_vtol_clamp_constrained_repair_attempt() {
        struct RepairRoute {calls:AtomicUsize,trace:Mutex<Vec<Value>>,source:PathBuf,adapter:crate::codex::CodexTransport}
        impl Transport for RepairRoute {
            fn provider(&self)->Provider{Provider::Fake}
            fn send<'a>(&'a self,req:&'a Request)->BoxFut<'a,Result<Value,ApiError>> {
                Box::pin(async move {
                    if req.body["model"]==crate::codex::MODEL {
                        let allowed=["inspect","render","check","make_part","boolean","make_parameters","bind_parameter","set_parameter","set_profile","place","save"];
                        let mut typed=req.clone();
                        if let Some(tools)=typed.body["tools"].as_array_mut(){tools.retain(|t|t["name"]=="cad_action"||t["name"]=="report");for t in tools {if t["name"]=="cad_action"{t["input_schema"]["properties"]["action"]["enum"]=json!(allowed);}}}
                        let result=self.adapter.send(&typed).await;
                        self.trace.lock().unwrap().push(json!({"route":"astra","request":typed.body,"response":match &result{Ok(v)=>v.clone(),Err(e)=>json!({"error":e.to_string()})}}));
                        let response=result?;
                        for call in response["content"].as_array().into_iter().flatten().filter(|b|b["type"]=="tool_use") {
                            if call["name"]=="report"{continue;}
                            if call["name"]!="cad_action"||!call["input"]["action"].as_str().is_some_and(|a|allowed.contains(&a)){return Err(ApiError::Config("repair evaluation permits only offered typed CAD actions and report".into()));}
                        }
                        return Ok(response);
                    }
                    let content=match self.calls.fetch_add(1,Ordering::SeqCst){
                        0=>vec![crate::fake::tool_use("open","cad_action",json!({"action":"open_document","args_json":json!({"path":self.source}).to_string()}))],
                        1=>vec![crate::fake::tool_use("repair","delegate",json!({"role":"cad","task":"Repair the actual v1 tail-joint geometry in this copied native session using only typed CAD actions. Current physical outputs are BoomRight, FinClampDrill1, LowerClampDrill1, TopStabilizer. Design requirements: both clamp halves must have nominal radius 5.0 mm seats coaxial with the existing boom and true geometric contact with it without positive-volume overlap; leave a 0.5 mm remaining split gap between the clamp halves. Keep BoomRight and TopStabilizer geometry unchanged. Preserve the fin contour and material outside the allowed root region [620,185,-22] to [780,255,8] mm, including its existing top seat. Inspect the native feature tree and choose the edits yourself. Preserve editable native dependencies. Verify the final four physical outputs with native check including both boom/clamp contact requirements, inspect and render the result, then save the four final physical outputs. Retain the original BoomRight and TopStabilizer output names; clearly identify the final upper-fin/clamp and lower-clamp output names in your report. Do not change or overwrite the original input file. If you cannot meet the requirements within this bounded task, report failure honestly and leave the trace. Missing fasteners, preload, compliance/friction and load verification must remain explicit; meeting these nominal geometric requirements is not proof of a fully functional joint, structural strength or flight readiness."}))],
                        _=>vec![crate::fake::text("bounded repair attempt concluded")],
                    };
                    let response=crate::fake::reply("claude-opus-5-5",content,10,10);self.trace.lock().unwrap().push(json!({"route":"scripted_host","request":req.body,"response":response}));Ok(response)
                })
            }
        }
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");let source=std::fs::canonicalize(root.join("out/tsung-vtol-tail-joint/assembly.FCStd")).unwrap();let original=std::fs::read(&source).unwrap();
        let output=root.join(format!("out/astra-vtol-clamp-repair/run-{}",std::process::id()));std::fs::create_dir_all(&output).unwrap();
        let transport=Arc::new(RepairRoute{calls:AtomicUsize::new(0),trace:Mutex::new(vec![]),source:source.clone(),adapter:crate::codex::CodexTransport::from_env()});
        let(tx,mut events)=crate::channel();let client=Client::new(transport.clone(),crate::Budget::new(25.,45.,None),tx,"vtol-clamp-repair");
        let mut cfg=RunConfig::new("vtol-clamp-repair","Constrained native repair attempt on copied actual v1 clamp",json!({}));cfg.out_dir=Some(output.clone());cfg.max_rounds=1;cfg.orchestrator_turns=3;cfg.reviewer_turns=0;cfg.specialist_turns=10;cfg.models.specialist=ModelCfg::new(crate::codex::MODEL,"low",4000);
        let outcome=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let trace=transport.trace.lock().unwrap().clone();std::fs::write(output.join("full-request-response-trace.json"),serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let mut event_trace=vec![];let mut report=None;let mut pending_save=None;let mut saved_selection=None;
        while let Ok(event)=events.try_recv(){
            match &event {
                Event::ToolCall{agent,tool,input,..} if agent=="specialist:cad" && tool=="report"=>report=input["summary"].as_str().map(str::to_owned),
                Event::ToolCall{agent,tool,input,..} if agent=="specialist:cad" && tool=="cad_action"=>{
                    pending_save=if input["action"]=="save"{serde_json::from_str::<Value>(input["args_json"].as_str().unwrap_or("")).ok().map(|p|p["objects"].clone())}else{None};
                }
                Event::ToolResult{agent,tool,ok,..} if agent=="specialist:cad"&&tool=="cad_action"&&*ok&&pending_save.is_some()=>saved_selection=pending_save.take(),
                _=>{}
            }
            event_trace.push(serde_json::to_value(event).unwrap());
        }
        std::fs::write(output.join("events.json"),serde_json::to_string_pretty(&event_trace).unwrap()).unwrap();std::fs::write(output.join("astra-report.txt"),report.as_deref().unwrap_or("No report produced within the bounded attempt.")).unwrap();
        let unchanged=std::fs::read(&source).unwrap()==original;
        let result=json!({"execution_completed":true,"repair_passed":null,"independent_oracle_pending":true,"source":source,"source_bytes_unchanged":unchanged,"model":crate::codex::MODEL,"astra_turns":trace.iter().filter(|t|t["route"]=="astra").count(),"turn_limit":10,"saved_selection":saved_selection,"report":report,"harness_stop":format!("{:?}",outcome.stop),"scope":"Bounded model-driven repair attempt; geometric success must be decided by a fresh native oracle, not this execution status"});
        std::fs::write(output.join("attempt.json"),serde_json::to_string_pretty(&result).unwrap()).unwrap();eprintln!("VTOL repair attempt evidence: {}",output.display());assert!(unchanged,"input source changed");
    }

    #[tokio::test]
    #[ignore = "live Astra subscription: exact manufacturer CAD component study"]
    async fn live_astra_cots_component_study() {
        struct CotsRoute { calls:AtomicUsize, trace:Mutex<Vec<Value>>, adapter:crate::codex::CodexTransport }
        impl Transport for CotsRoute {
            fn provider(&self)->Provider{Provider::Fake}
            fn send<'a>(&'a self,req:&'a Request)->BoxFut<'a,Result<Value,ApiError>> {
                Box::pin(async move {
                    if req.body["model"]==crate::codex::MODEL {
                        let allowed=["component_catalog","import_component","place","inspect","render","check","save"];
                        let mut typed=req.clone();
                        if let Some(tools)=typed.body["tools"].as_array_mut(){tools.retain(|t|t["name"]=="cad_action"||t["name"]=="report");for t in tools {if t["name"]=="cad_action"{t["input_schema"]["properties"]["action"]["enum"]=json!(allowed);}}}
                        let result=self.adapter.send(&typed).await;
                        self.trace.lock().unwrap().push(json!({"route":"astra","request":typed.body,"response":match &result{Ok(v)=>v.clone(),Err(e)=>json!({"error":e.to_string()})}}));
                        let response=result?;
                        for call in response["content"].as_array().into_iter().flatten().filter(|b|b["type"]=="tool_use") {
                            if call["name"]=="report"{continue;}
                            if call["name"]!="cad_action"||!call["input"]["action"].as_str().is_some_and(|a|allowed.contains(&a)){return Err(ApiError::Config("component study permits only offered typed actions".into()));}
                        }
                        return Ok(response);
                    }
                    let content=if self.calls.fetch_add(1,Ordering::SeqCst)==0 {
                        vec![crate::fake::tool_use("study","delegate",json!({"role":"cad","task":"Create a separate native manufacturer-component study, not an aircraft BOM change. Discover CAD for existing BOM catalog IDs tmotor_mn2806_evo_650kv and tmotor_p12x4_cf through component_catalog, verify exact model identities and import their actual manufacturer geometry using import_component. Preserve source size and supplier geometry. Choose explicit rigid placements that separate the components by at least 20 mm; do not imply an assembled motor/prop mount or infer verified mounting datums from bounds. Inspect the imported objects and provenance, check actual separation/overlap, render the study, and save its physical objects. Report the exact models, object names, placements, source provenance and limitations. Do not substitute either model for the reference aircraft's AOS2807/Gemfan7042, change a BOM, recreate envelopes, claim compatibility, electrical fit or flight readiness. Finish within eight turns."}))]
                    }else{vec![crate::fake::text("component study concluded")]};
                    let response=crate::fake::reply("claude-opus-5-5",content,10,10);self.trace.lock().unwrap().push(json!({"route":"scripted_host","request":req.body,"response":response}));Ok(response)
                })
            }
        }
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output=root.join(format!("out/astra-cots-component-study/run-{}",std::process::id()));std::fs::create_dir_all(&output).unwrap();
        let transport=Arc::new(CotsRoute{calls:AtomicUsize::new(0),trace:Mutex::new(vec![]),adapter:crate::codex::CodexTransport::from_env()});
        let(tx,mut events)=crate::channel();let client=Client::new(transport.clone(),crate::Budget::new(25.,45.,None),tx,"cots-component-study");
        let mut cfg=RunConfig::new("cots-component-study","Separate exact-manufacturer native component study",json!({}));cfg.out_dir=Some(output.clone());cfg.max_rounds=1;cfg.orchestrator_turns=2;cfg.reviewer_turns=0;cfg.specialist_turns=8;cfg.models.specialist=ModelCfg::new(crate::codex::MODEL,"low",4000);
        let outcome=run(client,crate::tools::tools::<Fixture>(),cfg).await;
        let trace=transport.trace.lock().unwrap().clone();std::fs::write(output.join("full-request-response-trace.json"),serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let mut event_trace=vec![];let mut report=None;
        while let Ok(event)=events.try_recv(){if let Event::ToolCall{agent,tool,input,..}=&event {if agent=="specialist:cad"&&tool=="report"{report=input["summary"].as_str().map(str::to_owned);}}event_trace.push(serde_json::to_value(event).unwrap());}
        std::fs::write(output.join("events.json"),serde_json::to_string_pretty(&event_trace).unwrap()).unwrap();std::fs::write(output.join("astra-report.txt"),report.as_deref().unwrap_or("No report within bounded run")).unwrap();
        std::fs::write(output.join("attempt.json"),serde_json::to_string_pretty(&json!({"model":crate::codex::MODEL,"astra_turns":trace.iter().filter(|t|t["route"]=="astra").count(),"harness_stop":format!("{:?}",outcome.stop),"report":report,"independent_native_oracle_pending":true,"scope":"A task-specific component import study, not a compatibility or flight assessment"})).unwrap()).unwrap();
        eprintln!("COTS study evidence: {}",output.display());
    }

}
