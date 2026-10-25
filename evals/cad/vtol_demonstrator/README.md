# Independent VTOL demonstrator evaluation

This evaluates the current artifact against `cad/projects/vtol-demonstrator/BUILD_PROMPT.txt`. A valid local model does not imply a complete hardware kit, flight readiness, or model one-shot success.

After the builder freezes its native file and matching manifest:

```sh
python3 evals/cad/vtol_demonstrator/run.py \
  out/vtol-demonstrator/assembly.FCStd \
  out/vtol-demonstrator/artifact-manifest.json \
  out/vtol-demonstrator-evaluation
```

The output directory must be new. Each native stage runs in a fresh FreeCAD process with a 300-second timeout. Logs, full pair evidence, parameter measurements, edited native checkpoint and final requirement report are retained. The original file must remain unchanged. The manifest-declared Python feature module is imported before FreeCAD opens the document; its exact file hash is pinned in the contract and rechecked in every process. Live recompute handlers and exactly one connected solid per declared manufactured part remain mandatory; kernel validity alone is insufficient. This runtime requires explicit module import: bare opening preserves cached shapes but may lose scripted regeneration. Bare-open failures must remain recorded separately, not represented as a repaired CAD artifact. Reopen evidence must identify the exact edited checkpoint, whose parent hash must identify the baseline.

Collision coverage includes declared physical airframe objects plus actual candidate `LiftMotor*`/`LiftProp*` geometry. Nonphysical reservations and swept volumes are listed separately; unknown hardware prevents a complete-aircraft clearance pass. Declared contacts never excuse overlap. Empty contact lists do not establish attachment or retention.

Span, chord, boom spacing and battery reservation edits are measured against native geometry and followed by physical validity checks. The unresolved servo interface remains pending; a dimensional envelope is not verified component fit. Parameter tests do not certify load paths or service access.

The access probe derives actual clamp-head positions and checks a proposed Ø6 × 200 mm straight upward driver envelope against every declared collision object. Any obstruction fails the service gate; clear local paths alone do not prove complete service access. The current-wing stage extracts the saved native wing and invokes AeroSandbox, OptVL and NeuralFoil at 18 m/s and alpha 0/4/8 degrees, retaining solver and source provenance. It does not assess full-aircraft stability.

Exports, propulsion performance and swept/control motion remain pending unless independently verified for the current artifact. Existing analysis of a different wing cannot satisfy this gate. The report links current native, builder dependency and manifest hashes; it preserves failures rather than repairing the candidate.

Run aggregation negative controls with:

```sh
python3 -m unittest discover -s evals/cad/vtol_demonstrator -p test_report.py -v
```

The lift sweep probe verifies the P12x4 central Ø6 mm native cylindrical bore before using its axis. An annular cylinder enclosing the actual native propeller bounds represents every rotation conservatively. Only the own propeller is excluded. Own motor, mount and clamp fasteners are checked. The central Ø6 mm shaft void is removed only after a native Boolean proves zero overlap with actual propeller material. A cylinder intersection is a potential interference requiring exact blade-sweep investigation. Cruise geometry remains unresolved. Native controls exercise a static-clear obstacle hit by the rotational envelope and reject an absent hub:

```sh
cad/freecad/run.sh evals/cad/vtol_demonstrator/native_sweep_controls.py
```

`recorded-result.json` records the actual app trial's layout-r2-saddles audit: 53 kernel-valid physical objects, including a failed manufactured-part topology gate because BatteryTrayCut1 has three disconnected solids, 1,830 pairs with 12 overlaps, eight obstructed driver paths, working parameter regeneration/reopen with the declared feature module, and current-wing solver evidence. The tightened annular sweep detects potential intersections with each own motor, so the sweep gate is inconclusive pending exact blade-sweep investigation. The earlier broad hardware-exclusion clearance is superseded. Original bare-open runtime failure remains retained alongside the corrected dependency-loading audit. No candidate geometry was repaired by the evaluator.

`recorded-r4-result.json` retains the subsequent authored repair assessment: all 2,485 modeled pairs clear, declared contacts met, and all eight proposed clamp driver paths clear. Geometry dimensions regenerate for all five edit families, but manufactured-part and edit/reopen acceptance fail because the tray is still three disconnected solids. Wing-only numerical analysis passes. The conservative lift annulus intersects each own motor, requiring a more precise sweep assessment. These local improvements do not establish complete-VTOL or one-shot-model success.

Manufactured-part topology controls:

```sh
cad/freecad/run.sh evals/cad/vtol_demonstrator/native_topology_controls.py
```

They distinguish a valid single solid from a kernel-valid compound of disconnected solids. When a final manifest identifies STEP, the runner invokes `export_probe.py` with a 300-second bound; a successful partial airframe equivalence check still leaves full export coverage pending because candidate hardware and reservations are omitted.

New audits resolve bounds, sweep obstacles, and driver origins/axes through native global assembly frames. They record auditor and core geometry-helper hashes per probe; prior r2/r4 reports are unchanged. Native sweep controls retain flat checks and add a translated/90-degree rotated parent. Driver access has the corresponding obstruction-invariance control:

```sh
cad/freecad/run.sh evals/cad/vtol_demonstrator/native_access_controls.py
```

The current driver probe follows each physical bolt's native head/shank links and chooses the outward head direction, including underside-access bridge bolts. It tests 26 R5/R6 bolt paths against all modeled collision instances. Nut-tool paths and reference-envelope occupancy remain unassessed and are listed explicitly. Controls cover flat, nested-rotation and underside-head cases without changing source geometry.

`recorded-r5-result.json` preserves six structural intersections and six blocked driver paths. `recorded-r6-result.json` records their subsequent geometry repair: 99 valid single physical solids, 5,671 modeled pairs with zero overlaps, all 71 declared contacts met, geometric parameter regeneration and fresh reopen, current-wing numerical checks, and 99/99 physical-airframe STEP solid equivalence. Six driver paths remain obstructed, 26 nut routes unassessed, and five control surfaces isolated. Complete hardware coverage, exact propeller sweeps, operating linkages, load paths and whole-aircraft performance remain unresolved. Parameter probes measure regeneration and validity; they do not repeat the complete interference audit after each edit.

`recorded-astra-r5-repair-result.json` preserves the actual Astra-repaired candidate assessment: 99 valid single physical solids, 5,671 pairs with no overlap and all 71 declared contacts met. The bounded edit stage timed out after recording five passing dimension cases, before completing the edited-checkpoint/reopen workflow. That stage fails; partial results are not promoted to a completed pass. Six proposed driver paths remain blocked, exact propeller sweeps remain inconclusive, and no STEP was supplied for independent export comparison. The unchanged candidate was not rerun after interruption.

## Focused inverted-V layout gate

```sh
cad/freecad/run.sh evals/cad/vtol_demonstrator/layout_acceptance.py \
  out/vtol-demonstrator-r9/assembly.FCStd \
  out/vtol-demonstrator-r9/artifact-manifest.json \
  out/vtol-demonstrator-r9/independent-layout.json
```

This bounded layout-specific CLI exits zero only when actual native cross-sections rise inward, mirrored Boolean differences are negligible, four tail outputs are valid single solids, boom bearings are nonpenetrating, old tail outputs and a flat center roof are absent, and retained winglet/pusher geometry is valid and included in collision accounting. It checks the shipped native opening macro's visibility configuration; headless artifacts do not prove GUI occlusion or viewport visibility. It does not replace collision, control-mixing, hinge, load or flight qualification.

`recorded-r9-layout-result.json` records the actual 30-degree halves, zero mirrored differences and zero-gap/nonoverlapping boom bearings. The rejected R8 native arch fails even with forged inverted-V metadata and declared output names. Independently measured R8 central sections are flat, so the geometric slope criterion also rejects it. Neither native artifact was changed.

```sh
cad/freecad/run.sh evals/cad/vtol_demonstrator/layout_negative_controls.py \
  out/vtol-demonstrator-r8/assembly.FCStd \
  out/vtol-demonstrator-r8/artifact-manifest.json \
  out/vtol-demonstrator-r9/layout-negative-controls
```
