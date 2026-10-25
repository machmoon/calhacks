//! The orchestrator's engineering plan: a DAG of tasks over the run, each
//! with an owner role, the tasks it needs first, the checks it must turn
//! green, and a status. The lead writes it with `plan_set`, moves tasks
//! with `plan_update`, and must name a task in every `delegate`. The
//! harness verifies "done" against the check report and, when a check a
//! finished task vouched for fails again, marks that task and everything
//! downstream of it stale so the lead replans instead of carrying on.
//!
//! Precedent, read from source (not recalled):
//!
//! * MetaGPT `metagpt/schema.py`, classes `Task` and `Plan`
//!   (geekan/MetaGPT@main): a task has `task_id`, `dependent_task_ids`,
//!   `instruction`, `assignee`, `is_finished`; `Plan._topological_sort`
//!   orders tasks by their dependencies; `current_task` is the first
//!   unfinished task in that order; `add_tasks` keeps what an existing
//!   plan and a new one share and takes the rest from the new one; and
//!   `reset_task` resets a task **and every task depending on it,
//!   recursively** (its own FIXME notes that a cyclic plan recurses
//!   forever, so here a cycle is rejected when the plan is set).
//!   `Plan::set` is `add_tasks`, `Plan::invalidate` is `reset_task`
//!   driven by the checks, [`Plan::ready`] is `current_task` widened to
//!   the whole frontier.
//! * OpenHands `openhands-tools/openhands/tools/task_tracker/definition.py`
//!   (OpenHands/software-agent-sdk@main): one `plan` command replaces the
//!   whole list, statuses `todo | in_progress | done`, "always `view` the
//!   current list before making changes". Claude Code's TodoWrite is the
//!   same shape (`pending | in_progress | completed`). `plan_set` is that
//!   whole-list write; the current plan is in every prompt, so there is no
//!   separate `view`.
//!
//! Deviations, and why: a task names the **checks** it must turn green,
//! so `done` is verified against the last report rather than asserted (the
//! rule of this codebase: nothing passes by say-so); a task undone by a
//! failing check is `stale`, not reset to `pending`, so the desktop can
//! show what a failure took back; `set` carries a task's status over by
//! id when its inputs and checks are unchanged, instead of MetaGPT's
//! common-prefix rule, which is tied to its linear task list.

use crate::agent::ToolOutput;
use crate::client::Client;
use crate::state::DesignState;
use crate::tools::DomainTools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wright_core::{Event, Finished, PlanStatus, PlanTask};

/// The plan the lead keeps in [`DesignState`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub goal: String,
    /// Topologically sorted: every task after the tasks it needs.
    pub tasks: Vec<PlanTask>,
    /// What the last check took back (set by [`Plan::invalidate`]), shown
    /// in the next tool result so the lead replans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

/// Owners a task may have besides the domain's specialist roles.
pub const FIXED_OWNERS: &[&str] = &["orchestrator", "reviewer"];

/// The orchestrator's instructions for the plan (appended to its system
/// prompt). The order is the one an engineering team works in.
pub const GUIDE: &str = "Work to an explicit plan. Before you design, call plan_set with the task graph for this vehicle: \
requirements (spec written and validated) -> sizing (the main performance choices) -> structures -> electronics/avionics -> sourcing (every line priced and in stock) -> fabrication (prints, build) -> review (submit). \
Give each task an id, an owner (you, a specialist role, or the reviewer), the ids of the tasks it needs first (inputs), and the checks it must turn green (checks); split a step into several tasks when different specialists own them. \
Every delegate names its plan task (plan_task); the harness marks it running, then done if its checks pass or blocked with what still fails. \
Mark your own tasks with plan_update (done is refused while a named check fails or an input is unfinished). \
When a check a done task vouched for fails again, that task and everything downstream of it become stale and the tool result says so: redo the stale tasks in order, or plan_set a revised plan (unchanged tasks keep their status).";

/// Kahn's topological sort that keeps the given order among ready tasks.
/// `Err` names a cycle or an unknown input.
fn sorted(tasks: &[PlanTask]) -> Result<Vec<PlanTask>, String> {
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    for t in tasks {
        for i in &t.inputs {
            if i == &t.id {
                return Err(format!("task {} lists itself as an input", t.id));
            }
            if !ids.contains(&i.as_str()) {
                return Err(format!("task {} needs `{i}`, which is not a task in this plan (tasks: {})", t.id, ids.join(", ")));
            }
        }
    }
    let mut out: Vec<PlanTask> = Vec::with_capacity(tasks.len());
    let mut left: Vec<&PlanTask> = tasks.iter().collect();
    while !left.is_empty() {
        let pos = left.iter().position(|t| t.inputs.iter().all(|i| out.iter().any(|o| &o.id == i)));
        match pos {
            Some(p) => out.push(left.remove(p).clone()),
            None => {
                let stuck: Vec<&str> = left.iter().map(|t| t.id.as_str()).collect();
                return Err(format!("the plan has a cycle among {}: a task cannot need its own result", stuck.join(", ")));
            }
        }
    }
    Ok(out)
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&PlanTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut PlanTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    fn ids(&self) -> String {
        self.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
    }

    /// Replace the plan (MetaGPT `add_tasks` / the tracker's `plan`).
    /// Validates ids, owners, inputs and checks, sorts topologically, and
    /// carries status and note over for a task whose id, inputs and checks
    /// are unchanged. Returns the ids carried over.
    pub fn set(&mut self, goal: &str, tasks: Vec<PlanTask>, owners: &[&str], checks: &[&str]) -> Result<Vec<String>, String> {
        if tasks.is_empty() {
            return Err("a plan needs at least one task".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &tasks {
            if t.id.trim().is_empty() {
                return Err("every task needs a non-empty id".into());
            }
            if !seen.insert(t.id.as_str()) {
                return Err(format!("task id `{}` is used twice", t.id));
            }
            if !owners.contains(&t.owner.as_str()) {
                return Err(format!("task {}: no owner `{}`; owners are {owners:?}", t.id, t.owner));
            }
            for c in &t.checks {
                if !checks.contains(&c.as_str()) {
                    return Err(format!("task {}: `{c}` is not a declared check; checks are {checks:?}", t.id));
                }
            }
        }
        let mut sorted = sorted(&tasks)?;
        let mut carried = vec![];
        for t in &mut sorted {
            match self.get(&t.id) {
                Some(old) if old.inputs == t.inputs && old.checks == t.checks && old.status != PlanStatus::Pending => {
                    t.status = old.status;
                    t.note = old.note.clone();
                    carried.push(t.id.clone());
                }
                _ => {
                    t.status = PlanStatus::Pending;
                    t.note = String::new();
                }
            }
        }
        self.goal = goal.to_string();
        self.tasks = sorted;
        self.notice = None;
        Ok(carried)
    }

    /// Inputs of `id` that are not done.
    fn unfinished_inputs(&self, id: &str) -> Vec<String> {
        let Some(t) = self.get(id) else { return vec![] };
        t.inputs.iter().filter(|i| self.get(i).is_none_or(|d| d.status != PlanStatus::Done)).cloned().collect()
    }

    /// Checks named by `id` that the report shows failing (or that it does
    /// not contain), as `name: outcome` lines. A regression case `check@r`
    /// counts against `check`.
    fn failing_checks(&self, id: &str, report: Option<&Finished>) -> Vec<String> {
        let Some(t) = self.get(id) else { return vec![] };
        t.checks
            .iter()
            .flat_map(|c| match report {
                None => vec![format!("{c}: no check report yet; run check first")],
                Some(r) => {
                    let mut v: Vec<String> = r
                        .outcomes
                        .iter()
                        .filter(|o| o.name.split('@').next() == Some(c.as_str()) && !o.passed())
                        .map(|o| format!("{o}"))
                        .collect();
                    if v.is_empty() && r.get(c).is_none() {
                        v.push(format!("{c}: not in the last report"));
                    }
                    v
                }
            })
            .collect()
    }

    /// The lead moves a task by hand (`plan_update`). `done` and `running`
    /// need every input done; `done` also needs every named check passing
    /// in `report`. Returns a one-line confirmation.
    pub fn update(&mut self, id: &str, status: PlanStatus, note: &str, report: Option<&Finished>) -> Result<String, String> {
        if self.get(id).is_none() {
            return Err(format!("no plan task `{id}`; tasks are {}", self.ids()));
        }
        if matches!(status, PlanStatus::Done | PlanStatus::Running) {
            let open = self.unfinished_inputs(id);
            if !open.is_empty() {
                return Err(format!("task {id} cannot be {status}: its inputs {open:?} are not done"));
            }
        }
        if status == PlanStatus::Done {
            let failing = self.failing_checks(id, report);
            if !failing.is_empty() {
                return Err(format!("task {id} is not done: its checks are not green:\n- {}", failing.join("\n- ")));
            }
        }
        let t = self.get_mut(id).expect("checked above");
        t.status = status;
        t.note = note.to_string();
        Ok(format!("task {id} is now {status}"))
    }

    /// A delegation begins work on `id` for `role`: the task must exist,
    /// be owned by that role, and have its inputs done. It becomes
    /// running.
    pub fn start(&mut self, id: &str, role: &str) -> Result<(), String> {
        let Some(t) = self.get(id) else {
            return Err(format!("no plan task `{id}`; tasks are {}", self.ids()));
        };
        if t.owner != role {
            return Err(format!("plan task {id} is owned by `{}`, not `{role}`: delegate to its owner or change the plan", t.owner));
        }
        if t.status == PlanStatus::Done {
            return Err(format!("plan task {id} is already done; pick a pending or stale task, or plan_set a revised plan"));
        }
        let open = self.unfinished_inputs(id);
        if !open.is_empty() {
            return Err(format!("plan task {id} needs {open:?} done first"));
        }
        let t = self.get_mut(id).expect("checked above");
        t.status = PlanStatus::Running;
        t.note = format!("delegated to {role}");
        Ok(())
    }

    /// The delegation for `id` returned: done if every named check passes
    /// in `report`, otherwise blocked with what still fails. Returns the
    /// line for the lead.
    pub fn finish(&mut self, id: &str, report: Option<&Finished>) -> String {
        let failing = self.failing_checks(id, report);
        let Some(t) = self.get_mut(id) else { return String::new() };
        if failing.is_empty() {
            t.status = PlanStatus::Done;
            t.note = if t.checks.is_empty() { "specialist reported".into() } else { format!("checks green: {}", t.checks.join(", ")) };
            format!("plan task {id} is done")
        } else {
            t.status = PlanStatus::Blocked;
            t.note = format!("checks still failing: {}", failing.join("; "));
            format!("plan task {id} is blocked: {}; delegate it again or revise the plan", failing.join("; "))
        }
    }

    /// MetaGPT `reset_task`, driven by the checks: every done task whose
    /// named check fails in `report` becomes stale, and so does every done
    /// or running task downstream of it, transitively. Returns the ids
    /// made stale (in plan order) and sets [`Plan::notice`].
    pub fn invalidate(&mut self, report: &Finished) -> Vec<String> {
        let mut stale: Vec<(String, String)> = vec![];
        for t in &self.tasks {
            if t.status != PlanStatus::Done {
                continue;
            }
            let failing = self.failing_checks(&t.id, Some(report));
            if !failing.is_empty() {
                stale.push((t.id.clone(), format!("check failed: {}", failing.join("; "))));
            }
        }
        if stale.is_empty() {
            // the last notice was delivered with the check that raised it
            self.notice = None;
            return vec![];
        }
        // downstream, transitively; tasks are in topological order so one
        // forward pass reaches everything
        let mut changed: Vec<String> = vec![];
        for i in 0..self.tasks.len() {
            let id = self.tasks[i].id.clone();
            let direct = stale.iter().find(|(s, _)| s == &id).map(|(_, why)| why.clone());
            let upstream: Vec<String> = self.tasks[i].inputs.iter().filter(|x| changed.contains(x)).cloned().collect();
            let why = match (direct, upstream.is_empty()) {
                (Some(w), _) => Some(w),
                (None, false) if matches!(self.tasks[i].status, PlanStatus::Done | PlanStatus::Running) => Some(format!("its inputs {upstream:?} are stale")),
                _ => None,
            };
            if let Some(why) = why {
                self.tasks[i].status = PlanStatus::Stale;
                self.tasks[i].note = why;
                changed.push(id);
            }
        }
        let first = stale.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(", ");
        self.notice = Some(format!(
            "PLAN: a check a done task vouched for failed ({first}); now stale: {}. Redo them in plan order (delegate names the task) or plan_set a revised plan.",
            changed.join(", ")
        ));
        changed
    }

    /// Tasks that can be worked now: pending or stale with every input
    /// done (MetaGPT's `current_task`, widened to the whole frontier).
    pub fn ready(&self) -> Vec<&PlanTask> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, PlanStatus::Pending | PlanStatus::Stale) && self.unfinished_inputs(&t.id).is_empty())
            .collect()
    }

    pub fn done(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|t| t.status == PlanStatus::Done)
    }

    /// One line per task, for prompts: `[done] req: Write the spec (orchestrator)`.
    pub fn lines(&self) -> Vec<String> {
        self.tasks
            .iter()
            .map(|t| {
                let mut s = format!("[{}] {}: {} ({})", t.status, t.id, t.title, t.owner);
                if !t.inputs.is_empty() {
                    s.push_str(&format!(" needs {}", t.inputs.join(", ")));
                }
                if !t.checks.is_empty() {
                    s.push_str(&format!(" checks {}", t.checks.join(", ")));
                }
                if !t.note.is_empty() {
                    s.push_str(&format!(" -- {}", t.note));
                }
                s
            })
            .collect()
    }

    /// `3/7 done, 1 stale`.
    pub fn summary(&self) -> String {
        let n = |s: PlanStatus| self.tasks.iter().filter(|t| t.status == s).count();
        let mut s = format!("{}/{} done", n(PlanStatus::Done), self.tasks.len());
        for (st, label) in [(PlanStatus::Running, "running"), (PlanStatus::Blocked, "blocked"), (PlanStatus::Stale, "stale")] {
            let k = n(st);
            if k > 0 {
                s.push_str(&format!(", {k} {label}"));
            }
        }
        s
    }
}

// ------------------------------------------------------------------ tools

/// Every owner a task may have for `domain`.
pub fn owners(domain: &dyn DomainTools) -> Vec<&'static str> {
    let mut o: Vec<&'static str> = FIXED_OWNERS.to_vec();
    o.extend(crate::prompts::roles(domain.name()).iter().map(|r| r.0));
    o
}

/// `plan_set` and `plan_update`, for the orchestrator only.
pub fn tools(domain: &dyn DomainTools) -> Vec<Value> {
    let owners = owners(domain);
    let checks: Vec<&str> = domain.checks().iter().map(|c| c.0).collect();
    let s = |d: &str| json!({"type": "string", "description": d});
    vec![
        crate::prompts::tool(
            "plan_set",
            "Write (or rewrite) the engineering plan: a DAG of tasks from requirements through sizing, structures, electronics/avionics, sourcing and fabrication to review. Each task names its owner, the tasks it needs first and the checks it must turn green. Rewriting keeps the status of tasks whose id, inputs and checks are unchanged. Rejected for an unknown owner, input or check, or a cycle.",
            json!({
                "goal": s("one line: what the finished vehicle must be"),
                "tasks": {
                    "type": "array",
                    "description": "the tasks, in any order",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": s("short id, e.g. \"req\", \"size\", \"struct\", \"elec\", \"source\", \"fab\", \"review\""),
                            "title": s("what the task produces, one line"),
                            "owner": {"type": "string", "enum": owners, "description": "who does it: you (orchestrator), a specialist role, or the reviewer"},
                            "inputs": {"type": "array", "items": {"type": "string"}, "description": "ids of the tasks whose results this one needs; [] for none"},
                            "checks": {"type": "array", "items": {"type": "string", "enum": checks}, "description": "declared checks this task must turn green; [] if none measures it"}
                        },
                        "required": ["id", "title", "owner", "inputs", "checks"],
                        "additionalProperties": false
                    }
                }
            }),
        ),
        crate::prompts::tool(
            "plan_update",
            "Move one plan task: pending, running, done or blocked, with a note. done is refused while any check the task names is failing or any input is unfinished. Tasks you delegate are moved by the harness; use this for your own tasks and to unblock or re-queue.",
            json!({
                "id": s("the task id"),
                "status": {"type": "string", "enum": ["pending", "running", "done", "blocked"]},
                "note": s("one line: the result, or why it is blocked")
            }),
        ),
    ]
}

#[derive(Deserialize)]
struct SetArgs {
    goal: String,
    tasks: Vec<TaskArg>,
}
#[derive(Deserialize)]
struct TaskArg {
    id: String,
    title: String,
    owner: String,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    checks: Vec<String>,
}
#[derive(Deserialize)]
struct UpdateArgs {
    id: String,
    status: PlanStatus,
    #[serde(default)]
    note: String,
}

/// Send the plan as an event (the desktop draws the latest one).
pub fn emit(client: &Client, plan: &Plan) {
    let _ = client.events.send(Event::Plan { run: client.run.clone(), goal: plan.goal.clone(), tasks: plan.tasks.clone() });
}

/// Handle `plan_set` / `plan_update`; `None` for any other tool.
pub fn call(state: &mut DesignState, client: &Client, domain: &dyn DomainTools, name: &str, input: &Value) -> Option<ToolOutput> {
    let out = match name {
        "plan_set" => {
            let a: SetArgs = match serde_json::from_value(input.clone()) {
                Ok(a) => a,
                Err(e) => return Some(ToolOutput::malformed(format!("input does not match the tool schema: {e}"))),
            };
            let tasks: Vec<PlanTask> = a
                .tasks
                .into_iter()
                .map(|t| PlanTask { id: t.id.trim().to_string(), title: t.title, owner: t.owner, inputs: t.inputs, checks: t.checks, status: PlanStatus::Pending, note: String::new() })
                .collect();
            let owners = owners(domain);
            let checks: Vec<&str> = domain.checks().iter().map(|c| c.0).collect();
            match state.plan.set(&a.goal, tasks, &owners, &checks) {
                Err(e) => ToolOutput::err(format!("plan rejected, nothing changed: {e}")),
                Ok(carried) => {
                    emit(client, &state.plan);
                    let kept = if carried.is_empty() { String::new() } else { format!("; kept the status of {}", carried.join(", ")) };
                    ToolOutput::ok(format!("plan set ({} tasks{kept}):\n{}", state.plan.tasks.len(), state.plan.lines().join("\n")))
                }
            }
        }
        "plan_update" => {
            let a: UpdateArgs = match serde_json::from_value(input.clone()) {
                Ok(a) => a,
                Err(e) => return Some(ToolOutput::malformed(format!("input does not match the tool schema: {e}"))),
            };
            if state.plan.is_empty() {
                return Some(ToolOutput::err("no plan yet: call plan_set first"));
            }
            match state.plan.update(&a.id, a.status, &a.note, state.report.as_ref()) {
                Err(e) => ToolOutput::err(e),
                Ok(line) => {
                    emit(client, &state.plan);
                    let ready: Vec<&str> = state.plan.ready().iter().map(|t| t.id.as_str()).collect();
                    ToolOutput::ok(format!("{line}. {}; ready: {ready:?}", state.plan.summary()))
                }
            }
        }
        _ => return None,
    };
    Some(out)
}

/// After every check: mark stale what the report took back, and announce
/// it. Returns the notice for the tool result, if any.
pub fn after_check(state: &mut DesignState, client: &Client) -> Option<String> {
    let report = state.report.clone()?;
    let changed = state.plan.invalidate(&report);
    if changed.is_empty() {
        return None;
    }
    emit(client, &state.plan);
    let _ = client.events.send(Event::AgentMessage {
        run: client.run.clone(),
        agent: "harness".into(),
        text: format!("plan: {} now stale after the check", changed.join(", ")),
    });
    state.plan.notice.clone()
}

/// A delegation names its plan task. With no plan, any delegation is
/// allowed (runs that never planned behave as before); with one, the
/// task must be named, owned by `role`, and ready. `Ok(Some(title))` when
/// a task was started.
pub fn begin_delegation(state: &mut DesignState, client: &Client, role: &str, plan_task: &str) -> Result<Option<String>, String> {
    let id = plan_task.trim();
    if state.plan.is_empty() {
        if id.is_empty() {
            return Ok(None);
        }
        return Err(format!("plan_task `{id}` names no plan: call plan_set first (or delegate with plan_task \"\")"));
    }
    if id.is_empty() {
        let ready: Vec<&str> = state.plan.ready().iter().map(|t| t.id.as_str()).collect();
        return Err(format!("every delegation names its plan task (plan_task); ready tasks: {ready:?}. Add one with plan_set if none fits"));
    }
    state.plan.start(id, role)?;
    emit(client, &state.plan);
    Ok(state.plan.get(id).map(|t| t.title.clone()))
}

/// The delegation for `plan_task` returned: settle its status from the
/// report and announce it. Returns the line for the lead.
pub fn end_delegation(state: &mut DesignState, client: &Client, plan_task: &str) -> Option<String> {
    let id = plan_task.trim();
    if id.is_empty() || state.plan.get(id).is_none() {
        return None;
    }
    let line = state.plan.finish(id, state.report.as_ref());
    emit(client, &state.plan);
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::{Evidence, Outcome, Source};

    fn task(id: &str, owner: &str, inputs: &[&str], checks: &[&str]) -> PlanTask {
        PlanTask {
            id: id.into(),
            title: format!("do {id}"),
            owner: owner.into(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            checks: checks.iter().map(|s| s.to_string()).collect(),
            status: PlanStatus::Pending,
            note: String::new(),
        }
    }
    fn report(pairs: &[(&str, f64)]) -> Finished {
        Finished {
            outcomes: pairs
                .iter()
                .map(|(n, m)| Outcome { name: n.to_string(), evidence: Evidence::Margin { value: *m, unit: "mm".into() }, detail: String::new(), source: Source::Geometry })
                .collect(),
        }
    }
    const OWNERS: &[&str] = &["orchestrator", "reviewer", "aero", "sourcing"];
    const CHECKS: &[&str] = &["span", "budget"];

    fn plan() -> Plan {
        let mut p = Plan::default();
        // given out of order: sorted topologically
        p.set(
            "a span",
            vec![task("review", "reviewer", &["source"], &[]), task("source", "sourcing", &["size"], &["budget"]), task("size", "aero", &["req"], &["span"]), task("req", "orchestrator", &[], &[])],
            OWNERS,
            CHECKS,
        )
        .unwrap();
        p
    }

    #[test]
    fn set_sorts_and_validates() {
        let p = plan();
        let ids: Vec<&str> = p.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["req", "size", "source", "review"]);
        assert_eq!(p.ready().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["req"]);
        let mut q = Plan::default();
        let e = q.set("g", vec![task("a", "aero", &["b"], &[]), task("b", "aero", &["a"], &[])], OWNERS, CHECKS).unwrap_err();
        assert!(e.contains("cycle"), "{e}");
        let e = q.set("g", vec![task("a", "aero", &["zz"], &[])], OWNERS, CHECKS).unwrap_err();
        assert!(e.contains("not a task"), "{e}");
        let e = q.set("g", vec![task("a", "plumber", &[], &[])], OWNERS, CHECKS).unwrap_err();
        assert!(e.contains("no owner"), "{e}");
        let e = q.set("g", vec![task("a", "aero", &[], &["lift"])], OWNERS, CHECKS).unwrap_err();
        assert!(e.contains("not a declared check"), "{e}");
        assert!(q.is_empty(), "a rejected plan changes nothing");
    }

    #[test]
    fn done_is_verified_against_the_report_and_the_inputs() {
        let mut p = plan();
        let e = p.update("size", PlanStatus::Done, "", Some(&report(&[("span", 5.0)]))).unwrap_err();
        assert!(e.contains("inputs [\"req\"]"), "{e}");
        p.update("req", PlanStatus::Done, "spec written", None).unwrap();
        let e = p.update("size", PlanStatus::Done, "", Some(&report(&[("span", -2.0)]))).unwrap_err();
        assert!(e.contains("not green") && e.contains("span"), "{e}");
        let e = p.update("size", PlanStatus::Done, "", None).unwrap_err();
        assert!(e.contains("no check report"), "{e}");
        p.update("size", PlanStatus::Done, "", Some(&report(&[("span", 5.0)]))).unwrap();
        assert_eq!(p.get("size").unwrap().status, PlanStatus::Done);
        assert_eq!(p.ready().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["source"]);
    }

    #[test]
    fn a_failing_check_makes_the_task_and_its_dependents_stale() {
        let mut p = plan();
        p.update("req", PlanStatus::Done, "", None).unwrap();
        p.update("size", PlanStatus::Done, "", Some(&report(&[("span", 5.0)]))).unwrap();
        p.update("source", PlanStatus::Done, "", Some(&report(&[("span", 5.0), ("budget", 40.0)]))).unwrap();
        // a regression case of span fails: size vouched for span
        let changed = p.invalidate(&report(&[("span", 5.0), ("span@r1", -3.0), ("budget", 40.0)]));
        assert_eq!(changed, ["size", "source"]);
        assert_eq!(p.get("size").unwrap().status, PlanStatus::Stale);
        assert_eq!(p.get("source").unwrap().status, PlanStatus::Stale);
        assert_eq!(p.get("req").unwrap().status, PlanStatus::Done, "upstream is untouched");
        assert_eq!(p.get("review").unwrap().status, PlanStatus::Pending, "never-done tasks stay pending");
        assert!(p.notice.as_deref().unwrap().contains("size, source"));
        assert_eq!(p.ready().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["size"]);
        // nothing new failing: no change, no notice
        assert!(p.invalidate(&report(&[("span", 5.0), ("budget", 40.0)])).is_empty());
    }

    #[test]
    fn delegation_starts_and_settles_a_task() {
        let mut p = plan();
        let e = p.start("size", "sourcing").unwrap_err();
        assert!(e.contains("owned by `aero`"), "{e}");
        let e = p.start("size", "aero").unwrap_err();
        assert!(e.contains("needs [\"req\"]"), "{e}");
        p.update("req", PlanStatus::Done, "", None).unwrap();
        p.start("size", "aero").unwrap();
        assert_eq!(p.get("size").unwrap().status, PlanStatus::Running);
        let line = p.finish("size", Some(&report(&[("span", -1.0)])));
        assert!(line.contains("blocked"), "{line}");
        p.start("size", "aero").unwrap();
        let line = p.finish("size", Some(&report(&[("span", 1.0)])));
        assert!(line.contains("done"), "{line}");
        assert!(p.start("size", "aero").unwrap_err().contains("already done"));
    }

    #[test]
    fn a_rewrite_keeps_unchanged_tasks() {
        let mut p = plan();
        p.update("req", PlanStatus::Done, "spec", None).unwrap();
        let carried = p
            .set(
                "a span, revised",
                vec![task("req", "orchestrator", &[], &[]), task("size", "aero", &["req"], &["span", "budget"]), task("review", "reviewer", &["size"], &[])],
                OWNERS,
                CHECKS,
            )
            .unwrap();
        assert_eq!(carried, ["req"]);
        assert_eq!(p.get("req").unwrap().note, "spec");
        assert_eq!(p.tasks.len(), 3);
        assert_eq!(p.summary(), "1/3 done");
    }
}
