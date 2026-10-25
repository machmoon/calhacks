# Official SO-ARM101 CAD source in FreeCAD

## Restore the pinned inputs after cloning

Downloaded CAD is a local cache, excluded from Git. Run
`python3 cad/projects/so-arm101/restore_sources.py` to restore the independent
URDF/mesh articulation bundle and its license/attribution files. Add `--bundle all`
for the original STEP reference assets as well. `--check-only` verifies existing
files without downloading. Every file must match its recorded byte count,
SHA256, and Git blob SHA1; a mismatched existing file fails rather than being
silently replaced. The default bundle is for assembly/articulation, not recovered
parametric solid feature history.

The official Hugging Face [SO-101 documentation](https://huggingface.co/docs/lerobot/so101) links to [TheRobotStudio/SO-ARM100](https://github.com/TheRobotStudio/SO-ARM100), whose current README covers SO-101 despite the older repository name. This acquisition is pinned to commit `a758567c3978dfeefe282ede0500085a48fe8f78`. The repository declares Apache-2.0; its complete license and attribution documents are preserved under `upstream/`.

`source-manifest.json` records exact download URLs, sizes, upstream Git blob SHA1 verification and SHA256 checksums. Only 20 selected files were downloaded (27,970,114 bytes), including the assembly and individual SO101 STEP parts. The complete inspected repository tree is preserved in `upstream/tree.json`. No FCStd/F3D/F3Z native author feature history was found in that tree.

The assembly is STEP AP214, named `SO101 Assembly v4.step` internally, dated 2025-04-15 and exported by Autodesk Translation Framework. `Import.insert` preserves its original component hierarchy and placements in FreeCAD. Imported BRep parts can be placed and used in subsequent native modeling operations; STEP does not recover the author's sketches, feature history or mechanical joint constraints.

## Native workflow

From the repository root, using the installed FreeCAD runtime:

```sh
cad/freecad/run.sh cad/projects/so-arm101/import_native.py
cad/freecad/run.sh cad/projects/so-arm101/audit_native.py
cad/freecad/run.sh cad/projects/so-arm101/preview_native.py
cad/freecad/run.sh cad/projects/so-arm101/edit_smoke.py
```

The stable source import is `out/so-arm101-native/official-assembly.FCStd`, accompanied by `import-report.json`, `native-audit.json` and an actual native-geometry preview. The original official STEP and the stable FCStd remain unchanged by the read-only audit and preview. `edit_smoke.py` creates a separately labeled test copy with a 1 mm isolated jaw placement change, verifies persistence after reopen, and reverts it in memory. That smoke test is not valid joint motion or a design revision.

## Observed source structure and limitations

The import contains 1,401 document objects, including 126 assembly groups and 267 BRep leaf features. Of those leaves, 266 have one solid each; one LED component is a valid nine-face surface without solid volume. Construction origins/axes/planes are not extra physical parts. Global leaf bounds match the imported root bounds: approximately 110.925 × 319.351 × 253.432 mm in the source's folded pose.

The source has six invalid servo-internal `XG-ZIJI` solids: `Part__Feature214`, `Part__Feature222`, `Part__Feature230`, `Part__Feature238`, `Part__Feature246` and `Part__Feature253`. Each has eight shells and 219 individually valid faces; whole-solid validation fails. The kernel's detailed check reports `Sub-shape not in shape`. These components and the resulting invalid parent aggregates are preserved, not silently healed or removed. See `source-defects.json`; no exact defect cause or repair equivalence is claimed.

The six native servo assembly instances are `ST3215_Servo_v2` and `ST3215_Servo_v001` through `ST3215_Servo_v005`. Their nested CAD labels include `SCS215`; labels alone do not independently certify the electrical variant or gear ratio. Detailed adapter-board components, cables and fasteners are included. The preview uses illustrative colors and includes the LED reference surface.

No collision, clearance, motion or structural verification is claimed. No kinematic constraints have been added. The STEP assembly frame is retained; it has not been registered to the official simulation URDF frames. The independent official URDF ledger is maintained separately under `out/so-arm101-spec/`.

## Required servos from official documentation

The follower uses six Feetech STS3215 servos, all 1/345 gearing. The pinned BOM's default is the 7.4 V variant; the README also describes a 12 V follower option with a matching power supply. The leader is always specified as 7.4 V and uses one 1/345, two 1/191 and three 1/147 servos. This acquisition preserves the published CAD; it does not substitute a differently named servo or assert an unverified motor variant.

## Articulated independent-asset baseline

`assemble_urdf.py` builds a separate FreeCAD document directly from the pinned official `so101_new_calib.urdf` and its 13 independent STL assets (17 instances). It does not read or copy the preassembled STEP/FCStd. `articulation-source-manifest.json` pins that additional source bundle.

```sh
cad/freecad/run.sh cad/projects/so-arm101/assemble_urdf.py
cad/freecad/run.sh cad/projects/so-arm101/preview_articulated.py
```

The outputs are under `out/so-arm101-candidate/`. Eight native `App::Part` link frames follow six angle controls on `JointParameters`, with fixed URDF origin frames and native rotation expressions. The gripper is revolute. Lower/upper angle metadata comes from the URDF; arbitrary GUI edits are not automatically clamped. Native expressions and embedded meshes survive reopen without a custom Python proxy or plugin.

This is explicitly a **mesh-native articulated assembly**, not detailed BRep reconstruction. It has no board electronics, cables or screw-by-screw BOM beyond the official URDF visuals. The geometry remains source-faithful even where an official mesh is not closed. The separate independent evaluator checks URDF frames, axes, subtree motion and reopen behavior; those results do not certify watertightness, collisions, strength or manufacturing readiness.

The generator here is an authored baseline. It is not the independently generated one-shot model trial, whose inputs and evaluation are maintained separately.

## Exact BRep replacement probe

`register_jaw_brep.py` and `check_jaw_registration.py` work only on the independent official moving-jaw STEP and STL. They fit a rigid source-asset transform, preserve the native BRep, and measure sampled surface agreement. Outputs are separate under `out/so-arm101-brep-jaw-probe/`. No preassembled arm or held-out pose is used, and no triangle mesh is converted to a faceted solid. Consult the resulting JSON metrics before treating a source-frame mapping as usable; sampling is not a continuous surface-equivalence certificate.
