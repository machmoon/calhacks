import { OpenOverlayButton } from "./components/OpenOverlayButton";
import { PdfAttachments } from "./PdfAttachments";
import { usePdfAttachments, PDF_DEFAULT_REQUEST } from "./usePdfAttachments";
import { cadProjectBrief } from "@/lib/cad-project-request";
import { useState } from "react";
import { Header } from "@/components";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { PauseIcon, PlayIcon, SquareIcon, StepForwardIcon } from "lucide-react";
import { EnvRow } from "./components/EnvRow";
import { ChecksTable } from "./components/ChecksTable";
import { ArtifactsList, RendersGallery } from "./components/Files";
import { LogPanel } from "./components/LogPanel";
import { ModelPanel } from "./components/ModelPanel";
import { Panel, Pill } from "./components/Panel";
import { PlanPanel } from "./components/PlanPanel";
import { Headlines, SpendLine, Verdict } from "./components/Summary";
import { useWrightRun } from "./useWrightRun";
import type { Domain } from "./types";

const DOMAINS: Domain[] = ["aircraft", "rocket", "balloon"];

const GATE_TONE: Record<string, "ok" | "warn" | "bad" | "info" | "idle"> = {
  running: "info",
  paused: "warn",
  stepping: "warn",
  stopped: "bad",
};

const Wright = () => {
  const w = useWrightRun();
  const pdf = usePdfAttachments(w.running);
  const [domain, setDomain] = useState<Domain>("balloon");
  const [request, setRequest] = useState("");
  const [replayPath, setReplayPath] = useState("");
  const [tab, setTab] = useState<"checks" | "files" | "renders">("checks");
  const [note, setNote] = useState("");

  const live = w.mode === "live";
  const nativeProject = w.state.executionKind === "native_freecad" || Boolean(pdf.attachment) || cadProjectBrief(w.state.request) !== null;
  const nativeRequest = Boolean(pdf.attachment) || cadProjectBrief(request) !== null;
  const displayedEnv = w.env && (nativeProject || nativeRequest)
    ? { ...w.env, items: w.env.items.filter((item) => item.name === "FreeCAD").map((item) => ({ ...item, required: true })) }
    : w.env;
  const canRun = (request.trim().length > 0 || Boolean(pdf.attachment)) && !pdf.blocked && !w.running && (nativeRequest
    ? w.env?.items.some((item) => item.name === "FreeCAD" && item.ok) === true
    : w.env?.ready === true);
  const gateTone = GATE_TONE[w.gate] ?? "idle";
  const start = () => w.start(pdf.attachment ? "aircraft" : domain, request.trim() || PDF_DEFAULT_REQUEST, pdf.attachment ? [pdf.attachment.id] : []);
  const statusLabel = w.mode === "idle" ? "Idle" : w.mode === "replay" ? "Replay" : w.gate.charAt(0).toUpperCase() + w.gate.slice(1);
  // Pause/approve/resume drive the agent harness; a native FreeCAD build has
  // no gate, so those three are hidden for it rather than shown dead.
  const gated = !nativeProject;

  // Layout follows Pluely upstream (github.com/iamsrikanthnani/pluely):
  // src/layouts/PageLayout.tsx's `pt-8` Header with the page action in
  // `rightSlot` (src/pages/dashboard/index.tsx puts its one button there),
  // then cards on a fixed gap. The composer is one bordered card with the
  // field on top and a toolbar under it, the shape of Pluely's completion
  // input (src/pages/app/components/completion/Input.tsx), whose `kbd`-style
  // shortcut hint (`text-[10px] ... rounded border border-input/50`) is
  // reused for Cmd+Enter.
  return (
    <div className="-mr-4 flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pb-4 pr-4 pt-8" data-testid="wright-page" {...pdf.dropProps}>
      <Header
        isMainTitle
        showBorder
        title="Wright"
        description="Describe a vehicle. Wright's agent team designs it and checks every claim."
        rightSlot={<OpenOverlayButton />}
      />

      {/* Composer */}
      <section
        aria-label="New run"
        className={cn(
          "shrink-0 rounded-xl border border-border/70 bg-black/5 transition-colors focus-within:border-ring/60 dark:bg-white/5",
          pdf.dragging && "border-dashed border-ring/60"
        )}
      >
        <textarea
          aria-label="Request"
          rows={3}
          value={request}
          onChange={(e) => setRequest(e.target.value)}
          onKeyDown={(e) => (e.metaKey || e.ctrlKey) && e.key === "Enter" && canRun && start()}
          placeholder="Describe the CAD build or drop a VTOL PDF"
          className="block min-h-[4.5rem] w-full resize-y bg-transparent px-3 pb-1 pt-2.5 text-sm outline-none placeholder:text-muted-foreground"
        />
        <div className="flex flex-wrap items-center gap-2 border-t border-border/70 px-2 py-1.5">
          <select
            aria-label="Domain"
            value={domain}
            onChange={(e) => setDomain(e.target.value as Domain)}
            disabled={w.running || Boolean(pdf.attachment)}
            title={pdf.attachment ? "An attached PDF always builds an aircraft" : undefined}
            className="h-8 rounded-lg border border-input bg-background px-2 text-xs capitalize outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50"
          >
            {DOMAINS.map((d) => (
              <option key={d} value={d}>{d}</option>
            ))}
          </select>
          <div className="min-w-0 flex-1">
            <PdfAttachments pdf={pdf} disabled={w.running} />
          </div>
          <div className="ml-auto flex items-center gap-2">
            <span className="hidden select-none rounded border border-input/50 bg-muted/30 px-1 text-[10px] text-muted-foreground sm:inline">⌘↵</span>
            <Button size="sm" disabled={!canRun} onClick={start}>
              Run
            </Button>
          </div>
        </div>
      </section>

      <EnvRow env={displayedEnv} checking={w.envChecking} onRecheck={() => void w.checkEnv()} />

      {w.error ? (
        <p role="alert" className="rounded-lg border border-destructive/20 bg-destructive/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
          {w.error}
        </p>
      ) : null}

      {/* Status strip and controls */}
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 rounded-xl border border-border/70 bg-black/5 px-3 py-2 dark:bg-white/5">
        <Pill tone={w.mode === "idle" ? "idle" : gateTone}>{statusLabel}</Pill>
        <span className="min-w-[6rem] flex-1 truncate text-xs text-muted-foreground" title={w.state.request}>
          {w.state.domain ? <span className="capitalize">{w.state.domain}: </span> : null}
          {w.state.request || "No run yet"}
        </span>
        <SpendLine state={w.state} />
        <Verdict state={w.state} />
        <div className="flex flex-wrap items-center gap-1.5">
          {live && w.running ? (
            <>
              {gated ? (
                <>
                  <input
                    aria-label="Note for the agents"
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                    placeholder="Note for the agents"
                    className="h-8 w-40 rounded-lg border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  />
                  <Button size="sm" variant="outline" disabled={w.gate === "paused"} onClick={() => { void w.control("pause", note); setNote(""); }}>
                    <PauseIcon /> Pause
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => { void w.control("approve", note); setNote(""); }}>
                    <StepForwardIcon /> Approve step
                  </Button>
                  <Button size="sm" variant="outline" disabled={w.gate === "running"} onClick={() => { void w.control("resume", note); setNote(""); }}>
                    <PlayIcon /> Resume
                  </Button>
                </>
              ) : null}
              <Button
                size="sm"
                variant="destructive"
                onClick={async () => {
                  await w.control("stop", note);
                  setNote("");
                  // Give the harness a moment to checkpoint, then end the child.
                  setTimeout(() => void w.kill(), 4000);
                }}
              >
                <SquareIcon /> Stop
              </Button>
            </>
          ) : null}
          {w.mode === "replay" ? (
            <>
              <Button size="sm" variant="outline" onClick={() => (w.cursor >= w.events.length ? (w.setCursor(0), w.setPlaying(true)) : w.setPlaying(!w.playing))}>
                {w.playing ? <PauseIcon /> : <PlayIcon />} {w.playing ? "Pause" : w.cursor >= w.events.length ? "Replay" : "Play"}
              </Button>
              <input
                aria-label="Replay position"
                type="range"
                min={0}
                max={w.events.length}
                value={w.cursor}
                onChange={(e) => { w.setPlaying(false); w.setCursor(Number(e.target.value)); }}
                className="w-32 accent-primary"
              />
              <span className="w-14 text-right text-[10px] tabular-nums text-muted-foreground">{w.cursor}/{w.events.length}</span>
            </>
          ) : null}
          {!w.running ? (
            <>
              <select
                aria-label="Recording"
                value={replayPath}
                onChange={(e) => setReplayPath(e.target.value)}
                onFocus={() => void w.refreshRuns()}
                className="h-8 max-w-[12rem] rounded-lg border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <option value="">Recording…</option>
                {w.runs.map((r) => (
                  <option key={r.path} value={r.path}>
                    {r.kind === "demo" ? "demo/" : "runs/"}{r.name} ({r.events})
                  </option>
                ))}
              </select>
              <Button size="sm" variant="outline" disabled={!replayPath} onClick={() => w.replay(replayPath, true)}>
                Replay
              </Button>
            </>
          ) : null}
        </div>
      </div>

      {/* Workbench: two columns from lg up; below that it stacks and the page scrolls. */}
      <div className="grid shrink-0 grid-cols-1 gap-3 lg:min-h-[26rem] lg:flex-1 lg:shrink lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <div className="flex min-h-[24rem] flex-col gap-3 lg:min-h-0">
          <PlanPanel goal={w.state.planGoal} tasks={w.state.plan} />
          <LogPanel log={w.state.log} />
        </div>
        <div className="flex flex-col gap-3 lg:min-h-0 lg:overflow-auto">
          <ModelPanel runDir={w.runDir} items={w.state.artifacts} />
          <Headlines items={w.state.headlines} />
          <Panel
            title="Evidence"
            className="min-h-48 flex-1"
            right={
              <div role="tablist" aria-label="Evidence" className="flex gap-0.5 rounded-lg bg-muted/60 p-0.5">
                {(["checks", "files", "renders"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    role="tab"
                    aria-selected={tab === t}
                    onClick={() => setTab(t)}
                    className={cn(
                      "rounded-md px-2 py-0.5 text-[10px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 lg:text-xs",
                      tab === t ? "bg-background font-medium text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {t === "checks" ? `Checks${w.state.checks ? ` (${w.state.checks.failed ? w.state.checks.failed + " failing" : "all pass"})` : ""}` : t === "files" ? `Files (${w.state.artifacts.length})` : `Renders (${w.state.renders.length})`}
                  </button>
                ))}
              </div>
            }
          >
            {tab === "checks" ? (
              <ChecksTable outcomes={w.state.checks?.outcomes ?? []} reports={w.state.checkReports} />
            ) : tab === "files" ? (
              <ArtifactsList runDir={w.runDir} items={w.state.artifacts} />
            ) : (
              <RendersGallery runDir={w.runDir} items={w.state.renders} />
            )}
          </Panel>
        </div>
      </div>
    </div>
  );
};

export default Wright;
