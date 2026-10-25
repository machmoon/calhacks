import { describe, expect, it } from "vitest";
import { fold } from "./reducer";
import { bundle, failureSentence, parseGroups, parseJudge, steps } from "./story";
import { checkRow } from "./reducer";
import type { WrightEvent } from "./types";

const art = (label: string, path: string): WrightEvent => ({ type: "artifact", run: "r", label, path });

describe("bundle", () => {
  it("finds the KiCad and CAD hand-off files", () => {
    const s = fold([
      art("KiCad board", "o/board.kicad_pcb"),
      art("KiCad schematic", "o/board.kicad_sch"),
      art("Design (netlist, placement, route)", "o/design.json"),
      art("verdicts", "o/kicad-verdicts.json"),
      art("Board STEP", "o/cad/board.step"),
      art("VTOL airframe", "v/assembly.FCStd"),
      art("VTOL assembly preview", "v/assembly-preview.stl"),
      art("airframe", "v/airframe.step"),
    ]);
    expect(bundle(s.artifacts)).toEqual({
      pcb: "o/board.kicad_pcb",
      sch: "o/board.kicad_sch",
      design: "o/design.json",
      verdicts: "o/kicad-verdicts.json",
      boardStep: "o/cad/board.step",
      fcstd: "v/assembly.FCStd",
      assemblyMesh: "v/assembly-preview.stl",
      cadStep: "v/airframe.step",
    });
  });
});

describe("steps", () => {
  it("tells the board story and skips a plan an agentless run never wrote", () => {
    const ev: WrightEvent[] = [
      { type: "run_started", run: "r", domain: "electronics", request: "servo board" },
      art("s", "o/board.kicad_sch"),
      art("b", "o/board.kicad_pcb"),
      { type: "checks", run: "r", passed: 2, failed: 1, outcomes: [
        { name: "erc", evidence: { Margin: { value: 0, unit: "errors" } }, detail: "", source: "Geometry" },
        { name: "drc", evidence: { Margin: { value: -2, unit: "errors" } }, detail: "", source: "Geometry" },
        { name: "parity", evidence: { Margin: { value: 0, unit: "errors" } }, detail: "", source: "Geometry" },
      ] },
    ];
    const s = fold(ev);
    const st = steps(s, bundle(s.artifacts), true);
    expect(st.map((x) => `${x.id}:${x.status}`)).toEqual([
      "prompt:done", "plan:skipped", "schematic:done", "board:done", "kicad:failed", "board3d:done", "airframe:active",
    ]);
  });

  it("uses the vehicle steps when there is no board", () => {
    const s = fold([{ type: "run_started", run: "r", domain: "rocket", request: "x" }]);
    expect(steps(s, bundle([]), false).map((x) => x.id)).toEqual(["prompt", "plan", "model3d", "renders", "checks"]);
  });
});

describe("failureSentence", () => {
  it("says what is short, by how much, and the first clause of the detail", () => {
    const r = checkRow({ name: "trace_ampacity", evidence: { Margin: { value: -0.647, unit: "A" } }, detail: "tightest GND: 1.5 mm track carries 4.35 A; more", source: "Physics" });
    expect(failureSentence(r)).toBe("Trace ampacity is short by 0.65 A: tightest GND: 1.5 mm track carries 4.35 A.");
    const m = checkRow({ name: "budget", evidence: { Missing: { why: "missing: 12 of 12 BOM lines unpriced" } }, detail: "", source: "Requirement" });
    expect(failureSentence(m)).toBe("Budget was not measured: 12 of 12 BOM lines unpriced.");
  });
});

describe("groups and judge", () => {
  it("reads a groups file, else the placer's notes", () => {
    expect(parseGroups({ groups: [{ name: "buck", refs: ["U1", "L1"], reason: "hot loop" }] }, null)).toEqual([
      { name: "buck", members: ["U1", "L1"], reason: "hot loop" },
    ]);
    const g = parseGroups(null, { place: { notes: ["template U1 fitted: C1, L1", "group decap:U1: U1 C1 L1"] } });
    expect(g).toEqual([{ name: "decap:U1", members: ["U1", "C1", "L1"], reason: "The placer's template: U1 fitted: C1, L1" }]);
  });

  it("marks the kept candidate as the winner", () => {
    const j = parseJudge({ judge: { kept: 1, ranking: [
      { seed: 4, score: { vias: 4, copper_mm: 251, blocking: 0, joins_needed: 0 } },
      { seed: 1, score: { vias: 6, copper_mm: 211, blocking: 0, joins_needed: 0 } },
    ] } });
    expect(j?.candidates.map((c) => [c.seed, c.winner])).toEqual([[4, false], [1, true]]);
  });
});
