// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { OpenOverlayButton } from "./OpenOverlayButton";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => { cleanup(); invoke.mockReset(); });
it("opens the native overlay through its command", async () => {
  invoke.mockResolvedValue(undefined); render(<OpenOverlayButton />);
  fireEvent.click(screen.getByRole("button", { name: "Open overlay" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("wright_show_overlay"));
  await waitFor(() => expect(screen.getByRole("button", { name: "Open overlay" })).toBeTruthy());
});
it("shows a failed open and allows retry", async () => {
  invoke.mockRejectedValue("main window unavailable"); render(<OpenOverlayButton />);
  fireEvent.click(screen.getByRole("button", { name: "Open overlay" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Could not open overlay: main window unavailable");
  expect(screen.getByRole("button", { name: "Open overlay" }).hasAttribute("disabled")).toBe(false);
});
