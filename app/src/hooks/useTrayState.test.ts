// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));

import { invoke } from "@tauri-apps/api/core";
import { useTrayState } from "./useTrayState";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => mockInvoke.mockClear());

describe("useTrayState", () => {
  it("reports visibility to the tray, and again when it changes", () => {
    const { rerender } = renderHook((props: { visible: boolean }) => useTrayState(props), {
      initialProps: { visible: true },
    });
    expect(mockInvoke).toHaveBeenLastCalledWith("tray_set_state", { visible: true });
    rerender({ visible: false });
    expect(mockInvoke).toHaveBeenLastCalledWith("tray_set_state", { visible: false });
    expect(mockInvoke).toHaveBeenCalledTimes(2);
  });
});
