import { useEffect, useMemo, useRef, useState } from "react";
import { EyeIcon, EyeOffIcon, MaximizeIcon, MinusIcon, PlusIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { wrightKicadSvg } from "../api";

/**
 * A board or schematic as KiCad plots it. kicad-cli writes one SVG per
 * layer on one shared viewBox (`pcb export svg --mode-multi`), so the
 * layers stack as images and a toggle is a visibility flip.
 *
 * The layer list (colour swatch, eye per layer, presets All / Front / Back /
 * Copper) follows KiCanvas, src/kicanvas/elements/kc-board/layers-panel.ts
 * (MIT). Pan by dragging, zoom with the wheel around the cursor, Fit resets:
 * the same mapping as the 3D view and KiCad itself.
 */

const ORDER = ["B.Cu", "F.Cu", "F.Mask", "F.Silkscreen", "F.SilkS", "F.Fab", "F.Courtyard", "F.CrtYd", "Edge.Cuts"];
const NAMES: Record<string, string> = {
  "B.Cu": "Back copper",
  "F.Cu": "Front copper",
  "F.Mask": "Front mask",
  "F.Silkscreen": "Silkscreen",
  "F.SilkS": "Silkscreen",
  "F.Fab": "Fabrication",
  "F.Courtyard": "Courtyards",
  "F.CrtYd": "Courtyards",
  "Edge.Cuts": "Board edge",
};
const DEFAULT_ON = new Set(["B.Cu", "F.Cu", "F.Silkscreen", "F.SilkS", "Edge.Cuts"]);
const PRESETS: Record<string, (l: string) => boolean> = {
  All: () => true,
  Front: (l) => l.startsWith("F.") || l === "Edge.Cuts",
  Back: (l) => l.startsWith("B.") || l === "Edge.Cuts",
  Copper: (l) => l.endsWith(".Cu") || l === "Edge.Cuts",
};

/** The layer's own colour: the commonest fill that is not black or white. */
function swatch(svg: string): string {
  const counts = new Map<string, number>();
  for (const m of svg.matchAll(/#[0-9A-Fa-f]{6}/g)) {
    const c = m[0].toUpperCase();
    if (c === "#FFFFFF" || c === "#000000") continue;
    counts.set(c, (counts.get(c) ?? 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "#888888";
}

interface Layer { name: string; url: string; color: string }

export const KicadView = ({ path, kind, className }: { path: string; kind: "pcb" | "sch"; className?: string }) => {
  const [layers, setLayers] = useState<Layer[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [on, setOn] = useState<Set<string>>(DEFAULT_ON);
  const [view, setView] = useState({ k: 1, x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let off = false;
    const made: string[] = [];
    setLayers(null);
    setError(null);
    setView({ k: 1, x: 0, y: 0 });
    wrightKicadSvg(path, kind)
      .then((ls) => {
        if (off) return;
        const sorted = [...ls].sort((a, b) => {
          const ia = ORDER.indexOf(a.layer), ib = ORDER.indexOf(b.layer);
          return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
        });
        setLayers(
          sorted.map((l) => {
            const url = URL.createObjectURL(new Blob([l.svg], { type: "image/svg+xml" }));
            made.push(url);
            return { name: l.layer, url, color: swatch(l.svg) };
          })
        );
      })
      .catch((e) => !off && setError(String(e)));
    return () => {
      off = true;
      made.forEach((u) => URL.revokeObjectURL(u));
    };
  }, [path, kind]);

  const zoomAt = (factor: number, cx?: number, cy?: number) =>
    setView((v) => {
      const r = box.current?.getBoundingClientRect();
      const px = cx ?? (r ? r.width / 2 : 0), py = cy ?? (r ? r.height / 2 : 0);
      const k = Math.min(40, Math.max(0.5, v.k * factor));
      const f = k / v.k;
      // Keep the point under the cursor fixed (transform-origin is top-left).
      return { k, x: px - (px - v.x) * f, y: py - (py - v.y) * f };
    });

  const pcb = kind === "pcb";
  const shown = useMemo(() => (layers ?? []).filter((l) => !pcb || on.has(l.name)), [layers, on, pcb]);

  return (
    <div className={cn("relative flex min-h-0 overflow-hidden rounded-lg border border-border/70", className)}>
      <div
        ref={box}
        data-testid={`kicad-${kind}`}
        className={cn("relative min-h-0 flex-1 cursor-grab touch-none select-none overflow-hidden active:cursor-grabbing", pcb ? "bg-[#0d1714]" : "bg-[#f5f4ef]")}
        onWheel={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - r.left, e.clientY - r.top);
        }}
        onPointerDown={(e) => {
          drag.current = { x: e.clientX, y: e.clientY };
          e.currentTarget.setPointerCapture?.(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (!drag.current) return;
          const dx = e.clientX - drag.current.x, dy = e.clientY - drag.current.y;
          drag.current = { x: e.clientX, y: e.clientY };
          setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy }));
        }}
        onPointerUp={() => (drag.current = null)}
        onDoubleClick={() => setView({ k: 1, x: 0, y: 0 })}
      >
        {error ? (
          <p className="absolute inset-0 flex items-center justify-center p-6 text-center text-xs text-red-500">{error}</p>
        ) : !layers ? (
          <div className="absolute inset-6 animate-pulse rounded-md bg-white/5" aria-label="Plotting with kicad-cli" />
        ) : (
          <div className="absolute inset-0 origin-top-left" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}>
            {shown.map((l) => (
              <img
                key={l.name}
                src={l.url}
                alt={pcb ? `${NAMES[l.name] ?? l.name} layer` : "Schematic"}
                draggable={false}
                className={cn("pointer-events-none absolute inset-4 h-[calc(100%-2rem)] w-[calc(100%-2rem)] object-contain", l.name === "F.Mask" && "opacity-25")}
              />
            ))}
          </div>
        )}
        <div className="absolute bottom-2 left-2 flex items-center gap-1 rounded-md border border-white/10 bg-black/50 p-0.5 text-white/80 backdrop-blur">
          <button type="button" aria-label="Zoom in" className="rounded p-1 hover:bg-white/10" onClick={() => zoomAt(1.3)}><PlusIcon className="size-3.5" /></button>
          <button type="button" aria-label="Zoom out" className="rounded p-1 hover:bg-white/10" onClick={() => zoomAt(1 / 1.3)}><MinusIcon className="size-3.5" /></button>
          <button type="button" aria-label="Fit" className="rounded p-1 hover:bg-white/10" onClick={() => setView({ k: 1, x: 0, y: 0 })}><MaximizeIcon className="size-3.5" /></button>
        </div>
      </div>

      {pcb && layers ? (
        <aside aria-label="Layers" className="flex w-44 shrink-0 flex-col border-l border-border/70 bg-background/60">
          <div className="flex flex-wrap gap-1 border-b border-border/60 p-2">
            {Object.keys(PRESETS).map((p) => (
              <button
                key={p}
                type="button"
                onClick={() => setOn(new Set(layers.map((l) => l.name).filter(PRESETS[p])))}
                className="rounded-md border border-border/70 px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                {p}
              </button>
            ))}
          </div>
          <ul className="min-h-0 flex-1 overflow-auto py-1">
            {[...layers].reverse().map((l) => {
              const visible = on.has(l.name);
              return (
                <li key={l.name}>
                  <button
                    type="button"
                    aria-pressed={visible}
                    onClick={() => setOn((s) => { const n = new Set(s); if (n.has(l.name)) n.delete(l.name); else n.add(l.name); return n; })}
                    className={cn("flex w-full items-center gap-2 px-2.5 py-1 text-left text-xs transition-colors hover:bg-accent", !visible && "text-muted-foreground")}
                  >
                    <span className="size-2.5 shrink-0 rounded-[3px]" style={{ background: l.color, opacity: visible ? 1 : 0.3 }} />
                    <span className="min-w-0 flex-1 truncate" title={l.name}>{NAMES[l.name] ?? l.name}</span>
                    {visible ? <EyeIcon className="size-3.5 shrink-0" /> : <EyeOffIcon className="size-3.5 shrink-0" />}
                  </button>
                </li>
              );
            })}
          </ul>
        </aside>
      ) : null}
    </div>
  );
};
