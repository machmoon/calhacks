import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRightIcon, CircleCheckIcon, CircleXIcon, WrenchIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import type { LogLine } from "../reducer";

/**
 * The agents' stream as a chat transcript: consecutive lines from one agent
 * sit under one name, messages read as prose, and each tool call folds to
 * one line with its result's icon (the transcript shape of LibreChat and
 * Open WebUI; VS Code's chat in the secondary side bar shows tool calls the
 * same way, one collapsible row each).
 */

interface Turn { agent: string; items: Item[] }
type Item = { kind: "text"; line: LogLine } | { kind: "tool"; call?: LogLine; result?: LogLine };

export function turns(log: LogLine[], showTools: boolean): Turn[] {
  const out: Turn[] = [];
  for (const l of log) {
    const isTool = l.kind === "call" || l.kind === "result";
    if (isTool && !showTools) continue;
    let t = out[out.length - 1];
    if (!t || t.agent !== l.agent) out.push((t = { agent: l.agent, items: [] }));
    if (l.kind === "call") t.items.push({ kind: "tool", call: l });
    else if (l.kind === "result") {
      const open = [...t.items].reverse().find((x) => x.kind === "tool" && !x.result && x.call && x.call.text.split(" ")[0] === l.text.split(":")[0]);
      if (open && open.kind === "tool") open.result = l;
      else t.items.push({ kind: "tool", result: l });
    } else t.items.push({ kind: "text", line: l });
  }
  return out;
}

const initials = (a: string) => (a ? a.replace(/[^A-Za-z0-9]/g, " ").trim().split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase() : "W");

const ToolRow = ({ call, result }: { call?: LogLine; result?: LogLine }) => {
  const [open, setOpen] = useState(false);
  const name = (call?.text ?? result?.text ?? "").split(/[\s:]/)[0];
  const ok = result?.ok;
  return (
    <div className="text-xs">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="flex w-full items-center gap-1.5 rounded-md px-1 py-0.5 text-left text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        <ChevronRightIcon className={cn("size-3 shrink-0 transition-transform", open && "rotate-90")} />
        <WrenchIcon className="size-3 shrink-0" />
        <span className="min-w-0 flex-1 truncate font-mono">{name}</span>
        {result ? (ok ? <CircleCheckIcon className="size-3.5 text-emerald-600 dark:text-emerald-400" aria-label="ok" /> : <CircleXIcon className="size-3.5 text-red-600 dark:text-red-400" aria-label="failed" />) : null}
      </button>
      {open ? (
        <div className="ml-5 mt-1 space-y-1 rounded-md bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
          {call ? <p className="whitespace-pre-wrap break-words text-muted-foreground">{call.text}</p> : null}
          {result ? <p className={cn("whitespace-pre-wrap break-words", ok === false && "text-red-600 dark:text-red-400")}>{result.text}</p> : null}
        </div>
      ) : null}
    </div>
  );
};

export const ActivityPanel = ({ log, footer, live }: { log: LogLine[]; footer: React.ReactNode; live: boolean }) => {
  const [showTools, setShowTools] = useState(true);
  const list = useMemo(() => turns(log, showTools), [log, showTools]);
  const end = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => {
    if (stick.current) end.current?.scrollIntoView({ block: "end" });
  }, [log.length]);

  return (
    <aside aria-label="Activity" className="flex min-h-0 flex-col border-l border-border/70 bg-muted/20">
      <div className="flex h-10 shrink-0 items-center justify-between gap-2 px-4">
        <h2 className="flex items-center gap-2 text-sm font-medium">
          Activity
          {live ? <span className="size-1.5 animate-pulse rounded-full bg-sky-500" aria-label="live" /> : null}
        </h2>
        <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
          <input type="checkbox" className="accent-primary" checked={showTools} onChange={(e) => setShowTools(e.target.checked)} />
          Tool calls
        </label>
      </div>
      <div
        className="min-h-0 flex-1 overflow-y-auto px-4"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {list.length === 0 ? (
          <p className="py-6 text-sm text-muted-foreground">The agents' messages appear here while a run works.</p>
        ) : (
          <ol className="space-y-4 pb-4" data-testid="wright-log">
            {list.map((t, i) => (
              <li key={i} className="flex gap-2.5">
                <span className={cn("mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-[7px] text-[10px] font-semibold", t.agent === "person" ? "bg-primary text-primary-foreground" : "bg-foreground/10 text-foreground/80")} aria-hidden>
                  {initials(t.agent)}
                </span>
                <div className="min-w-0 flex-1 space-y-1">
                  <p className="text-xs font-medium text-muted-foreground">{t.agent || "wright"}</p>
                  {t.items.map((it, k) =>
                    it.kind === "text" ? (
                      <p key={k} className={cn("whitespace-pre-wrap break-words text-[13px] leading-relaxed", it.line.kind === "notice" && "italic text-muted-foreground", it.line.kind === "feedback" && "text-sky-700 dark:text-sky-300")}>
                        {it.line.text}
                      </p>
                    ) : (
                      <ToolRow key={k} call={it.call} result={it.result} />
                    )
                  )}
                </div>
              </li>
            ))}
          </ol>
        )}
        <div ref={end} />
      </div>
      <div className="shrink-0 border-t border-border/70 p-3">{footer}</div>
    </aside>
  );
};
