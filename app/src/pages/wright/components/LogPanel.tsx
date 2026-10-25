import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { agentsIn, type LogLine } from "../reducer";
import { Empty, Panel } from "./Panel";

const KIND_STYLE: Record<LogLine["kind"], string> = {
  message: "text-foreground",
  call: "font-mono text-muted-foreground",
  result: "font-mono text-muted-foreground",
  feedback: "text-sky-600 dark:text-sky-400",
  notice: "italic text-muted-foreground",
};

export const LogPanel = ({ log }: { log: LogLine[] }) => {
  const [agent, setAgent] = useState<string>("all");
  const [calls, setCalls] = useState(true);
  const agents = useMemo(() => agentsIn(log), [log]);
  const shown = useMemo(
    () => log.filter((l) => (agent === "all" || l.agent === agent) && (calls || l.kind === "message" || l.kind === "feedback" || l.kind === "notice")),
    [log, agent, calls]
  );
  const end = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (stick.current) end.current?.scrollIntoView({ block: "end" });
  }, [shown.length]);

  return (
    <Panel
      title="Live log"
      className="flex-1"
      right={
        <>
          <label className="flex cursor-pointer items-center gap-1.5">
            <input type="checkbox" className="accent-primary" checked={calls} onChange={(e) => setCalls(e.target.checked)} />
            Tool calls
          </label>
          <select
            aria-label="Filter by agent"
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
            className="h-6 max-w-[10rem] rounded-md border border-input bg-background px-1.5 text-[10px] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50 lg:text-xs"
          >
            <option value="all">All agents · {log.length} entries</option>
            {agents.map((a) => (
              <option key={a} value={a}>{a}</option>
            ))}
          </select>
        </>
      }
    >
      <div
        ref={scroller}
        className="h-full overflow-auto"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {shown.length === 0 ? (
          <Empty>No log yet.</Empty>
        ) : (
          <ul className="space-y-1 px-3 py-2" data-testid="wright-log">
            {shown.map((l, i) => (
              <li key={i} className="flex gap-2 text-[11px] leading-snug">
                <span className="w-20 shrink-0 truncate pt-px text-[10px] font-medium text-muted-foreground lg:w-24" title={l.agent}>{l.agent}</span>
                <span className={cn("min-w-0 whitespace-pre-wrap break-words", KIND_STYLE[l.kind], l.kind === "result" && l.ok === false && "text-red-600 dark:text-red-400")}>
                  {l.text}
                </span>
              </li>
            ))}
          </ul>
        )}
        <div ref={end} />
      </div>
    </Panel>
  );
};
