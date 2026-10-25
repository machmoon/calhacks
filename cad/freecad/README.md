# Native FreeCAD bridge

The selected CAD backend is FreeCAD. This directory contains the initial typed
native API; the older `wright_cad.worker` remains the build123d fallback and is
not protocol-compatible with this API yet.

`run.sh` uses the Python interpreter bundled with the installed macOS app, sets
its module path, and starts one persistent JSON-RPC worker. Override
`WRIGHT_FREECAD_APP` for another installation. It also accepts a standalone native
FreeCAD Python script for modeling and tests. The typed JSON-RPC worker does not
execute arbitrary Python tool requests.

For the SO101 one-prompt script experiment, `evals/cad/so_arm101_one_shot/` uses
Wright's subscription transport to generate one script, then executes it once in
an isolated FreeCAD process. This is separate from the persistent typed worker;
it does not require reproducing every FreeCAD operation as a Rust action.

Runtime facts verified with FreeCAD 1.1.4 for script generation:

- `App::DocumentObjectGroup` can hold custom `App::PropertyAngle` parameters;
  `App::Feature` is not a supported document object type.
- Native `App::Part` placements and expressions can retain a joint hierarchy
  across saving and reopening without a custom Python proxy.
- `App::PropertyAngle` assignments use degrees; the Python rotation object's
  `Angle` value uses radians. Do not apply an additional unit conversion inside
  a native angle expression without testing it.
- Imported STL visuals remain meshes. Their source units must be established
  explicitly; the SO101 URDF assets require metres-to-millimetres conversion.

The SO101 evaluator checks source geometry, independent link transforms and
fresh-process editing. Mesh articulation is not a solid-model reconstruction,
manufacturing validation, or collision-free motion certification.

Methods: `ping`, `document {name?}`, `make_part {spec}`, `set_parameter
{object_name, parameter, value}`, `inspect`, and `save {directory, objects}`.
Part specs contain `name`, `kind` and `params`; supported kinds are `box`
(`x,y,z`), `cylinder` (`radius,height`), `tube` (`radius,bore_radius,height`), and `mount` (`x,y,z,bores`). Each bore is
`[start_xyz,end_xyz,radius]`. Distances are millimetres. Native booleans link to
native box/cylinder features. No mesh or frozen B-rep substitutes for those links.

Every mutation uses a FreeCAD transaction. Rejected geometry restores the
previous document. Save checks selected shapes, saves the native FCStd file,
exports STEP and independently reimports it to compare volume. This is not yet
an assembly/contact/manufacturing completeness check.

Verification (set `WRIGHT_CAD_OUT` to a dedicated result directory):

```sh
cad/freecad/run.sh cad/freecad/test_native.py
cad/freecad/run.sh cad/freecad/test_worker.py
```

Do not mark runtime verification complete merely because these scripts exist.
Results are recorded in `typed_tests.json` and `worker-tests.json` after running.

Verified locally with FreeCAD 1.1.4 / Open CASCADE 7.8.1: four native geometry and
rollback tests and the persistent protocol test pass. The smoke test also reads
all 50 solids from the aircraft STEP. This confirms interchange, not aircraft fit.

The Rocket Workbench is installed at the FreeCAD user Mod/Rocket path, pinned to
`20f74c12ad0c4b89cfe3a264bea476ff83443af8`, with `rocket-headless.patch` guarding
GUI selection/view operations. Run `evals/cad/freecad_openrocket.py` with
`WRIGHT_ROCKET_WORKBENCH` pointing there. Reports record the upstream revision
and local patch hash. No format-version check is bypassed.

## Wright agent integration

`crates/wright-cad` owns one persistent worker per agent run, shared by the lead
and specialists. `cad_action` is exposed through Wright's tool schemas and
handler; CAD specialists are available for aircraft, rocket and balloon runs.
Worker startup is lazy, so runs that do not use CAD do not need FreeCAD installed.
`WRIGHT_CAD_LAUNCHER` overrides the launcher path. Ordinary calls time out after 60 seconds; full assembly checks and export validation allow up to 300 seconds;
a stopped worker is not silently replaced with an empty document.

`place` sets absolute position and axis/angle rotation. `check` evaluates all
selected physical solid pairs, and required contact pairs independently of
interference. Construction tools/bases may not be selected alongside their final
boolean outputs. `save` returns native FCStd, checked STEP and nonempty GLB files;
the run chooses the output directory and emits artifact events to the desktop.
CAD mutations do not yet update the domain design or BOM automatically.

FreeCAD's headless glTF exporter requires explicit tessellation; otherwise it
silently writes nodes without meshes. Placed shapes are baked into temporary
display copies before tessellation; the native editable source is untouched. `exchange.py` checks mesh coverage and
records metres/Y-up metadata; the desktop converts this to millimetres/Z-up.

Successful agent mutations save `working.FCStd` under the run's CAD directory.
A later session using that same directory restores this checkpoint. Final
`assembly.FCStd`/STEP/GLB exports remain explicit `save` operations. No checkpoint
is treated as evidence that the domain design or manufacturing checks passed.

STEP saves use native AP242 export and verify schema, solids, volume and placed
bounding box on reimport. The desktop round-trip test checks a translated,
asymmetric part, not only an origin-centred cube. Tube bores reference outer
height with a native expression, preserved through save/reopen.

## Editable profiles and structural studies

`loft {rings}` and `loft_shell {rings,inner}` use native constrained Sketcher
sections and ruled solid Part lofts. `set_profile {object_name,points}` replaces
a section's coordinates transactionally. Section vertex counts must agree.

`extruded_skin {profile,direction,wall}` creates a constrained planar profile,
inward Part::Offset2D, linked solid extrusions and a cavity cut. `profile_plate
{profile_object,direction,thickness,bores}` extrudes an existing tagged profile
and subtracts cylindrical seats; ribs can reference the skin cavity and follow
wall edits. Offset `Value` must stay negative. Extrusion `LengthFwd` is editable.
Construction profiles cannot be saved or checked as physical parts.

`test_profiles.py` checks analytic volumes, saved feature links and invalid edit
rollback. `aircraft.py PLAN REFERENCE OUT` migrates the current aircraft proxy
geometry and compares every output. `wing_assembly.py` builds a hollow wing study
with provisional ribs and source spars; it does not establish flight readiness.

`check` always reports geometric contact `components` and `connected`. Set
`require_connected:true` for a single assembly to reject separated groups, even
when no explicit contact pair was supplied. Edges require measured separation
within the selected tolerance and no penetration. This graph describes geometric
contact only; it does not establish joint strength, fastener retention or loads.
Run `test_connectivity.py` for chain, separation, collision and tolerance cases.

The additional `rocket-compat.patch` separates document preferences from component
materials and supports the source nose-cone/coupler hierarchy. Source inventories,
selected analytic volume/placement checks and deliberate length mutations live
in `evals/cad/freecad_openrocket.py`; full coverage remains false. Explicit support for the documented 1.11 rigid-CAD subset is now tested; unknown 1.12 and 2.0 versions remain rejected. Use the paired report verifier for current evidence.

STEP export uses plain temporary feature copies to prevent workbench Group links
from recursively duplicating selected children. Volume disagreements require
one-to-one solid matches with equal bounds and empty differences both ways;
otherwise export fails. The STEP validation sidecar records which check was used.
Run `test_exchange.py` to verify grouped selection and rejection of material loss.

`rocket-railbutton.patch` imports round source spool envelopes and linear patterns.
The source reference is the center of the base; source radius/height/spacing are
checked independently. Nonzero screw-head height is explicitly unsupported, and
no unstated screw bore is added. See `test_rocket_railbuttons.py`.

`verify_rocket_patches.py` reconstructs all installed changes from the pinned
upstream revision and the three recorded patches, then compares their bytes.
The evaluator captures untracked Python additions in its patch and fingerprints
all workbench Python sources as well as the exchange implementation.

## Opening and editing existing native models

`open_document {path}` opens an existing FCStd only in an empty CAD session.
The Rust session checkpoints it into the run directory; subsequent edits and
exports preserve the source file. Reopening validates the document, including
live recompute handlers for Python-backed features. A missing workbench cannot
silently become a frozen snapshot accepted as an editable feature tree.

`inspect` reports `proxy_type`, `assembly_container` and `editable_parameters`.
For unscaled Rocket body tubes, inner tubes and couplers, Length, Thickness and
signed AxialOffset are supported (Thickness is withheld for filled components).
Edits call the native workbench setters so automatic child diameters and assembly
positions update. Tube material volume is checked against its live parameters;
invalid edits roll back. Direct `place` on Rocket features is rejected because
placement belongs to the assembly; use supported AxialOffset edits instead.

Group membership is distinct from boolean construction dependencies. A physical
body tube and an internal tube can be exported together. Aggregate Rocket/Stage
containers cannot be selected as additional physical solids. Other Rocket feature
parameters remain read-only through this typed API.

`test_rocket_session.py` verifies opening, missing-proxy rejection, source
preservation, length/reference updates, automatic child diameter updates, invalid
wall rollback, physical parent/child export and live checkpoint recovery. It uses
the native simple-rocket benchmark artifact generated by the import evaluator.

GLB inspection metadata: mesh node names are stable native object IDs; node
extras retain `label`, `params`, and `parameter_bindings` (object name, property,
value, unit). Expression-driven dimensions are not offered as editable inputs.
`check` evidence is fingerprinted against the selected cleaned B-reps. `save`
exports `geometry-check.json` and embeds a summary in GLB asset extras; changed
geometry or output selection yields `status: not_checked` with no stale collision
flags. A current passing report covers only its selected geometry/contact scope.

Run the native metadata regression with:
`WRIGHT_CAD_OUT=out/freecad-metadata cad/freecad/run.sh cad/freecad/test_metadata.py`.

Shared layout controls:
- `make_parameters {name, parameters: {Gap: {value:12,min:4,max:20}}}` creates a
  native dimension table with positive millimetre bounds.
- `bind_parameter {object_name, parameter, source_object, source_parameter,
  scale:1, offset_mm:0}` creates a native linear expression. Targets are supported
  native dimensions or `Placement.Base.x/y/z`; Rocket assembly properties are
  excluded because their workbench setters control assembly updates.
- `set_parameter` edits the shared source; driven target dimensions are read-only.
  Invalid bounds or recomputed shapes roll back. Tables survive native checkpoint
  restoration and appear in inspection, but cannot be exported as physical parts.

`test_layout.py` tests binding and bounds rollback, read-only driven properties,
native persistence and contact preservation. `evals/cad/internal_layout_edit.py`
uses only worker actions to edit the saved 28-part fixture and verify all 378
pairs, unchanged housing and the resulting clearances. Its success is controlled
integration evidence, not a measurement of independent model design ability.

`check_translation {moving:[names],obstacles:[names],translation:[dx,dy,dz],samples:11}`
screens straight rigid translation without modifying the source. Conservative
swept AABBs against actual obstacle B-reps prove `clear` within the reported
intersection-volume tolerance. Any exact sampled collision yields `blocked`.
If swept bounds intersect but sampled poses do not, the result is `inconclusive`,
never a sampled-only pass. Selections must be nonempty and disjoint; the sample
count is bounded to 2–101. No joint solver, rotation search, cable/flex/tolerance or
fastener/tool-access certification is implied.

`test_motion.py` includes a thin-between-samples obstacle that must remain
inconclusive. `evals/cad/internal_service_access.py` checks the saved two-deck
fixture with the lid present/absent and the lower component obstructed by the
upper deck.

`boolean {name,operation:"cut"|"fuse"|"common",base,tool}` creates a new native
Part feature from two distinct existing physical single solids. `name` must be
new; operands are never replaced or rewired. Native dependencies remain editable
and survive checkpoints. Empty/invalid/multiple-solid results, cuts with no
measurable removal and fuses with no measurable addition are rejected. Volume
significance is 1e-6 mm³. Cuts prove actual removed geometry and no added solid
in the native feature construction frame; independent curved-solid volume
integrals are not used as a subtraction proof. Successful proofs use a bounded
exact-BRep cache. `test_curved_cut_validation.py` covers the real curved skin,
placed cuts, corrupt/no-op controls and reopen. Dependency traversal rejects cycles and bounds depth to
64 and visited/active objects to 512. The worker retains its request timeout.

Native scripts may explicitly open a complex source with
`worker.open_document(path, defer_source_cut_proofs=True)` for unverified draft
inspection/editing. Shape validity remains checked; only unchanged source-cut
material proofs are deferred. New or changed cuts are still strictly proved.
`check` distinguishes `pair_checks_passed` from overall `passed`, and render and
preview report pending cut fingerprints. Checked `save` is blocked while proofs
are pending. `save_preview` and checkpoints preserve an unverified policy marker;
restore conservatively marks source cuts pending again, never trusts saved proof
claims. A separate native audit can call `booleans.validate_cut_feature(obj)` and
then `cut_proofs.record_verified(obj)` after success. This is a script-level draft
workflow, not an asynchronous proof platform or a fit/strength certification.

Tagged boolean constraints are rechecked on subsequent parameter/placement edits
and document reopen; failed edits roll back. Export the final result only: selecting
it together with a construction ancestor is rejected. Disconnected fusion and
multi-solid output are deliberately outside this action's scope. No arbitrary
Python, selector expression or topology rewiring is accepted.

Run `cad/freecad/run.sh cad/freecad/test_booleans.py` for native operations,
no-effect/empty rejection, transactional dependent edits, selection and saved
recompute coverage. Evidence is written to `out/freecad-booleans/result.json`.

A cut may explicitly set `retain_tool_as_part:true` when its tool is an actual mating component. This preserves the native dependency while permitting that named component in output selection; a fresh overlap check rejects duplicated material. Construction bases and default tools remain excluded. The intent survives nested features and native reopen. Run `test_retained_mating_parts.py` for mating-seat, dependent edit, export and negative-control evidence.

## Native visual inspection

`render {objects,width?,height?,azimuth_degrees?,elevation_degrees?,deflection_mm?,max_triangles?,angular_deflection_radians?}`
returns a PNG image in `content_blocks` plus camera, part colors, visible-pixel
count and selected-geometry fingerprint metadata. It renders detached copies of
actual native B-reps using an orthographic per-pixel depth buffer; it does not
change source geometry or visibility, require a GUI, or imply a passing fit.

Defaults are 640 × 480 pixels, azimuth −52°, elevation 24°, 0.5 mm display
mesh deflection, 0.35 radians angular deflection and 50,000 triangles. Explicit
MeshPart angular subdivision prevents small curved supplier parts from silently
requesting enormous display meshes. Angular deflection accepts 0.05–1.0 radians
and changes only detached display triangulation. Width/height are integers from 64 to 1024;
azimuth is −360° to 360° and elevation −90° to 90°. Deflection is 0.05–10 mm,
with a relative floor of each object's diagonal/2000. Select 1–128 physical
objects; the hard limits are 20,000 source faces, 100,000 triangles and 80 million
projected triangle-bounding-box pixels. Exceeding a limit returns an explicit
error; select fewer objects, reduce image dimensions or coarsen deflection.
Object colors are stable across selection order. Positive elevation looks from
above the XY plane. The image is display triangulation, not exact geometry.

`cad/freecad/run.sh cad/freecad/test_render.py` verifies PNG dimensions/CRCs,
actual depth occlusion, draw-order independence, placed world bounds, unchanged
source B-reps, invalid inputs, budgets, geometry-fingerprint changes and angular
mesh control on an actual curved solid.

### Bounds of trimmed geometry

Use `native.shape_bounds(shape)` for reported dimensions and
`native.shape_bounds(shape, conservative=True)` for collision broad phases.
FreeCAD's ordinary `Shape.BoundBox` can retain the full supporting loft surface
for a thin `Part::Common` section. This both misreports rib thickness and causes
unnecessary exact pair intersections. The helper requests optimal bounds without
cached triangulation; conservative bounds include native shape tolerances.
`test_trimmed_bounds.py` checks the trimmed extent, independently verifies a
separated pair, and confirms a real overlapping pair is still detected.

For large native documents, use `inspect {summary:true,offset:0,limit:100}` to
page through lightweight names, dependencies and editable-parameter names. This
mode explicitly does not verify geometry. Use `inspect {objects:[names]}` for
current geometry on the particular parts or construction features you need.
Unqualified `inspect {}` retains the full-document behavior for compatibility.

### Linked physical mirrors

`mirror: {name, source, normal: [0,1,0], origin: [0,0,0]}` creates a
native `Part::Mirroring` across the specified plane. The source remains a physical
instance: both source and reflected output may be selected for checks and exports.
Shared dimensions and subsequent source edits propagate through the native link,
including after checkpoint reopening. Zero/nonfinite normals, unknown sources,
duplicate names and nonphysical/non-solid sources are rejected. New mirrors are
transactional and persist their validation marker. Mirroring does not establish
fit, mechanical retention, or handed hardware suitability; check the resulting
assembly explicitly. Native regression: `run.sh test_mirrored_parameters.py`.

### Compact assembly-check responses

`check` accepts `summary:true` for large assemblies. The kernel still checks every
selected pair; the response retains every clash and every explicitly required
contact, together with complete counts and connectivity components. Its
`pair_details_scope` and `omitted_pair_count` identify the response reduction.
Omitted pair details are not untested pairs. `contact_pair_count` counts all
non-penetrating pairs within the requested contact tolerance, including contacts
not explicitly required. Default `summary:false` preserves the full response.
The cached and saved `geometry-check.json` evidence always retains every pair;
actual geometry changes invalidate that cache as before. This reduces model
context size, not CAD computation or the scope of verification.

### Repeated geometry validity checks

Native inspection and typed Boolean validation cache kernel validity by SHA-256
of the complete cleaned BREP serialization, rather than by object name, shape
identity, prior check metadata or action count. The process-local LRU contains
at most 256 digest/boolean entries and no retained shapes. Any changed serialized
geometry is revalidated; live feature state, parameter constraints, topology
selection and assembly checks remain separate. This does not bypass FreeCAD's
internal Boolean-operation validation or prove manufacturing suitability.
Regression: `run.sh test_validity_cache.py`.

### STEP geometry equivalence

AP242 reimport validation now always matches each selected solid one-to-one using
empty Boolean differences in both directions. Equal total volume and bounds do
not suffice: a moved bore can satisfy both while changing the part. Native tight
bounds prune candidate matches; each accepted match still needs both differences.
The validation sidecar records the STEP SHA-256 and match evidence. A previous
sidecar is removed before exporting, so a failed overwrite cannot retain a stale
success report. The wrong-hole regression preserves volume and bounds deliberately
and must still fail. Earlier exports retain their historical validation scope;
this does not retroactively upgrade their evidence.

`make_part` kind `spline_loft` requires `rings`, `split_index`, and
`sampled_tolerance_mm`, with optional `ruled` (strict boolean, default `true`). Each ring is an ordered planar XYZ boundary (an optional
repeated closing point is accepted). `split_index` identifies the shared junction
between two interpolating B-spline branches; each branch needs at least four
points. Limits: 2–16 sections, 8–256 unique points per section, 2048 total supplied
points, coordinates within ±1e6 mm and sampled tolerance in (0,1] mm. Corresponding
sections must have equal source point counts. Result names must leave room for
`_Section15` within the native 96-character name limit.

Native sketches contain two blocked curves. `set_profile` preserves this type,
source point count, branch split and existing section placement expressions.
Section properties retain local source coordinates and fit evidence; the loft
records its representation. Both native geometry and provenance survive reopen.
Every source point must lie within 1e-6 mm of its curve, and 201 native curve samples
per branch must remain within the requested distance of the source polyline.
This sampled maximum is **not** a certified Hausdorff bound or exact equivalence
to the polygonal profile. `test_spline_profiles.py` exercises the actual typed
creation, editing, rollback, span binding, export and reopen paths on S3024 data.

`ruled: false` requests native smooth loft interpolation between sections; `true`
preserves straight ruled spans. Strings, numbers and null are rejected. The mode
and section provenance survive profile edits and native save/reopen. Smooth
interpolation changes geometry between sections: the section fit tolerance is
not a 3D wall-thickness or interference guarantee. Recheck those after changing
the mode. `test_spline_modes.py` verifies the actual typed worker paths, including
a three-section S3024 loft with six ruled faces versus four smooth faces, strict
boolean rejection, edits, dependency recomputation, and saved/reopened provenance.
