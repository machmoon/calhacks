import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { wrightAttachPdf, type PdfAttachment } from "./api";

export const MAX_PDF_BYTES = 20 * 1024 * 1024;
export const PDF_DEFAULT_REQUEST = "Build the VTOL from the attached specification";
export function validatePdf(name: string, size?: number): void {
  if (!name.toLowerCase().endsWith(".pdf")) throw new Error("Attach one PDF specification.");
  if (size !== undefined && (size === 0 || size > MAX_PDF_BYTES)) throw new Error("PDF must be nonempty and at most 20 MiB.");
}

export function usePdfAttachments(disabled = false) {
  const [attachment, setAttachment] = useState<PdfAttachment | null>(null);
  const [processing, setProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const current = useRef({ disabled, attachment }); current.current = { disabled, attachment };
  const inFlight = useRef(false);
  const attach = useCallback(async (items: File[] | string[]) => {
    if (current.current.disabled || inFlight.current) return;
    if (items.length !== 1 || current.current.attachment) { setError("Only one PDF is supported. Remove the current attachment first."); return; }
    inFlight.current = true; setProcessing(true); setError(null);
    try {
      const item = items[0];
      let result: PdfAttachment;
      if (typeof item === "string") {
        validatePdf(item); result = await wrightAttachPdf({ path: item });
      } else {
        validatePdf(item.name, item.size);
        result = await wrightAttachPdf({ name: item.name, bytes: Array.from(new Uint8Array(await item.arrayBuffer())) });
      }
      setAttachment(result);
    } catch (e) { setError(String(e)); }
    finally { inFlight.current = false; setProcessing(false); }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false; let unlisten: (() => void) | undefined;
    getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") setDragging(true);
      else setDragging(false);
      if (payload.type === "drop") void attach(payload.paths);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch((e) => setError(`PDF drop unavailable: ${String(e)}`));
    return () => { disposed = true; unlisten?.(); };
  }, [attach]);
  return { attachment, processing, error, dragging, blocked: processing || Boolean(error), attach,
    remove: () => { if (!inFlight.current && !current.current.disabled) { setAttachment(null); setError(null); } },
    clearError: () => setError(null),
    dropProps: {
      onDragOver: (e: React.DragEvent) => { if (e.dataTransfer.types.includes("Files")) { e.preventDefault(); setDragging(true); } },
      onDragLeave: () => setDragging(false),
      onDrop: (e: React.DragEvent) => { e.preventDefault(); setDragging(false); if (e.dataTransfer.files.length) void attach(Array.from(e.dataTransfer.files)); },
    },
  };
}
