// @vitest-environment jsdom
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { usePdfAttachments, validatePdf, MAX_PDF_BYTES } from "./usePdfAttachments";
import { PdfAttachments } from "./PdfAttachments";
import { wrightStart } from "./api";
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), native: false, drop: undefined as undefined | ((event: any) => void), stop: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => mocks.native }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: vi.fn(async (fn) => { mocks.drop = fn; return mocks.stop; }) }) }));
const attachment = { id: "pdf-1", name: "spec.pdf", sha256: "abc", pages: 2, preview: "Airframe brief", limitations: [] };
beforeEach(() => { mocks.invoke.mockReset(); mocks.stop.mockReset(); mocks.native = false; mocks.drop = undefined; });
describe("PDF attachment boundary", () => {
  it("rejects non-PDF, empty, oversized inputs", () => {
    expect(() => validatePdf("spec.txt", 10)).toThrow();
    expect(() => validatePdf("spec.pdf", 0)).toThrow();
    expect(() => validatePdf("spec.pdf", MAX_PDF_BYTES + 1)).toThrow();
    expect(() => validatePdf("SPEC.PDF", 100)).not.toThrow();
  });
  it("sends browser bytes and blocks during processing, then supports removal", async () => {
    let resolve!: (value: unknown) => void;
    mocks.invoke.mockImplementation(() => new Promise((r) => { resolve = r; }));
    const { result } = renderHook(() => usePdfAttachments());
    const file = { name: "spec.pdf", size: 4, arrayBuffer: async () => new Uint8Array([37,80,68,70]).buffer } as File;
    act(() => { void result.current.attach([file]); });
    expect(result.current.blocked).toBe(true);
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("wright_attach_pdf", { path: null, name: "spec.pdf", bytes: [37,80,68,70] }));
    await act(async () => resolve(attachment));
    expect(result.current.attachment?.id).toBe("pdf-1");
    act(() => result.current.remove());
    expect(result.current.attachment).toBeNull();
  });
  it("native drop sends a path and unregisters its listener", async () => {
    mocks.native = true; mocks.invoke.mockResolvedValue(attachment);
    const { result, unmount } = renderHook(() => usePdfAttachments());
    await waitFor(() => expect(mocks.drop).toBeDefined());
    await act(async () => mocks.drop!({ payload: { type: "drop", paths: ["/tmp/spec.pdf"] } }));
    expect(mocks.invoke).toHaveBeenCalledWith("wright_attach_pdf", { path: "/tmp/spec.pdf", name: null, bytes: null });
    expect(result.current.attachment?.name).toBe("spec.pdf");
    unmount(); expect(mocks.stop).toHaveBeenCalledOnce();
  });
  it("rejects multiple files and surfaces backend errors without a runnable attachment", async () => {
    mocks.invoke.mockRejectedValue("PDF exceeds four-page limit");
    const { result } = renderHook(() => usePdfAttachments());
    await act(async () => result.current.attach(["a.pdf", "b.pdf"]));
    expect(mocks.invoke).not.toHaveBeenCalled(); expect(result.current.blocked).toBe(true);
    await act(async () => result.current.attach(["a.pdf"]));
    expect(result.current.error).toContain("four-page"); expect(result.current.attachment).toBeNull();
  });
  it("renders a removable chip and disables its build action while blocked", () => {
    const remove = vi.fn(), submit = vi.fn();
    const pdf = { attachment, processing: false, error: null, dragging: false, blocked: true, attach: vi.fn(), remove, clearError: vi.fn(), dropProps: {} } as unknown as ReturnType<typeof usePdfAttachments>;
    render(<PdfAttachments pdf={pdf} onSubmit={submit} />);
    fireEvent.click(screen.getByRole("button", { name: "Build attached PDF" })); expect(submit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove spec.pdf" })); expect(remove).toHaveBeenCalledOnce();
  });
  it("carries attachment IDs through the native CAD start command", async () => {
    mocks.invoke.mockResolvedValue({}); await wrightStart("aircraft", "Build the VTOL", undefined, ["pdf-1"]);
    expect(mocks.invoke).toHaveBeenCalledWith("wright_start", { domain: "aircraft", request: "Build the VTOL", runDir: null, attachmentIds: ["pdf-1"] });
  });
});
