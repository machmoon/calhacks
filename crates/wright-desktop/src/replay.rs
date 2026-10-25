//! Replay a recorded `events.jsonl` on a timeline. Ported from the removed
//! web studio's `crates/wright-studio/src/replay.rs` (same line format with
//! an optional `"t"`, same default gaps per event type, same rule for the
//! recorded person's markup: stop there and hand the pen to the person at
//! the table; their markup replaces the recorded one, or "use recorded"
//! plays it). What changed: the studio streamed over SSE from a tokio task;
//! here the timeline is a cursor the UI ticks once a frame, and a scrubber
//! can move it anywhere (the state is re-folded from the start).

use std::path::{Path, PathBuf};

use anyhow::Context;
use wright_core::Event;

use crate::model::RunState;

#[derive(Debug, Clone)]
pub struct Recorded {
    /// Seconds since the start of the run, if the recording has it.
    pub t: Option<f64>,
    pub event: Event,
}

pub fn load_jsonl(path: &Path) -> anyhow::Result<Vec<Recorded>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).with_context(|| format!("{}:{}: not JSON", path.display(), i + 1))?;
        let t = v.get("t").and_then(|t| t.as_f64());
        let event: Event =
            serde_json::from_value(v).with_context(|| format!("{}:{}: not a wright_core::Event", path.display(), i + 1))?;
        out.push(Recorded { t, event });
    }
    Ok(out)
}

/// Default gap before an event when the recording has no timestamps
/// (the studio's values).
pub fn default_gap(ev: &Event) -> f64 {
    match ev {
        Event::RunStarted { .. } => 0.4,
        Event::AgentMessage { text, .. } => (0.7 + text.len() as f64 / 140.0).min(3.0),
        Event::ToolCall { .. } => 0.45,
        Event::ToolResult { .. } => 0.7,
        Event::Checks { .. } => 0.9,
        Event::Headlines { .. } => 0.1,
        Event::Render { .. } => 0.8,
        Event::Feedback { .. } => 1.2,
        Event::Spend { .. } => 0.2,
        Event::Artifact { .. } => 0.5,
        Event::Plan { .. } => 0.5,
        Event::RunFinished { .. } => 1.0,
        Event::Control { .. } => 0.6,
    }
}

/// What a live markup does during a replay (the studio's `LiveFeedback`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedback {
    /// Taken in place of the recorded markup; the recorded response follows.
    Substituted,
    /// The recording is over; nobody is listening.
    AfterEnd,
    /// Not at a recorded markup step: refused, so an early drawing never
    /// consumes the recorded pause (review 4, finding 7).
    NotAtMarkupStep,
}

pub struct Replay {
    pub path: PathBuf,
    /// The run dir (the recording's parent): recorded paths resolve here.
    pub dir: PathBuf,
    pub events: Vec<Recorded>,
    /// Events applied: `events[..cursor]` are in `state`.
    pub cursor: usize,
    pub playing: bool,
    pub speed: f64,
    /// Seconds waited toward the next event.
    waited: f64,
    /// Stopped at a recorded markup, waiting for the live person's.
    pub awaiting_feedback: bool,
    /// Live markups that replaced recorded ones: (index of the recorded
    /// feedback event, the live one).
    substitutes: Vec<(usize, Event)>,
    pub state: RunState,
}

impl Replay {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let events = load_jsonl(path)?;
        if events.is_empty() {
            anyhow::bail!("{} has no events", path.display());
        }
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Ok(Self::from_events(path.to_path_buf(), dir, events))
    }

    pub fn from_events(path: PathBuf, dir: PathBuf, events: Vec<Recorded>) -> Self {
        Replay {
            path,
            dir,
            events,
            cursor: 0,
            playing: true,
            speed: 1.0,
            waited: 0.0,
            awaiting_feedback: false,
            substitutes: vec![],
            state: RunState::default(),
        }
    }

    pub fn total(&self) -> usize {
        self.events.len()
    }

    pub fn run_id(&self) -> Option<String> {
        self.events.iter().find_map(|r| match &r.event {
            Event::RunStarted { run, .. } => Some(run.clone()),
            _ => None,
        })
    }

    fn gap_before(&self, i: usize) -> f64 {
        let ev = &self.events[i];
        match (i.checked_sub(1).and_then(|p| self.events[..=p].iter().rev().find_map(|r| r.t)), ev.t) {
            (Some(a), Some(b)) => (b - a).clamp(0.0, 10.0),
            (None, Some(_)) => 0.3,
            _ => default_gap(&ev.event),
        }
    }

    fn apply_index(&mut self, i: usize) {
        if let Some((_, live)) = self.substitutes.iter().find(|(k, _)| *k == i) {
            let live = live.clone();
            self.state.apply(&live);
        } else {
            let ev = self.events[i].event.clone();
            self.state.apply(&ev);
        }
    }

    /// Advance by `dt` seconds of wall time. Returns true if anything changed.
    pub fn tick(&mut self, dt: f64) -> bool {
        if !self.playing || self.awaiting_feedback || self.cursor >= self.events.len() {
            return false;
        }
        self.waited += dt * self.speed;
        let mut changed = false;
        while self.cursor < self.events.len() && self.waited >= self.gap_before(self.cursor) {
            // the recorded person's markup: stop and hand over the pen
            if matches!(self.events[self.cursor].event, Event::Feedback { .. })
                && !self.substitutes.iter().any(|(k, _)| *k == self.cursor)
            {
                self.awaiting_feedback = true;
                self.waited = 0.0;
                return true;
            }
            self.waited -= self.gap_before(self.cursor);
            self.apply_index(self.cursor);
            self.cursor += 1;
            changed = true;
        }
        changed
    }

    /// At a markup stop: play the recorded markup.
    pub fn use_recorded(&mut self) {
        if self.awaiting_feedback {
            self.awaiting_feedback = false;
            self.apply_index(self.cursor);
            self.cursor += 1;
        }
    }

    /// The live person's markup (a `Feedback` event already saved).
    pub fn live_feedback(&mut self, ev: Event) -> LiveFeedback {
        if self.awaiting_feedback {
            self.substitutes.push((self.cursor, ev));
            self.awaiting_feedback = false;
            self.apply_index(self.cursor);
            self.cursor += 1;
            LiveFeedback::Substituted
        } else if self.cursor >= self.events.len() {
            self.state.apply(&ev);
            LiveFeedback::AfterEnd
        } else {
            LiveFeedback::NotAtMarkupStep
        }
    }

    /// Jump to `cursor` events applied (the scrubber). A scrub is explicit,
    /// so it passes recorded markup without stopping.
    pub fn seek(&mut self, cursor: usize) {
        let cursor = cursor.min(self.events.len());
        if cursor < self.cursor {
            self.state = RunState::default();
            self.cursor = 0;
        }
        while self.cursor < cursor {
            self.apply_index(self.cursor);
            self.cursor += 1;
        }
        self.waited = 0.0;
        self.awaiting_feedback = false;
    }

    pub fn restart(&mut self) {
        self.substitutes.clear();
        self.seek(0);
        self.playing = true;
    }

    /// Indices of recorded markup and of check reports (timeline ticks).
    pub fn marks(&self) -> Vec<(usize, Mark)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match &r.event {
                Event::Feedback { .. } => Some((i, Mark::Feedback)),
                Event::Checks { failed, .. } => Some((i, if *failed == 0 { Mark::ChecksPass } else { Mark::ChecksFail })),
                Event::RunFinished { .. } => Some((i, Mark::Finished)),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Feedback,
    ChecksPass,
    ChecksFail,
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(run: &str, text: &str) -> Recorded {
        Recorded { t: None, event: Event::AgentMessage { run: run.into(), agent: "orchestrator".into(), text: text.into() } }
    }
    fn fb(note: &str) -> Event {
        Event::Feedback { run: "r".into(), view: "iso".into(), sketch_png: "feedback/x.png".into(), note: note.into() }
    }
    fn rec() -> Replay {
        let events = vec![
            Recorded { t: None, event: Event::RunStarted { run: "r".into(), domain: "aircraft".into(), request: "q".into() } },
            msg("r", "a"),
            Recorded { t: None, event: fb("recorded") },
            msg("r", "b"),
        ];
        Replay::from_events("x".into(), ".".into(), events)
    }

    #[test]
    fn stops_at_recorded_markup_and_takes_the_live_one() {
        let mut r = rec();
        for _ in 0..200 {
            r.tick(0.1);
        }
        assert!(r.awaiting_feedback);
        assert_eq!(r.cursor, 2);
        assert_eq!(r.live_feedback(fb("live")), LiveFeedback::Substituted);
        for _ in 0..200 {
            r.tick(0.1);
        }
        assert_eq!(r.cursor, 4);
        assert_eq!(r.state.feedback.len(), 1);
        assert_eq!(r.state.feedback[0].2, "live");
    }

    #[test]
    fn early_markup_is_refused_and_does_not_consume_the_pause() {
        let mut r = rec();
        r.tick(0.5);
        assert_eq!(r.live_feedback(fb("early")), LiveFeedback::NotAtMarkupStep);
        for _ in 0..200 {
            r.tick(0.1);
        }
        assert!(r.awaiting_feedback);
        r.use_recorded();
        assert_eq!(r.state.feedback[0].2, "recorded");
    }

    #[test]
    fn scrubbing_back_refolds_from_the_start() {
        let mut r = rec();
        r.seek(4);
        assert_eq!(r.state.log.len(), 3);
        r.seek(1);
        assert_eq!(r.state.log.len(), 0);
        assert_eq!(r.state.run.as_deref(), Some("r"));
    }

    #[test]
    fn the_demo_recording_loads_and_folds() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/aircraft/events.jsonl");
        let mut r = Replay::open(&p).unwrap();
        r.seek(r.total());
        let c = r.state.checks.as_ref().expect("checks");
        assert!(c.passed > 0);
        assert!(r.state.finished.is_some());
        assert!(r.state.bom_path(&r.dir).is_some());
    }
}
