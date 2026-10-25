# Independent SO101 camera-mount check

Run after the candidate has been executed and saved:

```sh
python3 evals/cad/so101_camera_mount/checks/run.py \
  --candidate out/so101-camera-mount/attempt-2/build/assembly.FCStd \
  --out out/so101-camera-mount/evaluation-2
```

The output directory must be new. Two bounded FreeCAD processes inspect the saved candidate, then reopen an evaluator-owned checkpoint and change PlateThickness by +1 mm and StandoffHeight by +2 mm. The candidate and original arm files are hash-checked and never saved by the evaluator.

Checks preserve original object types, labels, global placements, mesh/BRep leaf geometry, joint angle metadata, expressions and original child membership. New children are allowed. App::Part, App::Origin and document/link-group aggregate Shapes are derived from children; they are deliberately not accessed or hashed, avoiding transient shape-cache materialization. This exception does not exempt physical leaf geometry.

The evaluator checks a valid single-solid final mount, real final-solid changes after edits, plate and four drilled-boss dimensions/containment, camera frame, camera 21 × 12.5 mm hole pattern, registered wrist STEP collision/contact, coaxial wrist bores and nominal M3 shaft/head/driver paths, and separately labeled camera and ribbon reference space. Construction helpers are measured without counting them as duplicate physical parts. Mount hole diameters are measured; source PCB/wrist diameters do not impose identical clearance diameters on the new mount.

Attempt 2 passed all local gates at baseline, fresh reopen, T=4/H=6 and T=4/H=8. Evidence: `out/so101-camera-mount/evaluation-2/result.json` with per-state native details and evaluator hashes. Candidate SHA256: `cdd100c8baddb2f3e07f828122064226d096c207bae6b7d7167f81b7a843973b`.

This is static local CAD verification. It does not qualify whole-arm collision-free motion, strength, printability, selected screw length/thread engagement, captive-nut installation with motor 6 present, or a manufacturer-certified ribbon bend radius. Original arm visuals remain meshes. The independently registered wrist STEP is a valid solid; registration establishes measured bore/plane correspondence, not whole-part mesh/BRep equivalence. Camera and cable references are simplified assumptions, not manufacturer CAD.
