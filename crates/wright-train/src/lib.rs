//! Recorded Wright runs as post-training data.
//!
//! The format and the workflow follow SWE-smith's rejection-sampling
//! fine-tuning pipeline (SWE-bench/SWE-smith, `swesmith/train/traj_mgr/`
//! `collect_trajs.py` and `utils.py`): one JSON object per trajectory, its
//! `messages` as the model saw them with native tool calls (their style
//! "tool", `transform_traj_toolcalls`), **tagged** `resolved` by an
//! evaluation that is not the agent's own opinion, never filtered at
//! export; `instance_id`, `traj_id` and `model` beside it. The training
//! set is the `resolved == true` subset (`docs/guides/train_swe_agent.md`,
//! "Rejection Sampling Fine Tuning"). `tools` sits next to `messages` as
//! in the OpenAI fine-tuning JSONL schema, and the system prompt is the
//! first message, as in SWE-smith's output.
//!
//! Here the evaluation is the check report. A trajectory is `resolved`
//! when the run's final report, re-read from `state.json`, passes under
//! `wright_core::Finished::passed` (every declared check measured, finite
//! and non-negative; a report that shows nothing fails) **and** the
//! harness itself ended the run as passed (the reviewer found nothing
//! left). The graded reward `checks_passed` / `checks_total` comes from the
//! same report; what a policy can and cannot do to it is proved in
//! `lean/Wright/Reward.lean`, and the numbers it is made of are
//! cross-checked by the Haskell oracle (docs/verification.md).
//!
//! Sources: `<run>/transcripts/*.json` (written by the harness: the exact
//! `system`, `tools` and append-only `messages` of every agent
//! conversation), `<run>/events.jsonl` (the verdict, the objective, the
//! spend, every check report along the way) and `<run>/state.json` (the
//! final report). A run recorded before transcripts existed is
//! reconstructed from its events and marked `lossy`: tool results are cut
//! to 400 characters there and the first user turn is not recorded, so it
//! is for reading, not training.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use wright_core::{Event, Finished};

/// One agent conversation as the harness recorded it.
#[derive(Debug, Clone)]
pub struct Transcript {
    /// File stem: `orchestrator`, `reviewer-r2`, `specialist-aero-r1`.
    pub name: String,
    pub agent: String,
    pub model: Option<String>,
    pub system: String,
    pub tools: Vec<Value>,
    pub messages: Vec<Value>,
    /// Rebuilt from `events.jsonl` rather than recorded.
    pub lossy: bool,
}

/// What a run directory recorded that training reads.
#[derive(Debug, Clone)]
pub struct Run {
    pub dir: PathBuf,
    pub id: String,
    pub domain: String,
    pub request: String,
    /// `run_finished`: (passed, objective, label), if the run finished.
    pub finished: Option<(bool, f64, String)>,
    /// The last `spend` event: cumulative USD.
    pub spend_usd: f64,
    /// (passed, failed) of every `checks` event, in order.
    pub check_history: Vec<(usize, usize)>,
    /// The final check report from `state.json`, if it was written.
    pub report: Option<Finished>,
    pub transcripts: Vec<Transcript>,
}

/// The verifier's labels for one trajectory.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reward {
    /// `Finished::passed` on the final report: the binary reward.
    pub passed: bool,
    /// Declared checks that passed, out of all declared: the graded reward.
    pub checks_passed: usize,
    pub checks_total: usize,
    /// The harness's own verdict (`run_finished.passed`), kept apart.
    pub harness_passed: Option<bool>,
    pub objective: Option<f64>,
    pub objective_label: Option<String>,
    pub spend_usd: f64,
    /// [passed, failed] of every check report during the run, in order:
    /// the verifier's view of progress, for process rewards.
    pub check_history: Vec<[usize; 2]>,
}

/// One line of the export.
#[derive(Debug, Clone, Serialize)]
pub struct Trajectory {
    pub traj_id: String,
    pub instance_id: String,
    pub domain: String,
    pub request: String,
    pub agent: String,
    pub model: Option<String>,
    /// The final report passes and the harness ended the run as passed.
    pub resolved: bool,
    pub lossy: bool,
    pub reward: Reward,
    pub tools: Vec<Value>,
    /// `[{"role":"system",...}, ...]` in the Messages API shape, with
    /// `tool_use` / `tool_result` blocks as sent.
    pub messages: Vec<Value>,
}

/// Read a run directory. `events.jsonl` is required; `state.json` and
/// `transcripts/` are not (their absence makes every trajectory unresolved
/// or lossy, never silently fine).
pub fn read_run(dir: &Path) -> Result<Run> {
    let text = std::fs::read_to_string(dir.join("events.jsonl")).with_context(|| format!("reading {}/events.jsonl", dir.display()))?;
    let mut events = vec![];
    for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let e: Event = serde_json::from_str(line).with_context(|| format!("{}/events.jsonl line {}", dir.display(), i + 1))?;
        events.push(e);
    }
    let (mut id, mut domain, mut request) = (String::new(), String::new(), String::new());
    let mut finished = None;
    let mut spend_usd = 0.0;
    let mut check_history = vec![];
    for e in &events {
        match e {
            Event::RunStarted { run, domain: d, request: r } => {
                id = run.clone();
                domain = d.clone();
                request = r.clone();
            }
            Event::Checks { passed, failed, .. } => check_history.push((*passed, *failed)),
            Event::Spend { usd, .. } => spend_usd = *usd,
            Event::RunFinished { passed, objective, objective_label, .. } => finished = Some((*passed, *objective, objective_label.clone())),
            _ => {}
        }
    }
    if id.is_empty() {
        anyhow::bail!("{}/events.jsonl has no run_started event", dir.display());
    }
    let report = match std::fs::read_to_string(dir.join("state.json")) {
        Ok(s) => {
            let v: Value = serde_json::from_str(&s).with_context(|| format!("parsing {}/state.json", dir.display()))?;
            match v.get("report") {
                Some(r) if !r.is_null() => Some(serde_json::from_value::<Finished>(r.clone()).with_context(|| format!("{}/state.json report", dir.display()))?),
                _ => None,
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}/state.json", dir.display())),
    };
    let mut transcripts = read_transcripts(&dir.join("transcripts"))?;
    if transcripts.is_empty() {
        transcripts = from_events(&events, &request);
    }
    Ok(Run { dir: dir.to_path_buf(), id, domain, request, finished, spend_usd, check_history, report, transcripts })
}

fn read_transcripts(dir: &Path) -> Result<Vec<Transcript>> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(out) };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    for p in paths {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p)?).with_context(|| format!("parsing {}", p.display()))?;
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let messages = v["messages"].as_array().cloned().with_context(|| format!("{}: no messages", p.display()))?;
        out.push(Transcript {
            name,
            agent: v["agent"].as_str().unwrap_or("").to_string(),
            model: v["model"]["id"].as_str().map(str::to_string),
            system: v["system"].as_str().unwrap_or("").to_string(),
            tools: v["tools"].as_array().cloned().unwrap_or_default(),
            messages,
            lossy: false,
        });
    }
    Ok(out)
}

/// Rebuild each model agent's conversation from the event stream: an
/// `agent_message` and the `tool_call`s after it are one assistant turn,
/// the `tool_result`s that follow are the next user turn. The harness's
/// and the budget's own messages are not model turns and are skipped. The
/// first user turn (the task and the design state) was never recorded, so
/// a placeholder names the request; the result is marked lossy.
fn from_events(events: &[Event], request: &str) -> Vec<Transcript> {
    struct Build {
        messages: Vec<Value>,
        role: &'static str,
        blocks: Vec<Value>,
        open_ids: Vec<String>,
        calls: usize,
    }
    impl Build {
        fn flush(&mut self) {
            if !self.blocks.is_empty() {
                self.messages.push(json!({"role": self.role, "content": std::mem::take(&mut self.blocks)}));
            }
        }
        fn turn(&mut self, role: &'static str) {
            if self.role != role {
                self.flush();
                self.role = role;
            }
        }
    }
    let mut order: Vec<String> = vec![];
    let mut per: BTreeMap<String, Build> = BTreeMap::new();
    let placeholder = format!(
        "[reconstructed from events.jsonl: the first user turn (task and design state) was not recorded] A person asked: {request}"
    );
    let fresh = |order: &mut Vec<String>, agent: &str| {
        order.push(agent.to_string());
        Build { messages: vec![json!({"role": "user", "content": [{"type": "text", "text": placeholder}]})], role: "assistant", blocks: vec![], open_ids: vec![], calls: 0 }
    };
    for e in events {
        match e {
            Event::AgentMessage { agent, text, .. } if agent != "harness" && agent != "budget" => {
                let b = per.entry(agent.clone()).or_insert_with(|| fresh(&mut order, agent));
                b.turn("assistant");
                b.blocks.push(json!({"type": "text", "text": text}));
            }
            Event::ToolCall { agent, tool, input, .. } => {
                let b = per.entry(agent.clone()).or_insert_with(|| fresh(&mut order, agent));
                b.turn("assistant");
                b.calls += 1;
                let id = format!("toolu_{:04}", b.calls);
                b.open_ids.push(id.clone());
                b.blocks.push(json!({"type": "tool_use", "id": id, "name": tool, "input": input}));
            }
            Event::ToolResult { agent, ok, summary, .. } => {
                let b = per.entry(agent.clone()).or_insert_with(|| fresh(&mut order, agent));
                b.turn("user");
                let id = if b.open_ids.is_empty() { "toolu_unknown".to_string() } else { b.open_ids.remove(0) };
                b.blocks.push(json!({"type": "tool_result", "tool_use_id": id, "content": summary, "is_error": !ok}));
            }
            _ => {}
        }
    }
    order
        .into_iter()
        .map(|agent| {
            let mut b = per.remove(&agent).unwrap();
            b.flush();
            Transcript { name: agent.replace(':', "-"), agent, model: None, system: String::new(), tools: vec![], messages: b.messages, lossy: true }
        })
        .collect()
}

/// The verifier's labels for a run.
pub fn reward(run: &Run) -> Reward {
    let (passed, checks_passed, checks_total) = match &run.report {
        Some(r) => (r.passed(), r.outcomes.iter().filter(|o| o.passed()).count(), r.outcomes.len()),
        None => (false, 0, 0),
    };
    Reward {
        passed,
        checks_passed,
        checks_total,
        harness_passed: run.finished.as_ref().map(|f| f.0),
        objective: run.finished.as_ref().map(|f| f.1),
        objective_label: run.finished.as_ref().map(|f| f.2.clone()),
        spend_usd: run.spend_usd,
        check_history: run.check_history.iter().map(|&(p, f)| [p, f]).collect(),
    }
}

/// `resolved`: the final report passes and the harness ended the run as
/// passed. Either alone is not enough: a passing report with a reviewer
/// issue left open is not a finished design, and the harness's word
/// without the report is the agent grading itself.
pub fn resolved(run: &Run) -> bool {
    run.report.as_ref().is_some_and(Finished::passed) && run.finished.as_ref().is_some_and(|f| f.0)
}

/// Every agent conversation of a run as a trajectory, all carrying the
/// run's labels.
pub fn trajectories(run: &Run) -> Vec<Trajectory> {
    let reward = reward(run);
    let resolved = resolved(run);
    run.transcripts
        .iter()
        .map(|t| {
            let mut messages = Vec::with_capacity(t.messages.len() + 1);
            if !t.system.is_empty() {
                messages.push(json!({"role": "system", "content": t.system}));
            }
            messages.extend(t.messages.iter().cloned());
            Trajectory {
                traj_id: format!("{}.{}", run.id, t.name),
                instance_id: run.id.clone(),
                domain: run.domain.clone(),
                request: run.request.clone(),
                agent: t.agent.clone(),
                model: t.model.clone(),
                resolved,
                lossy: t.lossy,
                reward: reward.clone(),
                tools: t.tools.clone(),
                messages,
            }
        })
        .collect()
}

/// One run as the export read it: what a researcher needs to pick runs
/// before opening `trajectories.jsonl`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunSummary {
    pub id: String,
    pub dir: PathBuf,
    pub domain: String,
    pub request: String,
    /// Transcript names (`orchestrator`, `reviewer-r2`, `specialist-aero-r1`).
    pub agents: Vec<String>,
    pub trajectories: usize,
    pub resolved: bool,
    /// Every trajectory of the run is rebuilt from events (no transcripts).
    pub lossy: bool,
    pub reward: Reward,
}

/// Number of bins of the graded-reward histogram: `checks_passed /
/// checks_total` in tenths, the last bin being exactly 1 (a passing
/// report). A run with no report (`checks_total == 0`) is counted in
/// `no_report`, not in a bin.
pub const REWARD_BINS: usize = 11;

/// What an export wrote. Counts follow SWE-smith's collector
/// (`collect_trajs.py` prints found / resolved); the per-run rows and the
/// reward histogram are added here so the labels can be inspected without
/// reading the trajectories.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub runs: usize,
    pub trajectories: usize,
    pub resolved: usize,
    pub lossy: usize,
    /// Trajectories recorded by the harness (not rebuilt): `trajectories - lossy`.
    pub full: usize,
    pub written: usize,
    pub per_domain: BTreeMap<String, [usize; 2]>,
    /// Runs per graded-reward bin, `checks_passed / checks_total` in
    /// tenths; index 10 is the full reward.
    pub reward_histogram: [usize; REWARD_BINS],
    /// Runs whose `state.json` carried no report.
    pub no_report: usize,
    pub run_summaries: Vec<RunSummary>,
    pub skipped: Vec<String>,
    pub output: PathBuf,
}

/// The histogram bin of a graded reward; `None` when nothing was declared.
pub fn reward_bin(r: &Reward) -> Option<usize> {
    if r.checks_total == 0 {
        return None;
    }
    Some((r.checks_passed * (REWARD_BINS - 1)) / r.checks_total)
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} run(s): {} trajectories, {} resolved, {} lossy, {} full; {} written to {}", self.runs, self.trajectories, self.resolved, self.lossy, self.full, self.written, self.output.display())?;
        for (d, [n, r]) in &self.per_domain {
            writeln!(f, "  {d}: {n} trajectories, {r} resolved")?;
        }
        for r in &self.run_summaries {
            writeln!(
                f,
                "  {} [{}]: {} trajectories ({}), checks {}/{}, {}{}",
                r.id,
                r.domain,
                r.trajectories,
                r.agents.join(", "),
                r.reward.checks_passed,
                r.reward.checks_total,
                if r.resolved { "resolved" } else { "not resolved" },
                if r.lossy { ", lossy" } else { "" }
            )?;
        }
        let bins: Vec<String> = self.reward_histogram.iter().enumerate().map(|(i, n)| format!("{:.1}:{n}", i as f64 / (REWARD_BINS - 1) as f64)).collect();
        writeln!(f, "  reward histogram (checks passed / total): {}; no report: {}", bins.join(" "), self.no_report)?;
        for s in &self.skipped {
            writeln!(f, "  skipped: {s}")?;
        }
        Ok(())
    }
}

/// Export `dirs` to `out/trajectories.jsonl` (one JSON object per line) and
/// `out/summary.json`. With `resolved_only`, only resolved trajectories are
/// written (the RFT set); the counts still describe every run read. A run
/// directory that does not read is skipped and named, as SWE-smith's
/// collector does, not fatal.
pub fn export(dirs: &[PathBuf], out: &Path, resolved_only: bool) -> Result<Summary> {
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let path = out.join("trajectories.jsonl");
    let mut lines = String::new();
    let mut s = Summary { output: path.clone(), ..Default::default() };
    for d in dirs {
        let run = match read_run(d) {
            Ok(r) => r,
            Err(e) => {
                s.skipped.push(format!("{}: {e:#}", d.display()));
                continue;
            }
        };
        s.runs += 1;
        let rw = reward(&run);
        match reward_bin(&rw) {
            Some(b) => s.reward_histogram[b] += 1,
            None => s.no_report += 1,
        }
        let ts = trajectories(&run);
        s.run_summaries.push(RunSummary {
            id: run.id.clone(),
            dir: run.dir.clone(),
            domain: run.domain.clone(),
            request: run.request.clone(),
            agents: run.transcripts.iter().map(|t| t.name.clone()).collect(),
            trajectories: ts.len(),
            resolved: resolved(&run),
            lossy: !ts.is_empty() && ts.iter().all(|t| t.lossy),
            reward: rw,
        });
        for t in ts {
            s.trajectories += 1;
            s.resolved += t.resolved as usize;
            s.lossy += t.lossy as usize;
            s.full += (!t.lossy) as usize;
            let e = s.per_domain.entry(t.domain.clone()).or_default();
            e[0] += 1;
            e[1] += t.resolved as usize;
            if resolved_only && !t.resolved {
                continue;
            }
            lines.push_str(&serde_json::to_string(&t)?);
            lines.push('\n');
            s.written += 1;
        }
    }
    std::fs::write(&path, lines).with_context(|| format!("writing {}", path.display()))?;
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&s)?)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::{Evidence, Outcome, Source};

    fn outcome(name: &str, value: f64) -> Outcome {
        Outcome { name: name.into(), evidence: Evidence::Margin { value, unit: "mm".into() }, detail: String::new(), source: Source::Physics }
    }

    fn missing(name: &str) -> Outcome {
        Outcome { name: name.into(), evidence: Evidence::Missing { why: "never evaluated".into() }, detail: String::new(), source: Source::Physics }
    }

    /// A run directory: events, state (with `report`), one transcript.
    fn run_dir(tag: &str, outcomes: Vec<Outcome>, harness_passed: bool, with_transcript: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wright-train-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = "toy-1";
        let events = vec![
            Event::RunStarted { run: run.into(), domain: "toy".into(), request: "a span".into() },
            Event::Spend { run: run.into(), input_tokens: 10, output_tokens: 5, usd: 0.01, budget_usd: 1.0 },
            Event::AgentMessage { run: run.into(), agent: "orchestrator".into(), text: "Designing.".into() },
            Event::ToolCall { run: run.into(), agent: "orchestrator".into(), tool: "design".into(), input: json!({}) },
            Event::Checks { run: run.into(), passed: 1, failed: 1, outcomes: vec![outcome("span", -10.0), outcome("budget", 40.0)] },
            Event::ToolResult { run: run.into(), agent: "orchestrator".into(), tool: "design".into(), ok: false, summary: "1 failing".into() },
            Event::AgentMessage { run: run.into(), agent: "harness".into(), text: "not a model turn".into() },
            Event::ToolCall { run: run.into(), agent: "orchestrator".into(), tool: "edit_design".into(), input: json!({"patch_json": "{\"span_mm\": 110}"}) },
            Event::ToolResult { run: run.into(), agent: "orchestrator".into(), tool: "edit_design".into(), ok: true, summary: "ok".into() },
            Event::Checks { run: run.into(), passed: 2, failed: 0, outcomes: outcomes.clone() },
            Event::Spend { run: run.into(), input_tokens: 20, output_tokens: 9, usd: 0.03, budget_usd: 1.0 },
            Event::RunFinished { run: run.into(), passed: harness_passed, objective: 110.0, objective_label: "span mm".into() },
        ];
        let text: String = events.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();
        std::fs::write(dir.join("events.jsonl"), text).unwrap();
        let report = Finished { outcomes };
        std::fs::write(dir.join("state.json"), json!({"domain": "toy", "report": report, "design": {"span_mm": 110}}).to_string()).unwrap();
        if with_transcript {
            let t = dir.join("transcripts");
            std::fs::create_dir_all(&t).unwrap();
            let conv = json!({
                "agent": "orchestrator",
                "model": {"id": "claude-sonnet-5-5", "effort": "medium", "max_tokens": 4000},
                "system": "You lead the design.",
                "tools": [{"name": "design", "input_schema": {"type": "object"}}],
                "messages": [
                    {"role": "user", "content": [{"type": "text", "text": "A person asked: a span"}]},
                    {"role": "assistant", "content": [{"type": "text", "text": "Designing."}, {"type": "tool_use", "id": "o1", "name": "design", "input": {}}]},
                    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "o1", "content": "x".repeat(2000), "is_error": true}]},
                    {"role": "assistant", "content": [{"type": "tool_use", "id": "o2", "name": "submit", "input": {"summary": "done"}}]}
                ],
                "pending": [{"type": "tool_result", "tool_use_id": "o2", "content": "submitted"}]
            });
            std::fs::write(t.join("orchestrator.json"), conv.to_string()).unwrap();
        }
        dir
    }

    #[test]
    fn a_passing_report_and_a_passing_harness_make_a_resolved_trajectory() {
        let dir = run_dir("resolved", vec![outcome("span", 10.0), outcome("budget", 40.0)], true, true);
        let run = read_run(&dir).unwrap();
        let ts = trajectories(&run);
        assert_eq!(ts.len(), 1);
        let t = &ts[0];
        assert!(t.resolved && !t.lossy);
        assert_eq!(t.traj_id, "toy-1.orchestrator");
        assert_eq!(t.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(t.reward, Reward { passed: true, checks_passed: 2, checks_total: 2, harness_passed: Some(true), objective: Some(110.0), objective_label: Some("span mm".into()), spend_usd: 0.03, check_history: vec![[1, 1], [2, 0]] });
        // the system prompt leads, the recorded messages follow untouched
        // (the 2000-character tool result survives; events.jsonl would have
        // cut it to 400), the owed result stays out of the history
        assert_eq!(t.messages[0]["role"], "system");
        assert_eq!(t.messages.len(), 5);
        assert_eq!(t.messages[3]["content"][0]["content"].as_str().unwrap().len(), 2000);
        assert_eq!(t.messages.last().unwrap()["content"][0]["name"], "submit");
        assert_eq!(t.tools.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_check_is_not_resolved_however_the_harness_voted() {
        let dir = run_dir("missing", vec![outcome("span", 10.0), missing("budget")], true, true);
        let run = read_run(&dir).unwrap();
        let t = &trajectories(&run)[0];
        assert!(!t.resolved);
        assert_eq!((t.reward.passed, t.reward.checks_passed, t.reward.checks_total), (false, 1, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_negative_margin_is_not_resolved_and_neither_is_an_empty_report() {
        let dir = run_dir("negative", vec![outcome("span", -0.5), outcome("budget", 40.0)], true, true);
        assert!(!resolved(&read_run(&dir).unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = run_dir("empty", vec![], true, true);
        let run = read_run(&dir).unwrap();
        assert!(!resolved(&run));
        assert_eq!(reward(&run).checks_total, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_passing_report_the_harness_did_not_accept_is_not_resolved() {
        let dir = run_dir("harness-no", vec![outcome("span", 10.0), outcome("budget", 40.0)], false, true);
        let run = read_run(&dir).unwrap();
        assert!(run.report.as_ref().unwrap().passed());
        assert!(!resolved(&run));
        assert_eq!(reward(&run).harness_passed, Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_transcripts_the_events_give_a_lossy_trajectory() {
        let dir = run_dir("lossy", vec![outcome("span", 10.0), outcome("budget", 40.0)], true, false);
        let run = read_run(&dir).unwrap();
        let ts = trajectories(&run);
        assert_eq!(ts.len(), 1, "the harness's own messages are not an agent");
        let t = &ts[0];
        assert!(t.lossy && t.resolved);
        assert_eq!(t.agent, "orchestrator");
        // placeholder user turn, assistant (text + design), user (result),
        // assistant (edit_design), user (result)
        let roles: Vec<&str> = t.messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);
        assert_eq!(t.messages[1]["content"][1]["name"], "design");
        assert_eq!(t.messages[2]["content"][0]["tool_use_id"], t.messages[1]["content"][1]["id"]);
        assert_eq!(t.messages[2]["content"][0]["is_error"], true);
        assert_eq!(t.messages[4]["content"][0]["tool_use_id"], t.messages[3]["content"][0]["id"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_writes_jsonl_tags_and_filters() {
        let a = run_dir("export-a", vec![outcome("span", 10.0), outcome("budget", 40.0)], true, true);
        let b = run_dir("export-b", vec![outcome("span", -1.0), outcome("budget", 40.0)], true, true);
        let out = std::env::temp_dir().join(format!("wright-train-{}-export-out", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let missing_dir = std::env::temp_dir().join("wright-train-no-such-run");
        let s = export(&[a.clone(), b.clone(), missing_dir], &out, false).unwrap();
        assert_eq!((s.runs, s.trajectories, s.resolved, s.written, s.skipped.len()), (2, 2, 1, 2, 1));
        let text = std::fs::read_to_string(out.join("trajectories.jsonl")).unwrap();
        let rows: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().filter(|r| r["resolved"] == true).count(), 1);
        assert!(rows.iter().all(|r| r["messages"][0]["role"] == "system" && r["instance_id"] == "toy-1"));
        // per-run rows and the reward histogram: a 2/2 run in the last bin,
        // a 1/2 run in the 0.5 bin, the missing directory in neither
        assert_eq!(s.run_summaries.len(), 2);
        let ra = &s.run_summaries[0];
        assert_eq!((ra.id.as_str(), ra.domain.as_str(), ra.trajectories, ra.resolved, ra.lossy), ("toy-1", "toy", 1, true, false));
        assert_eq!(ra.agents, vec!["orchestrator".to_string()]);
        assert_eq!((ra.reward.checks_passed, ra.reward.checks_total), (2, 2));
        assert!(!s.run_summaries[1].resolved);
        let mut hist = [0; REWARD_BINS];
        hist[REWARD_BINS - 1] = 1;
        hist[5] = 1;
        assert_eq!(s.reward_histogram, hist);
        assert_eq!((s.no_report, s.full, s.lossy), (0, 2, 0));
        let text = s.to_string();
        assert!(text.contains("toy-1 [toy]: 1 trajectories (orchestrator), checks 2/2, resolved"), "{text}");
        assert!(text.contains("reward histogram"), "{text}");
        let written: Value = serde_json::from_str(&std::fs::read_to_string(out.join("summary.json")).unwrap()).unwrap();
        assert_eq!(written["run_summaries"].as_array().unwrap().len(), 2);
        assert_eq!(written["reward_histogram"][10], 1);
        let s = export(&[a.clone(), b.clone()], &out, true).unwrap();
        assert_eq!((s.trajectories, s.written), (2, 1));
        assert_eq!(std::fs::read_to_string(out.join("trajectories.jsonl")).unwrap().lines().count(), 1);
        assert!(out.join("summary.json").exists());
        for d in [a, b, out] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
