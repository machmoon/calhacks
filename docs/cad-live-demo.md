# Fixed-parameter live CAD demo

Pat requested speed ahead of audits and small geometry details, actual visible
construction, and parameters supplied by the prompt rather than Astra/Claude.

## Run

Paste the complete `~/Downloads/VTOL-prompt.txt` into Wright's existing overlay.
The tracked copy is `cad/projects/vtol-demonstrator/DEMO_PROMPT.txt`. The exact
WRIGHT_FAST_DEMO marker routes to `cad/project_runner/fast_demo.py`; the single
WRIGHT_DESIGN_JSON line supplies all nine supported dimensions. No model call
chooses parameters. Existing overlay routing calls this same Python entry point;
no Rust rebuild is needed for the marker. This measurement was made through the
production Python entry point, not by timing a fresh overlay button click.

The preceding long Downloads brief is retained as
`VTOL-prompt-full-engineering.txt`. The arm prompt now explicitly preserves the
pinned official URDF dimensions; no arm run was performed in this change.

## What is live

A new native document is created each run from the recipe and independent
component files. FreeCAD opens before construction. Real wing, body/tail,
hardware and final viewport images stream through existing render events. Native
kernel work can block updates between these milestones. There is no completed
assembly cache, prerecorded replay or artificial creation animation. FreeCAD
remains open after completion; closing that window ends its retained GUI process.

This fast candidate has 126 document objects and 30 declared physical objects.
It retains winglets, the inverted-V and rear pusher, imported lift motors and
propellers. Its solid fuselage envelope, simple motor posts, omitted internal
trays/ribs/small fasteners and deferred face refinement are recorded in the
manifest. It is a simplified prepared recipe, not a full engineering build or a
fresh model-authored design. STEP is exported but equivalence is not checked.
Collision, motion, fit, structural and simulation checks are explicitly NOT RUN.
The full-detail workflow remains separate and available without the fast marker.

## Measured evidence

`runs/cad-live-fast-05/` completed in **63.56 seconds** from runner start through
fresh construction, saving and final native viewport. Live views appeared at
24.13, 32.39, 56.32 and 63.54 seconds. Model calls: zero. The native file and its
live completion receipt identify the same SHA-256. The window stays editable.
`performance-report.html`, `cad-ready.json`, events and raw logs preserve evidence.
This is one measured current-version run under concurrent host load, not a
hard runtime guarantee on arbitrary machines or future workloads.

Earlier experiments remain available: CAD-first/all-six-software-view trials
were 160.24, 139.72 and 163.82 seconds (the first also had a subsequently fixed
readiness logging error; later audits were cancelled when the user changed
scope). Earlier live trials were 81.15 and 124.59 seconds; the latter showed
insufficient timing margin. Trial 03 failed on a final active-view lookup.
Trial 04 fixed explicit document lookup and took 119.24 seconds. Trial 05 adds
the explicitly requested geometry simplification and supplies the final result.
No failed trial is counted as a passing demonstration.

Regression coverage: fixed parameter parsing rejects missing/duplicate/out of
bounds data without inventing dimensions; readiness checks reject incomplete
artifacts; native display translation matches actually displaced CAD pixels and
preserves its source. Thirty project-runner tests and 17 audit tests passed in
this work. Native rendering negative controls passed. A full engineering audit
was deliberately not run for the final live-demo candidate.
