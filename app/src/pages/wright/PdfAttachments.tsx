import { useRef } from "react";
import type { usePdfAttachments } from "./usePdfAttachments";
// Pluely's outline Button at its `sm` step (src/components/ui/button.tsx),
// one notch smaller so it sits inside a composer toolbar.
const CHIP = "h-7 shrink-0 rounded-lg border border-input bg-background px-2.5 text-xs font-medium outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50";
export function PdfAttachments({ pdf, disabled = false, onSubmit }: { pdf: ReturnType<typeof usePdfAttachments>; disabled?: boolean; onSubmit?: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  return <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs" data-testid="pdf-attachments">
    <input ref={input} type="file" accept=".pdf,application/pdf" aria-label="Attach VTOL PDF" className="sr-only" disabled={disabled || pdf.processing} onChange={(e) => { if (e.target.files?.length) void pdf.attach(Array.from(e.target.files)); e.target.value = ""; }} />
    <button type="button" className={CHIP} disabled={disabled || pdf.processing || Boolean(pdf.attachment)} onClick={() => input.current?.click()}>Attach PDF</button>
    {pdf.processing ? <span role="status" className="text-muted-foreground">Reading the PDF…</span> : pdf.attachment ? <span className="inline-flex min-w-0 items-center gap-1 rounded-lg bg-muted px-2 py-1" title={pdf.attachment.preview}><span className="truncate">{pdf.attachment.name} · {pdf.attachment.pages} {pdf.attachment.pages === 1 ? "page" : "pages"}</span> <button type="button" aria-label={`Remove ${pdf.attachment.name}`} disabled={disabled} onClick={pdf.remove} className="rounded px-1 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">×</button></span> : <span className="truncate text-muted-foreground">{pdf.dragging ? "Drop the VTOL PDF here" : "or drop one PDF, up to 4 pages and 20 MiB"}</span>}
    {pdf.attachment && onSubmit && <button type="button" disabled={disabled || pdf.blocked} onClick={onSubmit} className={CHIP}>Build attached PDF</button>}
    {/* The reader's caveats fold into one disclosure so four long sentences do not push the field off a small overlay. */}
    {pdf.attachment?.limitations?.length ? <details className="basis-full text-muted-foreground"><summary className="w-fit cursor-pointer select-none rounded outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">What Wright reads from this PDF</summary><ul className="mt-1 list-disc space-y-0.5 pl-4">{pdf.attachment.limitations.map((note) => <li key={note}>{note}</li>)}</ul></details> : null}
    {pdf.error && <span role="alert" className="text-red-600 dark:text-red-400">{pdf.error} <button type="button" onClick={pdf.clearError} className="ml-1 rounded underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">Dismiss</button></span>}
  </div>;
}
