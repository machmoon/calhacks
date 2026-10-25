// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ChecksTable } from "./components/ChecksTable";
import { pickModel } from "./components/ModelPanel";
import { PlanPanel } from "./components/PlanPanel";

describe("pickModel", () => {
  it("prefers an assembly GLB, then the assembly STL, then part STLs without jigs", () => {
    expect(pickModel([{ label: "a", path: "x/assembly.stl" }, { label: "b", path: "x/assembly.glb" }])).toEqual({ kind: "glb", paths: ["x/assembly.glb"] });
    expect(pickModel([{ label: "p", path: "p.stl" }, { label: "assembly (STL, mm)", path: "cad/assembly.stl" }])).toEqual({ kind: "stl", paths: ["cad/assembly.stl"] });
    expect(pickModel([{ label: "nose", path: "nose.stl" }, { label: "fin alignment jig", path: "fin_alignment_jig.stl" }, { label: "bom", path: "bom.csv" }])).toEqual({ kind: "stl", paths: ["nose.stl"] });
    expect(pickModel([{ label: "bom", path: "bom.csv" }])).toBeNull();
  });
});

describe("ChecksTable", () => {
  it("lists failures and missing evidence before passes", () => {
    render(
      <ChecksTable
        reports={2}
        outcomes={[
          { name: "good", evidence: { Margin: { value: 1.5, unit: "A" } }, detail: "", source: "Physics" },
          { name: "bad", evidence: { Margin: { value: -0.5, unit: "kg" } }, detail: "", source: "Rating" },
          { name: "gone", evidence: { Missing: { why: "no solver" } }, detail: "", source: "Simulation" },
        ]}
      />
    );
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows.map((r) => r.getAttribute("data-verdict"))).toEqual(["fail", "missing", "pass"]);
    expect(screen.getByText("Gone was not measured: no solver.")).toBeTruthy();
  });
});

describe("PlanPanel", () => {
  it("marks a stale task distinctly from done", () => {
    const t = (id: string, status: "done" | "stale") => ({ id, title: id, owner: "o", inputs: [], checks: [], status, note: "" });
    render(<PlanPanel goal="g" tasks={[t("a", "done"), t("b", "stale")]} />);
    const items = screen.getAllByRole("listitem");
    expect(items.map((i) => i.getAttribute("data-status"))).toEqual(["done", "stale"]);
    expect(screen.getByText("1/2 done · 1 stale")).toBeTruthy();
  });
});
