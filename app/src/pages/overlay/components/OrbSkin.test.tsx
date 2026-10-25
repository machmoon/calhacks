// @vitest-environment jsdom
//
// The orb skin's two guarantees, both of which outrank how it looks:
//
//  1. At rest it is an orb and nothing else — no field, no run control.
//  2. Nothing in it may claim the microphone is open when it is not, and the
//     paid control is absent rather than greyed when pressing it would not be
//     valid.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: vi.fn() }));
// The mouth is polled for "am I talking"; the real one reaches for
// speechSynthesis at construction, which jsdom does not have.
vi.mock("@/lib/speech", () => ({
  speaker: { isSpeaking: () => false, speak: async () => {}, stop: () => {} },
}));

import type { EngineHealth } from "@/hooks";
import { setPushToTalk } from "@/hooks/useMicLevel";
import { OrbSkin, orbCaption, submittable, type OrbSkinProps } from "./OrbSkin";

const engine = (over: Partial<EngineHealth> = {}): EngineHealth =>
  ({
    baseUrl: "http://127.0.0.1:8081",
    ok: true,
    checking: false,
    lastCheckedAt: 1,
    detail: "",
    recheck: vi.fn(),
    ...over,
  }) as EngineHealth;

function draw(over: Partial<OrbSkinProps> = {}) {
  const props: OrbSkinProps = {
    engine: engine(),
    baseUrl: "http://127.0.0.1:8081",
    value: "",
    onChange: vi.fn(),
    onSubmit: vi.fn(),
    // Every test states motion explicitly: jsdom has no matchMedia, so the
    // default would silently be "animated" and the reduced-motion test would
    // be the only one that ever exercised the other branch.
    reducedMotion: false,
    isSpeaking: () => false,
    ...over,
  };
  return { props, ...render(<OrbSkin {...props} />) };
}

const gate = {
  value: "a 3.3V LDO",
  canStart: true,
  busy: false,
  hidden: false,
  engineOk: true,
  engineKnown: true,
};

afterEach(() => {
  cleanup();
  setPushToTalk(false);
});

describe("the resting frame", () => {
  it("is an orb and nothing else", () => {
    // The catalogue row, as an assertion: "Collapses to a listening orb; the
    // field appears when you speak or click." An orb plus a field plus a
    // toolbar is the design missed.
    draw();
    expect(screen.getByTestId("orb-skin-orb")).toBeTruthy();
    expect(screen.queryByTestId("orb-skin-field")).toBeNull();
    expect(screen.queryByTestId("orb-skin-input")).toBeNull();
    expect(screen.queryByTestId("orb-submit")).toBeNull();
  });

  it("says in words that a click is what opens the field", () => {
    draw();
    expect(screen.getByTestId("orb-skin-caption").textContent).toBe("Click to type.");
  });

  it("still carries the engine, because the orb took the engine dot's job", () => {
    draw({ engine: engine({ ok: false, lastCheckedAt: 2, detail: "connection refused" }) });
    expect(screen.getByTestId("engine-status").getAttribute("data-online")).toBe("false");
  });
});

describe("how the field appears", () => {
  it("opens on a click on the orb", () => {
    draw();
    fireEvent.click(screen.getByTestId("orb-skin-orb"));
    expect(screen.getByTestId("orb-skin-input")).toBeTruthy();
  });

  it("stays open while there is a draft, so typed text is never hidden by a collapse", () => {
    draw({ value: "a 3.3V LDO" });
    expect(screen.getByTestId("orb-skin-input")).toBeTruthy();
  });

  it("goes back to being an orb on Escape", () => {
    draw();
    fireEvent.click(screen.getByTestId("orb-skin-orb"));
    fireEvent.keyDown(screen.getByTestId("orb-skin-input"), { key: "Escape" });
    expect(screen.queryByTestId("orb-skin-input")).toBeNull();
    expect(screen.getByTestId("orb-skin-orb")).toBeTruthy();
  });
});

describe("the caption", () => {
  it("says what a click does, or that a run is under way", () => {
    expect(orbCaption(false)).toBe("Click to type.");
    expect(orbCaption(true)).toBe("Working on it.");
  });
});

describe("motion", () => {
  it("stops animating when the OS asks for less motion", () => {
    draw({ reducedMotion: true });
    expect(screen.getByTestId("orb-skin").getAttribute("data-motion")).toBe("still");
    expect(screen.getByTestId("voice-orb").getAttribute("data-motion")).toBe("still");
  });
});

describe("the control that spends money", () => {
  it("does not exist with nothing typed", () => {
    expect(submittable({ ...gate, value: "   " })).toBe(false);
    draw({ open: true, value: "" });
    expect(screen.queryByTestId("orb-submit")).toBeNull();
  });

  it("does not exist before the first engine probe has landed", () => {
    // Three engine states, not two: before a probe nobody knows whether a run
    // would reach anything, and offering to spend on a guess flattens the
    // unknown state into "up".
    expect(submittable({ ...gate, engineKnown: false })).toBe(false);
    draw({ open: true, value: "a 3.3V LDO", engine: engine({ lastCheckedAt: null }) });
    expect(screen.queryByTestId("orb-submit")).toBeNull();
  });

  it("does not exist while the engine is unreachable", () => {
    expect(submittable({ ...gate, engineOk: false })).toBe(false);
    draw({ open: true, value: "a 3.3V LDO", engine: engine({ ok: false }) });
    expect(screen.queryByTestId("orb-submit")).toBeNull();
  });

  it("does not exist while a run is already in flight, so one prompt cannot fire twice", () => {
    expect(submittable({ ...gate, busy: true })).toBe(false);
    draw({ open: true, value: "a 3.3V LDO", busy: true });
    expect(screen.queryByTestId("orb-submit")).toBeNull();
  });

  it("does not exist while the overlay is hidden or the run guard is closed", () => {
    expect(submittable({ ...gate, hidden: true })).toBe(false);
    expect(submittable({ ...gate, canStart: false })).toBe(false);
  });

  it("is absent rather than greyed. No disabled run control is ever rendered", () => {
    // "Absent, not greyed" is the rule; a disabled button is an invitation
    // with the reason hidden inside an attribute nobody can read.
    draw({ open: true, value: "a 3.3V LDO", engine: engine({ ok: false }) });
    for (const button of screen.queryAllByRole("button")) {
      expect(button.getAttribute("data-testid")).not.toBe("orb-submit");
    }
    expect(document.querySelector("[data-testid='orb-submit'][disabled]")).toBeNull();
  });

  it("appears, states its price, and is the only path to a run", () => {
    const { props } = draw({ open: true, value: "a 3.3V LDO" });
    const submit = screen.getByTestId("orb-submit");
    expect(submit.getAttribute("title")).toContain("costs money");
    fireEvent.click(submit);
    expect(props.onSubmit).toHaveBeenCalledTimes(1);
  });

  it("lets Enter start a run only when the same gate is open", () => {
    const { props } = draw({ open: true, value: "a 3.3V LDO", engine: engine({ ok: false }) });
    fireEvent.keyDown(screen.getByTestId("orb-skin-input"), { key: "Enter" });
    expect(props.onSubmit).not.toHaveBeenCalled();
    cleanup();

    const ok = draw({ open: true, value: "a 3.3V LDO" });
    fireEvent.keyDown(screen.getByTestId("orb-skin-input"), { key: "Enter" });
    expect(ok.props.onSubmit).toHaveBeenCalledTimes(1);
  });
});

describe("the field's own controls", () => {
  it("hands typing straight back to the caller, which owns the draft", () => {
    const { props } = draw({ open: true, value: "a" });
    fireEvent.change(screen.getByTestId("orb-skin-input"), { target: { value: "ab" } });
    expect(props.onChange).toHaveBeenCalledWith("ab");
  });
});
