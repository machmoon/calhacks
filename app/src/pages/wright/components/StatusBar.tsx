import { cn } from "@/lib/utils";
import type { RunState } from "../reducer";
import type { EnvStatus } from "../types";

/**
 * The quiet status row the desktop rule asks for, at the bottom where VS
 * Code and Zed put theirs: a dot per requirement (green, red when a
 * required one fails, hollow when an optional solver is missing and its
 * stage will be skipped), then spend. Details are in each item's tooltip.
 */
export const StatusBar = ({ env, checking, onRecheck, state }: { env: EnvStatus | null; checking: boolean; onRecheck: () => void; state: RunState }) => {
  const s = state.spend;
  return (
    <footer data-testid="wright-env" className="flex h-7 shrink-0 items-center gap-3 overflow-x-auto border-t border-border/70 px-3 text-[11px] text-muted-foreground">
      {env === null ? <span>Checking the environment…</span> : null}
      {env?.items.map((i) => (
        <span key={i.name} title={i.ok ? i.detail : i.fix || i.detail} className="flex shrink-0 items-center gap-1.5">
          <span className={cn("size-1.5 rounded-full", i.ok ? "bg-emerald-500" : i.required ? "bg-red-500" : "border border-muted-foreground/60")} />
          <span className={cn(!i.ok && i.required && "text-red-600 dark:text-red-400")}>{i.ok ? i.name : i.required ? `${i.name}: needs attention` : `${i.name}: skipped`}</span>
        </span>
      ))}
      <button type="button" onClick={onRecheck} disabled={checking} className="shrink-0 rounded px-1 hover:text-foreground disabled:opacity-50">
        {checking ? "Checking…" : "Recheck"}
      </button>
      <span className="flex-1" />
      {s ? (
        <span data-testid="wright-spend" className="flex shrink-0 items-center gap-2 tabular-nums" title={`${s.inputTokens} in / ${s.outputTokens} out tokens`}>
          ${s.usd.toFixed(2)} of ${s.budgetUsd.toFixed(2)}
          <span className="h-1 w-14 overflow-hidden rounded-full bg-muted">
            <span className={cn("block h-full", s.usd / s.budgetUsd > 0.85 ? "bg-red-500" : "bg-foreground/50")} style={{ width: `${Math.min(100, (s.usd / Math.max(s.budgetUsd, 1e-9)) * 100)}%` }} />
          </span>
          {Math.round((s.inputTokens + s.outputTokens) / 1000)}k tokens
        </span>
      ) : null}
    </footer>
  );
};

/** One sentence plus the fix for each required item that is not ready. */
export const EnvAlerts = ({ env }: { env: EnvStatus | null }) => (
  <>
    {(env?.items ?? [])
      .filter((i) => i.required && !i.ok)
      .map((i) => (
        <p key={i.name} role="alert" className="rounded-lg bg-red-500/10 px-3 py-2 text-[13px] text-red-700 dark:text-red-300">
          {i.fix}
        </p>
      ))}
  </>
);
