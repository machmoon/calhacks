//! Checkpoints, resume, and a person's pause / approve / stop over a run.
//!
//! **Checkpoint** (`<run>/checkpoint.json`). After every orchestrator step
//! (one model turn: its tool calls answered), after each review, and at the
//! end, the harness writes one [`Checkpoint`]: the shared [`DesignState`]
//! (the incumbent is `state.best`), the engineering plan when a lane wrote
//! one (`<run>/plan.json`), the spend, the orchestrator's conversation, and
//! a one-line summary of every other conversation so far. The shape follows
//! LangGraph's checkpointer (langchain-ai/langgraph
//! `libs/checkpoint/langgraph/checkpoint/base/__init__.py`): a `Checkpoint`
//! with a format version `v`, a monotonically increasing `id`, a timestamp
//! `ts` and the channel values, plus `CheckpointMetadata` with `source`
//! (`input` / `loop` / `update`), `step` and the parent's id. Here the
//! channels are named fields instead of a map, and only the latest
//! checkpoint is kept (LangGraph keeps the whole chain per thread, for
//! time travel; one file is enough to resume, and a run's design state is
//! large). The write is a temp file then `rename`, as `state::move_rejected`
//! already does, so a crash mid-write leaves the last good checkpoint.
//!
//! **Resume** (`wright run --resume RUN_DIR`). [`load`] reads the file,
//! [`RunConfig::resume`] carries it, and `harness::run` starts from it: the
//! state as it was, the orchestrator's history put back with the owed
//! `tool_result`s as `Conversation::pending` (the mechanism agent.rs already
//! uses to carry a conversation across rounds), the round and turn counters
//! where they stopped, the spend booked against the run budget again. This
//! is LangGraph's `Command(resume=...)` on a thread with a checkpointer
//! (`libs/langgraph/langgraph/types.py`, `interrupt` and `Command`): the
//! graph continues from the last checkpoint, not from the start. A finished
//! run has nothing to resume, and says so.
//!
//! **Control** (`<run>/control/` and [`Control`]). Four commands: `pause`,
//! `resume`, `approve`, `stop`. A person drops a file named after the
//! command into the mailbox dir (its content is a note the orchestrator
//! reads), or the desktop calls [`Control::pause`] and friends in-process.
//! The gate is checked before every orchestrator tool call. Paused, the
//! harness holds the call and polls for a command, the way Prefect's
//! `pause_flow_run` holds the flow and polls the API until the run is
//! `RUNNING` again or the timeout passes (PrefectHQ/prefect
//! `src/prefect/flow_runs.py`, `_in_process_pause`: `anyio.move_on_after(
//! timeout)` around a poll loop, then `FlowPauseTimeout`). When the person
//! resumes or approves, the call runs and its result opens with what they
//! did; when the pause times out (`WRIGHT_PAUSE_TIMEOUT_S`, default 3600 s
//! as Prefect's), the orchestrator gets `paused: waiting for approval timed
//! out` as the tool result, the run stops and the checkpoint remains for
//! `--resume` (Prefect's `suspend_flow_run`, `reschedule=True`: the process
//! exits and a later one continues). `stop` is answered the same way and
//! stops the run at once. `approve` arms step mode: every later call waits
//! for its own approve, until `resume`. Every transition is an
//! [`Event::Control`] on the run's channel, and `<run>/control/status.json`
//! says what the harness is waiting for.

use crate::agent::{Conversation, ToolOutput};
use crate::client::Client;
use crate::harness::RunConfig;
use crate::state::DesignState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use wright_core::Event;

/// Checkpoint format version (LangGraph's `Checkpoint.v`).
pub const FORMAT: u32 = 1;
pub const FILE: &str = "checkpoint.json";
pub const CONTROL_DIR: &str = "control";
pub const STATUS_FILE: &str = "status.json";
/// How often a held call looks at the mailbox (Prefect polls every 10 s;
/// a person at a table expects a click to land faster).
pub const POLL: Duration = Duration::from_millis(250);
pub const DEFAULT_PAUSE_TIMEOUT_S: u64 = 3600;

// ------------------------------------------------------------ checkpoint

/// Where the run was when the checkpoint was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// Mid-round: the orchestrator has made `turns_used` model calls this
    /// round and owes the model the `pending` tool results.
    Orchestrating { turns_used: u32 },
    /// The round's review is done; the next round starts fresh.
    Reviewed,
    /// The run ended; nothing to resume.
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spend {
    pub usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A conversation that can be put back: the orchestrator's. `system` and
/// `tools` are rebuilt from the prompts on resume (a prompt fix applies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedConversation {
    pub agent: String,
    pub model: String,
    pub messages: Vec<Value>,
    pub pending: Vec<Value>,
}

impl SavedConversation {
    pub fn of(conv: &Conversation, owed: &[Value]) -> Self {
        SavedConversation { agent: conv.agent.clone(), model: conv.model.id.clone(), messages: conv.messages.clone(), pending: owed.to_vec() }
    }
    /// Put the history back into a fresh conversation of the same role.
    pub fn restore(&self, conv: &mut Conversation) {
        conv.messages = self.messages.clone();
        conv.pending = self.pending.clone();
    }
}

/// One line per conversation the run has had (specialists, reviewers):
/// what a person reads to know what happened before the checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub agent: String,
    pub round: u32,
    pub turns: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub v: u32,
    /// Zero-padded step then the time: unique and sortable, as LangGraph's.
    pub id: String,
    pub parent_id: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    /// `input` (run start), `step` (an orchestrator turn), `review`,
    /// `control` (a pause / stop), `finish`.
    pub source: String,
    pub step: u64,
    pub run_id: String,
    pub domain: String,
    pub request: String,
    pub round: u32,
    #[serde(flatten)]
    pub phase: Phase,
    pub control: Gate,
    pub spend: Spend,
    pub state: DesignState,
    /// `<run>/plan.json` when the planning lane wrote one; opaque here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Value>,
    pub orchestrator: Option<SavedConversation>,
    #[serde(default)]
    pub conversations: Vec<ConversationSummary>,
    /// The transport's persistent sessions (`claude` CLI session ids per
    /// conversation prefix, `crate::cli`): a resumed run continues the
    /// orchestrator's CLI session instead of re-sending its transcript.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub transport_sessions: Value,
}

impl Checkpoint {
    /// The round `harness::run` continues from.
    pub fn resume_round(&self) -> u32 {
        match self.phase {
            Phase::Orchestrating { .. } => self.round.max(1),
            Phase::Reviewed => self.round + 1,
            Phase::Finished => self.round,
        }
    }
    /// Mid-round: how many orchestrator turns this round already used.
    pub fn turns_used(&self) -> u32 {
        match self.phase {
            Phase::Orchestrating { turns_used } => turns_used,
            _ => 0,
        }
    }
    /// What the orchestrator is told on resume.
    pub fn resume_note(&self) -> String {
        format!(
            "This run was resumed from a checkpoint (step {}, round {}) after it stopped (a crash, a budget stop, or a person stopping it). Your conversation above is intact and the tool results you were owed are included; the design state is as you left it. Continue where you left off.",
            self.step, self.round
        )
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Write `value` to `path` through a temp file and a rename, so a reader
/// (or the next `--resume`) never sees a half-written file.
pub fn write_atomic(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

/// The checkpoint in `run_dir`, if there is one.
pub fn load(run_dir: &Path) -> anyhow::Result<Checkpoint> {
    let p = run_dir.join(FILE);
    let text = std::fs::read_to_string(&p).map_err(|e| anyhow::anyhow!("reading {}: {e}", p.display()))?;
    let c: Checkpoint = serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {}: {e}", p.display()))?;
    if c.v != FORMAT {
        anyhow::bail!("{} is checkpoint format v{}, this build reads v{FORMAT}", p.display(), c.v);
    }
    Ok(c)
}

/// Writes the run's checkpoints; one per harness `run`.
pub struct Checkpointer {
    dir: Option<PathBuf>,
    step: u64,
    last_id: Option<String>,
    pub conversations: Vec<ConversationSummary>,
}

impl Checkpointer {
    /// Continues the step count of a checkpoint being resumed.
    pub fn new(cfg: &RunConfig) -> Self {
        let (step, last_id, conversations) = match &cfg.resume {
            Some(c) => (c.step, Some(c.id.clone()), c.conversations.clone()),
            None => (0, None, vec![]),
        };
        Checkpointer { dir: cfg.out_dir.clone(), step, last_id, conversations }
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(FILE))
    }

    /// Record a finished specialist / reviewer conversation for the summary list.
    pub fn note_conversation(&mut self, conv: &Conversation, round: u32, summary: &str) {
        self.conversations.push(ConversationSummary {
            agent: conv.agent.clone(),
            round,
            turns: conv.messages.iter().filter(|m| m["role"] == "assistant").count(),
            summary: summary.chars().take(300).collect(),
        });
    }

    /// Write the next checkpoint. `None` when the run has no out dir;
    /// an I/O failure is returned, never fatal to the run.
    #[allow(clippy::too_many_arguments)]
    pub fn save(
        &mut self,
        source: &str,
        cfg: &RunConfig,
        client: &Client,
        state: &DesignState,
        round: u32,
        phase: Phase,
        gate: Gate,
        orchestrator: Option<SavedConversation>,
    ) -> Option<std::io::Result<PathBuf>> {
        let dir = self.dir.clone()?;
        self.step += 1;
        let ts_ms = now_ms();
        let id = format!("{:06}-{ts_ms}", self.step);
        let spend = {
            let b = client.budget.lock().unwrap();
            Spend { usd: b.run_spent, input_tokens: b.run_input_tokens, output_tokens: b.run_output_tokens }
        };
        let plan = std::fs::read_to_string(dir.join("plan.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
        let ck = Checkpoint {
            v: FORMAT,
            id: id.clone(),
            parent_id: self.last_id.take(),
            ts_ms,
            source: source.into(),
            step: self.step,
            run_id: cfg.run_id.clone(),
            domain: state.domain.clone(),
            request: cfg.request.clone(),
            round,
            phase,
            control: gate,
            spend,
            state: state.clone(),
            plan,
            orchestrator,
            conversations: self.conversations.clone(),
            transport_sessions: client.transport.export_sessions(),
        };
        self.last_id = Some(id);
        let path = dir.join(FILE);
        Some(write_atomic(&path, &ck).map(|()| path))
    }
}

/// Book a resumed checkpoint's spend against the run budget, so the run
/// limit holds across the crash as if it had been one process.
pub fn restore_spend(client: &Client, ck: &Checkpoint) {
    let mut b = client.budget.lock().unwrap();
    b.run_spent += ck.spend.usd;
    b.run_input_tokens += ck.spend.input_tokens;
    b.run_output_tokens += ck.spend.output_tokens;
}

/// `wright run --resume RUN_DIR [flags]`: the positional domain and request
/// come from the checkpoint, `-o` is the run dir. Returns the argument list
/// `wright run` otherwise expects, and the checkpoint. Without `--resume`
/// the arguments come back as they were.
pub fn resume_args(args: &[String]) -> anyhow::Result<(Vec<String>, Option<Box<Checkpoint>>)> {
    let Some(i) = args.iter().position(|a| a == "--resume") else { return Ok((args.to_vec(), None)) };
    let Some(dir) = args.get(i + 1) else { anyhow::bail!("--resume needs the run directory") };
    let ck = load(Path::new(dir))?;
    if ck.phase == Phase::Finished {
        anyhow::bail!("{dir}/{FILE}: the run finished (step {}); nothing to resume", ck.step);
    }
    let mut out = vec![ck.domain.clone(), ck.request.clone()];
    out.extend(args.iter().enumerate().filter(|(k, _)| *k != i && *k != i + 1).map(|(_, a)| a.clone()));
    if !out.iter().any(|a| a == "-o") {
        out.push("-o".into());
        out.push(dir.clone());
    }
    Ok((out, Some(Box::new(ck))))
}

// --------------------------------------------------------------- control

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Pause,
    Resume,
    /// Let the next orchestrator step run; from `Running`, arm step mode.
    Approve,
    Stop,
}

impl Command {
    pub fn parse(s: &str) -> Option<Command> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pause" => Some(Command::Pause),
            "resume" | "continue" | "unpause" => Some(Command::Resume),
            "approve" | "step" | "next" => Some(Command::Approve),
            "stop" | "cancel" | "abort" => Some(Command::Stop),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Command::Pause => "pause",
            Command::Resume => "resume",
            Command::Approve => "approve",
            Command::Stop => "stop",
        }
    }
}

/// What the gate does to the next orchestrator step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    #[default]
    Running,
    /// Every call waits until `resume` (or one `approve` per call).
    Paused,
    /// Step mode: every call waits for its own `approve`.
    Stepping,
    Stopped,
}

impl Gate {
    pub fn name(self) -> &'static str {
        match self {
            Gate::Running => "running",
            Gate::Paused => "paused",
            Gate::Stepping => "stepping",
            Gate::Stopped => "stopped",
        }
    }
}

#[derive(Default)]
struct Inner {
    queue: Mutex<VecDeque<(Command, String)>>,
    notify: tokio::sync::Notify,
    gate: Mutex<Gate>,
    timeout: Mutex<Option<Duration>>,
}

/// The in-process control channel the desktop holds: cheap to clone, one
/// per run (`RunConfig::control`). Commands queue until the harness looks,
/// which it does before every orchestrator tool call and every [`POLL`]
/// while holding one.
#[derive(Clone, Default)]
pub struct Control(Arc<Inner>);

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Control({:?}, {} queued)", self.gate(), self.0.queue.lock().unwrap().len())
    }
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }
    /// Wait at most this long for a person while paused (tests; the
    /// default is `WRIGHT_PAUSE_TIMEOUT_S`, else an hour).
    pub fn with_timeout(self, t: Duration) -> Self {
        *self.0.timeout.lock().unwrap() = Some(t);
        self
    }
    pub fn send(&self, cmd: Command, note: impl Into<String>) {
        self.0.queue.lock().unwrap().push_back((cmd, note.into()));
        self.0.notify.notify_one();
    }
    pub fn pause(&self) {
        self.send(Command::Pause, "")
    }
    pub fn resume(&self) {
        self.send(Command::Resume, "")
    }
    pub fn approve(&self) {
        self.send(Command::Approve, "")
    }
    pub fn stop(&self) {
        self.send(Command::Stop, "")
    }
    /// The gate as the harness last set it (what a status line shows).
    pub fn gate(&self) -> Gate {
        *self.0.gate.lock().unwrap()
    }
    fn drain(&self) -> Vec<(Command, String)> {
        self.0.queue.lock().unwrap().drain(..).collect()
    }
}

/// The file mailbox: `<run>/control/<command>[-anything][.ext]`, content =
/// a note for the orchestrator. Files are consumed (deleted) when read,
/// oldest first.
pub struct Mailbox {
    pub dir: PathBuf,
}

impl Mailbox {
    pub fn new(run_dir: &Path) -> Self {
        let dir = run_dir.join(CONTROL_DIR);
        let _ = std::fs::create_dir_all(&dir);
        Mailbox { dir }
    }
    pub fn drain(&self) -> Vec<(Command, String)> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return vec![] };
        let mut found: Vec<(SystemTime, String, PathBuf, Command)> = vec![];
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name == STATUS_FILE || !p.is_file() {
                continue;
            }
            let word = name.split(['-', '.', '_', ' ']).next().unwrap_or("");
            if let Some(cmd) = Command::parse(word) {
                let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                found.push((t, name, p, cmd));
            }
        }
        found.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        found
            .into_iter()
            .map(|(_, _, p, cmd)| {
                let note = std::fs::read_to_string(&p).unwrap_or_default().trim().to_string();
                let _ = std::fs::remove_file(&p);
                (cmd, note)
            })
            .collect()
    }
    /// `status.json`: the gate and whether a call is being held, for a
    /// person (or the desktop) looking at the directory.
    pub fn write_status(&self, gate: Gate, waiting_on: Option<&str>) {
        let v = json!({
            "state": gate.name(),
            "waiting_on": waiting_on,
            "since_ms": now_ms(),
            "commands": ["pause", "resume", "approve", "stop"],
            "how": "create a file named after the command in this directory; its content is a note the orchestrator reads",
        });
        let _ = write_atomic(&self.dir.join(STATUS_FILE), &v);
    }
}

/// The harness's side of the gate: polls the channel and the mailbox,
/// applies commands, holds a tool call while paused, announces every
/// transition.
pub struct Keeper {
    pub control: Control,
    pub mailbox: Option<Mailbox>,
    gate: Gate,
    events: broadcast::Sender<Event>,
    run: String,
    timeout: Duration,
    /// Set when a stop or a timed-out pause ended the orchestrator's turn.
    pub stopped: Option<String>,
}

impl Keeper {
    pub fn new(cfg: &RunConfig, client: &Client) -> Self {
        let control = cfg.control.clone().unwrap_or_default();
        let timeout = control.0.timeout.lock().unwrap().unwrap_or_else(|| {
            let s = std::env::var("WRIGHT_PAUSE_TIMEOUT_S").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_PAUSE_TIMEOUT_S);
            Duration::from_secs(s)
        });
        let mailbox = cfg.out_dir.as_deref().map(Mailbox::new);
        // a resumed run comes back running: the person asked for it
        let gate = Gate::Running;
        *control.0.gate.lock().unwrap() = gate;
        if let Some(m) = &mailbox {
            m.write_status(gate, None);
        }
        Keeper { control, mailbox, gate, events: client.events.clone(), run: cfg.run_id.clone(), timeout, stopped: None }
    }

    pub fn gate(&self) -> Gate {
        self.gate
    }

    fn emit(&self, action: &str, note: &str) {
        let _ = self.events.send(Event::Control { run: self.run.clone(), action: action.into(), state: self.gate.name().into(), note: note.into() });
    }

    fn set(&mut self, gate: Gate, action: &str, note: &str) {
        self.gate = gate;
        *self.control.0.gate.lock().unwrap() = gate;
        self.emit(action, note);
    }

    /// Apply every queued command. Returns the notes of the commands that
    /// let a held call through (`resume`, `approve`), if any.
    fn apply_pending(&mut self, holding: bool) -> Option<String> {
        let mut cmds = self.control.drain();
        if let Some(m) = &self.mailbox {
            cmds.extend(m.drain());
        }
        let mut released = None;
        for (cmd, note) in cmds {
            match cmd {
                Command::Pause => self.set(Gate::Paused, "pause", &note),
                Command::Resume => {
                    self.set(Gate::Running, "resume", &note);
                    if holding {
                        released = Some(format!("resumed by the person{}", suffix(&note)));
                    }
                }
                Command::Approve => {
                    if holding {
                        self.set(Gate::Stepping, "approve", &note);
                        released = Some(format!("this step was approved by the person{}", suffix(&note)));
                    } else {
                        // from running: arm step mode (the next call waits)
                        self.set(Gate::Stepping, "approve", &note);
                    }
                }
                Command::Stop => {
                    self.set(Gate::Stopped, "stop", &note);
                    self.stopped = Some(format!("stopped by the person{}", suffix(&note)));
                }
            }
        }
        released
    }

    /// Before an orchestrator tool call. `Ok(None)`: run it. `Ok(Some(n))`:
    /// run it, and open its result with `n` (what the person did).
    /// `Err(out)`: answer with `out` instead; it stops the orchestrator.
    pub async fn before_tool(&mut self, tool: &str) -> Result<Option<String>, ToolOutput> {
        let released = self.apply_pending(false);
        if self.gate == Gate::Stopped {
            return Err(self.stopped_output());
        }
        if self.gate == Gate::Running {
            return Ok(released.filter(|_| false));
        }
        // held: say so, then poll until a person acts or the clock runs out
        self.emit("waiting", &format!("holding `{tool}` for approval"));
        if let Some(m) = &self.mailbox {
            m.write_status(self.gate, Some(tool));
        }
        let started = Instant::now();
        loop {
            if let Some(note) = self.apply_pending(true) {
                if let Some(m) = &self.mailbox {
                    m.write_status(self.gate, None);
                }
                return Ok(Some(note));
            }
            if self.gate == Gate::Stopped {
                if let Some(m) = &self.mailbox {
                    m.write_status(self.gate, None);
                }
                return Err(self.stopped_output());
            }
            let left = self.timeout.checked_sub(started.elapsed()).unwrap_or_default();
            if left.is_zero() {
                self.stopped = Some(format!("paused and nobody resumed it within {:.1} s", self.timeout.as_secs_f64()));
                self.emit("timeout", &format!("`{tool}` was never approved; stopping, checkpoint kept"));
                if let Some(m) = &self.mailbox {
                    m.write_status(self.gate, None);
                }
                return Err(ToolOutput::err(format!(
                    "paused: waiting for approval timed out after {:.1} s. This call was not executed. The run stops here and is checkpointed; `wright run --resume RUN_DIR` continues it.",
                    self.timeout.as_secs_f64()
                ))
                .stop());
            }
            tokio::select! {
                _ = self.control.0.notify.notified() => {}
                _ = tokio::time::sleep(POLL.min(left)) => {}
            }
        }
    }

    fn stopped_output(&self) -> ToolOutput {
        ToolOutput::err(format!(
            "paused: waiting for approval -- {}. This call was not executed; the run stops here and is checkpointed (`wright run --resume RUN_DIR` continues it).",
            self.stopped.as_deref().unwrap_or("stopped by the person")
        ))
        .stop()
    }

    /// Mark the end of the run in the status file.
    pub fn finish(&mut self) {
        if let Some(m) = &self.mailbox {
            m.write_status(self.gate, None);
        }
    }
}

fn suffix(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!(": {note}")
    }
}

/// Prefix a tool result with what the person did at the gate.
pub fn with_note(mut out: ToolOutput, note: Option<String>) -> ToolOutput {
    if let Some(n) = note {
        out.content = format!("[{n}]\n{}", out.content);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ModelCfg;
    use crate::fake::{reply, text, tool_use, ScriptedTransport};
    use crate::harness::{run, StopReason};
    use wright_core::{Artifacts, Bom, CheckId, Domain, Finished, Report};

    struct Fixture;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum C {
        Valid,
    }
    impl CheckId for C {
        const ALL: &'static [Self] = &[C::Valid];
        fn name(self) -> &'static str {
            "fixture"
        }
        fn describe(self) -> &'static str {
            "Checkpoint fixture"
        }
    }
    impl Domain for Fixture {
        const NAME: &'static str = "rocket";
        type Spec = Value;
        type Design = Value;
        type Check = C;
        fn spec_schema() -> Value {
            json!({"type": "object"})
        }
        fn design(_: &Value) -> anyhow::Result<Value> {
            Ok(json!({"fin_count": 3}))
        }
        fn check(_: &Value, _: &Value) -> Finished {
            let mut r = Report::<C>::new();
            r.margin(C::Valid, 1.0, "", wright_core::Source::Requirement, "fixture passes");
            r.finish()
        }
        fn bom(_: &Value) -> Bom {
            Bom::default()
        }
        fn build(_: &Value, _: &Path) -> anyhow::Result<Artifacts> {
            Ok(Artifacts::default())
        }
        fn objective(_: &Value) -> (f64, &'static str) {
            (1.0, "fixture")
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-ckpt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn cfg(dir: &Path, run_id: &str) -> RunConfig {
        let mut c = RunConfig::new(run_id, "a small rocket", json!({}));
        c.out_dir = Some(dir.to_path_buf());
        c.max_rounds = 2;
        c.reviewer_turns = 2;
        c
    }

    fn client(t: Arc<ScriptedTransport>, run: &str) -> (Client, broadcast::Receiver<Event>) {
        let (tx, rx) = crate::channel();
        (Client::new(t, crate::Budget::new(5.0, 45.0, None), tx, run), rx)
    }

    fn events(rx: &mut broadcast::Receiver<Event>) -> Vec<Event> {
        let mut out = vec![];
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn controls(evs: &[Event]) -> Vec<(String, String)> {
        evs.iter()
            .filter_map(|ev| match ev {
                Event::Control { action, state, .. } => Some((action.clone(), state.clone())),
                _ => None,
            })
            .collect()
    }

    /// Every agent's tool calls and messages, for a failure message.
    fn trail(evs: &[Event]) -> String {
        let mut out = vec![];
        for ev in evs.iter().cloned() {
            match ev {
                Event::ToolCall { agent, tool, .. } => out.push(format!("{agent} -> {tool}")),
                Event::ToolResult { agent, tool, ok, summary, .. } => out.push(format!("{agent} <- {tool} ok={ok}: {}", summary.chars().take(120).collect::<String>())),
                Event::AgentMessage { agent, text, .. } => out.push(format!("{agent}: {}", text.chars().take(120).collect::<String>())),
                Event::Control { action, state, note, .. } => out.push(format!("control {action} -> {state} {note}")),
                _ => {}
            }
        }
        out.join("\n")
    }

    #[test]
    fn atomic_write_leaves_no_temp_and_round_trips() {
        let d = tmp("atomic");
        let p = d.join(FILE);
        write_atomic(&p, &json!({"a": 1})).unwrap();
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left behind");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["a"], 1);
    }

    /// The run is killed after its first orchestrator step (the scripted
    /// transport runs out); the checkpoint holds the state and the owed
    /// tool result; `--resume` continues the same conversation and the run
    /// passes, with the spend carried over.
    #[tokio::test]
    async fn kill_after_a_step_then_resume_from_the_checkpoint() {
        let d = tmp("resume");
        // run 1: one step, then the "crash"
        let t1 = Arc::new(ScriptedTransport::new(vec![reply("m", vec![tool_use("o1", "design", json!({}))], 100, 10)]));
        let (c1, _rx1) = client(t1.clone(), "r1");
        let s1 = run(c1, crate::tools::tools::<Fixture>(), cfg(&d, "r1")).await;
        assert!(matches!(s1.stop, StopReason::Error(ref m) if m.contains("ran out")), "{:?}", s1.stop);
        let ck = load(&d).unwrap();
        assert_eq!(ck.phase, Phase::Orchestrating { turns_used: 1 });
        assert_eq!(ck.round, 1);
        assert!(ck.state.design.is_some(), "the design made before the crash is in the checkpoint");
        assert!(ck.state.best.is_some(), "the incumbent too");
        let orch = ck.orchestrator.as_ref().unwrap();
        assert_eq!(orch.messages.len(), 2, "user turn + assistant tool call");
        assert_eq!(orch.pending.len(), 1, "the design tool_result is owed");
        assert_eq!(orch.pending[0]["tool_use_id"], "o1");
        assert!(ck.spend.usd > 0.0);
        // it is the last checkpoint: a finish checkpoint would stop the resume
        assert!(ck.source == "step", "{}", ck.source);

        // run 2: `wright run --resume DIR`
        let (args, resumed) = resume_args(&["--resume".to_string(), d.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(args[0], "rocket");
        assert_eq!(args[1], "a small rocket");
        assert!(args.iter().any(|a| a == "-o"));
        let resumed = resumed.unwrap();
        let t2 = Arc::new(ScriptedTransport::new(vec![
            reply("m", vec![tool_use("o2", "submit", json!({"summary": "done"}))], 100, 10),
            reply("m", vec![tool_use("v1", "verdict", json!({"summary": "fine"}))], 100, 10),
        ]));
        let (c2, _rx2) = client(t2.clone(), &resumed.run_id);
        let mut cfg2 = cfg(&d, &resumed.run_id);
        cfg2.resume = Some(resumed);
        let s2 = run(c2, crate::tools::tools::<Fixture>(), cfg2).await;
        assert_eq!(s2.stop, StopReason::Passed, "{:?}", s2.stop);
        assert_eq!(s2.rounds, 1, "finished the interrupted round, no new one");
        assert!(s2.spent_usd > ck.spend.usd, "the first run's spend is carried: {} vs {}", s2.spent_usd, ck.spend.usd);
        let reqs = t2.requests.lock().unwrap();
        let msgs = reqs[0].body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "history + the resumed user turn: {}", reqs[0].body["messages"]);
        assert_eq!(msgs[1]["content"][0]["id"], "o1", "the earlier tool call is still in the history");
        let last = msgs[2]["content"].as_array().unwrap();
        assert_eq!(last[0]["type"], "tool_result", "the owed result leads the turn");
        assert!(last.iter().any(|b| b["text"].as_str().is_some_and(|t| t.contains("resumed from a checkpoint"))));
        // the finish checkpoint refuses a second resume
        let fin = load(&d).unwrap();
        assert_eq!(fin.phase, Phase::Finished);
        assert!(resume_args(&["--resume".to_string(), d.to_string_lossy().into_owned()]).unwrap_err().to_string().contains("nothing to resume"));
    }

    /// Paused before the run: the first tool call is held, a person
    /// resumes it with a note, and the orchestrator's tool result says so.
    #[tokio::test]
    async fn a_held_call_runs_when_the_person_resumes() {
        let d = tmp("pause");
        let control = Control::new().with_timeout(Duration::from_secs(5));
        control.pause();
        let t = Arc::new(ScriptedTransport::new(vec![
            reply("m", vec![tool_use("o1", "design", json!({}))], 1, 1),
            reply("m", vec![tool_use("o2", "submit", json!({"summary": "s"}))], 1, 1),
            reply("m", vec![tool_use("v1", "verdict", json!({"summary": "ok"}))], 1, 1),
        ]));
        let (c, mut rx) = client(t.clone(), "p1");
        let mut cfg = cfg(&d, "p1");
        cfg.control = Some(control.clone());
        let c2 = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            c2.send(Command::Resume, "go ahead, looks right");
        });
        let s = run(c, crate::tools::tools::<Fixture>(), cfg).await;
        let evs = events(&mut rx);
        let seen = controls(&evs);
        assert_eq!(s.stop, StopReason::Passed, "{:?}\n{}", s.stop, trail(&evs));
        let reqs = t.requests.lock().unwrap();
        let result = reqs[1].body["messages"][2]["content"][0]["content"].as_str().unwrap().to_string();
        assert!(result.starts_with("[resumed by the person: go ahead, looks right]"), "{result}");
        assert_eq!(seen[0], ("pause".to_string(), "paused".to_string()));
        assert_eq!(seen[1].0, "waiting");
        assert_eq!(seen[2], ("resume".to_string(), "running".to_string()));
        assert_eq!(control.gate(), Gate::Running);
        let status: Value = serde_json::from_str(&std::fs::read_to_string(d.join(CONTROL_DIR).join(STATUS_FILE)).unwrap()).unwrap();
        assert_eq!(status["state"], "running");
    }

    /// Step mode: `approve` from running arms it; each later call waits
    /// for its own approve.
    #[tokio::test]
    async fn step_mode_waits_for_every_call() {
        let d = tmp("step");
        let control = Control::new().with_timeout(Duration::from_secs(5));
        control.approve();
        let t = Arc::new(ScriptedTransport::new(vec![
            reply("m", vec![tool_use("o1", "design", json!({}))], 1, 1),
            reply("m", vec![tool_use("o2", "check", json!({}))], 1, 1),
            reply("m", vec![tool_use("o3", "submit", json!({"summary": "s"}))], 1, 1),
            reply("m", vec![tool_use("v1", "verdict", json!({"summary": "ok"}))], 1, 1),
        ]));
        let (c, mut rx) = client(t.clone(), "s1");
        let mut cfg = cfg(&d, "s1");
        cfg.control = Some(control.clone());
        let c2 = control.clone();
        tokio::spawn(async move {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(150)).await;
                c2.approve();
            }
        });
        let s = run(c, crate::tools::tools::<Fixture>(), cfg).await;
        let evs = events(&mut rx);
        let waits = controls(&evs).iter().filter(|(a, _)| a == "waiting").count();
        assert_eq!(s.stop, StopReason::Passed, "{:?}\n{}", s.stop, trail(&evs));
        assert_eq!(waits, 3, "design, check and submit were each held");
        let reqs = t.requests.lock().unwrap();
        for i in 1..=2 {
            let r = reqs[i].body["messages"][2 * i]["content"][0]["content"].as_str().unwrap().to_string();
            assert!(r.starts_with("[this step was approved by the person]"), "{r}");
        }
    }

    /// A `stop` file in the mailbox: the next call is refused with the
    /// paused-for-approval result, the run stops, the checkpoint stays.
    #[tokio::test]
    async fn a_stop_file_in_the_mailbox_stops_the_run() {
        let d = tmp("mailbox");
        std::fs::create_dir_all(d.join(CONTROL_DIR)).unwrap();
        std::fs::write(d.join(CONTROL_DIR).join("stop"), "we are out of time").unwrap();
        let t = Arc::new(ScriptedTransport::new(vec![
            reply("m", vec![tool_use("o1", "design", json!({}))], 1, 1),
            reply("m", vec![text("ok")], 1, 1),
        ]));
        let (c, mut rx) = client(t.clone(), "m1");
        let s = run(c, crate::tools::tools::<Fixture>(), cfg(&d, "m1")).await;
        assert!(matches!(s.stop, StopReason::Error(ref m) if m.contains("stopped by the person: we are out of time")), "{:?}", s.stop);
        assert!(s.state.design.is_none(), "the held call never ran");
        assert_eq!(t.calls(), 1, "no model call after the stop");
        assert!(!d.join(CONTROL_DIR).join("stop").exists(), "the command file was consumed");
        assert!(controls(&events(&mut rx)).iter().any(|(a, st)| a == "stop" && st == "stopped"));
        let ck = load(&d).unwrap();
        assert_eq!(ck.control, Gate::Stopped);
        assert!(ck.orchestrator.as_ref().unwrap().pending[0]["content"].as_str().unwrap().contains("paused: waiting for approval"));
        assert_eq!(ck.phase, Phase::Orchestrating { turns_used: 1 }, "a stopped run keeps its step checkpoint for --resume");
    }

    /// A pause nobody answers: the orchestrator gets the timeout as a
    /// tool result (no hang), the run stops, the checkpoint is resumable.
    #[tokio::test]
    async fn a_pause_times_out_into_a_tool_result() {
        let d = tmp("timeout");
        let control = Control::new().with_timeout(Duration::from_millis(300));
        control.pause();
        let t = Arc::new(ScriptedTransport::new(vec![reply("m", vec![tool_use("o1", "design", json!({}))], 1, 1)]));
        let (c, mut rx) = client(t.clone(), "t1");
        let mut cfg = cfg(&d, "t1");
        cfg.control = Some(control);
        let started = Instant::now();
        let s = run(c, crate::tools::tools::<Fixture>(), cfg).await;
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(matches!(s.stop, StopReason::Error(ref m) if m.contains("nobody resumed")), "{:?}", s.stop);
        assert!(controls(&events(&mut rx)).iter().any(|(a, _)| a == "timeout"));
        let ck = load(&d).unwrap();
        let owed = ck.orchestrator.as_ref().unwrap().pending[0]["content"].as_str().unwrap().to_string();
        assert!(owed.contains("paused: waiting for approval timed out"), "{owed}");
        assert_eq!(ck.phase, Phase::Orchestrating { turns_used: 1 });
    }

    #[test]
    fn command_words_and_mailbox_file_names() {
        assert_eq!(Command::parse("Approve"), Some(Command::Approve));
        assert_eq!(Command::parse("continue"), Some(Command::Resume));
        assert_eq!(Command::parse("nonsense"), None);
        let d = tmp("names");
        let m = Mailbox::new(&d);
        std::fs::write(m.dir.join("pause-1.txt"), "look at the fin").unwrap();
        std::fs::write(m.dir.join("README"), "not a command").unwrap();
        m.write_status(Gate::Running, None);
        let got = m.drain();
        assert_eq!(got, vec![(Command::Pause, "look at the fin".to_string())]);
        assert!(m.dir.join("README").exists() && m.dir.join(STATUS_FILE).exists());
    }

    #[test]
    fn saved_conversation_restores_history_and_pending() {
        let mut conv = Conversation::new("orchestrator", ModelCfg::new("m", "low", 10), "sys".into(), vec![]);
        conv.messages.push(json!({"role": "user", "content": "hi"}));
        let saved = SavedConversation::of(&conv, &[json!({"type": "tool_result"})]);
        let mut fresh = Conversation::new("orchestrator", ModelCfg::new("m", "low", 10), "sys2".into(), vec![]);
        saved.restore(&mut fresh);
        assert_eq!(fresh.messages.len(), 1);
        assert_eq!(fresh.pending.len(), 1);
        assert_eq!(fresh.system, "sys2", "the prompt is the current build's");
    }
}
