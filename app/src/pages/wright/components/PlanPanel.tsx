import { CircleCheckIcon, CircleDotIcon, CircleIcon, CircleAlertIcon, CircleXIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { planCounts } from "../reducer";
import type { PlanStatus, PlanTask } from "../types";
import { Empty } from "./Panel";

/**
 * The orchestrator's plan as a vertical timeline: a badge per task on a
 * rail, title, owner and inputs under it. Shaped on Primer's Timeline
 * (packages/react/src/Timeline/Timeline.tsx: Item, Badge, Body, condensed).
 */
const BADGE: Record<PlanStatus, { icon: typeof CircleIcon; cls: string; label: string }> = {
  done: { icon: CircleCheckIcon, cls: "text-emerald-600 dark:text-emerald-400", label: "done" },
  running: { icon: CircleDotIcon, cls: "text-sky-600 dark:text-sky-400 animate-pulse", label: "running" },
  pending: { icon: CircleIcon, cls: "text-muted-foreground", label: "pending" },
  blocked: { icon: CircleXIcon, cls: "text-red-600 dark:text-red-400", label: "blocked" },
  // Stale was done; a check it vouched for failed since. Amber, never plain done.
  stale: { icon: CircleAlertIcon, cls: "text-amber-600 dark:text-amber-400", label: "stale" },
};

export const PlanPanel = ({ goal, tasks }: { goal: string; tasks: PlanTask[] }) => {
  const c = planCounts(tasks);
  if (tasks.length === 0) return <Empty>No plan yet.</Empty>;
  return (
    <div>
      <div className="mb-3 flex items-baseline justify-between gap-3">
        {goal ? <p className="text-sm text-foreground/90">{goal}</p> : <span />}
        <span className="shrink-0 text-xs tabular-nums text-muted-foreground">{c.done}/{tasks.length} done{c.stale ? ` · ${c.stale} stale` : ""}</span>
      </div>
      <ol data-testid="wright-plan" className="relative">
        {tasks.map((t, i) => {
          const b = BADGE[t.status];
          const Icon = b.icon;
          return (
            <li key={t.id} className="relative flex gap-3 pb-4 last:pb-0" data-status={t.status}>
              {i < tasks.length - 1 ? <span aria-hidden className="absolute left-[9px] top-6 bottom-0 w-px bg-border" /> : null}
              <Icon className={cn("relative mt-0.5 size-[19px] shrink-0 bg-background", b.cls)} aria-label={b.label} />
              <div className="min-w-0 flex-1">
                <p className={cn("text-sm", t.status === "stale" ? "text-amber-700 dark:text-amber-400" : "text-foreground")}>{t.title}</p>
                <p className="text-xs text-muted-foreground">
                  {t.owner}
                  {t.inputs.length ? ` · needs ${t.inputs.join(", ")}` : ""}
                  {t.checks.length ? ` · ${t.checks.length} check${t.checks.length === 1 ? "" : "s"}` : ""}
                </p>
                {t.note ? <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground/80">{t.note}</p> : null}
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
};
