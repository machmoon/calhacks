import { useEffect, useState } from "react";
import { wrightOpen, wrightReadFile, wrightResolve } from "../api";
import { Empty } from "./Panel";

/** A recorded path found on this machine, or null while unknown / missing. */
export function useResolved(runDir: string | null, recorded: string): string | null | undefined {
  const [path, setPath] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    let off = false;
    setPath(undefined);
    if (!runDir) return;
    wrightResolve(runDir, recorded)
      .then((p) => !off && setPath(p))
      .catch(() => !off && setPath(null));
    return () => {
      off = true;
    };
  }, [runDir, recorded]);
  return path;
}

const ArtifactRow = ({ runDir, label, recorded }: { runDir: string | null; label: string; recorded: string }) => {
  const path = useResolved(runDir, recorded);
  const [err, setErr] = useState<string | null>(null);
  return (
    <li className="flex items-center justify-between gap-2 border-t border-border/30 px-3 py-1 text-[11px] first:border-t-0">
      <div className="min-w-0">
        <p className="truncate">{label}</p>
        <p className="truncate font-mono text-[10px] text-muted-foreground" title={recorded}>{recorded}</p>
        {err ? <p className="text-[10px] text-red-600 dark:text-red-400">{err}</p> : null}
      </div>
      <button
        type="button"
        disabled={!path}
        title={path === null ? "Not found on this machine" : "Open with the default app"}
        onClick={() => path && wrightOpen(path).catch((e) => setErr(String(e)))}
        className="shrink-0 rounded-md border border-input px-2 py-0.5 text-[10px] outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-40"
      >
        {path === null ? "Missing" : "Open"}
      </button>
    </li>
  );
};

export const ArtifactsList = ({ runDir, items }: { runDir: string | null; items: { label: string; path: string }[] }) =>
  items.length === 0 ? (
    <Empty>No files yet.</Empty>
  ) : (
    <ul data-testid="wright-artifacts">
      {items.map((a) => (
        <ArtifactRow key={a.path} runDir={runDir} label={a.label} recorded={a.path} />
      ))}
    </ul>
  );

const RenderTile = ({ runDir, view, recorded }: { runDir: string | null; view: string; recorded: string }) => {
  const path = useResolved(runDir, recorded);
  const [url, setUrl] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let off = false;
    let made: string | null = null;
    wrightReadFile(path)
      .then((bytes) => {
        if (off) return;
        const ext = path.split(".").pop()?.toLowerCase();
        const type = ext === "svg" ? "image/svg+xml" : ext === "png" ? "image/png" : "image/jpeg";
        made = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
        setUrl(made);
      })
      .catch((e) => !off && setErr(String(e)));
    return () => {
      off = true;
      if (made) URL.revokeObjectURL(made);
    };
  }, [path]);
  return (
    <figure className="overflow-hidden rounded-lg border border-border/60">
      <div className="flex aspect-[4/3] items-center justify-center bg-muted/30">
        {url ? (
          <button type="button" className="h-full w-full outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50" onClick={() => path && wrightOpen(path)} title="Open full size">
            <img src={url} alt={`${view} render`} className="h-full w-full object-contain" draggable={false} />
          </button>
        ) : (
          <span className="px-2 text-center text-[10px] text-muted-foreground">{path === null ? "Not found on this machine" : err ?? "Loading…"}</span>
        )}
      </div>
      <figcaption className="px-2 py-1 text-[10px] text-muted-foreground">{view}</figcaption>
    </figure>
  );
};

export const RendersGallery = ({ runDir, items }: { runDir: string | null; items: { view: string; path: string }[] }) =>
  items.length === 0 ? (
    <Empty>No renders yet.</Empty>
  ) : (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-2 p-2" data-testid="wright-renders">
      {items.map((r) => (
        <RenderTile key={r.view} runDir={runDir} view={r.view} recorded={r.path} />
      ))}
    </div>
  );
