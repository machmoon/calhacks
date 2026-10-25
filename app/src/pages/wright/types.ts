// The event vocabulary of the `wright` CLI. Mirrors
// wright/crates/wright-core/src/event.rs (serde tag "type", snake_case) and
// wright-core/src/check.rs (Outcome, Evidence, Source), which serialise
// enums externally tagged: {"Margin": {value, unit}} / {"Missing": {why}}.

export type Domain = "aircraft" | "rocket" | "balloon";

export type PlanStatus = "pending" | "running" | "done" | "blocked" | "stale";

export interface PlanTask {
  id: string;
  title: string;
  owner: string;
  inputs: string[];
  checks: string[];
  status: PlanStatus;
  note: string;
}

export type Evidence =
  | { Margin: { value: number | null; unit: string } }
  | { Missing: { why: string } };

export type CheckSource =
  | "Physics"
  | "Geometry"
  | "Rating"
  | "Requirement"
  | "Slicer"
  | "Simulation";

export interface Outcome {
  name: string;
  evidence: Evidence;
  detail: string;
  source: CheckSource;
}

export type Bound = "at_least" | "at_most" | "info";

export interface Headline {
  label: string;
  value: number;
  unit: string;
  limit: number | null;
  bound: Bound;
  check: string | null;
}

export type WrightEvent =
  | { type: "run_started"; run: string; domain: string; request: string; execution_kind?: string }
  | { type: "agent_message"; run: string; agent: string; text: string }
  | { type: "tool_call"; run: string; agent: string; tool: string; input: unknown }
  | { type: "tool_result"; run: string; agent: string; tool: string; ok: boolean; summary: string }
  | { type: "checks"; run: string; passed: number; failed: number; outcomes: Outcome[] }
  | { type: "headlines"; run: string; items: Headline[] }
  | { type: "render"; run: string; view: string; path: string }
  | { type: "feedback"; run: string; view: string; sketch_png: string; note: string }
  | { type: "spend"; run: string; input_tokens: number; output_tokens: number; usd: number; budget_usd: number }
  | { type: "artifact"; run: string; label: string; path: string }
  | { type: "run_finished"; run: string; passed: boolean; objective: number; objective_label: string }
  | { type: "plan"; run: string; goal: string; tasks: PlanTask[] }
  | { type: "control"; run: string; action: string; state: string; note: string };

export interface RunListing {
  name: string;
  path: string;
  kind: "demo" | "run";
  domain: string;
  request: string;
  events: number;
  modified_ms: number;
}

export interface TailResult {
  lines: string[];
  next: number;
  status: { state?: string; waiting_on?: string | null } | null;
  running: boolean;
  /** One plain sentence (cause and fix) when the child exited with an error. */
  failure: string | null;
}

export interface EnvItem {
  name: string;
  ok: boolean;
  detail: string;
  fix: string;
  /** Blocks Run when not ok. */
  required: boolean;
}

export interface EnvStatus {
  items: EnvItem[];
  ready: boolean;
}

export interface WrightStatus {
  running: boolean;
  pid: number | null;
  run_dir: string | null;
  log: string | null;
}
