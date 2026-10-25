# CAD handoff (for Astra), 2026-10-24

## Direction: FreeCAD

Pat decided that headless FreeCAD (FreeCADCmd 1.1.4) is the CAD backend, with Wright as the layer on top of it. The decision and the evidence for it are in docs/TODO.md, in the section "CAD backend decision: FreeCAD (Pat, 10/24)". Everything below was built on build123d / OCP, which is Open CASCADE 7.8. FreeCAD is also built on OCCT, so the kernel-level findings and most of the outer layer still apply.

| Piece | Verdict for FreeCAD |
|---|---|
| Worker-pool protocol (`cad/wright_cad/worker.py`: newline-delimited JSON-RPC 2.0 over stdio, stdout dup'd so prints go to stderr, a `ready` notification) | **Keep.** One persistent FreeCADCmd process per worker. The cold-import cost is the reason the pool exists: build123d took 20–24 s to import here under load, about 12 s of it scikit-learn. |
| Part-library contract (`@part`, `Param`, `Annotated` typed and bounded params, one function per `cad/parts/<domain>/<name>.py`, named joints, `describe`/`check_params`) | **Keep the contract.** Swap the body: part functions return FreeCAD `Part.Shape`/PartDesign bodies instead of build123d Parts. `library.py` itself has no build123d import and ports as-is. |
| Declarative `Assembly` / `Loc` / `Mate` (`cad/wright_cad/assembly.py`) | **Keep as the agent-facing layer.** Map `Mate` onto FreeCAD Assembly joints (src/Mod/Assembly/JointObject.py), or keep resolving it ourselves from joint placements. |
| STEP AP242 with names and colours (`kernel.export_step_ap242`) | **Keep the knowledge, drop the code.** OCCT's `write.step.schema` must be set before the writer exists, and build123d's `MakeHeader(0)` path ignores it. FreeCAD has the same OCCT statics: set `Interface_Static` `write.step.schema=AP242DIS` (FreeCAD's Import prefs) and check `FILE_SCHEMA` in the output. Also note that the FILE_SCHEMA line can wrap across lines. |
| GLB export, edges.json | **Port.** FreeCAD's ImportGui/glTF export needs the GUI on some versions, so test it headless. The OCCT `RWGltf_CafWriter` route is what build123d's `export_gltf` uses. |
| Interference/clearance (`BRepAlgoAPI_Common` volume plus `BRepExtrema_DistShapeShape`, bounding-box broad phase, declared contacts vs clashes vs "floating") | **Keep.** These are the same OCCT calls, through `Part.Shape.common()` / `distToShape()`. |
| Mass, CG and inertia (`BRepGProp.VolumeProperties`, inertia rotated into the world plus parallel axis) | **Keep the math.** In FreeCAD use `Shape.Volume`, `CenterOfMass`, `MatrixOfInertia`. |
| truck (Rust B-rep) | **Drop.** See the evidence below. |
| csg.rs replacement by manifold | Independent of FreeCAD. See "csg.rs verdict". |

## What exists

**Python, under `cad/`**
- `cad/wright_cad/`: `library.py` (part contract), `assembly.py` (declarative assembly), `materials.py` (densities with sources, colours, SimReady non-visual tokens), `kernel.py` (build, assemble, AP242 STEP, GLB, edges, mass properties, interference), `worker.py` (JSON-RPC worker), `__init__.py`.
- Other agents have since added `validation.py`, `build.py`, `aircraft.py`, and edited `assembly.py`, `worker.py` and `library.py`. I have not reviewed those edits.
- `cad/parts/common/`: `tube`, `disc`, `revolved`, `plate`, `block` (mine). `axial_tube` and `loft` came from others.
- Domain directories `cad/parts/rocket|air|balloon/` are empty. The rich example parts (rocket injector plate, air wing rib, balloon enclosure) and the per-domain `assembly.py` were **not written**: I was stopped before them. `cad/parts/aircraft/rib.py` and `airframe/trapezoid_fin.py` are someone else's.

**Rust**
- None. `crates/wright-cad` was never created, there is no workspace member, and no Stage impl. This means the `cad`/`usd` stages and their `wright-cli` registration still need to be written.

**Environment**
- Shared venv: `scratchpad/venv312` (Python 3.12) with usd-core 0.26.11, build123d 0.13.0, cadquery-ocp-novtk 8.0.1.1.0 and manifold3d 3.5.4. All are logged in docs/TODO.md "Downloads log".
- I deleted the old `venv-wright`.

**Scratch evidence**
These live outside the repo, in `/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/`:
- `truckbench/` holds the Rust bench (csg.rs vs truck). Its binary is in the shared `target/release/truckbench`.
- `bench_py/bench.py` runs OCC vs manifold3d and validates truck's STEP output in OCC.
- `bench/` holds the result JSONs (`rust_bench.json`, `py_bench.json`) and the STEP/STL outputs.
- `simready_docs/` holds the NVIDIA SimReady spec files and validator sources (single raw files).

### How to run the worker (works today)
```
cd cad && printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"build","params":{"part":"common.tube","params":{"od":50,"wall":2,"length":300},"key":"t1","cache":"/tmp/c"}}' \
  | $SCRATCH/venv312/bin/python wright_cad/worker.py
```
Methods: `ping`, `describe{part}`, `list{domain?}`, `plan{domain,design}`, `build{part,params,key,cache,material?,density?,stl?}`, `assemble{name,instances,out_dir}`.

Measured in a two-part smoke test (a tube with a revolved cap mated top→bottom):
- tube build: 79 ms; including its BRep, STEP and JSON writes, 167 ms;
- revolved build: 10 ms;
- assemble (placement by mate, mass/CG/inertia, pair checks, STEP, GLB, edges): well under 1 s;
- the whole session took about 20 s wall, almost all of it the import.

`assembly.step` reads back in OCC (`STEPControl_Reader` returns RetDone, and `BRepCheck_Analyzer` reports it valid). Product names come through (`t`, `g`, `a`, `b`).

## Kernel evaluation (evidence)

The test is the same four real Wright parts in every kernel, at Wright's dimensions:
1. the rocket fin-alignment jig: ring minus 4 slots;
2. a tank bulkhead;
3. the balloon +x foam wall with 2 conical camera ports;
4. a VTOL sd7062 rib, 210 mm chord, with 10 mm and 3 mm spar holes.

Volume errors are measured against analytic volumes.

| Part | csg.rs (wright-geom) | truck 0.6 / shapeops 0.4 | OCC 7.8 (build123d) | manifold3d 3.5.4 |
|---|---|---|---|---|
| jig | 92–131 ms, −0.17 %, **950 non-manifold edges** (T-junctions) | **panicked** after 6.2 s (`transversal/integrate/mod.rs:110` unwrap) | 217 ms, exact (1e-14 %), BRepCheck valid, 8 CYLINDER + 16 PLANE faces | 11 ms, −0.17 %, manifold |
| bulkhead | 0.04 ms, −0.41 % (40-gon) | ok; OCC reads its STEP, valid | 0.4 ms, exact | 0.2 ms, −0.41 % |
| wall + cone ports | 1–11 ms, +1.14 %, 298 bad edges | **returned None** after 24.8–26.5 s | 21 ms, +0.26 % vs my approximate analytic, valid, 2 CONE faces | 2 ms, +1.14 % |
| rib + holes | 159–306 ms, +0.011 %, 924 bad edges | 7.8 s, +0.02 %, 4 bad tessellation edges; needed manual orientation fix (first run was inside-out) | 436 ms, exact, valid, 2 CYLINDER faces | 4 ms, +0.011 % |

**Why truck was dropped:**
- `truck-shapeops/src/lib.rs` documents that booleans are "supported only for shapes where faces intersect transversally" and fillets only on "a single edge whose end vertices are each adjacent to exactly three faces".
- `truck-modeling/src/geometry.rs` `enum Surface` has no analytic cylinder or cone. Circles are NURBS, and revolves export as `SURFACE_OF_REVOLUTION`.
- `truck-stepio/src/out/mod.rs` hard-codes `FILE_SCHEMA(('ISO-10303-203'))`.

These match the failures in the table. That leaves the decision between OCC and FreeCAD, and FreeCAD is OCC.

**opencascade-rs:** I did not test it. `opencascade-sys` builds OCCT from source with cmake, which is not installed, and the build would need gigabytes of disk. The OCP wheel ships the dylibs but no headers.

### csg.rs verdict
**Replace for printing/STL; keep only as a fallback until replaced.** Its booleans leave T-junctions: 950, 298 and 924 non-manifold edges on three of four parts. Its STLs are therefore not watertight 2-manifolds. The volumes are fine, so interference volumes are still roughly right.

manifold gets the same volumes, guarantees a manifold result, and is 8–40× faster. The Rust options I found and did **not** try are `manifold-csg` (FFI to the C++ library, needs a C++ build) and `manifold-rust` (a pure-Rust port, 0.18). If FreeCAD/OCC produces the printed-part STLs from the B-rep, csg.rs is only left in wright-geom's all-pairs interference check, and that can move to OCC (`common`) too.

## SimReady USD (not started; spec read)

The spec files are in `scratchpad/simready_docs/`, from `github.com/NVIDIA/simready-foundation` `nv_core/...`. The rules to target:
- **Stage:** `upAxis = "Z"`, `metersPerUnit = 1.0`, `defaultPrim` set (FET_001 UN.006, UN.007, HI.004). Everything under one Xform root (HI.001, HI.003). Meshes have extents and normals (VG.002, VG.027).
- **Rigid bodies** (`capabilities/physics_bodies/physics_rigid_bodies/validation.py`):
  - `PhysicsRigidBodyAPI` on each part (RB.001), and at least 2 of them for multibody (RB.MB.001);
  - mass authored on the body or on all of its colliders (RB.007), inertia satisfying the triangle inequality and `principalAxes` normalised (RB.008);
  - `CollisionAPI` on a Gprim (RB.COL.001), with every body having one (RB.013);
  - nested bodies joined by a joint (RB.012).
- **Joints:** `physics:body0`/`body1` targets must exist (physics_joints JT.001).
- **Physics material:** `material:binding:physics` to a `UsdShade.Material` with `PhysicsMaterialAPI` (physics_materials PMT.001).
- **Semantics:** `SemanticsLabelsAPI:<taxonomy>` with `semantics:labels:<taxonomy>` as a `token[]` on every renderable gprim (SL.001). Wikidata labels use the `wikidata_qcode` instance with values `^Q[0-9]+$`.
- **Non-visual material:** `omni:simready:nonvisual:base|coating|attributes`. The allowed values are in `materials.py` (`nonvisual`).
- **Validation:** Pixar `usdchecker` logic is `UsdUtils.ComplianceChecker` in usd-core. Port the rule bodies above into a small checker.

The mass, CG and inertia per part should come from the B-rep properties described above.

## Half-done or broken (state when I stopped)
- **Rust:** no `crates/wright-cad`, no Stage impls (`cad`, `usd`), nothing registered in `crates/wright-cli/src/stages.rs`, no part cache in Rust. The plan was to hash the part file, the domain's `_*.py`, `common/`, `wright_cad/` and the params; `library._dependency_files` already lists those files.
- **Stage inputs:** `wright_core::stage::input_hash` does not hash `cad/` library files. A part-library edit with an unchanged design will therefore skip the `cad` stage. It needs a stage fingerprint hook, which belongs to the core owner.
- **Example parts and assemblies:** none of the rich examples (injector plate with O-ring groove and orifice pattern, wing rib, enclosure) exist, and there is no per-domain `assembly.py` for rocket, air or balloon.
- **Explode:** default exploded offsets are just 0.5 × (part CG − assembly CG). That is crude.
- **Workspace build:** `cargo check --workspace` currently fails in **other agents'** crates: wright-fab `fab.rs` cannot resolve `wright_core` (looks like a missing Cargo dependency), and there was a wright-cli argument-count error earlier. I made no Rust edits to the workspace. The only repo files I touched are `cad/` and the Downloads log lines in docs/TODO.md.
- **Domain runs:** I made test runs in `scratchpad/cadruns/`. The aircraft run took 26 min, mostly slicing. The balloon and rocket runs need a real spec: `{}` fails with "missing field mission/name".

## Ranked next steps
1. Get FreeCADCmd running headless (needs Pat's OK for the 650 MB DMG / 2 GB install and disk space). Then port `worker.py` to run under FreeCADCmd unchanged except for the kernel module, and confirm the import time.
2. Port `kernel.build`, `assemble` and `export_step_ap242` to FreeCAD `Part`/Import. Assert `FILE_SCHEMA` AP242 and BRepCheck-valid in a test.
3. Write `crates/wright-cad` (pool of N workers, part cache keyed by `_dependency_files` plus params, Stage impls `cad` and `usd`) and register both in `wright-cli` `registry()`.
4. Write one rich example part and an `assembly.py` per domain, then hand the contract to the air, rocket and balloon workers.
5. Write the USD exporter from `assembly.json` (mass, CG, inertia, contacts become fixed joints) plus a validator ported from the SimReady rules above.
6. Have printed-part STLs come from the B-rep, then retire csg.rs booleans (or swap them for manifold) in wright-geom.

## Desktop viewer (wright-desktop; owner of the app shell, not of CAD)

The desktop app only *displays* assemblies; it does no CAD editing and has
no kernel. Everything an editor needs to hook into is in two files.

* **Loading: `crates/wright-desktop/src/scene.rs`.** `scene_files(artifacts, run_dir)`
  picks the source: a `.glb` named by an `Event::Artifact`, else the first
  `<run_dir>/cad/*.glb` (one with "assembly" in its name first), else the
  run's STLs (the guide's rule). `Scene::load_gltf(path)` reads the GLB with
  the `gltf` crate:
  * every node with a mesh is one **part**, named by `node.name()` (else the
    mesh name); node transforms are accumulated to world;
  * TRIANGLES primitives are faces; LINES / LINE_STRIP / LINE_LOOP
    primitives are the B-rep **feature edges** drawn black. With no line
    primitives, edges are derived from the triangles by angle (three.js
    `EdgesGeometry` rule, 30 degrees);
  * node (or mesh) `extras` may carry `{"material": str, "mass_g": num |
    "mass_kg": num, "params": {name: value}, "interferes_with": [part
    names]}`; shown in the part panel (params read-only), masses give the
    CG marker, `interferes_with` paints the part red;
  * units: millimetres expected; a model under 20 units across is taken as
    metres (glTF's unit) and scaled x1000.
  The result is `Scene { positions, normals, part_of_vertex, edge_positions,
  part_of_edge_vertex, parts: Vec<ScenePart> }`: a triangle soup plus part
  names. STEP is not read directly; derive a GLB from it.
* **Drawing and picking: `crates/wright-desktop/src/viewer3d.rs`.**
  `Viewer::set_scene(Scene)` uploads it; `Viewer.selected` / `hover` are
  part indices; `Viewer::project` maps world to screen for overlays;
  `Scene::pick` is the CPU ray cast (honours explode and section).
* **Where an editor hooks in.** In `crates/wright-desktop/src/app.rs`,
  `parts_panel` shows the selected part's `params` read-only: an editor
  would replace that grid with its own widgets, call its regeneration, and
  hand the new GLB to `Viewer::set_scene(Scene::load_gltf(..))` (or patch the
  affected parts' ranges in `Scene` and bump `Viewer.generation`). The app
  rescans the run's artifacts every frame and reloads when the chosen files
  or their modification times change, so a GLB rewritten in place reloads.

## Addendum for Astra, 2026-10-24 ~17:20: "I don't want to reinvent FreeCAD" (Pat)

Pat's instruction, verbatim: "fix it. like i dont want to reinvent freecad". The main
Claude session started this and was told CAD is ChatGPT's lane, so it stopped; the
state below is where it was handed back. No domain code was changed by that attempt.

What still reinvents FreeCAD today:
- Every domain builds its solids with the in-house mesh kernel (`crates/wright-geom`:
  shapes.rs primitives + csg.rs, a csg.js port whose booleans leave non-watertight
  STLs; see the kernel table above). Call sites: crates/wright-balloon/src/{geometry.rs,
  build.rs}, crates/wright-rocket/src/geometry.rs, crates/wright-air/src/{geometry.rs,
  print.rs}; wright-guide and wright-fab read those meshes.
- crates/wright-fab/src/mill.rs hand-rolls mill toolpaths that FreeCAD CAM generates
  with real post-processors (src/Mod/CAM/Path/Post/scripts).

Target (recorded in CLAUDE.md): every solid, boolean, STL/STEP/GLB export, mass
property and interference test goes through the persistent FreeCAD worker
(`crates/wright-cad`, `cad/freecad/worker.py`); csg.rs stays only as the fallback when
FreeCAD is not installed; FreeCAD CAM supplies mill toolpaths; lathe.rs, renders,
slicing, CFD, FEA and flight sims stay in-house. Acceptance: FreeCAD STLs watertight,
volumes within 1 % of the old meshes on each demo design, every domain test and
`cargo clippy --workspace --all-targets -- -D warnings` green, `scripts/verify.sh
--quick` green. Shared-tree rules in CLAUDE.md apply (one cargo lock across sessions,
no `cargo clean`, keep disk > 3 GB).

State left by the stopped attempt (16:49-16:59), kept because it compiles and is
self-contained; yours to finish, change or delete:
- `crates/wright-geom/src/kernel.rs` (409 lines): every `shapes` primitive and every
  `Mesh` boolean/translate/rotate now also records a `Solid` program node
  (`Mesh.solid: Option<Arc<kernel::Solid>>`, lib.rs). `kernel::realize` sends the
  programs to one persistent FreeCAD worker, which rebuilds them as OCCT B-reps,
  writes watertight STLs (MeshPart.meshFromShape), measures volume/CG/inertia, runs
  all-pairs interference with Shape.common, exports AP242 STEP + GLB, and the
  FreeCAD tessellation replaces the csg mesh. `WRIGHT_GEOM_KERNEL=csg` or a missing
  FreeCAD keeps csg.rs and says so in the report.
- `cad/freecad/kernel_program.py` (224 lines) + `METHODS['kernel_build']` in
  `cad/freecad/worker.py` (line ~425): the worker side.
- `crates/wright-balloon/src/build.rs` ~line 44: the balloon rack is the first caller.
  Rocket and air were not switched (the agent was stopped there).


## To Claude: AI CAD branch and scope — 2026-10-24

Pat requested a branch for the AI CAD work and notification to Claude. The shared
checkout is now on `codex/so-arm101-ai-cad`. This repository has no initial commit;
the branch is unborn and all existing files remain untracked. No files were
committed, pushed, reset, or isolated into another checkout. Other sessions in this
same directory see the same branch and files.

Current scope from Pat: **AI CAD only**, using SO-ARM101 as the one-prompt CAD
benchmark, with target October 25 at 09:00 America/Los_Angeles. Balloon, rocket,
and VTOL work is stopped. FreeCAD remains the CAD engine; use its Python API and
CLI for modeling, with Rust as the thin agent/orchestration overlay. Do not build
a replacement CAD platform or require a new typed action for every FreeCAD feature.

Current CAD work and evidence locations:
- `cad/projects/so-arm101/source-manifest.json` pins official TheRobotStudio/SO-ARM100
  commit `a758567c3978dfeefe282ede0500085a48fe8f78`; 20 selected assets were acquired
  with Git blob SHA1 and SHA256 verification. Source license: Apache-2.0.
- `cad/projects/so-arm101/import_native.py` imports the official reference assembly;
  output work is under `out/so-arm101-native/`. Check completion evidence before
  claiming success. Importing a preassembled STEP is not an AI one-shot assembly pass.
- `out/so-arm101-spec/official-kinematic-ledger.json` records the official URDF's
  8 links, 6 revolute joints and 1 fixed joint, with local-frame transforms/limits.
  Those limits do not establish collision-free or safe physical motion.
- `evals/cad/training/so_arm101/` contains CAD corpus generation/curation and tests.
  Agent reports 3 passing native synthetic reference traces (2 train, 1 held out),
  with reopen and negative checks. They are authored reference programs, not model
  performance results. Official SO101 assets stay benchmark-only. No model weights
  have been trained; the SO101 one-shot benchmark has not passed yet.

CAD agents may still be finishing their owned outputs. Preserve shared edits and
inspect current evidence before resuming. The next CAD milestone is a frozen
one-prompt assembly task with withheld reference placements, native editable output,
part coverage, joint/geometry checks, and successful reopen without manual repair.


## AI CAD branch update — 2026-10-24

At Pat's request, origin is now https://github.com/machmoon/calhacks.git and the
initial source commit a0fe9f9 was pushed to main. Pat then requested a separate
AI CAD branch: the shared checkout is now on `codex/so-arm101-ai-cad`, based on
that main commit. The earlier unborn-branch note is superseded. Work continues
only on SO101/AI CAD, including independent native articulation and a recorded
single-prompt subscription Astra attempt. Generated outputs and downloaded
upstream assets remain ignored; source manifests preserve their identities.


## SO101 one-prompt CAD milestone — 2026-10-24

The reusable experiment is `evals/cad/so_arm101_one_shot/`; it uses Wright's
existing subscription CodexTransport and one isolated native FreeCAD script
execution. It is an evaluation-local executable, not yet a production `wright`
CLI subcommand. Reproduction commands and compact results are checked in there.

Prompt iteration 1 failed on unsupported FreeCAD type App::Feature. Iteration 2
added verified runtime API guidance and generated a new, unmodified script that
passed all seven independent mesh assembly/articulation gates. It assembled 17
provided official mesh instances in eight link frames with six persistent joint
controls. Fresh-process reopen and editing passed. Both attempts and their hashes
are preserved; this is not a first-ever-attempt pass or a measured success rate.

Actual model artifact: `out/so-arm101-one-shot/attempt-2/assembly.FCStd`.
Independent evidence: `out/so-arm101-evaluation/actual-attempt-2/result.json`.
Host-rendered actual geometry: `out/so-arm101-one-shot/attempt-2-preview/`.
Source remained unchanged during evaluation and rendering. Authored reference
baseline artifacts in `out/so-arm101-candidate/` are separate from model results.

Limitations: supplied STL geometry, not recovered solid feature history; one
source mesh is not watertight; no collision-clearance certification or native
out-of-range enforcement claim. The separate jaw registration probe preserves a
real source STEP BRep and measures sampled surface agreement. It is not a full
solid-arm reconstruction. AI CAD remains the only active scope.


## SO101 camera-mount assistant task (2026-10-24)

Current scope is small useful edits in existing FreeCAD projects. Wright generated
an editable camera mount for Raspberry Pi Camera Module 3 Standard on the supplied
SO101 wrist. Prompt and manufacturer dimensions: `evals/cad/so101_camera_mount/`
and `cad/projects/so101-camera-mount/camera-spec.json`.

Two model attempts are recorded. Attempt 1 failed its preservation self-check
because it hashed derived assembly caches. Attempt 2 passed independent native
checks for source preservation, one solid, wrist and camera interfaces, nominal
fastener access, reference clearances, reopen, and T/H geometry updates. This is
not a one-shot-success claim. No original source meshes were rebuilt.

Native artifact: `out/so101-camera-mount/attempt-2/build/assembly.FCStd`.
GUI execution evidence: `out/so101-camera-mount/attempt-2/viewport-capture/`.
The same unmodified repaired module was executed again in FreeCAD and viewport
frames captured at actual modeling checkpoints. This is a captured rerun, not
a continuous first-attempt desktop recording. The capture host is reusable;
it does not author model geometry.

Evidence is archived under `evals/cad/so101_camera_mount/recorded/`. Local static
geometric validation is not strength qualification or a full robot-motion sweep.
Continue building this existing-project edit workflow rather than a new CAD stack.

## Addendum 2 for Astra, 2026-10-24 ~19:40: electronics in CAD (Pat)
Pat: "in the cad we do a bad job for the electronics importing the pcb and the wires and stuff."
Ask: import real PCBs (vendor STEP under cad/components/vendor, KiCad-exported STEP from the
electronics pipeline) as placed parts with connectors, and model the wiring harness (wires, bundles, routing
clearances) as geometry, so the assembly shows boards, connectors and cable runs, not empty boxes.

## Addendum 3 for Astra, 2026-10-24 ~22:30: VTOL trace cad-project-1792903184 (Pat)
Analysis of the 692 s trace (planning 17 s, freecad_build 230 s, audit 430 s). The model is
2.5% of wall time; the critical path is renders plus serial probes. Asks, biggest win first:

Speed
1. Run the eight independent probes in parallel processes. Their times sum to 428 s, so they
   run serially today; the slowest probe (edits, 175 s) bounds the parallel version.
2. Start the audit while the renders run; no probe needs the pictures. With 1 and 2 the run
   is about 4 minutes instead of 11.5.
3. Renders are 200 s of the 230 s build (~32 s per view, 67 s for cutaway plus exploded).
   Render views in parallel, or render from the bounded display mesh (exports in 3 s)
   instead of FreeCAD's renderer.
4. pairs (108 s): bounding-box broad phase before the exact boolean, so only overlapping
   boxes get the exact test (the FCL broad-phase/narrow-phase split).
5. edits (175 s): run the span/chord/boom/servo/battery edits as separate parallel rebuilds.

Correctness
6. Propeller envelopes overlap only their own motor (LiftProp* vs LiftMotor*). That is the
   designed shaft interface; declare allowed contacts per pair (as MoveIt's allowed
   collision matrix does) so propeller_sweeps can resolve instead of staying inconclusive.
7. Service access treats the closed assembly as fixed. Model removable parts (Hatch) and
   check bolts after their removal: front tray bolts are likely false positives. Rear tray
   bolts (also blocked by wing skins) and tail clamp bolts (blocked by tail surfaces) look
   like real design issues: needs an access hole or a moved bolt.
8. Probe-to-gate mapping: pairs=True and exports=True leave their gates pending; edits=False
   leaves its gate pending instead of failed. Either promote the probe result or state the
   extra evidence the gate needs.
9. 12 of 17 gates have no probe (mass_propulsion_performance, bom_geometry_coverage,
   load_paths, electrical_and_wiring, ...). Wright's wright-air checks already cover
   mass and propulsion; wire them into this path so acceptance is reachable.
10. Minor: phase label prints "STEP?" for "STEP;" (encoding); an empty 0.0 s phase
   "Rendering actual native assembled and internal cutaway views" is a stale label;
   sub-second phases are missing from the progress list.

## Addendum 4 for Astra, 2026-10-24 ~23:00: demo priority, PCB into the VTOL (Pat)
Pat's top demo priority is the full stack: AI PCB design flowing into CAD. Wright's new Rust
electronics pipeline (wright-electronics) designs the VTOL servo power board (6 V, 5 A buck,
TPS5450), passes kicad-cli ERC/DRC/parity, and exports the board STEP with real component
bodies (kicad-cli pcb export step --subst-models). Current size about 46.5 x 26 mm.
Ask: take that STEP (path will be written below by the electronics lane), seat it in the VTOL
avionics bay on standoffs at its mounting holes, run your collision/access probes on it, and
render one view showing the board in the airframe. A connector-side clearance for the servo
harness is a bonus. Keep it on the existing FreeCAD assembly; no new CAD stack.

Electronics lane note (lane E, 2026-10-24 23:55). STEP path: `out/demo-pcb/cad/board.step`, made by
`wright run --domain electronics "servo power board for the VTOL: 6 V, 5 A, five servos from a 4S pack, fits a 60 x 40 mm bay" -o out/demo-pcb`
(any `-o DIR` gives `DIR/cad/board.step`, plus `DIR/cad/board.glb` and `DIR/cad/manifest.json` with
`width_mm`/`height_mm`/`thickness_mm`). Last run: outline 33.5 x 43.0 mm, 1.6 mm board, kicad-cli's default STEP origin,
components on top. The size changes per run (placement seeds), so read it from
the manifest. **Mounting holes:** four M3 holes (3.2 mm drill) sit in the board corners (`Board.holes`, added 10/25); seat it on M3 standoffs there. The STEP has no body for J3 (XT30, no KiCad model), U1 (the
footprint names an HTSOP model KiCad 10 does not ship) or L1 (SRP1245A, not shipped); the manifest's
`component_models` check lists them. The servo headers (J1, J2, J4-J6) and the XT30 are the harness side.

- 10/25 (cost+latency session): `cad/freecad/worker.py` `inspect_tree` (lines 101-102, file mtime 10/24 23:14) checks `exhaustive_distances` and `require_connected`, which are not its parameters (they belong to `check`). Every `inspect_tree` call raises `NameError: name 'exhaustive_distances' is not defined`, and this fails `wright-agents` lib test `harness::cad_visual_tests::wing_analysis_emits_json_plot_and_loads_project_artifacts` (harness.rs:2280). Fix: drop those two lines from `inspect_tree`. Not touched by Claude sessions.
