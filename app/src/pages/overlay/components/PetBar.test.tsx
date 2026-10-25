// @vitest-environment jsdom
//
// The folded pet: clicking it unfolds the bar, hovering shows Speak and
// Write, Speak drives the overlay's push-to-talk recorder (and its transcript
// goes to the draft, never to submit), Write's Enter hands the text up.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: vi.fn() }));

// The recorder is the same hook VoiceControl's push-to-talk uses; stubbed so
// what is under test is the pet's wiring to it, not MediaRecorder.
const voice = vi.hoisted(() => ({
  status: "idle" as "idle" | "recording" | "transcribing" | "error",
  start: vi.fn(async () => {}),
  stop: vi.fn(),
  cancel: vi.fn(),
  onTranscript: undefined as undefined | ((text: string) => void),
}));
vi.mock("@/hooks/useVoiceInput", () => ({
  useVoiceInput: (options: { onTranscript?: (text: string) => void }) => {
    voice.onTranscript = options.onTranscript;
    return {
      status: voice.status,
      transcript: "",
      error: "",
      elapsedS: 3,
      start: voice.start,
      stop: voice.stop,
      cancel: voice.cancel,
    };
  },
}));
const floor = vi.hoisted(() => ({ release: vi.fn(), take: vi.fn() }));
vi.mock("@/lib/speech", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/speech")>()),
  takeMicFloor: () => {
    floor.take();
    return floor.release;
  },
}));

import { PetBar } from "./PetBar";

const base = {
  pet: "corgi" as const,
  baseUrl: "http://127.0.0.1:8081",
  hidden: false,
  onExpand: () => {},
  onTranscript: () => {},
  onWrite: () => {},
  writing: false,
  onWritingChange: () => {},
  busy: false,
  awaitingApproval: false,
  status: "idle",
};

beforeEach(() => {
  voice.status = "idle";
  voice.start.mockClear();
  voice.stop.mockClear();
  voice.cancel.mockClear();
  floor.take.mockClear();
  floor.release.mockClear();
});
afterEach(() => cleanup());

describe("PetBar", () => {
  it("clicking the pet unfolds the bar", () => {
    const onExpand = vi.fn();
    render(<PetBar {...base} onExpand={onExpand} />);
    fireEvent.click(screen.getByTestId("pet-open"));
    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("idles, then waves and shows Speak and Write on hover", () => {
    render(<PetBar {...base} />);
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("idle");
    expect(screen.getByTestId("pet-actions").getAttribute("data-shown")).toBe("0");

    fireEvent.pointerEnter(screen.getByTestId("pet-bar"));
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("waving");
    expect(screen.getByTestId("pet-actions").getAttribute("data-shown")).toBe("1");
    expect(screen.getByTestId("pet-speak")).toBeTruthy();
    expect(screen.getByTestId("pet-write")).toBeTruthy();
  });

  it("shows the actions to the keyboard too: focus inside reveals them", () => {
    render(<PetBar {...base} />);
    act(() => screen.getByTestId("pet-speak").focus());
    expect(screen.getByTestId("pet-actions").getAttribute("data-shown")).toBe("1");
  });

  it("Speak starts the overlay's push-to-talk recorder, holding the mic floor", () => {
    render(<PetBar {...base} />);
    fireEvent.click(screen.getByTestId("pet-speak"));
    expect(floor.take).toHaveBeenCalledTimes(1);
    expect(voice.start).toHaveBeenCalledTimes(1);
  });

  it("a Speak transcript goes to the draft handler, never to Write's send", () => {
    const onTranscript = vi.fn();
    const onWrite = vi.fn();
    render(<PetBar {...base} onTranscript={onTranscript} onWrite={onWrite} />);
    act(() => voice.onTranscript?.("a 3.3V LDO"));
    expect(onTranscript).toHaveBeenCalledWith("a 3.3V LDO");
    expect(onWrite).not.toHaveBeenCalled();
  });

  it("while recording, the check stops (and transcribes) and the cross cancels", () => {
    voice.status = "recording";
    render(<PetBar {...base} />);
    expect(screen.getByTestId("pet-actions").getAttribute("data-shown")).toBe("1");
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("waiting");
    fireEvent.click(screen.getByTestId("pet-speak-stop"));
    expect(voice.stop).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("pet-speak-cancel"));
    expect(voice.cancel).toHaveBeenCalledTimes(1);
  });

  it("Write asks the page to open the field", () => {
    const onWritingChange = vi.fn();
    render(<PetBar {...base} onWritingChange={onWritingChange} />);
    fireEvent.click(screen.getByTestId("pet-write"));
    expect(onWritingChange).toHaveBeenCalledWith(true);
  });

  it("Write submits the typed text on Enter, then closes the field", () => {
    const onWrite = vi.fn();
    const onWritingChange = vi.fn();
    render(<PetBar {...base} writing onWrite={onWrite} onWritingChange={onWritingChange} />);
    const input = screen.getByTestId("pet-write-input") as HTMLInputElement;
    // The field is focused because Write was pressed, not on its own.
    expect(document.activeElement).toBe(input);
    fireEvent.change(input, { target: { value: "  a USB-C power board  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onWrite).toHaveBeenCalledWith("a USB-C power board");
    expect(onWritingChange).toHaveBeenCalledWith(false);
  });

  it("Write sends nothing for an empty line, and Escape closes the field", () => {
    const onWrite = vi.fn();
    const onWritingChange = vi.fn();
    render(<PetBar {...base} writing onWrite={onWrite} onWritingChange={onWritingChange} />);
    const input = screen.getByTestId("pet-write-input");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onWrite).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onWritingChange).toHaveBeenCalledWith(false);
  });

  it("does not take focus on its own when it mounts", () => {
    render(<PetBar {...base} />);
    expect(document.activeElement).toBe(document.body);
  });

  it("acts out the run like a Codex pet: working, then ready for review", () => {
    const { rerender } = render(<PetBar {...base} busy />);
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("running");
    rerender(<PetBar {...base} status="done" />);
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("review");
    rerender(<PetBar {...base} status="error" />);
    expect(screen.getByTestId("pet-sprite").getAttribute("data-row")).toBe("failed");
  });

  it("holds a still frame under prefers-reduced-motion", () => {
    const matchMedia = vi.fn().mockReturnValue({
      matches: true,
      addEventListener: () => {},
      removeEventListener: () => {},
    });
    vi.stubGlobal("matchMedia", matchMedia);
    try {
      render(<PetBar {...base} busy />);
      const sprite = screen.getByTestId("pet-sprite");
      expect(sprite.getAttribute("data-row")).toBe("idle");
      expect(sprite.getAttribute("data-frame")).toBe("0");
      expect(screen.getByTestId("pet-bar").getAttribute("data-motion")).toBe("still");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("can be the bear", () => {
    render(<PetBar {...base} pet="bear" />);
    expect(screen.getByTestId("pet-sprite").getAttribute("data-pet")).toBe("bear");
  });
});
