import { useMemo, useState } from "react";
import { CircleCheckIcon, CircleDashedIcon, CircleXIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { checkRow, type CheckRow, type CheckVerdict } from "../reducer";
import { failureSentence, humanName } from "../story";
import type { Outcome } from "../types";
import { Empty } from "./Panel";

/**
 * Checks as a problems list: an icon and colour that carry the verdict, the
 * check, one plain sentence, the margin. Shaped on VS Code's Problems view
 * (src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts: severity
 * icon, message, source), problems first.
 */
export const VERDICT_UI: Record<CheckVerdict, { icon: typeof CircleCheckIcon; cls: string; label: string }> = {
  pass: { icon: CircleCheckIcon, cls: "text-emerald-600 dark:text-emerald-400", label: "Pass" },
  fail: { icon: CircleXIcon, cls: "text-red-600 dark:text-red-400", label: "Fail" },
  missing: { icon: CircleDashedIcon, cls: "text-amber-600 dark:text-amber-400", label: "Not measured" },
};

const fmt = (v: number) => (Math.abs(v) >= 1000 || (v !== 0 && Math.abs(v) < 0.01) ? v.toExponential(2) : v.toFixed(3).replace(/\.?0+$/, ""));

export const ChecksTable = ({ outcomes, reports, compact }: { outcomes: Outcome[]; reports: number; compact?: boolean }) => {
  const [onlyBad, setOnlyBad] = useState(false);
  const rows: CheckRow[] = useMemo(() => outcomes.map(checkRow), [outcomes]);
  const sorted = useMemo(() => {
    const rank = { fail: 0, missing: 1, pass: 2 } as const;
    return [...rows].filter((r) => !onlyBad || r.verdict !== "pass").sort((a, b) => rank[a.verdict] - rank[b.verdict]);
  }, [rows, onlyBad]);
  if (rows.length === 0) return <Empty>No checks yet.</Empty>;
  const bad = rows.filter((r) => r.verdict !== "pass").length;
  return (
    <div>
      {compact ? null : (
        <div className="flex items-center justify-between pb-2 text-xs text-muted-foreground">
          <span>
            <span className="font-medium text-foreground tabular-nums">{rows.length - bad}/{rows.length}</span> pass
            {reports > 1 ? ` · report ${reports}` : ""}
          </span>
          <label className="flex cursor-pointer items-center gap-1.5">
            <input type="checkbox" className="accent-primary" checked={onlyBad} onChange={(e) => setOnlyBad(e.target.checked)} />
            Only problems
          </label>
        </div>
      )}
      <table className="w-full text-left text-[13px]" data-testid="wright-checks">
        <thead className="sr-only">
          <tr>
            <th>Result</th>
            <th>Check</th>
            <th>Margin</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((r) => {
            const v = VERDICT_UI[r.verdict];
            const Icon = v.icon;
            return (
              <tr key={r.name} data-verdict={r.verdict} className="border-t border-border/50 align-top first:border-t-0">
                <td className="w-7 py-2 pr-1">
                  <Icon className={cn("size-4", v.cls)} aria-label={v.label} />
                </td>
                <td className="py-2 pr-3">
                  <p className="font-medium">{humanName(r.name)} <span className="ml-1 text-[11px] font-normal text-muted-foreground">{r.source}</span></p>
                  <p className={cn("mt-0.5 text-xs leading-relaxed", r.verdict === "pass" ? "text-muted-foreground" : "text-foreground/85")}>
                    {r.verdict === "pass" ? r.detail.split(/;\s/)[0] : failureSentence(r)}
                  </p>
                </td>
                <td className={cn("w-24 py-2 text-right font-mono text-xs tabular-nums", r.verdict === "fail" ? v.cls : "text-muted-foreground")}>
                  {r.margin === null ? "—" : `${r.margin > 0 ? "+" : ""}${fmt(r.margin)}`}
                  {r.margin !== null && r.unit ? <span className="ml-1 font-sans text-[10px]">{r.unit}</span> : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};
