# Native VTOL audit and display improvements

The native workflow remains FreeCAD geometry driven by code/CLI, with Wright's
Tauri overlay in `app/`. These changes improve that workflow; they do not certify
a flight-ready aircraft.

## Implemented

- Independent audit jobs execute as bounded subprocesses (default three workers;
  `WRIGHT_AUDIT_WORKERS=1` selects serial execution). Timings are synchronized and
  atomic. Probe limits are configurable with `WRIGHT_PROBE_TIMEOUT` (default 600 s,
  bounded to 30–1200 s); timeout/error results remain explicit.
- An atomic `audit-ready.json` freezes native geometry, support module and STEP
  before rendering. Audit and display then overlap. Cancellation cleans up owned
  process groups. The final result still checks the native revision hash.
- Five edit cases use independent processes and one temporary native document
  each. Isolated cases no longer restore/rebuild a document they will discard;
  span saves its already-validated changed state. Reopen remains dependent on
  the saved edited artifact. The serial regression path still restores between
  cases.
- Actual native display tessellation uses bounded face-local normals and a Rust
  raster worker. A geometry-keyed, bounded tessellation cache reuses unchanged
  meshes. Cutaway display clips native triangles; it is explicitly an uncapped
  visualization, not a manufactured Boolean section. Python remains the fallback.
- The pre-existing collision broad phase remains. Selection no longer computes
  an unused second set of exact bounds. The audit skips exact distances for
  unconstrained nonpenetrating pairs, but computes every relevant Boolean and
  every declared contact distance. This mode explicitly does not certify global
  connectivity. STEP comparison validates each imported solid once, not once
  per candidate pair.
- Own motors are not allowlisted out of propeller clearance checks. Conservative
  radial bands refine the false-positive full disk near the hub. The actual
  motor is tested; moved-motor and blade-intrusion negative controls fail.
- r10 adds tray-driver ports, reverses tail-clamp bolts for underside access and
  replaces the trapped top hatch with a side cover. Its declared 200 mm lateral
  removal clears a conservative swept box. All tested driver paths clear.
  Cover retention, sealing, strength and full service operations remain open.
- Pair and parameter-transformation results can pass their stated local scope;
  hardware fit is not inferred from reservation edits. Export coverage and other
  incomplete requirements keep explicit missing-evidence reasons. HTML reports
  now show those reasons and probe start times.
- A revision/hash-bound Wright-air design/spec adapter is connected. It refuses
  to substitute a default aircraft. This candidate lacks native mass/material/CG
  and exact hardware reconciliation, so mass/propulsion qualification remains
  pending; the existing analytical layout is not automatically this inverted-V
  native assembly. `engineering-inputs.json` records the missing inputs and known
  purchased-mass subtotal, not a claimed takeoff mass.
- Phase labels preserve semicolons; completed sub-second phases are published
  from the native phase trace. The obsolete empty render phase is removed.

## Actual evidence (local generated files)

`runs/cad-optimized-r10/` was a fresh subscription Astra request through the
production project-runner entry point. Its nine parameter values exactly match
`cad-project-1792903184`; the serviceability changes come from the revised recipe.
The native file contains 304 document objects and 100 valid physical solids.
The GUI capture confirms FreeCAD opened the saved model. All selected pairs,
all five parameter transformations, fresh-process reopen and physical-airframe
STEP equivalence passed. The four refined lift envelopes and all tested driver
paths clear; the declared side-cover removal also clears. Missing aircraft-level
requirements remain pending.

The measured run took **811.36 s (13.52 min)** under heavy concurrent machine
load, compared with the earlier 692.06 s baseline. It does **not** demonstrate the
suggested four-minute target or an end-to-end speedup. A late resource snapshot
and the raw timing traces are retained. Native render/audit intervals overlap;
adding their durations or adding the edits aggregate to its children is invalid.
The later display-cache and isolated-edit cleanup optimizations are separately
validated and were not all present at the beginning of that timed run.

`runs/cad-optimized-r10/render-benchmark/result.json` compares the same native
model/camera/resolution with warmed tessellation: Python 98.49 s, Rust 26.75 s,
a **3.68×** observed wall-time ratio. Pixel-channel difference is **zero** and the
source hash is unchanged. Parent CPU times exclude the Rust subprocess; this is
a display comparison, not evidence for a four-minute full build.

The first parallel diagnostic exceeded 300 s for several workers; it is retained
as a failed experiment, not suppressed. Two superseded diagnostic runs were
cancelled explicitly; they are not acceptance evidence.

## Regression checks

- 17 offline audit/report/concurrency/revision-binding tests pass.
- Project runner, cancellation, short-phase publication and timing tests pass.
- All nine PDF tests pass on retry (one earlier run hit its processing deadline
  during severe host contention; the PDF timeout was not weakened).
- Native optimization controls cover motor/blade intrusion, moved-motor cache,
  blocked hatch removal, collision/contact parity, refused unmeasured connectivity,
  pixel parity, clipping/source preservation, and display-cache placement edits.
- Existing native compact-check/export/freshness regression passes.

Native artifacts and timing JSON remain under ignored `runs/`; no downloaded
vendor files or generated binaries are committed. See the adjacent handoff for
ownership and the remaining engineering qualification work.

## Final current-code run under load

`runs/cad-optimized-r11-under-load/` completed in **420.58 s (7.01 min)**.
All nine design parameters match the earlier candidate. Planning took 20.16 s,
native build/display 150.38 s, and the overlapping audit 341.71 s. First published
render was 106.05 s (baseline 75.80 s). This is 39.2% less total elapsed time than
the historical 692.06 s baseline, but host loads differ; it is an observed run
comparison, not an isolated causal benchmark or a four-minute result.

All probes completed. The selected collision scope, five edit transformations,
fresh-process reopen, four refined lift sweeps, declared cover removal, tested
driver paths and 100-solid physical STEP equivalence passed. Five qualification
gates pass and twelve remain pending; none fail or are inconclusive. Native
source preservation passed. This run requested GUI opening but did not produce
a GUI capture receipt; the previous r10 run did. Its generated CAD renders and
saved native model are actual artifacts, not an assertion of GUI launch success.

Five-second resource sampling observed 1,628.72 MiB peak runner-tree RSS and
795.7% peak aggregate CPU, with one-minute system load from 39.93 to 78.94.
Sampling began after startup and excludes macOS-launched GUI processes; these
are sampled peaks, not continuous maxima. The user explicitly kept other jobs
running. Raw samples, probe starts/durations and the HTML report are retained.

A first launch in `cad-optimized-r11-measurement/` stopped before planning because
the isolated worktree lacked the planner binary. The successful run explicitly
set WRIGHT_CAD_MODEL_RUNNER to the existing installed subscription runner. No
fallback model or fabricated candidate was used. All 44 Python regression tests
passed together on this final implementation. The final isolated span-save
optimization also passed a separate fresh-process reopen and source-hash check.
