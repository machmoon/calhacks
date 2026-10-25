// One pure fold from events to what the page shows. A port of
// wright/crates/wright-desktop/src/model.rs `RunState::apply` (the egui
// app's reducer): a live run applies events as the tail returns them, a
// replay applies them up to the cursor.

import type {
  Headline,
  Outcome,
  PlanTask,
  WrightEvent,
} from "./types";

export type LogKind = "message" | "call" | "result" | "feedback" | "notice";

export interface LogLine {
  agent: string;
  kind: LogKind;
  /** Only for kind "result". */
  ok?: boolean;
  text: string;
}

export interface Spend {
  inputTokens: number;
  outputTokens: number;
  usd: number;
  budgetUsd: number;
}

export interface RunState {
  run: string | null;
  domain: string;
  request: string;
  executionKind: string | null;
  log: LogLine[];
  checks: { passed: number; failed: number; outcomes: Outcome[] } | null;
  /** How many check reports so far (one per design iteration). */
  checkReports: number;
  headlines: Headline[];
  planGoal: string;
  plan: PlanTask[];
  spend: Spend | null;
  /** Deduplicated by path, in order. */
  artifacts: { label: string; path: string }[];
  /** Latest per view, in order of first appearance of the replacement. */
  renders: { view: string; path: string }[];
  feedback: { view: string; sketch: string; note: string }[];
  /** The harness's gate after the last control event. */
  gate: string;
  /** Pass/fail counts of every check report, in order: the before/after. */
  checkHistory: { passed: number; failed: number }[];
  /** Each headline's value at every headlines event, by label. */
  headlineHistory: Record<string, number[]>;
  finished: { passed: boolean; objective: number; label: string } | null;
}

export const initialState = (): RunState => ({
  run: null,
  domain: "",
  request: "",
  executionKind: null,
  log: [],
  checks: null,
  checkReports: 0,
  headlines: [],
  planGoal: "",
  plan: [],
  spend: null,
  artifacts: [],
  renders: [],
  feedback: [],
  gate: "running",
  checkHistory: [],
  headlineHistory: {},
  finished: null,
});

/** Compact JSON of a tool input, cut at `n` characters (model.rs `short`). */
export function short(value: unknown, n: number): string {
  const s = JSON.stringify(value) ?? "null";
  const chars = Array.from(s);
  return chars.length > n ? `${chars.slice(0, n).join("")}...` : s;
}

/** Fold one event into a new state. The input state is not mutated. */
export function apply(state: RunState, ev: WrightEvent): RunState {
  switch (ev.type) {
    case "run_started":
      return { ...state, run: ev.run, domain: ev.domain, request: ev.request, executionKind: ev.execution_kind ?? null };
    case "agent_message":
      return { ...state, log: [...state.log, { agent: ev.agent, kind: "message", text: ev.text }] };
    case "tool_call":
      return {
        ...state,
        log: [...state.log, { agent: ev.agent, kind: "call", text: `${ev.tool} ${short(ev.input, 220)}` }],
      };
    case "tool_result":
      return {
        ...state,
        log: [...state.log, { agent: ev.agent, kind: "result", ok: ev.ok, text: `${ev.tool}: ${ev.summary}` }],
      };
    case "checks":
      return {
        ...state,
        checkReports: state.checkReports + 1,
        checks: { passed: ev.passed, failed: ev.failed, outcomes: ev.outcomes },
        checkHistory: [...state.checkHistory, { passed: ev.passed, failed: ev.failed }],
      };
    case "headlines": {
      const headlineHistory = { ...state.headlineHistory };
      for (const h of ev.items) headlineHistory[h.label] = [...(headlineHistory[h.label] ?? []), h.value];
      return { ...state, headlines: ev.items, headlineHistory };
    }
    case "render":
      return {
        ...state,
        renders: [...state.renders.filter((r) => r.view !== ev.view), { view: ev.view, path: ev.path }],
      };
    case "feedback":
      return {
        ...state,
        feedback: [...state.feedback, { view: ev.view, sketch: ev.sketch_png, note: ev.note }],
        log: [...state.log, { agent: "person", kind: "feedback", text: `on ${ev.view}: ${ev.note}` }],
      };
    case "spend":
      return {
        ...state,
        spend: {
          inputTokens: ev.input_tokens,
          outputTokens: ev.output_tokens,
          usd: ev.usd,
          budgetUsd: ev.budget_usd,
        },
      };
    case "artifact":
      return state.artifacts.some((a) => a.path === ev.path)
        ? state
        : { ...state, artifacts: [...state.artifacts, { label: ev.label, path: ev.path }] };
    case "run_finished":
      return { ...state, finished: { passed: ev.passed, objective: ev.objective, label: ev.objective_label } };
    case "plan":
      return { ...state, planGoal: ev.goal, plan: ev.tasks };
    case "control":
      return {
        ...state,
        gate: ev.state,
        log: [
          ...state.log,
          {
            agent: "person",
            kind: "notice",
            text: ev.note ? `${ev.action} -> ${ev.state}: ${ev.note}` : `${ev.action} -> ${ev.state}`,
          },
        ],
      };
  }
}

export function fold(events: Iterable<WrightEvent>): RunState {
  let s = initialState();
  for (const e of events) s = apply(s, e);
  return s;
}

/** Parse JSON lines; a line that is not an event is returned in `skipped`. */
export function parseLines(lines: string[]): { events: WrightEvent[]; skipped: number } {
  const events: WrightEvent[] = [];
  let skipped = 0;
  for (const line of lines) {
    if (!line.trim()) continue;
    try {
      const v = JSON.parse(line);
      if (v && typeof v.type === "string") events.push(v as WrightEvent);
      else skipped++;
    } catch {
      skipped++;
    }
  }
  return { events, skipped };
}

// ---- derived views -------------------------------------------------------

export type CheckVerdict = "pass" | "fail" | "missing";

export interface CheckRow {
  name: string;
  verdict: CheckVerdict;
  /** null when the check has no measurement. */
  margin: number | null;
  unit: string;
  source: string;
  detail: string;
}

/**
 * Outcome::passed in wright-core/src/check.rs: a finite margin >= 0 passes;
 * a negative one fails; Missing evidence (or a non-finite margin) is its own
 * verdict, and fails the run, never passes by omission.
 */
export function checkRow(o: Outcome): CheckRow {
  if ("Margin" in o.evidence) {
    const { value, unit } = o.evidence.Margin;
    const finite = typeof value === "number" && Number.isFinite(value);
    return {
      name: o.name,
      verdict: !finite ? "missing" : value >= 0 ? "pass" : "fail",
      margin: finite ? value : null,
      unit,
      source: o.source,
      detail: o.detail,
    };
  }
  return { name: o.name, verdict: "missing", margin: null, unit: "", source: o.source, detail: o.evidence.Missing.why };
}

export function planCounts(plan: PlanTask[]): Record<PlanTask["status"], number> {
  const c = { pending: 0, running: 0, done: 0, blocked: 0, stale: 0 };
  for (const t of plan) c[t.status]++;
  return c;
}

export function agentsIn(log: LogLine[]): string[] {
  return Array.from(new Set(log.map((l) => l.agent)));
}

/** Does a headline meet its limit? null for info or no limit. */
export function headlineMet(h: Headline): boolean | null {
  if (h.limit === null || h.bound === "info") return null;
  return h.bound === "at_least" ? h.value >= h.limit : h.value <= h.limit;
}
