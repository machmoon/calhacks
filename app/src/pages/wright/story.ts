// What a run shows, derived from its state: which artifacts are the PCB and
// CAD hand-offs, the steps of the story (prompt -> plan -> schematic ->
// board -> KiCad checks -> 3D -> airframe for a board run), and one plain
// sentence per failing check. Pure, so it is tested (story.test.ts).

import { checkRow, type CheckRow, type RunState } from "./reducer";
import type { Outcome } from "./types";

type Artifact = { label: string; path: string };

const ext = (p: string) => p.toLowerCase().split(".").pop() ?? "";
const base = (p: string) => p.split("/").pop() ?? p;

export interface Bundle {
  pcb?: string;
  sch?: string;
  pro?: string;
  /** design.json beside the board: netlist, placement, judge, route. */
  design?: string;
  /** kicad-verdicts.json: erc / drc / parity counts. */
  verdicts?: string;
  /** Claude's placement groups, when the run wrote them. */
  groups?: string;
  /** The board STEP (kicad-cli pcb export step). */
  boardStep?: string;
  /** The vehicle in FreeCAD, and a mesh of it to draw. */
  fcstd?: string;
  cadStep?: string;
  assemblyMesh?: string;
}

/** Sort artifacts into the hand-off files the page needs. Later wins. */
export function bundle(items: Artifact[]): Bundle {
  const b: Bundle = {};
  for (const { label, path } of items) {
    const e = ext(path);
    const name = base(path).toLowerCase();
    const both = `${label} ${path}`.toLowerCase();
    if (e === "kicad_pcb") b.pcb = path;
    else if (e === "kicad_sch") b.sch = path;
    else if (e === "kicad_pro") b.pro = path;
    else if (name === "kicad-verdicts.json") b.verdicts = path;
    else if (name === "design.json" && /kicad|netlist|placement|board|route/.test(both)) b.design = path;
    else if (/groups/.test(name) && e === "json") b.groups = path;
    else if (e === "fcstd") b.fcstd = path;
    else if (e === "step" || e === "stp") {
      if (/board|pcb/.test(name)) b.boardStep = path;
      else b.cadStep = path;
    } else if ((e === "stl" || e === "glb") && /assembl|airframe|vtol/.test(both)) b.assemblyMesh = path;
  }
  return b;
}

export type StepId =
  | "prompt" | "plan" | "schematic" | "board" | "kicad" | "board3d" | "airframe"
  | "model3d" | "renders" | "checks";
export type StepStatus = "done" | "active" | "failed" | "pending" | "skipped";

export interface Step {
  id: StepId;
  label: string;
  status: StepStatus;
  /** A few words under the label: a count, a verdict. */
  hint: string;
}

const KICAD_CHECKS = ["erc", "drc", "parity"];

export const isBoardRun = (s: RunState, b: Bundle) =>
  Boolean(b.pcb || b.sch) || s.domain === "electronics";

/**
 * The steps of the story. A step is done when its evidence exists, failed
 * when its checks fail, active for the first unfinished step of a live run,
 * skipped when later steps finished without it (no plan in an agentless
 * run), pending otherwise. Each step is also a view of the run.
 */
export function steps(s: RunState, b: Bundle, live: boolean): Step[] {
  const rows = (s.checks?.outcomes ?? []).map(checkRow);
  const byName = (n: string) => rows.find((r) => r.name === n);
  const kicad = KICAD_CHECKS.map(byName).filter((r): r is CheckRow => !!r);
  const kicadFailed = kicad.some((r) => r.verdict !== "pass");
  const plan = s.plan.length > 0 || Boolean(b.groups);
  const failing = rows.filter((r) => r.verdict !== "pass").length;
  const checksHint = rows.length ? (failing ? `${failing} of ${rows.length} failing` : `${rows.length} pass`) : "";

  const list: { id: StepId; label: string; hint: string; done: boolean; failed: boolean }[] = [];
  const add = (id: StepId, label: string, done: boolean, hint = "", failed = false) => list.push({ id, label, done, hint, failed });

  add("prompt", "Prompt", Boolean(s.run), s.domain ? s.domain : "");
  add("plan", "Plan", plan, s.plan.length ? `${s.plan.filter((t) => t.status === "done").length}/${s.plan.length} tasks` : b.groups ? "groups" : "");
  if (isBoardRun(s, b)) {
    add("schematic", "Schematic", Boolean(b.sch));
    add("board", "Board", Boolean(b.pcb), "placed, routed");
    add("kicad", "KiCad checks", kicad.length > 0, kicad.length ? (kicadFailed ? "failing" : "ERC DRC parity") : "", kicadFailed);
    add("board3d", "3D board", Boolean(b.pcb || b.boardStep));
    add("airframe", "In the airframe", Boolean(b.fcstd || b.assemblyMesh || b.cadStep), byName("board_fits_bay")?.verdict === "pass" ? "fits the bay" : "");
  } else {
    add("model3d", "3D model", s.artifacts.some((a) => ["stl", "glb"].includes(ext(a.path))));
    add("renders", "Renders", s.renders.length > 0, s.renders.length ? `${s.renders.length} views` : "");
    add("checks", "Checks", rows.length > 0, checksHint, failing > 0);
  }

  const lastDone = list.reduce((i, x, k) => (x.done ? k : i), -1);
  let activeGiven = false;
  return list.map((x, k) => {
    let status: StepStatus;
    if (x.done) status = x.failed ? "failed" : "done";
    else if (k < lastDone) status = "skipped";
    else if (live && !activeGiven) { status = "active"; activeGiven = true; }
    else status = "pending";
    return { id: x.id, label: x.label, hint: x.hint, status };
  });
}

/** `trace_ampacity` -> `Trace ampacity`. */
export const humanName = (n: string) => {
  const t = n.replace(/[_-]+/g, " ").trim();
  return t ? t[0].toUpperCase() + t.slice(1) : n;
};

const fmt = (v: number) => {
  const a = Math.abs(v);
  return a >= 100 ? a.toFixed(0) : a >= 1 ? a.toFixed(2).replace(/\.?0+$/, "") : a.toPrecision(2);
};

/**
 * One plain sentence for a check that is not passing: what is short and by
 * how much, then the checker's own detail (cut at the first `;`, which is
 * where the checkers start listing every instance).
 */
export function failureSentence(r: CheckRow): string {
  const detail = r.detail.split(/;\s/)[0].replace(/^missing:\s*/i, "").trim();
  if (r.verdict === "missing") return `${humanName(r.name)} was not measured: ${detail || "no evidence recorded"}.`.replace(/\.\.$/, ".");
  if (r.verdict === "fail") {
    const by = r.margin === null ? "" : ` by ${fmt(r.margin)}${r.unit ? ` ${r.unit}` : ""}`;
    return `${humanName(r.name)} is short${by}${detail ? `: ${detail}` : ""}.`.replace(/\.\.$/, ".");
  }
  return detail || humanName(r.name);
}

export const rowsOf = (outcomes: Outcome[]) => outcomes.map(checkRow);

// ---- design.json and groups -------------------------------------------------

export interface Group {
  name: string;
  members: string[];
  reason: string;
}

/**
 * Placement groups: a groups file (`{groups: [{name, refs|members, reason}]}`,
 * the PLACEMENT-GROUPS shape Wright's `simple/grouping.py` used), else the
 * placer's own notes in design.json (`group decap:U1: U1 C1 C2`).
 */
export function parseGroups(groupsFile: unknown, design: unknown): Group[] {
  const g = (groupsFile as { groups?: unknown })?.groups;
  if (Array.isArray(g)) {
    return g.map((x: Record<string, unknown>, i) => ({
      name: String(x.name ?? x.id ?? `group ${i + 1}`),
      members: (Array.isArray(x.refs) ? x.refs : Array.isArray(x.members) ? x.members : []).map(String),
      reason: String(x.reason ?? x.why ?? ""),
    }));
  }
  const notes = (design as { place?: { notes?: unknown } })?.place?.notes;
  if (!Array.isArray(notes)) return [];
  const fitted = notes.map(String).find((n) => /^template .* fitted:/.test(n));
  return notes
    .map(String)
    .map((n) => /^group\s+([^:]+(?::[A-Z]+\d+)?):\s*(.+)$/.exec(n))
    .filter((m): m is RegExpExecArray => !!m)
    .map((m) => ({
      name: m[1],
      members: m[2].split(/\s+/).filter(Boolean),
      reason: fitted ? `The placer's template: ${fitted.replace(/^template /, "")}` : "",
    }));
}

export interface Candidate {
  seed: number;
  vias: number;
  copperMm: number;
  blocking: number;
  winner: boolean;
}

/** The placement candidates the judge routed and ranked, winner marked. */
export function parseJudge(design: unknown): { candidates: Candidate[]; seeds: number | null; seconds: number | null } | null {
  const j = (design as { judge?: Record<string, unknown> })?.judge;
  if (!j || !Array.isArray(j.ranking)) return null;
  const kept = typeof j.kept === "number" ? j.kept : null;
  const ranking = j.ranking as { seed: number; score: Record<string, number> }[];
  const winnerSeed = kept ?? ranking[0]?.seed;
  return {
    candidates: ranking.map((r) => ({
      seed: r.seed,
      vias: r.score?.vias ?? 0,
      copperMm: r.score?.copper_mm ?? 0,
      blocking: (r.score?.blocking ?? 0) + (r.score?.joins_needed ?? 0),
      winner: r.seed === winnerSeed,
    })),
    seeds: typeof j.seeds === "number" ? j.seeds : null,
    seconds: typeof j.seconds === "number" ? j.seconds : null,
  };
}

export interface KicadVerdict {
  check: string;
  ran: boolean;
  errors: number;
  warnings: number;
  unconnected: number;
  toolVersion: string;
  details: string[];
}

export function parseVerdicts(v: unknown): KicadVerdict[] {
  if (!Array.isArray(v)) return [];
  return v.map((x: Record<string, unknown>) => ({
    check: String(x.check ?? ""),
    ran: Boolean(x.ran),
    errors: Number(x.errors ?? 0),
    warnings: Number(x.warnings ?? 0),
    unconnected: Number(x.unconnected ?? 0),
    toolVersion: String(x.tool_version ?? ""),
    details: Array.isArray(x.details) ? x.details.map(String) : [],
  }));
}
