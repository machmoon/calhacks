# Native internal-assembly edit corpus

This is a reproducible synthetic data/evaluation foundation, not a trained model
or a measurement of an agent's success rate. `evaluate.py` executes known reference
operations against real native FreeCAD features. A future agent must independently
produce an operation sequence; reference execution success is not model accuracy.

Run from the repository root:

```sh
python3 evals/cad/training/evaluate.py --validate-only
cad/freecad/run.sh evals/cad/training/evaluate.py
```

Results go to `out/cad-training`: editable FCStd fixtures after the successful edit,
`report.json` with native version and manifest/evaluator hashes, and `train.jsonl`.
Only passing `train` records enter JSONL. Validation/test trajectories stay in the
separate report and must never be concatenated into training inputs. All fixtures
are authored here, with no external CAD/training data or network access.

## Existing coverage and the gap

`cad/freecad/test_native.py` already checks primitive edits, tube expressions and
rollback; `test_profiles.py` checks constrained profiles; `test_connectivity.py`
checks contact/connectivity; `test_rocket_session.py` checks imported native edits.
`evals/cad/freecad_openrocket.py` and source-reference verifiers check supported
import coverage and dimensions. We do not duplicate those source benchmarks.
This suite adds many hidden physical parts and repeated fastener interfaces,
semantic constraints across edits, native persistence, and isolated corpus splits.

## Schema and semantics

`cases.json` version 1 uses millimetres. Each case requires unique `id`, `split`
(train/validation/test), topology `family`, shell radius, tray width, bolt count,
stack levels, and target tray thickness. `load_cases` rejects malformed values,
duplicate IDs and any train/test topology-family overlap. Four-bolt patterns train;
a scaled four-bolt stack validates; a six-bolt pattern is held out. This small,
public test set is a regression holdout, not a secret/generalization benchmark.

Every generated trajectory record has schema version, fixture ID/specification,
synthetic-native-oracle provenance, natural-language task, ordered typed operations,
evidence location and explicit manufacturing-approval false. Operations map to
native `set_parameter` with object name, parameter and numeric value. Native
fixture setup uses `make_part` and FreeCAD expressions; authoring expressions is
not claimed to be available through the current agent tool API.

Each tray/bolt interface has a cylindrical axis and bore pattern, nominal radial
clearance 0.2 mm, and 1 mm axial protrusion above its tray. Bolt height is constrained
by `TrayN_Blank.Height + 2 mm`. The evaluator checks that the expression itself
survives as well as its evaluated value. It measures clearance using native B-rep
distance and nonpenetration using boolean common volume for **every physical pair**.
Shell material volume is preserved; tray volumes have independent analytic checks.
FCStd reopen repeats the geometric and dependency checks.

Three adversarial edits must be caught in every fixture: a 1.4 mm bore around a
1.5 mm shaft; a removed dependency with unchanged current geometry; and a hidden
head translated into its washer. Reports retain the failed constraint names and
measured evidence. These failed states never enter the successful training corpus.

## Scope and next training gate

The fixtures contain unthreaded cylindrical shanks/heads/washers, simple square
trays and an open cylindrical shell. Trays are not attached to the shell. Thus
collision freedom is not global assembly connectivity or load-path validity.
There is no joint solver, solved DOF, preload, thread engagement, nut, tool-access,
thermal/structural calculation, manufacturing tolerance stack or flight approval.
The six-bolt holdout increases pattern cardinality but shares the generator;
broader real geometry and unseen construction strategies remain necessary.

Before tuning weights: select the model and license, record actual agent tool
requests/responses on these fixtures, evaluate unassisted held-out tasks, add human
review, then train only reviewed training traces. Keep failed attempts with their
native evidence for preference/ranking evaluation, never relabel them successful.
No provider-format upload or paid training is performed here.

## Verified local evidence (2026-10-24)

`out/cad-training-verified/report.json` records a passing FreeCAD run for 40, 53,
and 58 physical parts. Their edited-state reports contain 868, 1,495, and 1,783
individual checks respectively; all three native reopen checks pass. All nine
negative controls are detected. Five corpus-integrity unit tests pass via
`python3 -m unittest discover -s evals/cad/training -v`. The recorded evaluator
hash matches the final source. Exactly one verified train trajectory is emitted;
validation and test examples are excluded. Formal contracts are in
`cases.schema.json` and `trajectory.schema.json`.

## Model-driven clamp repair evaluation

The ignored Rust test `live_astra_vtol_clamp_constrained_repair_attempt` runs the
subscription-backed CAD specialist against a copy of the actual v1 VTOL joint,
with ten model turns and typed native actions only. It supplies target dimensions
and protected geometry, not parameter names or an editing recipe. The execution
test records an attempt; a passing Rust test does not establish repair success.
After it finishes, run the independent checker against its evidence directory:

```sh
cad/freecad/run.sh evals/cad/training/verify_clamp_repair.py out/astra-vtol-clamp-repair/run-PID
```

The checker reopens the model's saved file and checks nominal coaxial R5 seats,
boom contacts without overlap, a 0.5 mm split gap, unchanged boom/stabilizer and
fin material outside the permitted root region, source integrity, and live native
radius dependencies. Its transient dependency probes restore parameters in memory
and never save either input or model artifact. A failure is retained as a failed
attempt; the checker does not repair model output. Even a pass establishes nominal
geometry only, not fasteners, preload, friction, compliance, strength or flight
readiness. Keep this held-out attempt separate from training traces.

The full-aircraft read-only case is
`live_astra_vtol_full_assembly_diagnosis`. After its report, independently replay
its actual chosen contacts/tolerances with:

```sh
cad/freecad/run.sh evals/cad/training/replay_diagnosis_checks.py out/astra-vtol-full-assembly-diagnosis/run-PID
```

This verifies native pair evidence, source integrity, usage and scoped discovery.
It does not grade completeness of the model's mechanical reasoning; retain a
separate claim review and explicitly list unassessed interfaces. The first case's
three replays matched in `out/astra-vtol-full-assembly-diagnosis/run-93531/`.

The compact variant `live_astra_vtol_full_assembly_compact_diagnosis` uses the same
assembly and requests `check summary:true`. Replay always recomputes the full
native result and verifies the exact all-clashes/declared-contacts projection,
counts and components. `test_diagnosis_replay.py` includes omission and corruption
negative controls; it does not import FreeCAD for these pure comparison tests.
The first compact run and measured comparison are retained in
`out/astra-vtol-full-assembly-compact-diagnosis/run-96086/evaluation.json`.
