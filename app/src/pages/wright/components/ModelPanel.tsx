import { useCallback, useEffect, useMemo, useState } from "react";
import { ModelViewer } from "@/components/ModelViewer";
import { stlToGlb } from "@/lib/gltf/stl";
import { wrightReadFile, wrightResolve } from "../api";
import { Empty, Panel } from "./Panel";

type Artifact = { label: string; path: string };

/**
 * Which files make the 3D view: a recorded GLB if there is one (assembly
 * first), else the assembly STL, else every part STL except print jigs and
 * fixtures. Pure, so it is tested.
 */
export function pickModel(items: Artifact[]): { kind: "glb" | "stl"; paths: string[] } | null {
  const ext = (p: string) => p.toLowerCase().split(".").pop();
  const glbs = items.filter((a) => ext(a.path) === "glb");
  if (glbs.length) {
    const g = glbs.find((a) => /assembl/i.test(a.path + a.label)) ?? glbs[0];
    return { kind: "glb", paths: [g.path] };
  }
  const stls = items.filter((a) => ext(a.path) === "stl");
  const assembly = stls.find((a) => /assembl/i.test(a.path + a.label));
  if (assembly) return { kind: "stl", paths: [assembly.path] };
  const parts = stls.filter((a) => !/jig|fixture|print/i.test(a.path + a.label));
  return parts.length ? { kind: "stl", paths: parts.map((a) => a.path) } : null;
}

export const ModelPanel = ({ runDir, items }: { runDir: string | null; items: Artifact[] }) => {
  const pick = useMemo(() => pickModel(items), [items]);
  const key = pick ? `${runDir}|${pick.kind}|${pick.paths.join(",")}` : "";
  const [resolved, setResolved] = useState<string[] | null>(null);

  useEffect(() => {
    setResolved(null);
    if (!pick || !runDir) return;
    let off = false;
    Promise.all(pick.paths.map((p) => wrightResolve(runDir, p)))
      .then((r) => !off && setResolved(r.filter((x): x is string => !!x)))
      .catch(() => !off && setResolved([]));
    return () => {
      off = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const load = useCallback(async () => {
    if (!pick || !resolved || resolved.length === 0) throw new Error("The model files were not found on this machine.");
    if (pick.kind === "glb") return wrightReadFile(resolved[0]);
    const parts = await Promise.all(
      resolved.map(async (p) => ({ name: p.split("/").pop() ?? p, bytes: await wrightReadFile(p) }))
    );
    return stlToGlb(parts);
  }, [pick, resolved]);

  return (
    <Panel title="3D view" right={pick ? <span>{pick.paths.length} {pick.kind.toUpperCase()}</span> : null} className="h-64 shrink-0" bodyClassName="p-0">
      {!pick ? (
        <Empty>No model yet.</Empty>
      ) : resolved === null ? (
        <Empty>Loading the model…</Empty>
      ) : (
        <ModelViewer path={key} load={load} className="h-full rounded-none border-0" />
      )}
    </Panel>
  );
};
