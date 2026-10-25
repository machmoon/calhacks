// @vitest-environment jsdom
//
// One voice control: push-to-talk. A tap starts and stops a recording, a
// hold records while held, and while I am talking a click stops me.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const voice = {
  status: "idle" as string,
  transcript: "",
  error: "",
  elapsedS: 0,
  start: vi.fn(async () => {}),
  stop: vi.fn(),
  cancel: vi.fn(),
};
vi.mock("@/hooks/useVoiceInput", () => ({ useVoiceInput: () => voice }));

import { speaker } from "@/lib/speech";
import { HOLD_MS, MIC_TITLE, VoiceControl } from "./VoiceControl";

function draw(props: Partial<React.ComponentProps<typeof VoiceControl>> = {}) {
  return render(
    <VoiceControl baseUrl="http://127.0.0.1:8081" onTranscript={() => {}} {...props} />
  );
}

afterEach(() => {
  cleanup();
  voice.status = "idle";
  vi.clearAllMocks();
});

describe("VoiceControl", () => {
  it("is one control, and says what it does", () => {
    draw();
    const mic = screen.getByTestId("voice-start");
    expect(mic.getAttribute("title")).toBe(MIC_TITLE);
    expect(mic.getAttribute("data-stops")).toBe("mic");
  });

  it("a tap records, and nothing on it starts a run", () => {
    draw();
    fireEvent.click(screen.getByTestId("voice-start"));
    expect(voice.start).toHaveBeenCalledTimes(1);
  });

  it("press and hold records, and the release does not start a second recording", () => {
    vi.useFakeTimers();
    draw();
    const button = screen.getByTestId("voice-start");
    fireEvent.pointerDown(button);
    act(() => {
      vi.advanceTimersByTime(HOLD_MS + 10);
    });
    expect(voice.start).toHaveBeenCalledTimes(1);
    voice.status = "recording";
    fireEvent.pointerUp(button);
    expect(voice.stop).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("while recording it shows a real clock and the two ways out", () => {
    voice.status = "recording";
    voice.elapsedS = 4;
    draw();
    expect(screen.getByTestId("voice-elapsed").textContent).toContain("4s/");
    expect(screen.getByTestId("voice-stop")).toBeTruthy();
    expect(screen.getByTestId("voice-cancel")).toBeTruthy();
  });

  it("the menu says how to talk to me", () => {
    draw();
    fireEvent.contextMenu(screen.getByTestId("voice-control"));
    expect(screen.getByTestId("voice-menu").textContent).toContain("press and hold");
  });

  it("speaks in the first person, and never calls itself an assistant", () => {
    const { container } = draw();
    expect(container.innerHTML.toLowerCase()).not.toMatch(/assistant|helper/);
  });
});

describe("the one control while I am talking", () => {
  it("stops my voice on a click, and does not record", () => {
    const silence = vi.spyOn(speaker, "stop").mockImplementation(() => {});
    try {
      draw({ speaking: true });
      const mic = screen.getByTestId("voice-start");
      expect(mic.getAttribute("data-stops")).toBe("voice");
      fireEvent.click(mic);
      expect(silence).toHaveBeenCalledOnce();
      expect(voice.start).not.toHaveBeenCalled();
    } finally {
      silence.mockRestore();
    }
  });

  it("says what the click does, in words", () => {
    draw({ speaking: true });
    expect(screen.getByTestId("voice-start").getAttribute("title") ?? "").toContain("stop me");
  });
});
