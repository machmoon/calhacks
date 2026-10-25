import { CheckIcon, MinusIcon, XIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Step, StepId } from "../story";

/**
 * The run's story as a horizontal stepper whose steps are also the tabs of
 * the workspace. Shaped on Mantine's Stepper
 * (packages/@mantine/core/src/components/Stepper/Stepper.tsx: a numbered or
 * completed icon, label and description, separators between steps,
 * `allowStepSelect`), with WAI-ARIA tab semantics so arrow keys move.
 */
const ICON: Record<Step["status"], string> = {
  done: "bg-emerald-600 text-white dark:bg-emerald-500",
  failed: "bg-red-600 text-white dark:bg-red-500",
  active: "border-2 border-sky-500 text-sky-600 dark:text-sky-400",
  pending: "border border-border text-muted-foreground",
  skipped: "border border-dashed border-border text-muted-foreground",
};

export const Stepper = ({ steps, current, onSelect }: { steps: Step[]; current: string; onSelect: (id: StepId) => void }) => (
  <div
    role="tablist"
    aria-label="Run steps"
    className="flex min-w-0 items-start overflow-x-auto pb-1"
    onKeyDown={(e) => {
      if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
      const i = steps.findIndex((s) => s.id === current);
      const n = steps[(i + (e.key === "ArrowRight" ? 1 : steps.length - 1)) % steps.length];
      onSelect(n.id);
      (e.currentTarget.querySelector(`[data-step="${n.id}"]`) as HTMLElement | null)?.focus();
    }}
  >
    {steps.map((s, i) => {
      const sel = s.id === current;
      return (
        <div key={s.id} className="flex min-w-0 flex-1 items-start last:flex-none">
          <button
            type="button"
            role="tab"
            data-step={s.id}
            data-status={s.status}
            aria-selected={sel}
            tabIndex={sel ? 0 : -1}
            onClick={() => onSelect(s.id)}
            className={cn(
              "group flex shrink-0 items-start gap-2 rounded-lg px-1.5 py-1 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50",
              sel ? "bg-accent" : "hover:bg-accent/60"
            )}
          >
            <span className={cn("mt-px flex size-5 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold tabular-nums", ICON[s.status])}>
              {s.status === "done" ? <CheckIcon className="size-3" strokeWidth={3} /> : s.status === "failed" ? <XIcon className="size-3" strokeWidth={3} /> : s.status === "skipped" ? <MinusIcon className="size-3" /> : i + 1}
            </span>
            <span className="flex flex-col">
              <span className={cn("whitespace-nowrap text-[13px] leading-5", sel ? "font-medium text-foreground" : "text-foreground/80")}>{s.label}</span>
              <span className={cn("whitespace-nowrap text-[11px] leading-4", s.status === "failed" ? "text-red-600 dark:text-red-400" : "text-muted-foreground")}>
                {s.hint || (s.status === "active" ? "working" : s.status === "skipped" ? "not in this run" : " ")}
              </span>
            </span>
          </button>
          {i < steps.length - 1 ? (
            <span aria-hidden className={cn("mx-1 mt-[14px] h-px min-w-3 flex-1", s.status === "done" || s.status === "failed" ? "bg-emerald-600/50" : "bg-border")} />
          ) : null}
        </div>
      );
    })}
  </div>
);
