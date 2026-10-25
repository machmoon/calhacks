# Model-authored SO101 camera mount in real FreeCAD

This task adds a new editable BRep camera mount to the existing model-generated
SO101 mesh assembly. The camera is Raspberry Pi Camera Module 3 Standard, with
manufacturer drawing dimensions in `cad/projects/so101-camera-mount/camera-spec.json`.
Wrist coordinates come from independently measured official STEP geometry, with
source STEP-to-mesh bore registration recorded in the interface JSON. Source arm
objects must remain unchanged. No final mount geometry is authored by this runner.

`prepare.py` verifies the input FCStd hash against the measured interface, then
freezes the full camera specification, interface, prompt and source hashes. The
existing `wright-so-arm101-one-shot` executable calls Wright's subscription-only
CodexTransport with exact gpt-6-astra/low and records the generated Python module.
There is no API-billing or alternate-model fallback.

```sh
python3 evals/cad/so101_camera_mount/prepare.py \
  out/so-arm101-one-shot/attempt-2/assembly.FCStd \
  out/so101-camera-mount/interface/measurements.json \
  cad/projects/so101-camera-mount/camera-spec.json \
  out/so101-camera-mount/frozen-1
/tmp/wright-astra-target/debug/wright-so-arm101-one-shot \
  out/so101-camera-mount/frozen-1/request.json \
  out/so101-camera-mount/attempt-1
python3 evals/cad/so101_camera_mount/check_submission.py \
  out/so101-camera-mount/attempt-1/candidate.py
```

Build the transport executable using the manifest and commands documented in
`../so_arm101_one_shot/README.md` if it is not already available. Use fresh output
directories for new attempts; never overwrite the failed attempt to imply success.

The submitted module must define `build(input_path, output_dir, checkpoint=None)`.
It is deliberately **not executed by this runner**. The GUI host loads the module
and calls build, supplying `checkpoint(label)` to capture actual document stages.
The original plan was GUI-first capture. Runtime/GUI failures led to an explicit
headless repair-validation step followed by a real GUI replay of the same unchanged
module. Label replay captures accordingly; they are not evidence that the successful
module first executed in the GUI. No fabricated animation or manually authored
replacement mount counts as model work. `check_submission.py` checks only module structure and import
boundaries; it is not a geometry verifier or a general Python security sandbox.

The public measurable contract requires final single-solid CameraMount with native
construction dependencies, CameraMountParameters.PlateThickness/StandoffHeight,
CameraPlate, four drilled CameraStandoff helpers, CameraPlateFrame, CameraFrame,
CameraReferenceEnvelope and CameraRibbonClearance. Helper geometry is construction,
not extra physical assembly parts. The camera and ribbon are explicitly simplified
reference envelopes. Camera hardware uses four M2 locations; the verified wrist
uses two M3-clearance holes. Standoff and ribbon allowances are prototype design
requirements, not manufacturer cable-bend or structural guarantees.

The independent verifier, not the submitted script's self-report, decides source
preservation, valid connected BRep, hole locations, material/keepout conflicts and
fresh-process parametric edits. GUI checkpoints show real operations but do not
replace those geometric checks. No physical readiness or collision-free joint
range is implied.

## Recorded camera-mount development

`recorded/` retains both exact model requests and candidate modules, response hashes
and token usage, public camera/interface facts, and the unchanged first candidate's
traced failure. Response Python payloads are not duplicated because they are already
preserved exactly as candidate.py. No credentials or auth files were accessed.

Attempt 1 created real geometry but rejected its own preservation comparison:
FreeCAD assembly containers acquired computed aggregate shapes and a transient
`_Part_ShapeCache` after receiving the new BRep descendant. The diagnostic found no
original leaf mesh, placement, expression or joint-control changes. The authorized
new child was the only membership addition. The host did not edit that candidate.

Astra received the prior code and measured diagnostic facts, then submitted attempt
2. It retained strict original leaf/hierarchy/transform/control checks and excluded
computed aggregates/runtime caches from leaf-content comparison. The unchanged
module saved `out/so101-camera-mount/attempt-2/build/assembly.FCStd` after 5.86 seconds
in native FreeCAD. Build-time self-checks report one valid solid, 2815.737905 mm³,
zero overlap with the declared reference envelopes, and unchanged source files.
These self-checks are not independent acceptance; see the recorded summary for
current evaluator status. Parent GUI capture runs the same module in a separate
output directory and must be described as an actual native replay.

To reproduce an authorized headless build from the recorded model module:

```sh
cad/freecad/run.sh evals/cad/so101_camera_mount/execute_build.py \
  evals/cad/so101_camera_mount/recorded/attempt-2/candidate.py \
  out/so-arm101-one-shot/attempt-2/assembly.FCStd \
  out/so101-camera-mount/reproduction
```

The host refuses reuse of an execution directory, records actual checkpoint object
lists, and verifies candidate/source hashes. Independent verification must still
check native parameter edits after reopening and source preservation; GUI images
are not a substitute for those checks.
