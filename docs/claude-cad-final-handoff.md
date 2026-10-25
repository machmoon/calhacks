# AI CAD optimization handoff — 2026-10-24

Pat asked Astra to finish the optimization work and merge it into Wright main
before handing it back to Claude. This replaces the premature handoff in 9aef995.

## Maintained application

Wright's desktop is `app/`, including the overlay PDF workflow merged in
3d706d3. FreeCAD remains the CAD kernel and code/CLI is the primary build path.
Do not develop a separate CAD stack. Preserve unrelated dirty changes in
Pat's original checkout. Downloaded vendor assets and generated runs are ignored.

## Implemented backlog

The ten trace-review items have implementation and regression coverage:
bounded independent probes, audit/render overlap on a frozen revision, bounded
Rust display rasterization and mesh caching, elimination of duplicate bounds and
unnecessary contact distances, isolated edit rebuilds, conservative own-motor
propeller refinement, removable-cover/service geometry, explicit gate mapping,
a revision-bound Wright-air adapter with missing-input reporting, and phase-label
and sub-second-progress fixes. The collision broad phase already existed and
was retained. See `cad-optimization-results.md` for exact scope and measurements.

Do not interpret implementation completion as aircraft qualification. The
Wright-air adapter is wired but this candidate still lacks reconciled native
mass/material/CG and hardware inputs. It refuses to certify a default aircraft.
Full cruise sweep, electrical integration, retention, hardware freeze, motion,
load paths and other incomplete requirements remain visibly pending. Do not
turn those green without new evidence.

## Verification

The current native candidate retains the inverted-V, winglets and rear pusher.
It adds tray-driver ports, underside tail-clamp access and a side-removable
cover. Cover retention/sealing/strength are not qualified. All 44 Python tests
passed together. Native negative controls and export/reopen checks passed;
actual timed-run measurements are recorded in the adjacent results document.
The PDF feature remains a prepared, bounded recipe, not arbitrary synthesis.

## Operational notes

Default audit concurrency is three independent FreeCAD processes; configure
`WRIGHT_AUDIT_WORKERS` and `WRIGHT_PROBE_TIMEOUT` as documented in
`cad/project_runner/README.md`. Isolated edit processes discard their documents;
span alone saves an edited artifact for a fresh-process reopen. Never sum
concurrent phase durations or count the edits aggregate again with its children.
The Rust worker renders native tessellation only. No generated display image
establishes geometry, fit, structural strength or flight performance.

The user explicitly chose to keep measuring under other jobs' load. Resource
samples describe the observed run, not a controlled idle-host comparison.

## Final result and notification

Current-code run: `runs/cad-optimized-r11-under-load/performance-report.html`.
Total 420.58 s; first render 106.05 s. The historical baseline was 692.06 s;
loads differ and the four-minute target remains unmet. All local probes
completed; five qualification gates passed and twelve remain pending. Read the
adjacent results document before making performance or engineering claims.

This handoff is committed with the implementation for Claude to read from main.
Direct Terminal notification was blocked by the computer-control tool, and the
Superset CLI installation points to a missing app. Do not infer delivery or
acknowledgment of a message from the existence of this file.

## Live demo update (Pat's later speed-first instruction)

Read `cad-live-demo.md`. The new Downloads VTOL prompt fixes all supported
parameters and selects a zero-model-call live FreeCAD demo. Current measured
run `cad-live-fast-05` took 63.56 seconds. It deliberately simplifies small and
internal geometry and skips engineering audits; those are NOT RUN, not passed.
Actual construction milestones stream to the overlay and native FreeCAD remains
open. Normal full-detail execution remains separate. The final report includes
actual native viewport images and all earlier failed/slow experiments are kept.
