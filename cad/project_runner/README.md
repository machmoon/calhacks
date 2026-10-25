# Native project runner

The desktop routes an explicit `cad/projects/vtol-demonstrator/BUILD_PROMPT.txt`
request here instead of the schematic pipeline. This is a prepared FreeCAD
recipe with subscription-backed parameter planning, **not arbitrary aircraft
generation or a validated recreation of the reference aircraft**.

## Setup

Use the existing signed-in Codex subscription transport. Build its runner:

```sh
cargo build --manifest-path evals/cad/so_arm101_one_shot/Cargo.toml
export WRIGHT_CAD_MODEL_RUNNER="$PWD/evals/cad/so_arm101_one_shot/target/debug/wright-so-arm101-one-shot"
```

If `CARGO_TARGET_DIR` is configured, point the variable at that target instead.
The default local installation is `out/tools/wright-cad-model`. No API key is
required by this path. The transport requests `gpt-6-astra` and fails visibly if
it is unavailable; there is no silent model or design fallback.

## Run

```sh
python3 cad/project_runner/run.py \
  --brief cad/projects/vtol-demonstrator/BUILD_PROMPT.txt \
  --request 'Read and execute cad/projects/vtol-demonstrator/BUILD_PROMPT.txt.' \
  --output-dir /absolute/fresh/run-directory
```

The runner records the brief and model request, parses a literal nine-parameter
design as data and requires a structured layout rationale with references, alternative
tradeoffs and unresolved checks. It executes the native FreeCAD recipe, requests that FreeCAD open
its output, and runs independent checks against the saved artifact hash.
`events.jsonl` supplies desktop progress, artifacts, renders and check outcomes.
`audit/result.json` distinguishes passed, failed, pending and inconclusive gates.
The initial implementation always leaves complete-aircraft qualification pending.

Open `cad/open.FCMacro` to load the native model with its scripted-feature module.
Keep the referenced source module available; the FCStd file alone does not bundle
the Python feature implementation. Manufacturer components and dimensional
reservations are classified separately in the artifact manifest. Export coverage
and verification limitations must be read before using an export.

## Tests

```sh
python3 -m unittest discover -s cad/project_runner -p 'test_*.py'
```

These cover brief containment and the parameter parser, not aircraft performance.

## Layout correction regression

The original Astra run chose five dimensions; tail architecture was fixed by the
recipe. R8's rejected arch was an authored recipe choice, not a learned model
optimization. The current planner explicitly records the user-selected inverted-V
configuration. Numeric recipe defaults are not user specifications or optimized values.
The native acceptance check inspects cross-sections, mirrored geometry, boom
contact, omitted old tail outputs and retained winglet/pusher geometry. A rationale
or manifest label alone cannot pass it. These are workflow/contract improvements;
no model weights were fine-tuned. Flight controls, strength and aerodynamic
qualification remain separate.

## PDF input

The desktop overlay accepts one PDF of up to four pages and 20 MiB. It preserves
and hashes the original, extracts per-page text, and rasterizes every page for
Astra's visual input. Scanned pages are supported through page images; no OCR
accuracy is claimed. The PDF runtime needs pypdf, pypdfium2 and Pillow; set
WRIGHT_PDF_PYTHON when these are not in an existing configured or bundled runtime.
Preparation never downloads dependencies.

PDF interpretation records the source page or recipe-default explanation for
every parameter. Unsupported layouts, dimensions and requirements stop before
building rather than silently substituting the default design. The current
supported CAD target remains this prepared inverted-V VTOL recipe, not arbitrary
hardware synthesis from any PDF. Run evidence includes the frozen PDF, page
images, model request, interpreted requirements, native assembly and checks.

## Execution profiling

Each run writes timestamped `events.jsonl`, `timing-trace.jsonl`, and a final
`timing-summary.json`. Native builder boundaries are recorded separately in
`cad/build-phases.jsonl`; independent probes flush their starts and outcomes to
`audit/probe-timings.jsonl` and an atomic JSON snapshot. Completed PNGs are
published while the builder is still running, and audit stage transitions appear
in the app log. The final engineering report remains authoritative.

```sh
python3 cad/project_runner/render_timing_report.py runs/YOUR_RUN
```

This creates a local HTML report and `performance-report.json`, including active
stage observations, first-render publication time, model usage/tool names,
native phases, probe durations, and engineering gate statuses. Nested intervals
overlap and are not added together. The model subprocess duration includes
subscription login and model execution; internal reasoning time is not observable.
Reports omit model payloads and image data. An unfinished observation does not
prove its process is still running.

## Audit and display execution

The saved native/STEP milestone starts independent audit subprocesses while
native display views render. `WRIGHT_AUDIT_WORKERS` controls bounded concurrency
(default 3, range 1–8). `WRIGHT_PROBE_TIMEOUT` controls each probe's deadline
(default 600 seconds, range 30–1200). Five parameter edits are isolated; fresh
reopen depends on the saved span edit. Timings overlap and must not be summed.

The display renderer compiles the std-only Rust raster worker with local `rustc`
and caches it under `out/display-raster/`. With no compiler it uses Python;
`WRIGHT_RASTER_BACKEND=python` explicitly selects that reference renderer.
Rasterization and uncapped cutaway display do not modify native CAD or establish
engineering validity. Read `docs/cad-optimization-results.md` for measurements,
negative controls and outstanding qualification inputs.

## Fast live demo with fixed parameters

Paste `cad/projects/vtol-demonstrator/DEMO_PROMPT.txt` (also installed as
`~/Downloads/VTOL-prompt.txt`) into the existing overlay. Its WRIGHT_FAST_DEMO
marker selects the live demo path; exactly one WRIGHT_DESIGN_JSON line supplies
all nine supported dimensions. Missing, duplicate or out-of-bounds inputs fail
without a model call. This prepared recipe is not fresh model-generated design.
PDF interpretation remains on the normal audited/planned path and is rejected
in fixed-parameter mode rather than silently ignored.

The native FreeCAD window opens before construction. Actual wing, airframe,
hardware and final construction milestones recompute and redraw the document;
viewport images stream through existing render events. There is no replay,
finished-model cache or artificial animation. Kernel operations can block GUI
updates between milestones. FreeCAD remains open after completion for editing.
Only the process created by this run is terminated on pre-completion failure.

This mode saves editable native CAD, the support script, STEP and real viewport
images, but deliberately does not run solid-validity, collision, motion, fit,
simulation, structural or export-equivalence audits. `demo_built_unverified`
and `engineering_checks: not_run` remain explicit; run_finished.passed is false
because aircraft qualification is not established. Completion timing includes
runtime startup, construction, saves and the final visible native viewport.

Normal execution and `--cad-first` retain the engineering audit. CAD-first
prioritizes all six software-rendered views before the audit; it is separate
from the faster live-view/no-audit demo requested by Pat.

## Fast follow-up edits in the same FreeCAD session

New live demos keep a typed parameter-edit queue on the GUI thread. In the
overlay submit the normal project brief path, plus exactly one line each:

```
WRIGHT_EDIT_SESSION: /absolute/path/to/previous/run/cad
WRIGHT_EDIT_JSON: {"Span": 1850, "WingletHeight": 95}
```

All changes run in one undoable transaction and one recompute. Only the nine
bounded design parameters are allowed. A stale revision or a parameter changed
outside the session is refused. A native snapshot and viewport image are saved
alongside the original model without overwriting it. STEP is not refreshed and
engineering checks are not run. Closing the FreeCAD process ends the session;
this interface does not start a replacement or silently do a full rebuild.
On a client timeout, outcome is unknown; inspect the queue result before retrying.
