# Independent SO-ARM101 CAD evaluator

This evaluates the frozen `public_contract.json`: official mesh components assembled in native FreeCAD with persistent articulation. It does **not** certify detailed BRep assembly, watertightness, collision clearance, manufacturing suitability or physical operation. `passed` refers only to this scoped contract; separate qualification fields remain explicit.

Authoring uses FreeCAD Python/CLI. The evaluator does not import candidate code, call a candidate setter, invoke the generator, or fit candidate part placements to reference geometry. Independent standard-library URDF math supplies expected transforms. The original candidate file is hashed before/after and never saved over. The reopened artifact is an evaluator-owned posed copy.

## Run

From the repository root:

```sh
python3 evals/cad/so_arm101/run.py \
  --candidate out/so-arm101-one-shot/attempt-2/assembly.FCStd \
  --out out/so-arm101-evaluation/new-run
```

The output directory must be new. Each of two FreeCAD processes has a default 240-second timeout; `--timeout` accepts 10–600 seconds. Partial logs/measurements survive failures. A missing native artifact is an explicit overall failure.

`reference_manifest.json` fixes the official URDF and 13 STL files at upstream commit `a758567c3978dfeefe282ede0500085a48fe8f78`. `cases.json` fixes private evaluation poses and thresholds. Only the public contract, URDF and source meshes belong in candidate inputs; do not provide the evaluator, private cases, reference-frame ledger or hand-authored baseline to the model.

The first process checks source triangulation, controls and all link/visual transforms at zero, six individual joint edits and a combined edit, then saves a posed copy. The second process opens that copy, checks geometry once again, and applies a different combined edit. Triangles are not rechecked on every pose. Axis direction and axis-line errors are independently inferred from measured changes. Matrix finiteness, orthonormality, determinant and exact measurement-state coverage are checked before grading.

Local mesh geometry is compared to independently parsed binary source STL coordinates scaled to millimetres. Vertex correspondence must be within 0.0001 mm and the unoriented triangle multiset must match. This preserves source triangle geometry; it is not a general equivalence test for remeshed surfaces or a BRep reconstruction test. Native FreeCAD origin lines/planes/points are excluded only when owned by an `App::Origin` belonging to an `App::Part`; additional physical geometry remains a coverage failure.

## Verification evidence

- `out/so-arm101-evaluation/baseline-v1`: retained initial evaluator failure caused by counting native origin features as extra geometry.
- `baseline-v2`: corrected baseline passes scoped gates, source unchanged.
- `actual-attempt-1`: missing FCStd, overall failure. No repaired artifact substituted.
- `actual-attempt-2`: all seven scoped gates pass in 29.67 seconds; native artifact SHA256 `7164b2b4b70f08b2a58a00f1df3ff164eb411befdd93761ba788d2af3ea39cc6` remains unchanged.
- `negative-controls.json`: actual native copies with an omitted visual, extra box and frozen joint expressions are rejected; a real 1000× wrong-scale mesh is rejected. Originals are preserved.
- `source-mesh-characterization.json`: official `motor_holder_so101_base_v1.stl` is not reported solid by FreeCAD. The candidate preserves it; `mesh_watertightness_passed` is false even when the articulation contract passes.

Run scorer negative controls after producing baseline-v2 measurements:

```sh
python3 -m unittest discover -s evals/cad/so_arm101 -p test_evaluator.py
cad/freecad/run.sh evals/cad/so_arm101/test_mesh_geometry.py
```

Ten scorer tests include missing/duplicate observations, omitted/extra geometry, frozen transforms, nonfinite/scaled matrices, incorrect placement and lost reopened limits. Native negative fixture generation is separate in `create_negative_fixtures.py`; it modifies only evaluator-owned copies of the hand-authored baseline, never AI submissions. Collision/swept-clearance checks and native enforcement of out-of-range manual edits are not assessed by this version. Limits metadata and all tested articulation values are checked against the frozen official description.
