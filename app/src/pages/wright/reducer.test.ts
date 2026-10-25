import { describe, expect, it } from "vitest";
import { agentsIn, apply, checkRow, fold, headlineMet, initialState, parseLines, planCounts, short } from "./reducer";
import type { WrightEvent } from "./types";

const run = "r1";

describe("wright reducer", () => {
  it("records the run header", () => {
    const s = apply(initialState(), { type: "run_started", run, domain: "balloon", request: "go" });
    expect([s.run, s.domain, s.request]).toEqual(["r1", "balloon", "go"]);
  });

  it("logs messages, calls and results in order and does not mutate", () => {
    const s0 = initialState();
    const s = fold([
      { type: "agent_message", run, agent: "orchestrator", text: "hello" },
      { type: "tool_call", run, agent: "aero", tool: "size", input: { a: 1 } },
      { type: "tool_result", run, agent: "aero", tool: "size", ok: false, summary: "boom" },
    ]);
    expect(s0.log).toHaveLength(0);
    expect(s.log.map((l) => l.kind)).toEqual(["message", "call", "result"]);
    expect(s.log[1].text).toBe('size {"a":1}');
    expect(s.log[2]).toMatchObject({ ok: false, text: "size: boom" });
  });

  it("truncates long tool inputs at 220 characters", () => {
    const text = short({ x: "y".repeat(500) }, 220);
    expect(Array.from(text)).toHaveLength(223);
    expect(text.endsWith("...")).toBe(true);
  });

  it("keeps the latest checks and counts the reports", () => {
    const base: WrightEvent = { type: "checks", run, passed: 1, failed: 0, outcomes: [] };
    const s = fold([base, { ...base, passed: 3, failed: 2 } as WrightEvent]);
    expect(s.checkReports).toBe(2);
    expect(s.checks).toMatchObject({ passed: 3, failed: 2 });
  });

  it("replaces a render of the same view and moves it last", () => {
    const s = fold([
      { type: "render", run, view: "top", path: "a.png" },
      { type: "render", run, view: "side", path: "b.png" },
      { type: "render", run, view: "top", path: "c.png" },
    ]);
    expect(s.renders).toEqual([
      { view: "side", path: "b.png" },
      { view: "top", path: "c.png" },
    ]);
  });

  it("deduplicates artifacts by path", () => {
    const a = { type: "artifact", run, label: "state", path: "x/state.json" } as const;
    const s = fold([a, { ...a, label: "again" }, { ...a, path: "x/bom.csv", label: "bom" }]);
    expect(s.artifacts.map((x) => x.label)).toEqual(["state", "bom"]);
  });

  it("replaces the plan whole and counts statuses, stale apart from done", () => {
    const task = (id: string, status: "done" | "stale" | "pending") => ({
      id, title: id, owner: "o", inputs: [], checks: [], status, note: "",
    });
    const s = fold([
      { type: "plan", run, goal: "g1", tasks: [task("a", "pending")] },
      { type: "plan", run, goal: "g2", tasks: [task("a", "done"), task("b", "stale")] },
    ]);
    expect(s.planGoal).toBe("g2");
    expect(planCounts(s.plan)).toMatchObject({ done: 1, stale: 1, pending: 0 });
  });

  it("tracks spend, headlines, finish and control gate", () => {
    const s = fold([
      { type: "spend", run, input_tokens: 10, output_tokens: 5, usd: 0.5, budget_usd: 5 },
      { type: "headlines", run, items: [{ label: "Mass", value: 1, unit: "kg", limit: null, bound: "info", check: null }] },
      { type: "control", run, action: "pause", state: "paused", note: "look" },
      { type: "run_finished", run, passed: true, objective: 3.2, objective_label: "kg" },
    ]);
    expect(s.spend).toEqual({ inputTokens: 10, outputTokens: 5, usd: 0.5, budgetUsd: 5 });
    expect(s.headlines).toHaveLength(1);
    expect(s.gate).toBe("paused");
    expect(s.log[s.log.length - 1]?.text).toBe("pause -> paused: look");
    expect(s.finished).toEqual({ passed: true, objective: 3.2, label: "kg" });
  });

  it("logs feedback as a person line", () => {
    const s = apply(initialState(), { type: "feedback", run, view: "top", sketch_png: "s.png", note: "fatter" });
    expect(s.log[0]).toMatchObject({ agent: "person", kind: "feedback", text: "on top: fatter" });
    expect(s.feedback).toEqual([{ view: "top", sketch: "s.png", note: "fatter" }]);
  });

  it("parses lines and counts the ones that are not events", () => {
    const { events, skipped } = parseLines(['{"type":"spend","run":"r","input_tokens":1,"output_tokens":1,"usd":0,"budget_usd":1}', "", "not json", '{"x":1}']);
    expect(events).toHaveLength(1);
    expect(skipped).toBe(2);
  });
});

describe("check rows", () => {
  const o = (evidence: any) => ({ name: "n", evidence, detail: "d", source: "Physics" as const });
  it("passes at zero margin, fails below, treats missing as its own verdict", () => {
    expect(checkRow(o({ Margin: { value: 0, unit: "A" } })).verdict).toBe("pass");
    expect(checkRow(o({ Margin: { value: -0.1, unit: "A" } })).verdict).toBe("fail");
    const m = checkRow(o({ Missing: { why: "no solver" } }));
    expect(m).toMatchObject({ verdict: "missing", margin: null, detail: "no solver" });
  });
  it("never passes a non-finite margin (JSON null for NaN)", () => {
    expect(checkRow(o({ Margin: { value: null, unit: "" } })).verdict).toBe("missing");
  });
});

describe("headlines and agents", () => {
  it("judges a headline against its bound", () => {
    const h = (bound: any, value: number, limit: number | null) => ({ label: "l", value, unit: "", limit, bound, check: null });
    expect(headlineMet(h("at_least", 5, 4))).toBe(true);
    expect(headlineMet(h("at_most", 5, 4))).toBe(false);
    expect(headlineMet(h("info", 5, null))).toBeNull();
  });
  it("lists agents once", () => {
    const ev: WrightEvent[] = [
      { type: "agent_message", run, agent: "a", text: "" },
      { type: "agent_message", run, agent: "b", text: "" },
      { type: "agent_message", run, agent: "a", text: "" },
    ];
    expect(agentsIn(fold(ev).log)).toEqual(["a", "b"]);
  });
});

it("preserves explicit native execution routing without exposing an implementation path", () => {
  const state = apply(initialState(), { type: "run_started", run, domain: "aircraft", request: "Build the attached VTOL", execution_kind: "native_freecad" });
  expect(state.executionKind).toBe("native_freecad");
  expect(state.request).toBe("Build the attached VTOL");
  expect(apply(initialState(), { type: "run_started", run, domain: "balloon", request: "old recording" }).executionKind).toBeNull();
});
