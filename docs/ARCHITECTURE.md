# Wright: architecture

Wright designs vehicles a person can go build -- a VTOL drone, a student
rocket down to its engine, a high-altitude balloon -- with a team of Claude
agents whose only way to act is calling deterministic engineering tools, and
whose every claim must survive checks that cannot pass by omission.

Built at Cal Hacks 2026 (from 2026-10-24). It is a rebuild, not a port, of an
earlier prototype (the Python baseline's `silkscreen.aircraft`), informed by what went
wrong there. That code is not copied here.

## What was wrong with the prototype, and the rule each lesson became

Thirteen real defects were found in the prototype in one day (four by
inspecting renders, nine by an adversarial reviewer agent). Every one traces
to an architectural cause:

| Defect (prototype) | Cause | Rule in Wright |
|---|---|---|
| prop discs never checked against each other; receiver drawn inside the FC; battery wholly outside the pod passed | checks iterate what someone remembered to list | geometry checks run over **all pairs** of a part set, never a hand list |
| no servo -> no servo check -> pass; missing ESC -> zero mass; infeasible hover -> 0 A -> full current margin | `if x: check(x)`: absence meant pass | `wright_core::check`: a closed enum of checks; unevaluated or missing evidence **fails**; NaN fails |
| `wire_ampacity` used the same inequality as the wire sizing | checks restated the sizing rule | every check names its `Source`; a check must use a model independent of the rule that chose the part |
| layout and CAD each defined prop discs, servo pocket, bay -- and drifted | two definitions of one geometry | one geometric model; physics and CAD both read it |
| 4 motors at full throttle assumed a stiff pack; no burst limit; no voltage ratings | components modelled alone | system-level solves (electrical network with sag and ratings) |
| nothing carried the wing's root moment across the centreline | load paths implicit | explicit load paths: every load reaches the CG through named members |
| neutral-point term 57x too small (per degree vs per radian) | bare floats in mixed units | `wright_core::units` newtypes; constructors name the unit |
| optimizer "won" with unpriced batteries | missing data read as 0 | catalog rows validated per role on load; unpriced = failed budget |
| requirements-only specs crashed the search; eval only ever ran from a full design | no seed, untested path | every domain designs from a requirements-only spec; evals start there |
| coordinate descent over string part IDs, minutes per run in Python | slow, local search | Rust; enumerate discrete part combos with continuous inner sizing, bounded by cheap screens |
| the only LLM was a requirements writer; the reviewer that found 9 bugs ran by hand | agents not in the loop | agents in the loop: designers, a hostile reviewer, and every confirmed bug becomes a permanent check |
| no slicing, print time or build steps | stopped at STL | prints are sliced (PrusaSlicer / OrcaSlicer / Bambu Studio CLI); time and grams are checks |

## Crates

| crate | owns |
|---|---|
| `wright-core` | units, `check` (the no-omission report), `part` (BOM, provenance), `event` (the run's event stream), the `Domain` trait |
| `wright-geom` | the one geometric model: meshes/solids, booleans, STL/3MF export, all-pairs interference and clearance, slicer CLI driver |
| `wright-air` | fixed-wing and VTOL quadplanes: aero buildup, propulsion, pack network, structure + load paths, layout, search, ArduPilot params; eval vs the 2025 130-mile build |
| `wright-rocket` | student rockets: motor or liquid engine (chamber, injector, nozzle, regen/ablative), airframe, recovery, stability, 6-DOF-lite trajectory; eval vs a Berkeley rocket-club vehicle |
| `wright-balloon` | high-altitude balloons: lift gas, envelope, ascent/burst, descent under chute, payload power and tracking link budget; eval vs Pat's team balloon |
| `wright-agents` | the Claude harness: orchestrator, specialist and reviewer agents, tools over `Domain`, budget guard, prompt caching; sponsor integrations |
| `wright-desktop` | the native desktop app (egui/eframe): start a run in-process, live events, 3D view with part picking, checks, BOM, log, spend, analysis media, build guide, **drawing on the view** -> feedback to the agents, replay with a timeline |
| `wright-guide` | z-buffer STL renderer and the static, printable HTML build guide |
| `wright-cli` | `wright design|check|schema|slice|run|report`; `wright` / `wright app` opens the desktop app |
| `lean/` | Lean 4 proofs about the check semantics (no clause passes on missing evidence; worst outcome wins) |

## Contracts

* A domain implements `wright_core::Domain`. `design` must accept a
  requirements-only spec. `check` returns a `Finished` with one outcome per
  declared check. `bom` lines are priced or the budget check fails.
* Every catalog row carries `url` and `retrieved`. Data shared across domains
  lives under `data/`.
* Every number that changes an answer is a named constant with its source in
  a `rules.rs` of its crate. Unsourced assumptions say so.
* Events (`wright_core::Event`) are the only channel between the harness and
  the desktop app (live, in-process on a broadcast channel; replayed from
  `events.jsonl`).
* Determinism: the same spec designs the same vehicle twice.

## Desktop app (`crates/wright-desktop`)

Wright is not a web app: there is no server, no browser page, no container.
`wright-desktop` (also `wright` with no arguments, or `wright app`) is one
native window.

* **Live run.** Domain + request -> `wright_agents::run` on a tokio runtime
  on a background thread (the same call `wright run` makes); events go to the
  window through a std channel and to `runs/<id>/events.jsonl`
  (`harness::spawn_event_log`). "Offline" uses `wright_agents::fake`
  (design -> submit -> approve): real domain code and checks, no model, $0.
* **Replay.** `--replay events.jsonl` or drop the file on the window. A
  cursor over the recorded events, play/pause/speed and a scrubber; the state
  is re-folded from the start on a backward scrub. At a recorded `Feedback`
  the replay stops and hands the pen over (ported from the web studio's
  replay, same rules: early markup is refused with "markup is replayed at
  the recorded step" and does not consume the pause).
* **Markup.** Pen / highlight / arrow / eraser over the 3D view or a render;
  the sent PNG is a screenshot of exactly what the person saw, saved to
  `<run dir>/feedback/` and sent as `Event::Feedback` with the current run
  id (an empty id is refused). The selected part's name is appended to the
  note. Replay sketches go to a temp dir, never into a recording.
* **3D.** STLs from the run's `Artifact` events (assembly if there is one,
  else every non-jig STL; same rule as `wright_guide::from_artifacts`), read
  with `wright_geom::Mesh::read_stl`, drawn with OpenGL in an egui paint
  callback, orbit/pan/zoom, click to select (CPU ray cast). A single-file
  assembly is split into connected pieces so clicks select something.

Prior art read and followed: egui `examples/custom_3d_glow` (GL in a paint
callback), `crates/egui_demo_lib/src/demo/painting.rs` (strokes),
`examples/screenshot` (capturing the view), egui_commonmark (checklists).
**Rerun was evaluated and not used**: `rerun` 0.40 with `native_viewer`
resolves to 778 crates (arrow, datafusion, wgpu, re_* ...) against 183 for
eframe + egui_extras, i.e. several GB of shared `target/` on a disk with
~8 GB free; and its viewer is built to be fed logs, while Wright's window
must also host a live harness, markup that goes back to the agents, and a
replay that pauses for a person (Rerun's `examples/rust/extend_viewer_ui`
only adds side panels around its viewer).

## Artifacts manifest

Analysis agents (CFD, FEA, fabrication timelapses, parametric CAD/USD,
flight sim) write their outputs under the run's out dir, one directory per
kind, each with a `manifest.json`:

```
<out_dir>/<kind>/manifest.json
{
  "kind": "cfd" | "fea" | "fab" | "cad" | "usd" | "sim",
  "title": "string",
  "items": [
    {
      "label": "string",
      "path": "string, relative to the manifest's directory",
      "media": "mp4" | "png" | "gif" | "step" | "usda" | "json" | "csv",
      "summary": "string",
      "metrics": { "name": number }
    }
  ]
}
```

* `path` must stay inside the manifest's directory: absolute paths, `..`,
  `.` and symlinks that resolve outside it are refused (shown as an error).
* `summary` and `metrics` may be omitted (empty). Unknown `media` values are
  listed with Open/Reveal buttons.
* The desktop app rescans every 3 s while a run is open, so agents can write
  manifests as they finish. Videos are played from frames extracted once by
  ffmpeg (`WRIGHT_FFMPEG`, else /opt/homebrew/bin/ffmpeg) into a temp cache;
  png/gif are shown inline; step/usda get "Open in Finder"; json/csv "Open".
* Rust type: `wright_desktop::media::Manifest`.
