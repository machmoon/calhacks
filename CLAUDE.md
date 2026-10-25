# Wright: working rules for Claude sessions in this repo

Product (Pat, 10/24 18:00): Wright is an assistant for hardware engineers. It
runs as an overlay on the engineer's existing tools and architecture (FreeCAD,
KiCad, the solvers, the shop). It drives those tools the way the engineer
does, using computer use where no API exists. It builds on what the engineer
already has and does not replace it.

Wright is a local-first Rust application with three parts: a versioned
engineering model, Claude agents that propose changes, and an independent
verification layer that decides what is accepted. Before changing anything,
read `docs/ARCHITECTURE.md` (the lessons the design came from) and
`docs/TODO.md` (the living checklist). The PCB pipeline is planned in
`docs/electronics-plan.md`.

## Target architecture (decided 2026-10-24; keep edits inside it)

| layer | technology | responsibility |
|---|---|---|
| interface | the Wright desktop app, `app/` in this repo (Tauri 2 + React, crate `wright-app`; its only home), with a workbench page that drives the `wright` CLI. `wright-desktop` (egui) is retired (Pat, 10/24 17:00) | requirements, inspection, feedback, approvals, replay |
| agent harness | Rust + Tokio (`wright-agents`). Claude runs through the CLI subscription by default; the API and Microsoft Foundry are alternate transports | planning, specialist delegation, tool calls, checkpoints, merge, lessons, tracing |
| engineering core | Rust: `wright-core`, `wright-geom`, the vehicle domains (`wright-air`, `wright-rocket`, `wright-balloon`) and electronics (`wright-eda` IR, `wright-kicad`, `wright-sch`, `wright-place`, `wright-route`, `wright-electronics`) | the authoritative design model, units as types, no-omission checks, revisions |
| engineering workers | FreeCAD (headless, persistent), KiCad (`kicad-cli`), PrusaSlicer, SU2, AVL/OptVL, gmsh + CalculiX, JSBSim, RocketPy, Cantera, Creao Build Desk (sourcing) | bounded jobs against one design revision. Local tools run as subprocesses; Creao is reached over its API |
| verification | Rust checks, `kicad-cli` ERC/DRC/parity for boards, the Haskell oracle (`haskell/`), Lean specifications (`lean/`) | evaluate evidence, enforce the acceptance rule |
| storage | JSON state, `events.jsonl`, stage manifests, artifact directories under the run dir | runs, revisions, jobs, evidence, lessons, regressions, provenance |
| external | Creao Build Desk (sourcing), the model transports | vendor data and model responses, never the source of truth |

Decisions that stand:

- **FreeCAD is the CAD kernel.** Wright is the overlay and has no in-house
  B-rep (Pat, 10/24 16:50: "I don't want to reinvent FreeCAD"). Every
  solid, boolean, STL/STEP/GLB export, mass property and interference test
  goes through the persistent FreeCAD worker (`wright-cad`). `wright-geom`'s
  `csg.rs` is a fallback only for when FreeCAD is missing. FreeCAD CAM
  supplies the mill toolpaths.
- **KiCad is in scope** (Pat, 10/24 21:50). The prior Python PCB engine is
  being rewritten as the Wright electronics crates (plan:
  `docs/electronics-plan.md`). KiCad files are the output format, and
  `kicad-cli` ERC/DRC/parity is the authority.
- **Storage stays files until after Cal Hacks.** SQLite is a post-event
  change.
- **Creao (agent.creao.ai) is the sourcing worker.** The Build Desk agent
  prices every BOM line from live listings, and Wright validates every
  quote (`wright-creao`). Its output is external data, never the source of
  truth.
- **Foundry is a transport, not the default.**
- **The predecessor prototype is "the Python baseline".** Use that name
  (or `baseline` in identifiers, e.g. `*.baseline.json`) in docs, comments
  and code. Never use its old product name in this repo (Pat, 10/24). Keep
  every comparison with it, including the rows where it wins.

**CAD is back with Claude sessions** (Pat, 10/25 00:30: "there is no more
handoff"). Astra (Pat's ChatGPT session) finished the CAD optimization work
and merged it into main; read `docs/claude-cad-final-handoff.md` and
`docs/cad-optimization-results.md` before changing `crates/wright-cad` or
`cad/`. FreeCAD stays the kernel; don't start a separate CAD stack, and don't
turn a pending CAD gate green without new evidence.

**Desktop rule** (Pat, 10/24 17:55): Wright has no local engine server.
- The Tauri shell spawns `wright run` as a child process.
- The app opens straight on the Wright page, with a quiet status row:
  binary, Claude transport and network, with solvers as optional badges.
- When something fails, show one plain-sentence error that includes the
  fix command.
- No ports and no "start the engine" gate.

## Principles the code must keep

1. **The design model is the source of truth.** Every part has a stable id,
   dimensions, material, connections and requirements. Each accepted edit
   is a new revision. A CAD, physics or BOM change either updates the model
   or stays an unaccepted proposal. The three must never describe different
   vehicles or boards.
2. **One execution contract for every worker.** A job takes a design
   revision, parameters and a tool version. It returns artifacts,
   measurements, status and provenance (`wright_core::Stage` / `Manifest`).
   Jobs run as separate processes with timeouts and isolated working dirs.
   Record enough to retry a job without duplicating accepted results.
3. **Three outcomes, never conflated:** the tool ran; the result is valid
   for this revision; the design meets its requirements.
4. **Acceptance is a separate, deterministic decision.** The agent may
   request it and can never grant it. Acceptance needs all of these:
   - every mandatory check on the current revision
   - every required analysis job complete
   - evidence bound to the same inputs and tool versions
   - passing regression cases
   - any required human approval

   Optional stages (renders, films) may fail without blocking. A required
   stage (structural analysis) blocks.
5. **No check passes by omission.** Checks are a closed set. A check that is
   unmeasured or has no evidence fails. Lean proves this on the model and
   Rust enforces it. Both sides of every comparison are judged by the same
   checks, graded in the reference's favour where data is missing.
6. **Lean and Haskell have different jobs.** Lean formalizes the acceptance
   rules: missing evidence cannot pass, failed required checks block, and
   evidence from another revision cannot authorize delivery. Haskell
   independently recomputes selected physics and geometry, and the results
   are compared with Rust under stated tolerances. Neither verifies the
   Rust code by itself, and docs must say so.
7. **Three memories, kept apart:**
   - run state: design, plan, pending jobs, checkpoints
   - lessons: retrieved guidance with provenance, advisory only
   - regression cases: executable conditions every future candidate must pass

   Lessons never override a check.

## Open work in priority order (owners in docs/TODO.md)

1. Revision ids on the design and on every manifest and artifact. The CAD
   stage and everything downstream go stale when `cad/` or the design
   changes, and the acceptance decision refuses evidence from another
   revision.
2. Required vs optional stages in the acceptance decision. Today the runner
   continues after any stage error.
3. One explicit `accept` function that the orchestrator calls and cannot
   bypass, with the Lean properties as its contract.
4. Later: an executable reference evaluator derived from the Lean spec,
   with the Rust checker differential-tested against it; SQLite for
   structured state.

## How to work here

- **Languages.** Code in Rust. Use Lean and Haskell for verification. Use
  C++ only when a solver needs it. No new Python, except thin subprocess
  scripts for a solver that only ships a Python API; keep those under the
  crate that owns them.
- **Prior art first.** Before writing a solver, kernel, viewer or format,
  research the maintained open-source implementation and integrate it as a
  subprocess or dependency. Name the file the design follows in a comment
  and in `docs/TODO.md`.
- **Shared tree.** Many sessions share this tree and one cargo lock.
  - Put new logic in a new module.
  - Keep edits to shared files small and additive, and re-read the file
    first.
  - Never run `cargo clean`, never reformat a whole file, and never kill
    another session's cargo.
  - Keep each crate compiling at every save.
- **Disk, downloads, commits.**
  - Keep disk above 3 GB free (`df -g /System/Volumes/Data`).
  - No downloads without asking Pat first (name, source, size).
  - Nothing is committed or pushed without Pat's OK.
  - Never post, send or react on Pat's accounts unless asked.
- **Honest numbers.**
  - Never present the synthetic studio fixture as a real run.
  - Re-run the evals before quoting numbers.
  - The eval tables in `README.md` and `docs/devpost.md` must match
    `evals/*/result.md`, including the rows where the reference wins.
- **Done means:**
  - `cargo test -p <crate>` passes.
  - `cargo clippy -p <crate> --all-targets -- -D warnings` passes.
  - Offline tests cover the behaviour.
  - The TODO line is ticked with a date.
- **Solver paths** come from `scripts/env.sh` (`WRIGHT_PYTHON`,
  `WRIGHT_SU2`). KiCad comes from `WRIGHT_KICAD_CLI` and
  `WRIGHT_KICAD_SHARE`.
