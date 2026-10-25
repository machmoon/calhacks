import { useCallback, useMemo, useState } from "react";
import { BoxesIcon, CircleCheckIcon, CircleXIcon, TrophyIcon } from "lucide-react";
import { ModelViewer } from "@/components/ModelViewer";
import { stlToGlb } from "@/lib/gltf/stl";
import { cn } from "@/lib/utils";
import { wrightKicadGlb, wrightOpen, wrightReadFile } from "../api";
import { checkRow, headlineMet, type RunState } from "../reducer";
import { failureSentence, type Step, type StepId } from "../story";
import { BarList, IterationBars, Sparkline } from "./Charts";
import { ChecksTable, VERDICT_UI } from "./ChecksTable";
import { ArtifactsList } from "./Files";
import { OpenIn } from "./Handoff";
import { useImageUrl, useResolvedMany } from "./hooks";
import { KicadView } from "./KicadView";
import { pickModel } from "./ModelPanel";
import { PlanPanel } from "./PlanPanel";
import type { RunFiles } from "./runFiles";

export interface ViewProps {
  state: RunState;
  runDir: string | null;
  files: RunFiles;
  steps: Step[];
  go: (id: StepId | "files" | "allchecks") => void;
}

const num = (v: number) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2).replace(/\.?0+$/, ""));

/** A titled block inside a view. Section, not card: spacing carries it. */
const Section = ({ title, right, children, className }: { title: string; right?: React.ReactNode; children: React.ReactNode; className?: string }) => (
  <section className={cn("min-w-0", className)}>
    <div className="mb-2 flex items-center justify-between gap-2">
      <h3 className="text-xs font-medium text-muted-foreground">{title}</h3>
      {right}
    </div>
    {children}
  </section>
);

const Missing = ({ children }: { children: React.ReactNode }) => (
  <div className="flex h-full min-h-48 items-center justify-center rounded-lg border border-dashed border-border/80 p-6 text-center text-sm text-muted-foreground">{children}</div>
);

// ---- Prompt (overview) --------------------------------------------------------

export const PromptView = ({ state, files, steps, go }: ViewProps) => {
  const rows = (state.checks?.outcomes ?? []).map(checkRow);
  const bad = rows.filter((r) => r.verdict !== "pass");
  const kicad = ["erc", "drc", "parity"].map((n) => rows.find((r) => r.name === n)).filter(Boolean);
  const next = steps.find((s) => s.id !== "prompt" && (s.status === "done" || s.status === "failed"));
  return (
    <div className="space-y-6">
      <figure className="border-l-2 border-primary/60 pl-4">
        <blockquote className="text-lg font-medium leading-snug tracking-tight text-pretty">{state.request || "No request recorded."}</blockquote>
        <figcaption className="mt-1 text-xs capitalize text-muted-foreground">{state.domain || "run"}{state.run ? ` · ${state.run}` : ""}</figcaption>
      </figure>

      {state.headlines.length ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-2" data-testid="wright-headlines">
          {state.headlines.map((h) => {
            const met = headlineMet(h);
            return (
              <div key={h.label} className="rounded-lg bg-muted/40 px-3 py-2.5" data-met={String(met)}>
                <p className="text-xs text-muted-foreground">{h.label}</p>
                <div className="mt-0.5 flex items-end justify-between gap-2">
                  <p className="text-xl font-semibold tabular-nums tracking-tight">
                    {num(h.value)} <span className="text-xs font-normal text-muted-foreground">{h.unit}</span>
                  </p>
                  <Sparkline values={state.headlineHistory[h.label] ?? []} limit={h.bound === "info" ? null : h.limit} good={met} />
                </div>
                {h.limit !== null && h.bound !== "info" ? (
                  <p className={cn("text-[11px]", met === false ? "text-red-600 dark:text-red-400" : "text-muted-foreground")}>
                    {h.bound === "at_least" ? "needs ≥" : "needs ≤"} {num(h.limit)}
                  </p>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : null}

      <div className="grid gap-6 md:grid-cols-[minmax(0,1fr)_auto]">
        <Section title="Checks" right={<button type="button" onClick={() => go("allchecks")} className="text-xs text-muted-foreground hover:text-foreground">See all</button>}>
          {rows.length === 0 ? (
            <p className="text-sm text-muted-foreground">No checks yet.</p>
          ) : (
            <div className="space-y-2">
              <p className="text-sm">
                <span className="text-2xl font-semibold tabular-nums tracking-tight">{rows.length - bad.length}</span>
                <span className="text-muted-foreground"> of {rows.length} pass</span>
                {kicad.length ? (
                  <span className="ml-3 inline-flex gap-1 align-middle">
                    {kicad.map((r) => (
                      <span key={r!.name} className={cn("rounded px-1.5 py-0.5 text-[11px] font-medium uppercase", r!.verdict === "pass" ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400" : "bg-red-500/15 text-red-700 dark:text-red-400")}>
                        {r!.name}
                      </span>
                    ))}
                  </span>
                ) : null}
              </p>
              <ul className="space-y-1.5">
                {bad.slice(0, 4).map((r) => {
                  const v = VERDICT_UI[r.verdict];
                  return (
                    <li key={r.name} className="flex gap-2 text-sm">
                      <v.icon className={cn("mt-0.5 size-4 shrink-0", v.cls)} aria-label={v.label} />
                      <span className="text-foreground/85">{failureSentence(r)}</span>
                    </li>
                  );
                })}
                {bad.length > 4 ? <li className="pl-6 text-xs text-muted-foreground">and {bad.length - 4} more</li> : null}
              </ul>
            </div>
          )}
        </Section>
        {state.checkHistory.length > 1 ? (
          <Section title="Per iteration">
            <IterationBars history={state.checkHistory} />
          </Section>
        ) : null}
      </div>

      {files.groups.length ? (
        <Section title="Part groups">
          <GroupChips groups={files.groups} />
        </Section>
      ) : null}

      {next ? (
        <button type="button" onClick={() => go(next.id)} className="text-sm text-primary underline-offset-4 hover:underline">
          Next: {next.label} →
        </button>
      ) : null}
    </div>
  );
};

// ---- Plan and groups ------------------------------------------------------------

const GroupChips = ({ groups }: { groups: RunFiles["groups"] }) => (
  <ul className="grid gap-2 sm:grid-cols-2" data-testid="wright-groups">
    {groups.map((g) => (
      <li key={g.name} className="rounded-lg bg-muted/40 p-3">
        <p className="flex items-center gap-1.5 text-sm font-medium"><BoxesIcon className="size-3.5 text-muted-foreground" /> {g.name}</p>
        <div className="mt-1.5 flex flex-wrap gap-1">
          {g.members.map((m) => (
            <span key={m} className="rounded-[5px] border border-border/80 bg-background px-1.5 py-0.5 font-mono text-[11px]">{m}</span>
          ))}
        </div>
        {g.reason ? <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{g.reason}</p> : null}
      </li>
    ))}
  </ul>
);

export const PlanView = ({ state, files }: ViewProps) => (
  <div className="space-y-6">
    <Section title="Claude's plan">
      {state.plan.length ? <PlanPanel goal={state.planGoal} tasks={state.plan} /> : <p className="text-sm text-muted-foreground">This run wrote no agent plan.</p>}
    </Section>
    <Section title="Part groups, with the reason for each">
      {files.groups.length ? <GroupChips groups={files.groups} /> : <p className="text-sm text-muted-foreground">No placement groups recorded.</p>}
    </Section>
  </div>
);

// ---- Schematic and board -------------------------------------------------------------

export const SchematicView = ({ files }: ViewProps) =>
  files.resolved.sch ? (
    <KicadView path={files.resolved.sch} kind="sch" className="h-full min-h-[24rem]" />
  ) : (
    <Missing>{files.recorded.sch ? "The schematic file is not on this machine." : "No schematic yet."}</Missing>
  );

export const BoardView = ({ files }: ViewProps) => {
  const j = files.judge;
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      {files.resolved.pcb ? (
        <KicadView path={files.resolved.pcb} kind="pcb" className="min-h-[22rem] flex-1" />
      ) : (
        <Missing>{files.recorded.pcb ? "The board file is not on this machine." : "No board yet."}</Missing>
      )}
      {j && j.candidates.length ? (
        <Section
          title={`Placement: ${j.seeds ? `${j.seeds} seeds, ` : ""}${j.candidates.length} routed and judged`}
          right={<span className="text-[11px] text-muted-foreground">fewest blocked nets, then vias, then copper</span>}
        >
          <BarList
            bars={j.candidates.map((c) => ({
              key: String(c.seed),
              value: c.copperMm,
              highlight: c.winner,
              muted: c.blocking > 0,
              label: (
                <>
                  {c.winner ? <TrophyIcon className="size-3.5 text-emerald-600 dark:text-emerald-400" aria-label="kept" /> : null}
                  <span className={cn("font-medium", c.winner && "text-emerald-700 dark:text-emerald-300")}>Seed {c.seed}</span>
                  <span className="text-muted-foreground">{c.vias} vias{c.blocking ? ` · ${c.blocking} blocked` : ""}</span>
                </>
              ),
              display: `${c.copperMm.toFixed(0)} mm`,
            }))}
          />
        </Section>
      ) : null}
    </div>
  );
};

// ---- KiCad checks -------------------------------------------------------------------

const KICAD_TITLE: Record<string, string> = { erc: "ERC", drc: "DRC", parity: "Parity" };
const KICAD_WHAT: Record<string, string> = {
  erc: "Electrical rules on the schematic",
  drc: "Design rules on the routed board",
  parity: "Schematic and board describe the same circuit",
};

export const KicadChecksView = ({ state, files }: ViewProps) => {
  const rows = (state.checks?.outcomes ?? []).map(checkRow);
  const names = ["erc", "drc", "parity"];
  const cards = names
    .map((n) => ({ n, row: rows.find((r) => r.name === n), v: files.verdicts.find((v) => v.check === n) }))
    .filter((c) => c.row || c.v);
  if (cards.length === 0) return <Missing>No KiCad verdicts yet.</Missing>;
  const version = files.verdicts.find((v) => v.toolVersion)?.toolVersion;
  return (
    <div className="space-y-5">
      <div className="grid gap-3 sm:grid-cols-3">
        {cards.map(({ n, row, v }) => {
          const pass = row ? row.verdict === "pass" : v ? v.ran && v.errors === 0 && v.unconnected === 0 : false;
          const Icon = pass ? CircleCheckIcon : CircleXIcon;
          return (
            <div key={n} data-verdict={pass ? "pass" : "fail"} className={cn("rounded-xl p-4", pass ? "bg-emerald-500/10" : "bg-red-500/10")}>
              <div className="flex items-center justify-between">
                <p className="text-lg font-semibold tracking-tight">{KICAD_TITLE[n]}</p>
                <Icon className={cn("size-6", pass ? "text-emerald-600 dark:text-emerald-400" : "text-red-600 dark:text-red-400")} aria-label={pass ? "pass" : "fail"} />
              </div>
              <p className="mt-0.5 text-xs text-muted-foreground">{KICAD_WHAT[n]}</p>
              {v ? (
                <dl className="mt-3 grid grid-cols-3 gap-1 text-center">
                  {([["errors", v.errors], ["unconnected", v.unconnected], ["warnings", v.warnings]] as const).map(([k, x]) => (
                    <div key={k}>
                      <dd className={cn("text-lg font-semibold tabular-nums", k !== "warnings" && x > 0 && "text-red-600 dark:text-red-400")}>{x}</dd>
                      <dt className="text-[10px] text-muted-foreground">{k}</dt>
                    </div>
                  ))}
                </dl>
              ) : null}
            </div>
          );
        })}
      </div>
      <p className="text-xs text-muted-foreground">Run by kicad-cli{version ? ` ${version}` : ""}, KiCad's own checker. Warnings are listed in the report; only errors and unconnected items fail.</p>
      <Section title="Every check on this board">
        <ChecksTable outcomes={state.checks?.outcomes ?? []} reports={state.checkReports} compact />
      </Section>
    </div>
  );
};

// ---- 3D -----------------------------------------------------------------------------------

export const Board3DView = ({ files }: ViewProps) => {
  const pcb = files.resolved.pcb;
  const load = useCallback(() => (pcb ? wrightKicadGlb(pcb) : Promise.reject(new Error("No board file."))), [pcb]);
  if (!pcb) return <Missing>No board yet.</Missing>;
  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <ModelViewer path={`board3d|${pcb}`} load={load} className="min-h-[22rem] flex-1" />
      <p className="text-xs text-muted-foreground">Exported by kicad-cli with KiCad's library 3D models. The STEP for CAD is beside it{files.resolved.boardStep ? "" : " once the run writes it"}.</p>
    </div>
  );
};

export const AirframeView = ({ state, runDir, files }: ViewProps) => {
  const mesh = files.resolved.assemblyMesh;
  const load = useCallback(async () => {
    if (!mesh) throw new Error("No assembly mesh.");
    if (mesh.toLowerCase().endsWith(".glb")) return wrightReadFile(mesh);
    return stlToGlb([{ name: mesh.split("/").pop() ?? "assembly", bytes: await wrightReadFile(mesh) }]);
  }, [mesh]);
  const fit = (state.checks?.outcomes ?? []).map(checkRow).find((r) => r.name === "board_fits_bay");
  const hero = state.renders.find((r) => /vtol|assembl|seat|airframe/i.test(r.view)) ?? state.renders[0];
  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {mesh ? (
        <ModelViewer path={`airframe|${mesh}`} load={load} className="min-h-[22rem] flex-1" />
      ) : hero ? (
        <RenderImage runDir={runDir} recorded={hero.path} alt={hero.view} className="min-h-[22rem] flex-1" />
      ) : (
        <Missing>No airframe assembly in this run.</Missing>
      )}
      {fit ? (
        <p className="flex items-start gap-2 text-sm">
          {(() => { const v = VERDICT_UI[fit.verdict]; return <v.icon className={cn("mt-0.5 size-4 shrink-0", v.cls)} aria-label={v.label} />; })()}
          <span>{fit.verdict === "pass" ? `Fits the bay: ${fit.detail}` : failureSentence(fit)}</span>
        </p>
      ) : null}
    </div>
  );
};

const RenderImage = ({ runDir, recorded, alt, className }: { runDir: string | null; recorded: string; alt: string; className?: string }) => {
  const [path] = useResolvedMany(runDir, [recorded]);
  const { url, error } = useImageUrl(path);
  return (
    <div className={cn("flex items-center justify-center overflow-hidden rounded-lg bg-muted/30", className)}>
      {url ? (
        <button type="button" className="h-full w-full" onClick={() => path && wrightOpen(path)} title="Open full size">
          <img src={url} alt={alt} className="h-full w-full object-contain" draggable={false} />
        </button>
      ) : (
        <span className="text-xs text-muted-foreground">{path === null ? "Not found on this machine" : error ?? "Loading…"}</span>
      )}
    </div>
  );
};

export const Model3DView = ({ state, runDir }: ViewProps) => {
  const pick = useMemo(() => pickModel(state.artifacts), [state.artifacts]);
  const resolved = useResolvedMany(runDir, pick?.paths ?? []);
  const ready = resolved.every((p) => p !== undefined);
  const found = resolved.filter((p): p is string => !!p);
  const load = useCallback(async () => {
    if (!pick || found.length === 0) throw new Error("The model files were not found on this machine.");
    if (pick.kind === "glb") return wrightReadFile(found[0]);
    return stlToGlb(await Promise.all(found.map(async (p) => ({ name: p.split("/").pop() ?? p, bytes: await wrightReadFile(p) }))));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pick, found.join("|")]);
  if (!pick) return <Missing>No model yet.</Missing>;
  if (!ready) return <Missing>Loading the model…</Missing>;
  return <ModelViewer path={`${runDir}|${found.join(",")}`} load={load} className="h-full min-h-[22rem]" />;
};

export const RendersView = ({ state, runDir }: ViewProps) => {
  const [sel, setSel] = useState(0);
  if (state.renders.length === 0) return <Missing>No renders yet.</Missing>;
  const cur = state.renders[Math.min(sel, state.renders.length - 1)];
  return (
    <div className="flex h-full min-h-0 flex-col gap-3" data-testid="wright-renders">
      <RenderImage runDir={runDir} recorded={cur.path} alt={`${cur.view} render`} className="min-h-[20rem] flex-1" />
      <div className="flex gap-2 overflow-x-auto pb-1" role="tablist" aria-label="Views">
        {state.renders.map((r, i) => (
          <button
            key={r.view}
            type="button"
            role="tab"
            aria-selected={i === sel}
            onClick={() => setSel(i)}
            className={cn("w-28 shrink-0 overflow-hidden rounded-md border text-left transition-colors", i === sel ? "border-primary" : "border-border/70 hover:border-foreground/40")}
          >
            <Thumb runDir={runDir} recorded={r.path} alt={r.view} />
            <span className="block truncate px-1.5 py-1 text-[11px] text-muted-foreground">{r.view}</span>
          </button>
        ))}
      </div>
    </div>
  );
};

const Thumb = ({ runDir, recorded, alt }: { runDir: string | null; recorded: string; alt: string }) => {
  const [path] = useResolvedMany(runDir, [recorded]);
  const { url } = useImageUrl(path);
  return <div className="aspect-[4/3] bg-muted/40">{url ? <img src={url} alt={alt} className="h-full w-full object-cover" draggable={false} /> : null}</div>;
};

export const ChecksView = ({ state }: ViewProps) => (
  <div className="space-y-5">
    {state.checkHistory.length > 1 ? (
      <Section title="Checks per iteration">
        <div className="flex items-end gap-4">
          <IterationBars history={state.checkHistory} />
          <p className="pb-5 text-xs text-muted-foreground">
            {state.checkHistory[0].passed} → {state.checkHistory[state.checkHistory.length - 1].passed} passing
          </p>
        </div>
      </Section>
    ) : null}
    <ChecksTable outcomes={state.checks?.outcomes ?? []} reports={state.checkReports} />
  </div>
);

export const FilesView = ({ state, runDir, files }: ViewProps) => (
  <div className="space-y-4">
    <div className="flex flex-wrap gap-2">
      <OpenIn app="kicad" path={files.resolved.pcb} label="Board in KiCad" />
      <OpenIn app="kicad" path={files.resolved.sch} label="Schematic in KiCad" />
      <OpenIn app="freecad" path={files.resolved.fcstd ?? files.resolved.boardStep} label="Open in FreeCAD" />
    </div>
    <ArtifactsList runDir={runDir} items={state.artifacts} />
  </div>
);

