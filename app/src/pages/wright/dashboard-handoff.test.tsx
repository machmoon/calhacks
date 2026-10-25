// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { handoffWrightRun, WRIGHT_RUN_STARTED } from "./dashboard-handoff";
import { DashboardRunSync } from "./DashboardRunSync";
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), emitTo: vi.fn(), listen: vi.fn(), callback: undefined as undefined | (() => void), stop: vi.fn(), label: null as string | null }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => Boolean(mocks.label) }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ label: mocks.label }) }));
vi.mock("@tauri-apps/api/event", () => ({ emitTo: mocks.emitTo, listen: mocks.listen }));
afterEach(() => { cleanup(); vi.clearAllMocks(); mocks.callback = undefined; mocks.label = null; });
it("shows the full dashboard before notifying only that window", async () => {
  const order: string[] = [];
  mocks.invoke.mockImplementation(async () => { order.push("show"); });
  mocks.emitTo.mockImplementation(async () => { order.push("notify"); });
  await handoffWrightRun("/runs/current");
  expect(order).toEqual(["show", "notify"]);
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("open_dashboard");
  expect(mocks.emitTo).toHaveBeenCalledWith("dashboard", WRIGHT_RUN_STARTED, { runDir: "/runs/current" });
});
it("does not retry or start another run if showing the dashboard fails", async () => {
  mocks.invoke.mockRejectedValueOnce("window unavailable");
  await expect(handoffWrightRun("/runs/current")).rejects.toBe("window unavailable");
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("open_dashboard");
  expect(mocks.emitTo).not.toHaveBeenCalled();
});
function Location() { return <div data-testid="location">{useLocation().pathname}</div>; }
function draw(path: string) {
  mocks.listen.mockImplementation(async (_event, callback) => { mocks.callback = callback; return mocks.stop; });
  return render(<MemoryRouter initialEntries={[path]}><DashboardRunSync /><Location /></MemoryRouter>);
}
it("never routes the floating overlay into the dashboard", () => {
  draw("/"); expect(mocks.listen).not.toHaveBeenCalled(); expect(screen.getByTestId("location").textContent).toBe("/");
});
it("routes an existing full dashboard and unregisters the listener", async () => {
  const view = draw("/settings");
  await waitFor(() => expect(mocks.callback).toBeDefined());
  act(() => mocks.callback!());
  expect(screen.getByTestId("location").textContent).toBe("/wright");
  view.unmount(); expect(mocks.stop).toHaveBeenCalled();
});

it("repairs an already-clipped main panel without subscribing it as a dashboard", async () => {
  mocks.label = "main"; draw("/wright");
  await waitFor(() => expect(screen.getByTestId("location").textContent).toBe("/"));
  expect(mocks.listen).not.toHaveBeenCalled();
});
