import { cn } from "@/lib/utils";

/**
 * Small charts in plain SVG/CSS (no chart library is installed). BarList
 * follows Tremor's src/components/BarList/BarList.tsx: bar width as a share
 * of the largest value, the label inside the bar, the value at the right.
 */

export const Sparkline = ({ values, limit, good }: { values: number[]; limit?: number | null; good?: boolean | null }) => {
  if (values.length < 2) return null;
  const all = limit === null || limit === undefined ? values : [...values, limit];
  const lo = Math.min(...all), hi = Math.max(...all);
  const span = hi - lo || 1;
  const W = 72, H = 22;
  const x = (i: number) => (i / (values.length - 1)) * W;
  const y = (v: number) => H - 2 - ((v - lo) / span) * (H - 4);
  const d = values.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const stroke = good === false ? "stroke-red-500" : good === true ? "stroke-emerald-500" : "stroke-foreground/60";
  return (
    <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} aria-hidden className="overflow-visible">
      {limit !== null && limit !== undefined ? <line x1={0} x2={W} y1={y(limit)} y2={y(limit)} className="stroke-muted-foreground/50" strokeDasharray="2 2" /> : null}
      <path d={d} fill="none" strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" className={stroke} />
      <circle cx={x(values.length - 1)} cy={y(values[values.length - 1])} r={2} className={cn("fill-current", good === false ? "text-red-500" : good === true ? "text-emerald-500" : "text-foreground")} />
    </svg>
  );
};

/** Pass/fail per check report: the run's before and after. */
export const IterationBars = ({ history }: { history: { passed: number; failed: number }[] }) => {
  if (history.length === 0) return null;
  const max = Math.max(...history.map((h) => h.passed + h.failed), 1);
  return (
    <div className="flex items-end gap-1.5" role="img" aria-label={`Checks per report: ${history.map((h) => `${h.passed} pass ${h.failed} fail`).join(", ")}`}>
      {history.map((h, i) => (
        <div key={i} className="flex flex-col items-center gap-1">
          <div className="flex h-16 w-6 flex-col-reverse overflow-hidden rounded-[4px] bg-muted/50">
            <div className="bg-emerald-500/80" style={{ height: `${(h.passed / max) * 100}%` }} />
            <div className="bg-red-500/80" style={{ height: `${(h.failed / max) * 100}%` }} />
          </div>
          <span className="text-[10px] tabular-nums text-muted-foreground">{i + 1}</span>
        </div>
      ))}
    </div>
  );
};

export interface Bar { key: string; label: React.ReactNode; value: number; display: string; highlight?: boolean; muted?: boolean }

export const BarList = ({ bars, className }: { bars: Bar[]; className?: string }) => {
  const max = Math.max(...bars.map((b) => b.value), 1);
  return (
    <ul className={cn("space-y-1.5", className)}>
      {bars.map((b) => (
        <li key={b.key} className="flex items-center gap-3" data-highlight={b.highlight ? "" : undefined}>
          <div className="relative h-7 min-w-0 flex-1">
            <div
              className={cn("absolute inset-y-0 left-0 rounded-[5px] transition-all", b.highlight ? "bg-emerald-500/25 ring-1 ring-emerald-500/60" : "bg-muted", b.muted && "opacity-50")}
              style={{ width: `${Math.max(4, (b.value / max) * 100)}%` }}
            />
            <div className="absolute inset-y-0 left-2 flex items-center gap-1.5 whitespace-nowrap text-xs">{b.label}</div>
          </div>
          <span className="w-20 shrink-0 text-right text-xs tabular-nums text-muted-foreground">{b.display}</span>
        </li>
      ))}
    </ul>
  );
};
