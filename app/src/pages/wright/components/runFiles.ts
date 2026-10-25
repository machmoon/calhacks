import { useMemo } from "react";
import type { RunState } from "../reducer";
import { bundle, parseGroups, parseJudge, parseVerdicts, type Bundle } from "../story";
import { useJson, useResolvedMany } from "./hooks";

const KEYS = ["pcb", "sch", "pro", "design", "verdicts", "groups", "boardStep", "fcstd", "cadStep", "assemblyMesh"] as const;
export type Resolved = Partial<Record<(typeof KEYS)[number], string | null | undefined>>;

/** The run's hand-off files found on this machine, plus the JSON they hold. */
export function useRunFiles(runDir: string | null, state: RunState) {
  const recorded: Bundle = useMemo(() => bundle(state.artifacts), [state.artifacts]);
  const paths = useResolvedMany(runDir, KEYS.map((k) => recorded[k]));
  const resolved: Resolved = Object.fromEntries(KEYS.map((k, i) => [k, paths[i]]));
  const design = useJson(resolved.design);
  const verdictsJson = useJson(resolved.verdicts);
  const groupsJson = useJson(resolved.groups);
  return {
    recorded,
    resolved,
    groups: useMemo(() => parseGroups(groupsJson, design), [groupsJson, design]),
    judge: useMemo(() => parseJudge(design), [design]),
    verdicts: useMemo(() => parseVerdicts(verdictsJson), [verdictsJson]),
    design,
  };
}

export type RunFiles = ReturnType<typeof useRunFiles>;
