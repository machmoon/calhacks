import { describe, expect, it } from "vitest";
import { cadProjectBrief, nativeCadSubmission } from "./cad-project-request";
describe("CAD project routing", () => {
  it("routes the user's exact brief reference despite sentence punctuation", () => {
    expect(cadProjectBrief("Read and execute cad/projects/vtol-demonstrator/BUILD_PROMPT.txt. First resolve hardware."))
      .toBe("cad/projects/vtol-demonstrator/BUILD_PROMPT.txt");
  });
  it("does not redirect an actual VTOL circuit request", () => {
    expect(cadProjectBrief("Design a VTOL power-monitor PCB")).toBeNull();
  });
  it("accepts quoted project references", () => {
    expect(cadProjectBrief('Read `cad/projects/demo/BUILD_PROMPT.txt`.'))
      .toBe("cad/projects/demo/BUILD_PROMPT.txt");
  });
});

it("routes a PDF-only submission to native aircraft CAD without a PCB fallback", () => {
  expect(nativeCadSubmission("", ["pdf-123"])).toEqual({ domain: "aircraft", request: "Build the VTOL from the attached specification", attachmentIds: ["pdf-123"] });
  expect(nativeCadSubmission("Build this", ["pdf-123"])?.request).toBe("Build this");
  expect(nativeCadSubmission("An LED PCB")).toBeNull();
});
