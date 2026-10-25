import { headlineMet, type RunState } from "../reducer";
import type { Headline } from "../types";
import { Empty, Panel, Pill } from "./Panel";

const num = (v: number) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2).replace(/\.?0+$/, ""));

const HeadlineCard = ({ h }: { h: Headline }) => {
  const met = headlineMet(h);
  const limit =
    h.limit === null || h.bound === "info" ? null : `${h.bound === "at_least" ? "need ≥" : "need ≤"} ${num(h.limit)} ${h.unit}`;
  return (
    <div
      className={`rounded-lg border px-2.5 py-1.5 ${met === false ? "border-red-500/40" : met === true ? "border-emerald-500/30" : "border-border"}`}
      data-met={String(met)}
    >
      <p className="text-[10px] text-muted-foreground">{h.label}</p>
      <p className="text-sm font-semibold tabular-nums">
        {num(h.value)} <span className="text-[10px] font-normal text-muted-foreground">{h.unit}</span>
      </p>
      {limit ? <p className="text-[10px] text-muted-foreground">{limit}</p> : null}
    </div>
  );
};

export const Headlines = ({ items }: { items: Headline[] }) => (
  <Panel title="Headlines" bodyClassName={items.length ? "p-2" : undefined} className="shrink-0">
    {items.length === 0 ? (
      <Empty>No headline numbers yet.</Empty>
    ) : (
      <div className="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-1.5" data-testid="wright-headlines">
        {items.map((h) => (
          <HeadlineCard key={h.label} h={h} />
        ))}
      </div>
    )}
  </Panel>
);

export const SpendLine = ({ state }: { state: RunState }) => {
  const s = state.spend;
  if (!s) return null;
  const pct = s.budgetUsd > 0 ? Math.min(100, (s.usd / s.budgetUsd) * 100) : 0;
  return (
    <span className="flex items-center gap-2 text-[11px]" data-testid="wright-spend" title={`${s.inputTokens} in / ${s.outputTokens} out tokens`}>
      <span className="tabular-nums">${s.usd.toFixed(2)}</span>
      <span className="text-muted-foreground">of ${s.budgetUsd.toFixed(2)}</span>
      <span className="h-1 w-16 overflow-hidden rounded-full bg-muted">
        <span className={`block h-full ${pct > 85 ? "bg-red-500" : "bg-foreground/60"}`} style={{ width: `${pct}%` }} />
      </span>
      <span className="text-muted-foreground">{Math.round((s.inputTokens + s.outputTokens) / 1000)}k tok</span>
    </span>
  );
};

export const Verdict = ({ state }: { state: RunState }) => {
  const f = state.finished;
  if (!f) return null;
  return (
    <Pill tone={f.passed ? "ok" : "bad"}>
      {f.passed ? "PASS" : "not passing"} · {f.label} {f.objective.toFixed(2)}
    </Pill>
  );
};
