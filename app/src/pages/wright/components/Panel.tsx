import { cn } from "@/lib/utils";

/**
 * The bordered section every Wright panel sits in. Spacing and type follow
 * Pluely upstream (github.com/iamsrikanthnani/pluely): the card surface is
 * src/pages/chats/index.tsx's `!bg-black/5 dark:!bg-white/5` on
 * `border-border/70 rounded-xl` (src/pages/dashboard/components/Usage.tsx),
 * the header strip is the completion popover's `px-4 py-2 border-b
 * bg-muted/30` with an `text-xs font-semibold` title and `text-[10px]` meta
 * (src/pages/app/components/completion/Input.tsx), and the responsive type
 * steps (`text-xs lg:text-sm`, `text-[10px] lg:text-xs`) are Header/index.tsx's.
 */
export const Panel = ({
  title,
  right,
  children,
  className,
  bodyClassName,
}: {
  title: string;
  right?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}) => (
  <section className={cn("flex min-h-0 flex-col overflow-hidden rounded-xl border border-border/70 bg-black/5 dark:bg-white/5", className)}>
    <div className="flex min-h-9 shrink-0 flex-wrap items-center justify-between gap-x-2 gap-y-1 border-b border-border/70 bg-muted/30 px-3 py-1.5">
      <h2 className="select-none text-xs font-semibold text-foreground lg:text-sm">{title}</h2>
      {right ? <div className="flex min-w-0 flex-wrap items-center gap-2 text-[10px] text-muted-foreground lg:text-xs">{right}</div> : null}
    </div>
    <div className={cn("min-h-0 flex-1 overflow-auto", bodyClassName)}>{children}</div>
  </section>
);

export const Empty = ({ children }: { children: React.ReactNode }) => (
  <p className="select-none px-3 py-4 text-xs text-muted-foreground">{children}</p>
);

type Tone = "ok" | "bad" | "warn" | "info" | "idle";

const TONES: Record<Tone, string> = {
  ok: "border-emerald-500/40 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  bad: "border-red-500/40 bg-red-500/10 text-red-600 dark:text-red-400",
  warn: "border-amber-500/50 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  info: "border-sky-500/40 bg-sky-500/10 text-sky-600 dark:text-sky-400",
  idle: "border-border bg-muted/40 text-muted-foreground",
};

export const Pill = ({
  tone,
  children,
  dashed,
  title,
}: {
  tone: Tone;
  children: React.ReactNode;
  dashed?: boolean;
  title?: string;
}) => (
  <span
    title={title}
    className={cn(
      "inline-flex w-fit shrink-0 items-center rounded-full border px-2 py-0.5 text-[10px] font-medium leading-none",
      dashed && "border-dashed",
      TONES[tone]
    )}
  >
    {children}
  </span>
);
