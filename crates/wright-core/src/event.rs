//! What a design run says while it runs: the harness (`wright-agents`)
//! emits these, the desktop app (`wright-desktop`) shows them live and replays them
//! and turns a person's sketch on a render back into [`Event::Feedback`].
//! One vocabulary, so the page and the agents cannot disagree.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted { run: String, domain: String, request: String },
    AgentMessage { run: String, agent: String, text: String },
    ToolCall { run: String, agent: String, tool: String, input: serde_json::Value },
    ToolResult { run: String, agent: String, tool: String, ok: bool, summary: String },
    /// A complete check report for the current design.
    Checks { run: String, passed: usize, failed: usize, outcomes: Vec<crate::Outcome> },
    /// The numbers a builder asks for first, each against its requirement
    /// (`Domain::headlines`): structured, so no page has to parse a check's
    /// detail text to show them.
    Headlines { run: String, items: Vec<Headline> },
    /// A rendered view a person can draw on (PNG path or URL).
    Render { run: String, view: String, path: String },
    /// A person's markup on a render: the stroked PNG plus their words.
    Feedback { run: String, view: String, sketch_png: String, note: String },
    Spend { run: String, input_tokens: u64, output_tokens: u64, usd: f64, budget_usd: f64 },
    Artifact { run: String, label: String, path: String },
    RunFinished { run: String, passed: bool, objective: f64, objective_label: String },
    /// The orchestrator's engineering plan, whole, after every change
    /// (`wright_agents::plan`): the desktop draws the latest one.
    Plan { run: String, goal: String, tasks: Vec<PlanTask> },
    /// A person's control over the run (`wright_agents::checkpoint`):
    /// `action` is the command or what the harness did (`pause`, `resume`,
    /// `approve`, `stop`, `waiting`, `timeout`), `state` the gate after it
    /// (`running`, `paused`, `stepping`, `stopped`), `note` their words.
    Control { run: String, action: String, state: String, note: String },
}

/// One task of the engineering plan (shape after MetaGPT `metagpt/schema.py`
/// `Task`: id, dependent task ids, assignee; see `wright_agents::plan`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    /// `orchestrator`, `reviewer`, or a specialist role.
    pub owner: String,
    /// Ids of the tasks whose results this one needs first.
    pub inputs: Vec<String>,
    /// Declared checks this task must turn green.
    pub checks: Vec<String>,
    pub status: PlanStatus,
    /// The latest result, or why it is blocked or stale.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    Running,
    Done,
    Blocked,
    /// Was done; a check it vouched for (or an input) failed since.
    Stale,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PlanStatus::Pending => "pending",
            PlanStatus::Running => "running",
            PlanStatus::Done => "done",
            PlanStatus::Blocked => "blocked",
            PlanStatus::Stale => "stale",
        })
    }
}

/// Which side of `limit` is good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bound {
    AtLeast,
    AtMost,
    /// No limit; shown for information (mass, apogee when unconstrained).
    Info,
}

/// One headline number: "Endurance 49.5 min (need 45)".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Headline {
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub limit: Option<f64>,
    pub bound: Bound,
    /// The check that judges it, if any (its pass/fail decides the colour).
    pub check: Option<String>,
}
