import type { EnvStatus } from "../types";
import { Pill } from "./Panel";

/**
 * What Wright needs, as a quiet row of badges. The required three (the
 * wright binary, a signed-in Claude, a network) block Run when red; an
 * optional solver that is missing reads "skipped": that stage will not run.
 */
export const EnvRow = ({
  env,
  checking,
  onRecheck,
}: {
  env: EnvStatus | null;
  checking: boolean;
  onRecheck: () => void;
}) => {
  const blocking = env?.items.filter((i) => i.required && !i.ok) ?? [];
  return (
    <div data-testid="wright-env">
      <div className="flex flex-wrap items-center gap-1.5">
        {env === null ? <span className="text-[10px] text-muted-foreground">Checking the environment…</span> : null}
        {env?.items.map((i) => (
          <Pill key={i.name} tone={i.ok ? "ok" : i.required ? "bad" : "idle"} dashed={!i.ok && !i.required} title={i.detail}>
            {i.ok ? i.name : i.required ? `${i.name}: needs attention` : `${i.name}: skipped`}
          </Pill>
        ))}
        <button
          type="button"
          onClick={onRecheck}
          disabled={checking}
          className="rounded-md px-1.5 py-0.5 text-[10px] text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50 lg:text-xs"
        >
          {checking ? "Checking…" : "Recheck"}
        </button>
      </div>
      {blocking.map((i) => (
        <p key={i.name} role="alert" className="mt-2 rounded-lg border border-destructive/20 bg-destructive/10 px-3 py-1.5 text-xs text-red-600 dark:text-red-400">
          {i.fix}
        </p>
      ))}
    </div>
  );
};
