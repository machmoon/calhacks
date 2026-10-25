# Wright desktop UI redesign (2026-10-24)

Goal: a new person opens Wright and finds nothing unfamiliar, and a judge sees
the design and its evidence, not a log. The real tools (KiCad, FreeCAD) open on
demand; the app shows the interesting summary.

The app is `app/` in this repo (Tauri 2 + React, crate `wright-app`); the
page is `src/pages/wright/`. The desktop rule in `CLAUDE.md` still holds: no
engine server, no "start the engine" gate, a quiet status row, one plain
sentence plus the fix when something fails.

## Layout

```
┌ app nav ┬──────────────────────────────────────────────────────┬──────────────────┐
│ Wright  │ Wright / demo/aircraft ▾      [Search  ⌘K]  [Overlay] │                  │ toolbar
│ Settings├──────────────────────────────────────────────────────┤ Activity         │
│ Logs    │ Aircraft · "A 3D-printed VTOL quadplane…"   ✓ PASS   │  orchestrator    │
│         │ ●Request──●Design──●Build──◐Check──○Verdict  ▶ ━━━○  │   message text   │
│         │ [Overview][Checks 44][3D][Renders 5][Board][Schem.][Files] [Open in KiCad] │
│         │ ┌──────────────────────────────────────────────────┐ │   ▸ tool  ✓      │
│         │ │ Overview: hero render | headline cards + spark-  │ │  reviewer        │
│         │ │ lines; checks per iteration (before/after bars); │ │   message text   │
│         │ │ failing checks, one sentence each; plan timeline │ │                  │
│         │ │ Board: KiCad SVG layers stacked, layer toggles,  │ │ [note][⏸][⏭][■]  │
│         │ │ ERC/DRC chips. 3D: orbit viewer. Renders: hero + │ │ ┌──────────────┐ │
│         │ │ filmstrip.                                       │ │ │ Describe…    │ │ composer
│         │ └──────────────────────────────────────────────────┘ │ │ ▾dom 📎   Run│ │
├─────────┴──────────────────────────────────────────────────────┴──────────────────┤
│ ● wright ● Claude ● network  FreeCAD  KiCad  PrusaSlicer · Recheck    $0.16 / $5  │ status bar
└───────────────────────────────────────────────────────────────────────────────────┘
```

- Left: the app's existing nav sidebar (unchanged).
- Centre: the workspace. Run header, stage stepper, artifact tabs. Tabs appear
  only when the run has that kind of artifact (Board/Schematic only for a
  `.kicad_pcb`/`.kicad_sch`, 3D only for GLB/STL), Overview/Checks/Files always.
  Contextual hand-off buttons sit at the right of the tab bar.
- Right: Activity, a chat-style stream of agent messages (tool calls folded to
  one line with a pass/fail icon), live controls, and the composer at the
  bottom.
- Bottom: a status bar with the environment badges and spend. A required item
  that fails shows one sentence with the fix above the workspace.
- Cmd+K: a command palette with runs (replay), actions (new run, recheck,
  open overlay, open in KiCad/FreeCAD) and tab switches.
- Empty state: example prompts per domain and the four most recent recordings.

## Where each convention comes from (source read, not docs)

| Decision | Source |
|---|---|
| Centre workspace with a right-docked chat/agent panel and a bottom status bar | VS Code `src/vs/workbench/browser/parts/auxiliarybar/auxiliaryBarPart.ts` (the secondary side bar Copilot Chat lives in); Zed `crates/workspace/src/dock.rs` (`DockPosition::{Left, Bottom, Right}`) and `assets/settings/default.json` (project, outline, git, collab panels default `"dock": "right"`; Zed's agent panel defaults left, so this is VS Code's choice, not Zed's) |
| Command palette: Dialog > Input > List > Group(heading) > Item, arrow keys, Enter, `aria-activedescendant`, `role=option`, `aria-selected` | cmdk `cmdk/src/index.tsx` (pacocoursey/cmdk). Built on the already-installed `@radix-ui/react-dialog`, as cmdk's own `Dialog` is; cmdk itself is not installed and not added |
| Artifact panel: Radix Tabs root, title left, tab switch plus actions (refresh, download, close) right | LibreChat `client/src/components/Artifacts/Artifacts.tsx` |
| Stage stepper: horizontal, completed/progress icons, separators between steps | Mantine `packages/@mantine/core/src/components/Stepper/Stepper.tsx` (`completedIcon`, `progressIcon`, `orientation`) |
| Plan as a vertical timeline with a badge per item | Primer `packages/react/src/Timeline/Timeline.tsx` (`Timeline.Item`, `Timeline.Badge`, `condensed`) |
| Checks: a severity icon, the message, the source, problems first | VS Code `src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts` (`SeverityIcon` + message + source columns) |
| PCB layer list: colour swatch, eye toggle per layer, presets All / Front / Back / Copper | KiCanvas `src/kicanvas/elements/kc-board/layers-panel.ts` (MIT) |
| 3D: left drag orbit, right/shift drag pan, wheel zoom, arrow keys | three.js `examples/jsm/controls/OrbitControls.js` (`mouseButtons`, `keys`); the app's own WebGL2 renderer `src/lib/gltf/renderer.ts` already follows it, so no three.js install |
| Small bar lists for comparisons | Tremor `src/components/BarList/BarList.tsx` (bar width as % of max, label inside, value right) |

Deviations, stated: no three.js or `<model-viewer>` (not installed; the
in-house renderer already orbits). The PCB is drawn from `kicad-cli pcb export
svg --mode-multi` per layer, stacked, rather than parsed in the browser the way
KiCanvas does, because KiCad's own plot is the authority and costs nothing to
add. Schematic is `kicad-cli sch export svg`.

## Files

```
app/src/pages/wright/
  index.tsx                 page shell: toolbar, workspace, activity, status bar, palette
  reducer.ts                + checkHistory, headlineHistory, stages(), failureSentence(), pick*()
  types.ts                  + KicadLayer
  api.ts                    + wrightKicadSvg, wrightOpenIn
  components/
    Toolbar.tsx             breadcrumb run switcher, Cmd+K button, overlay
    CommandPalette.tsx      cmdk-shaped palette on Radix Dialog
    Stepper.tsx             Request -> Design -> Build -> Check -> Verdict
    Workspace.tsx           run header, tabs, hand-off buttons
    Overview.tsx            hero render, headline cards with sparklines, iteration chart, failures, plan
    Charts.tsx              Sparkline, IterationBars (plain SVG)
    ChecksTable.tsx         restyled: icon + name + one sentence + margin
    PlanPanel.tsx           restyled as a timeline (same data-status contract)
    BoardViewer.tsx         stacked KiCad SVG layers, toggles, pan/zoom, ERC/DRC chips
    Renders.tsx             hero + filmstrip
    Files.tsx               icons per type, Open / Open in KiCad / Open in FreeCAD
    ActivityPanel.tsx       chat-style agent stream (replaces LogPanel in the page)
    Composer.tsx            the request box, moved into the activity column
    StatusBar.tsx           env badges + spend (replaces EnvRow in the page)
    EmptyState.tsx          example prompts + recent recordings
app/src-tauri/src/wright.rs  + wright_kicad_svg, wright_open_in (additive)
app/src-tauri/src/lib.rs     register the two commands
```
