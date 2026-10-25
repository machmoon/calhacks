# SO-ARM101 CAD reference and one-shot evaluation proposal

Prepared 2026-10-24. Scope: AI CAD geometry, assembly, articulation and native editability only. Parent owns the final benchmark freeze. The deadline is 2026-10-25 09:00 America/Los_Angeles (16:00 UTC). No control policy, firmware, commissioning, dynamics or physical experiments are included.

## Frozen upstream evidence

Use TheRobotStudio/SO-ARM100 commit [`a758567c3978dfeefe282ede0500085a48fe8f78`](https://github.com/TheRobotStudio/SO-ARM100/tree/a758567c3978dfeefe282ede0500085a48fe8f78), with the follower `Simulation/SO101/so101_new_calib.urdf`. Source texts, SHA256 manifest, exact joint/visual ledger and independently calculated zero-pose frames are under `out/so-arm101-spec/`. The asset-import agent independently selected the same revision. Preserve the repository's [Apache-2.0 LICENSE](https://github.com/TheRobotStudio/SO-ARM100/blob/a758567c3978dfeefe282ede0500085a48fe8f78/LICENSE), upstream attribution and modification notices with redistributed derivatives. The inspected tree has no separate NOTICE file; this records upstream licensing, not independent clearance of third-party marks.

The [official URDF](https://github.com/TheRobotStudio/SO-ARM100/blob/a758567c3978dfeefe282ede0500085a48fe8f78/Simulation/SO101/so101_new_calib.urdf) contains eight links, six revolute joints and one fixed gripper reference frame. Its 17 visual instances comprise 11 printed components and six servo instances, using 13 distinct mesh files. Five servo instances use the horn model and one uses the no-horn model. This is a semantic coverage layer, **not the complete detailed STEP BOM**: the official assembly contains additional detailed hardware. Freeze that imported hierarchy separately, including every actual instance and any explicit exclusion.

| Joint, proximal to distal | Parent → child | Source limits, radians |
|---|---|---:|
| shoulder_pan | base_link → shoulder_link | −1.91986 … 1.91986 |
| shoulder_lift | shoulder_link → upper_arm_link | −1.74533 … 1.74533 |
| elbow_flex | upper_arm_link → lower_arm_link | −1.69 … 1.69 |
| wrist_flex | lower_arm_link → wrist_link | −1.65806 … 1.65806 |
| wrist_roll | wrist_link → gripper_link | −2.74385 … 2.84121 |
| gripper | gripper_link → moving_jaw_so101_v1_link | −0.174533 … 1.74533 |

All six axes are local +Z **after** the joint-origin rotation. At zero pose their world directions are approximately −Z, +Y, +Y, +Y, −X, and (0, −0.998815, −0.048663), respectively. Origins and visual offsets are preserved verbatim in the ledger. Compute `T_child = T_parent × T_origin × R_axis(q)` and `T_visual = T_link × T_visual_origin`, with origin rotation `Rz(yaw) Ry(pitch) Rx(roll)`. Convert metres to FreeCAD millimetres once.

Do not substitute the old-calibration, leader or camera variants. The [simulation README](https://github.com/TheRobotStudio/SO-ARM100/blob/a758567c3978dfeefe282ede0500085a48fe8f78/Simulation/SO101/README.md) describes different zero conventions and an unresolved gripper mapping. For CAD, the pinned jaw is revolute, not prismatic. Use the numerical source origins/limits without recentering them: the wrist-roll and jaw limits are not numerically symmetric around zero despite the README's broad “middle” description. The README also warns about removed base collision meshes, while this pinned URDF actually contains four base collision entries and 17 total. Inspect actual geometry coverage rather than assuming either documentation or collision meshes are complete.

A binary inspection of the official simulation mounting-plate STL found raw extents 0.051 × 0.042 × 0.0076, consistent with metres; its URDF has no mesh scale. This is not the usual millimetre STL convention. Individual STEP and simulation mesh local frames also need not match. Freeze any source-to-source rigid registration before evaluation. Never fit each candidate part independently: that would erase placement errors. Only one declared global frame alignment is permissible during scoring.

## Measurable acceptance gates

These are proposed **evaluator thresholds**, not manufacturing tolerances. Parent must validate and freeze them before generation.

1. **Coverage and geometry:** all expected logical and physical instances, no duplicates or proxy replacements. Imported components retain real source BReps. Compare source-normalized shapes using actual symmetric set differences and surface distance, not scalar volume subtraction. Suggested shape thresholds: max(1e−5 mm³, 1e−6 × reference volume) symmetric-difference volume, plus 0.01 mm surface distance. Record shells/meshes distinctly; do not award solid geometry credit to a mesh conversion merely because it renders.
2. **Assembly registration:** each declared instance, joint-axis line and attachment datum within 0.1 mm and 0.1° of frozen reference after the single global alignment. Repeated servos must have distinct instance identities. Evaluate both translation and orientation; bounding boxes alone are insufficient.
3. **Articulation:** six persistent native angle controls with correct local axes, hierarchy and source limits (1e−6 rad numerical limit tolerance). Moving a joint moves its complete descendant subtree, preserves sibling/upstream parts and rigid intra-link relationships. The fixed gripper frame remains attached to its specified link. Reject out-of-range queries explicitly rather than silently clamping.
4. **Native editability and reopen:** save FCStd, close its process, reopen in a fresh FreeCAD process, then apply new evaluator-selected angles and recompute. Compare resulting transforms independently. Suggested reopen-only numerical tolerance: 1e−6 mm and 1e−6°. A pre-rendered pose sequence or script that only writes unrelated solids does not satisfy persistent assembly editability. Imported STEP history is not native parametric part history; report these capabilities separately.
5. **Motion geometry:** evaluate individual-joint and combined interior poses against an independently validated source trajectory. Use ≤2° sampled increments as a proposed sampling bound, then refine near minimum clearances. Finite samples do not certify a continuous sweep. Check actual geometry, including base and mounting parts. Joint limits do not establish a collision-free configuration box. Freeze intended contact/interference allowances by specific part/interface and measured source behavior; never exempt every adjacent-link pair. Include a known colliding pose and out-of-range query as negative controls.
6. **Evidence:** native file, source/hash manifest, instance/joint ledger, exact inputs, authoring script and execution log, fresh-process check results, and exterior/articulated previews. Record building time, external audit time, timeout and unknown states separately. FreeCAD Python/CLI is the primary authoring path; typed helpers and Rust orchestration are optional conveniences.

## Feasible one-shot cases

**A — assembly:** provide the frozen disassembled official component bundle and an explicitly declared reference information set; request a native follower assembly with six articulated axes. If URDF transforms are supplied, call this CAD assembly automation, not geometric inference. If the finished official STEP assembly is supplied, call it import/articulation, not reconstruction. Do not conflate these tracks.

**B — saved-model articulation:** test each joint independently, one combined interior pose and a new pose after reopening. Parent selects held-out values only after validating the official reference. The independent reference math in `out/so-arm101-spec/build_reference.py` uses only the standard library and URDF, not the candidate's FreeCAD functions. It provides public zero-frame data, not a frozen hidden query set.

**C — optional native geometry edit:** create one explicitly dimensioned adapter at a frozen real arm interface, then change its specified spacing while preserving the original arm and mating datum. Parent must first fix the actual attachment frame, dimensions, clearances and expected change region. This tests new parametric geometry; A/B alone primarily test assembly and articulation. Omit C if its oracle cannot be frozen before the deadline.

Freeze prompt, input files, model settings, resource budget, oracle implementation and tolerances before one candidate attempt. Permit the model's own internal retries within that attempt, retaining the complete trace. Score its first submitted artifact before any human repair or evaluator-feedback iteration. Keep hidden queries and evaluator code out of candidate inputs. Report individual gates rather than hiding a missing limb or immobile joint in an average score.

Before evaluating the candidate, verify the evaluator rejects: a missing/duplicated part, a box proxy, a 1000× scale mistake, a world-Z axis substitution, moving only one mesh of a link, a jaw made prismatic, a saved static pose without dependencies, silent limit clamping, and blanket adjacent-link collision exclusions. Reference mismatches discovered before freeze are reference defects; do not tune the oracle to a candidate result.
