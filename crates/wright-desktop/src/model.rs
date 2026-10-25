//! What the app shows about a run, folded from its `wright_core::Event`s.
//! One fold for both modes: a live run applies events as the harness sends
//! them; a replay rebuilds the fold up to the timeline cursor.

use std::path::{Path, PathBuf};
use wright_core::{Event, Headline, Outcome};

#[derive(Debug, Clone, PartialEq)]
pub enum LogKind {
    Message,
    Call,
    Result { ok: bool },
    Feedback,
    Notice,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub agent: String,
    pub kind: LogKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Spend {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usd: f64,
    pub budget_usd: f64,
}

#[derive(Debug, Clone)]
pub struct Checks {
    pub passed: usize,
    pub failed: usize,
    pub outcomes: Vec<Outcome>,
}

#[derive(Debug, Clone)]
pub struct Finished {
    pub passed: bool,
    pub objective: f64,
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct RunState {
    pub run: Option<String>,
    pub domain: String,
    pub request: String,
    pub log: Vec<LogLine>,
    pub checks: Option<Checks>,
    /// How many check reports so far (one per design iteration).
    pub check_reports: usize,
    pub headlines: Vec<Headline>,
    /// The orchestrator's engineering plan: its goal and the latest tasks.
    pub plan_goal: String,
    pub plan: Vec<wright_core::PlanTask>,
    pub spend: Option<Spend>,
    /// (label, recorded path), in order, deduplicated by path.
    pub artifacts: Vec<(String, String)>,
    /// (view, recorded path)
    pub renders: Vec<(String, String)>,
    /// (view, sketch path, note)
    pub feedback: Vec<(String, String, String)>,
    pub finished: Option<Finished>,
}

fn short(v: &serde_json::Value, n: usize) -> String {
    let s = v.to_string();
    if s.chars().count() > n {
        format!("{}...", s.chars().take(n).collect::<String>())
    } else {
        s
    }
}

impl RunState {
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a Event>) -> Self {
        let mut s = RunState::default();
        for e in events {
            s.apply(e);
        }
        s
    }

    pub fn notice(&mut self, text: impl Into<String>) {
        self.log.push(LogLine { agent: "app".into(), kind: LogKind::Notice, text: text.into() });
    }

    pub fn apply(&mut self, ev: &Event) {
        match ev {
            Event::RunStarted { run, domain, request } => {
                self.run = Some(run.clone());
                self.domain = domain.clone();
                self.request = request.clone();
            }
            Event::AgentMessage { agent, text, .. } => {
                self.log.push(LogLine { agent: agent.clone(), kind: LogKind::Message, text: text.clone() })
            }
            Event::ToolCall { agent, tool, input, .. } => self.log.push(LogLine {
                agent: agent.clone(),
                kind: LogKind::Call,
                text: format!("{tool} {}", short(input, 220)),
            }),
            Event::ToolResult { agent, tool, ok, summary, .. } => self.log.push(LogLine {
                agent: agent.clone(),
                kind: LogKind::Result { ok: *ok },
                text: format!("{tool}: {summary}"),
            }),
            Event::Checks { passed, failed, outcomes, .. } => {
                self.check_reports += 1;
                self.checks = Some(Checks { passed: *passed, failed: *failed, outcomes: outcomes.clone() });
            }
            Event::Headlines { items, .. } => self.headlines = items.clone(),
            Event::Render { view, path, .. } => {
                self.renders.retain(|(v, _)| v != view);
                self.renders.push((view.clone(), path.clone()));
            }
            Event::Feedback { view, sketch_png, note, .. } => {
                self.feedback.push((view.clone(), sketch_png.clone(), note.clone()));
                self.log.push(LogLine { agent: "person".into(), kind: LogKind::Feedback, text: format!("on {view}: {note}") });
            }
            Event::Spend { input_tokens, output_tokens, usd, budget_usd, .. } => {
                self.spend = Some(Spend { input_tokens: *input_tokens, output_tokens: *output_tokens, usd: *usd, budget_usd: *budget_usd })
            }
            Event::Artifact { label, path, .. } => {
                if !self.artifacts.iter().any(|(_, p)| p == path) {
                    self.artifacts.push((label.clone(), path.clone()));
                }
            }
            Event::RunFinished { passed, objective, objective_label, .. } => {
                self.finished = Some(Finished { passed: *passed, objective: *objective, label: objective_label.clone() })
            }
            Event::Plan { goal, tasks, .. } => {
                self.plan_goal = goal.clone();
                self.plan = tasks.clone();
            }
            Event::Control { action, state, note, .. } => self.log.push(LogLine {
                agent: "person".into(),
                kind: LogKind::Notice,
                text: if note.is_empty() { format!("{action} -> {state}") } else { format!("{action} -> {state}: {note}") },
            }),
        }
    }

    /// The BOM CSV the run wrote, if any.
    pub fn bom_path(&self, dir: &Path) -> Option<PathBuf> {
        self.artifacts
            .iter()
            .filter(|(_, p)| p.ends_with("bom.csv") && !p.contains("guide/"))
            .find_map(|(_, p)| locate(dir, p))
            .or_else(|| Some(dir.join("bom.csv")).filter(|p| p.is_file()))
    }
}

/// A path an event recorded (relative to the recording's working
/// directory, or to the run dir, or absolute), found on this machine: as
/// given, else under `dir` with the recorded path's leading components
/// dropped one at a time. Same rule as `wright report` (wright-cli).
pub fn locate(dir: &Path, recorded: &str) -> Option<PathBuf> {
    let p = Path::new(recorded);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    if p.exists() {
        return Some(p.to_path_buf());
    }
    let comps: Vec<_> = p.components().collect();
    (0..comps.len()).map(|k| dir.join(comps[k..].iter().collect::<PathBuf>())).find(|c| c.exists())
}

/// Does `ev` belong to a run? (All events carry one.)
pub fn run_of(ev: &Event) -> &str {
    match ev {
        Event::RunStarted { run, .. }
        | Event::AgentMessage { run, .. }
        | Event::ToolCall { run, .. }
        | Event::ToolResult { run, .. }
        | Event::Checks { run, .. }
        | Event::Headlines { run, .. }
        | Event::Render { run, .. }
        | Event::Feedback { run, .. }
        | Event::Spend { run, .. }
        | Event::Artifact { run, .. }
        | Event::Plan { run, .. }
        | Event::RunFinished { run, .. }
        | Event::Control { run, .. } => run,
    }
}

/// Short name of an event for the timeline.
pub fn event_label(ev: &Event) -> String {
    match ev {
        Event::RunStarted { domain, .. } => format!("run started ({domain})"),
        Event::AgentMessage { agent, .. } => format!("{agent} says"),
        Event::ToolCall { agent, tool, .. } => format!("{agent} -> {tool}"),
        Event::ToolResult { tool, ok, .. } => format!("{tool} {}", if *ok { "ok" } else { "error" }),
        Event::Checks { passed, failed, .. } => format!("checks {passed}/{}", passed + failed),
        Event::Headlines { .. } => "headlines".into(),
        Event::Render { view, .. } => format!("render {view}"),
        Event::Feedback { view, .. } => format!("person's markup on {view}"),
        Event::Spend { usd, .. } => format!("spend ${usd:.2}"),
        Event::Artifact { label, .. } => format!("file: {label}"),
        Event::Plan { tasks, .. } => format!("plan {}/{} done", tasks.iter().filter(|t| t.status == wright_core::PlanStatus::Done).count(), tasks.len()),
        Event::RunFinished { passed, .. } => format!("finished: {}", if *passed { "PASS" } else { "not passing" }),
        Event::Control { action, state, .. } => format!("control: {action} -> {state}"),
    }
}
