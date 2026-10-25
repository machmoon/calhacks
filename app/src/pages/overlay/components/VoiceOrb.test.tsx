// @vitest-environment jsdom
//
// The orb, and the rule that outranks how it looks: it must never animate as
// though listening when the microphone is not open.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: vi.fn() }));
// The mouth is polled for "am I talking"; the real one reaches for
// speechSynthesis at construction, which jsdom does not have.
vi.mock("@/lib/speech", () => ({
  speaker: { isSpeaking: () => false, speak: async () => {}, stop: () => {} },
}));

import type { EngineHealth } from "@/hooks";
import { setPushToTalk } from "@/hooks/useMicLevel";
import { orbHealth, orbIsHot, orbState, orbTitle, VoiceOrb } from "./VoiceOrb";

const base = {
  engineOk: true,
  engineKnown: true,
  recording: false,
  thinking: false,
  speaking: false,
};

const engine = (over: Partial<EngineHealth> = {}): EngineHealth =>
  ({
    ok: true,
    checking: false,
    lastCheckedAt: 1,
    detail: null,
    recheck: vi.fn(),
    ...over,
  }) as unknown as EngineHealth;

afterEach(() => {
  cleanup();
  setPushToTalk(false);
});

describe("orbState", () => {
  it("cannot be driven by a switch, because no switch is an input", () => {
    expect(Object.keys(base)).not.toContain("enabled");
    expect(orbState({ ...base, recording: false })).not.toBe("recording");
    expect(orbState({ ...base, recording: true })).toBe("recording");
  });

  it("puts the finger on the button above the machine's own work", () => {
    expect(orbState({ ...base, recording: true, thinking: true, speaking: true })).toBe(
      "recording"
    );
    expect(orbState({ ...base, thinking: true, speaking: true })).toBe("speaking");
    expect(orbState({ ...base, thinking: true })).toBe("thinking");
  });

  it("falls back to the engine dot it replaced when there is nothing else to say", () => {
    expect(orbState(base)).toBe("idle");
    expect(orbState({ ...base, engineOk: false })).toBe("fault");
    expect(orbState({ ...base, engineKnown: false })).toBe("unknown");
  });

  it("keeps engine health beside the voice state, never folded into it", () => {
    const recordingWhileDown = { ...base, recording: true, engineOk: false };
    expect(orbState(recordingWhileDown)).toBe("recording");
    expect(orbHealth(recordingWhileDown)).toBe("down");
    expect(orbHealth({ engineOk: true, engineKnown: false })).toBe("unknown");
  });

  it("says something different in every state", () => {
    const states = ["fault", "unknown", "idle", "recording", "thinking", "speaking"] as const;
    const said = states.map((s) => orbTitle(s, "up", "http://127.0.0.1:8081"));
    expect(new Set(said).size).toBe(states.length);
    expect(orbTitle("recording", "down", "http://x", "connection refused")).toContain(
      "connection refused"
    );
  });

  it("knows which states mean the microphone is open", () => {
    expect(orbIsHot("recording")).toBe(true);
    expect(orbIsHot("idle")).toBe(false);
    expect(orbIsHot("thinking")).toBe(false);
    expect(orbIsHot("speaking")).toBe(false);
  });
});

describe("VoiceOrb", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    });
  afterEach(() => vi.useRealTimers());

  const orb = () => screen.getByTestId("voice-orb");

  it("keeps the engine dot's job: the same handle, and click still re-checks", () => {
    const recheck = vi.fn();
    render(<VoiceOrb engine={engine({ recheck })} baseUrl="http://127.0.0.1:8081" />);
    const button = screen.getByTestId("engine-status");
    expect(button.getAttribute("data-online")).toBe("true");
    fireEvent.click(button);
    expect(recheck).toHaveBeenCalled();
  });

  it("marks an unreachable engine in shape as well as colour", () => {
    render(
      <VoiceOrb engine={engine({ ok: false, detail: "refused" })} baseUrl="http://x" />
    );
    expect(orb().getAttribute("data-state")).toBe("fault");
    expect(orb().getAttribute("data-health")).toBe("down");
    // Two non-colour carriers: a broken ring and a slash through the core.
    expect(screen.getByTestId("orb-health-ring")).toBeTruthy();
    expect(screen.getByTestId("orb-fault-slash")).toBeTruthy();
  });

  it("still shows the engine is down while it is recording", () => {
    render(<VoiceOrb engine={engine({ ok: false })} baseUrl="http://x" recording />);
    expect(orb().getAttribute("data-state")).toBe("recording");
    expect(screen.getByTestId("orb-health-ring")).toBeTruthy();
  });

  it("holds still when the system asks for less motion", () => {
    render(<VoiceOrb engine={engine()} baseUrl="http://x" recording reducedMotion />);
    expect(orb().getAttribute("data-motion")).toBe("still");
    // The state is still legible: it is the motion that goes, not the answer.
    expect(orb().getAttribute("data-state")).toBe("recording");
  });

  it("shows work and speech as their own states", () => {
    const { rerender } = render(
      <VoiceOrb engine={engine()} baseUrl="http://x" thinking />
    );
    expect(orb().getAttribute("data-state")).toBe("thinking");
    expect(screen.getByTestId("orb-arc")).toBeTruthy();
    rerender(
      <VoiceOrb engine={engine()} baseUrl="http://x" isSpeaking={() => true} />
    );
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(orb().getAttribute("data-state")).toBe("speaking");
  });

  it("holds completely still when the microphone is closed", () => {
    // A green core breathing at a shut microphone is one glance away from
    // the lie. `idle` therefore has no animation at all, and the CSS is what
    // this asserts, because there is nothing else to look at.
    render(<VoiceOrb engine={engine()} baseUrl="http://x" />);
    expect(orb().getAttribute("data-state")).toBe("idle");
    const css = orb().querySelector("style")?.textContent ?? "";
    expect(css).not.toMatch(/\[data-state="idle"\][^{]*\{[^}]*animation/);
    expect(css).toMatch(/\[data-state="recording"\][^{]*\{[^}]*animation/);
  });

  it("shows the microphone open while push-to-talk holds it", () => {
    render(<VoiceOrb engine={engine()} baseUrl="http://x" />);
    expect(orb().getAttribute("data-state")).toBe("idle");
    act(() => setPushToTalk(true));
    expect(orb().getAttribute("data-state")).toBe("recording");
    act(() => setPushToTalk(false));
    expect(orb().getAttribute("data-state")).toBe("idle");
  });
});
