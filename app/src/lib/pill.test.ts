import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import { runWord } from "./pill";

describe("runWord", () => {
  const up = { status: "idle", stepsStatus: "idle", engineOk: true, engineSettled: true };

  it("does not guess before the first probe lands", () => {
    expect(runWord({ ...up, engineSettled: false, engineOk: false })).toBe(
      "Checking the engine"
    );
  });

  it("puts a dead engine above everything else, because nothing else can be true", () => {
    expect(runWord({ ...up, engineOk: false, stepsStatus: "waiting" })).toBe(
      "Engine down"
    );
  });

  it("names each run state", () => {
    expect(runWord({ ...up, stepsStatus: "running" })).toBe("Working");
    expect(runWord({ ...up, status: "running" })).toBe("Working");
    expect(runWord({ ...up, stepsStatus: "waiting" })).toBe("Waiting for you");
    expect(runWord({ ...up, stepsStatus: "error" })).toBe("A stage failed");
    expect(runWord({ ...up, status: "done" })).toBe("Run finished");
  });

  it("prints an idle state rather than leaving the pill blank", () => {
    expect(runWord(up)).toBe("Nothing running");
  });
});
