# CAD takeover progress — 2026-10-24

The requested goal remains open: exceed the reference VTOL's CAD complexity,
model Pat's Agents in Space project, evaluate against open rocket designs, and
support a college-level rocket. No complete-design superiority or flight claim
has been established.

## Backend

The handoff in `docs/TODO.md` selects FreeCAD. Pat explicitly authorized installation
below the earlier 4 GB free-space floor. The official FreeCAD 1.1.4 arm64 DMG was
downloaded and its SHA-256 verified:
`071343b4abb70492b75c973f41eaf1d2528f9b9c7ea018d22a4f46ae14d27ac0`.
Installed at `/Users/patliu/Applications/FreeCAD.app`; native FreeCAD 1.1.4 /
Open CASCADE 7.8.1 runtime verification passed. The installation volume was
detached and our downloaded DMG removed; approximately 6 GB remained free.

`cad/freecad/run.sh` runs the bundled Python with FreeCAD's libraries. The native
JSON-RPC worker supports persistent documents, typed boxes/cylinders/tubes/seated
mounts, edits with transaction rollback, inspection, FCStd save and checked STEP
export. It is now exposed through Wright agent tools and save artifact events; direct desktop parameter editing remains incomplete. Four native
geometry/rollback tests and a ten-request persistent-worker integration test pass.
`out/freecad-native/` holds results and saved editable artifacts. The independent
smoke test in `out/freecad-smoke/` also imported all 50 aircraft solids successfully.
A valid STEP import does not fix the assembly's remaining clashes.

The existing build123d/Open CASCADE environment is usable at:
`/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/venv312/bin/python`.
It remains a temporary fallback and independent geometry oracle.

FreeCAD integration should investigate the existing
[FreeCAD Rocket Workbench](https://github.com/davesrocketshop/Rocket), which the
[OpenRocket project](https://github.com/openrocket/openrocket) lists as its CAD
integration. Its `importORK.open(path)` imports into a FreeCAD document and
recomputes. The workbench is installed in FreeCAD's user Mod/Rocket directory at upstream
commit `20f74c12ad0c4b89cfe3a264bea476ff83443af8`. A small recorded patch guards
headless selection/view/appearance access; it does not change geometry or format
version checks. `out/freecad-openrocket/` records the unmodified baseline and
`out/freecad-openrocket-headless/` records the patched importer with patch hash.
All three full import probes still fail: the simple example reaches ten shaped
objects then hits missing material `type`; dual deployment hits an unsupported
nose-cone coupler configuration; the two-stage example is version 1.11, beyond
the importer's supported 1.10. Upstream silently catches parse errors, so the
probe explicitly captures diagnostics and fails partial documents.

## Implemented and tested

- Strict parameter types, finite values, integral counts and safe part IDs.
- Empty/duplicate assemblies, invalid placements and nonexistent contacts fail.
- Contact declarations no longer suppress solid overlap. Invalid shapes, failed
  intersection/distance operations, and nonfinite evidence fail closed.
- JSON-RPC errors are classified; malformed requests do not kill the worker.
- `python -m wright_cad.build PLAN.json -o OUT` builds a complete assembly in one
  kernel process and caches parts by source, parameters and kernel versions.
- Tests verify STEP re-import, GLB output, contact/overlap/separation and cache
  invalidation after one part changes.
- Added tabbed sheet fins, polygonal B-rep lofts/shells and tubes between endpoints.
- `cad_geometry` aircraft example exports the existing domain geometry; the
  `wright_cad.aircraft` adapter converts it without recomputing layout. Prop
  sweep envelopes are explicitly listed separately from solid parts.

Run tests with `PYTHONPATH=cad <cad-python> -m unittest discover -s cad/tests -v`.
All 15 CAD tests pass. In addition to prior geometry and protocol coverage, they
check cylindrical seats and redundant loft section removal with kink preservation.
The aircraft exporter ran using the existing compiled Rust library artifacts;
this does not claim a fresh whole-workspace Cargo validation.

## Full aircraft audit and corrections

`out/cad-audit/` holds the converted source aircraft: 50 physical solids and five
separately identified prop sweep envelopes. All 1,225 solid pairs were considered.
The initial result had **44 clashes and two disconnected contacts**.

`evals/cad/aircraft_mounts.py` adds explicit cylindrical seats at ten boom/mount
interfaces. `out/cad-seated-mounts/` holds the resulting native B-reps, STEP, GLB,
plan, report and preview: **34 clashes and two disconnected contacts** remain.
No contact exemption was used to hide penetration. Seats still need fasteners,
closure and load verification. The 0.105 mm spar/joiner gaps remain unresolved.

Intermediate loft sections are removed only where corresponding vertices are
linearly redundant. A wing dropped from 386 to 66 faces without changing volume.
The full seated build took about 18.6 seconds versus 81.5 seconds initially; this
combines the geometry changes and optimization, not an isolated timing benchmark.

The current wing geometry is a solid outer envelope, not a manufactured skin.
Its reported mass and generic component materials are not a trustworthy aircraft
mass estimate. Full airframe CAD therefore still FAILS completeness and fit.

## Agents in Space requirements

Pat identified Functor Systems as the source. Public repository evidence identifies
Palloon Systems (`youwen/hack-2026`) with a Jetson Orin Nano Super 8 GB node and a
replacement Raspberry Pi 5 4 GB video node with a Logitech C270. Pinned source
links and unknown hardware are in `cad/projects/agents_in_space/requirements.json`.
Allspark and handoff submodules returned 404 without sign-in; the public parent,
lumberloon and video-pi documentation were readable.

The official Pi 5 drawing and visually checked mounting pattern are retained in
`cad/projects/agents_in_space/references/`. Its dimensions are approximate and not
production data; missing component/cooler/cable envelopes and physical fit remain
explicit requirements. Battery, radio and Insta360 model information is pending.

`cad/freecad/pi_mount_fixture.py` builds a preliminary native Pi mounting fixture:
one perforated tray and four tubular standoffs. `out/palloon-pi-fixture/` contains
FCStd, STEP, five STL files, analytic-volume and STEP-roundtrip checks, and ten
pairwise contact/interference checks (all pass). The tray and standoff dimensions
are explicit provisional design choices. Board components, cooler, cables,
fasteners, suspension and flight loads are not yet validated. This is not the
complete balloon payload.

## Wing structure fixture

`evals/cad/wing_fixture.py` now builds a seven-part, 600 mm span fixture: five
SD7062 ribs and two spars. It passes clash/contact checks across all 21 pairs
(including bounding-box broad-phase exclusions). The assembly, named part tree,
STEP, GLB and rib STL are under `out/cad-wing-fixture/`; `preview.png` is rendered
from the actual B-reps. This is a rib/spar fixture, not a completed wing.

The source SD7062 coordinates have crossing trailing-edge segments. The fixture
explicitly merges its nominal trailing-edge endpoints, moving each 0.0021 mm,
and records that normalization in `plan.json`. The part builder refuses the
original crossing profile. Its regression test covers this defect and bore
breakout. The original aerodynamic table is unchanged.

## Independent geometry benchmark

`evals/cad/openrocket_parts.py` checks dimensions and analytic volumes against
pinned source files from OpenRocket, and then reimports the emitted STEP and
checks its volume again. Source URL, commit and SHA-256 are retained, together
with OpenRocket's license. It reports every unsupported component, and a missing
component prevents full coverage from passing.

Current result: 46 component definitions pass; 30 unsupported; no numerical
failures. Full benchmark FAILS (expected until coverage is complete). Counts
are definitions, not fin/ring pattern instance counts. The benchmark does not
check assembly placement, flight performance, stability or fabrication readiness.

## Reference interpretation

The [VTOL reference](https://www.reddit.com/r/diydrones/comments/1l89dcr/i_made_a_3d_printed_vtol_that_can_fly_130_miles/)
is 1200 mm span, 770 mm long and 2.55 kg according to its author. In comments the
author says the full-range flight was not completed because of a crash and
battery damage. The 130-mile figure must not be used as verified flight evidence.
The README now distinguishes the range claim from a completed flight.

## Remaining work

1. Resolve remaining aircraft clashes with manufactured shells, ribs and actual
   joints; preserve honest scope and mass accounting.
2. Extend the verified FreeCAD bridge to domain geometry and assembly/joints;
   resolve importer compatibility without silently dropping unsupported features.
3. Build native editable feature trees, joints and domain part libraries; integrate
   CAD actions and the desktop viewer. The persistent native worker is now connected to lead/specialist agent tools and the desktop viewer.
4. Complete source component coverage and assembly placement checks; add
   adversarial mutations and manufacturing feature checks.
5. Complete sourced payload hardware geometry and build/test the actual assembly.
6. Establish college-level airframe/avionics/recovery CAD requirements and evaluate
   them honestly. The existing propulsion/flight eval is not a CAD completeness test.

## Preview verification

The previous matplotlib 3D painter ordering could show triangles through other
parts. `evals/cad/raster.py` now uses a per-pixel depth buffer for orthographic
previews, with an order-independence regression test. Geometry validation still
comes from native B-rep operations, not images.

## Agent/native integration (latest work)

- Added `crates/wright-cad`: persistent subprocess client with 60-second request
  bounds; transport failure kills the process and cannot silently reset a design.
- Lead and specialist handlers share one native document through `cad_action`.
  Part creation, parameter edits, placement, solid/contact checks and export are
  available; each aerospace domain has a CAD specialist role.
- Save emits FCStd, STEP and GLB artifact events. The native exporter is explicitly
  tessellated and checked for omitted parts (its default headless output was empty).
- GLB carries explicit metres/Y-up metadata; the desktop loader converts that
  metadata to its millimetres/Z-up frame without guessing from model extent.
- Rust transport tests pass for collisions despite declared contact, separation,
  rejected placement, persistent PID and worker timeout. A real-FreeCAD scripted
  agent integration test passed shared lead/specialist document edits, collision
  feedback and save. The wider agent suite passed: 63 tests and two intentionally ignored live-network tests. All seven scene tests now pass, including actual placed FreeCAD GLB import at the original dimensions. The three Rust transport tests pass, including native checkpoint resume.
- Domain design/BOM synchronization, rich native domain geometry, full aircraft
  fit, complete payload enclosure, and rocket import coverage remain incomplete.

Native exchange now explicitly requests AP242 through `Part.setStaticValue`
before creating the exporter; the preference-only attempt remained AP214 and
was rejected. STEP reimport verifies schema, volume, solid count and world-space
bounds. The headless GLB exporter also lost meshing on translated shapes; temporary
display copies bake the placement before tessellation, leaving the source feature
tree intact. A translated 25 x 10 x 2 mm part was exported and independently read
by the desktop at [3,-4,5] to [28,6,7] mm. Tube bore height is a native expression
linked to outer height, so changing length preserves the through bore.

## Native aircraft profiles and hollow wing work

The existing seated-mount aircraft now rebuilds as native FreeCAD features:
50 physical solids, 120 document objects and 36 fully constrained sketch profiles.
All 50 outputs match the previous CAD volume, bounding box and centroid within
the migration tolerances. AP242 STEP and GLB export checks pass. The migration
retains all 34 clashes and two disconnected contacts; this is backend equivalence,
not aircraft completion. Evidence: `out/freecad-aircraft/result.json`.

Typed native operations now include constrained section lofts, hollow lofts,
inward-offset extruded skins and ribs linked to a skin's cavity profile. Profile
edits are transactional and checkpointed through the agent session. Five native
profile tests pass: analytic loft geometry, hollow loft rollback, oblique sections,
real skin cavity/linked length, and rib outline updates after wall-thickness edits.
Evidence: `out/freecad-profiles/profile-tests.json`.

A native thickness experiment reported a valid shape while returning the original
filled wing. It was rejected. The production skin feature instead uses a native
2D inward offset, linked extrusions and a boolean cavity, with explicit material
removal checks. Shape validity alone is not evidence of a hollow structure.

The native hollow-wing study now has nine physical solids: a 0.45 mm skin,
five cavity-linked 2 mm ribs at explicitly provisional stations, and the three
source spar/joiner parts. All 36 part pairs have zero penetration. Fifteen required
rib interfaces contact correctly; the source joiner/main-spar radial clearance
remains 0.105 mm, so the fit report correctly fails its 0.1 mm contact threshold.
Skin material volume is 114,657.225 mm³ versus the 2,333,380.049 mm³ filled envelope.
This is geometry only: no mass claim or structural approval follows. Root joints,
printing/segmentation, controls, tip closures and load checks are still missing.
Native FCStd, checked AP242 STEP, GLB and evidence are in `out/freecad-wing/`.

## Full aircraft structural iteration and connectivity

The hollow skin/rib features are now integrated into both aircraft wings.
`out/freecad-aircraft-structural/` contains native FCStd, checked AP242 STEP and
GLB for 60 physical solids. The other 48 parts retain their original volume and
bounds. Across all 1,770 pairs, the original 34 clashes become 28; original direct
contact requirements are retained and expose 12 missing contacts. Several skin
contacts now need explicit attachment paths through ribs; servo mounts and root
interfaces remain unfinished. Two new root ribs intersect the pod, so their
layout needs integration rather than treating the isolated wing as finished.

A connectivity audit over those saved native measurements finds 23 geometric
contact groups (`connectivity.json`). It counts only nonpenetrating contacts
within tolerance. These groups are not proof of load-bearing connections.
The native `check` action now reports groups and accepts `require_connected:true`
to reject separated assemblies without relying solely on declared pairs.
Seven connectivity/placement/save cases pass, including rejection and rollback of a
helper move that invalidates its dependent cut. Five profile tests pass again.

All four Rust CAD integration tests pass in an isolated narrow Cargo build:
worker errors/interference/connectivity, timeout handling, checkpoint recovery,
and constrained profile edits recovered in a later session. The ordinary worker
limit stays 60 seconds; full assembly checks and export validation allow 300
seconds (the 60-part build/check/export took about 115 seconds in total).

Native saves now persist physical-part visibility and hide construction helpers without requiring the GUI. Reopening both wing and structural-aircraft documents verifies the intended visible parts and unchanged volumes.

## Native OpenRocket import compatibility and adversarial checks

Two pinned examples now complete native import: the simple model rocket and
Dual parachute deployment. The compatibility patch separates `docprefs` from
component material parsing and allows the source design's tube coupler under a
nose cone. The original source files and format-version guard remain unchanged.
`cad/freecad/rocket-compat.patch` records the changes separately from GUI guards.

The evaluator now inventories every source component definition and records
created, ignored, failed and unvisited entries. The simple example creates nine
entries (including its stage) and ignores three; the dual-deployment example
creates sixteen (including its stage) and ignores three. Parachute, shock-cord
and mass-component omissions remain explicit. The high-power example still stops
at unsupported format 1.11: all 49 entries remain unvisited, with no numerical
checks claimed. None passes full CAD coverage.

Fifteen independent checks pass across the two supported imports: selected tube
material volumes, fin instance counts, and bottom-referenced axial placements.
A +1 mm inner-tube length mutation is detected by the volume check in each model.
Results and paired negative-control evidence are in `out/freecad-openrocket-verified/`
and `out/freecad-openrocket-negative-control/`; run
`python3 evals/cad/verify_openrocket_reports.py` to verify their relationship.
Material properties, remaining placement cases, joints and fabrication are unverified.

Partial supported-component exports now pass AP242 STEP and GLB validation:
10 solids for the simple example and 17 for dual deployment. Workbench Group
links originally caused recursive duplicate export; temporary plain feature
copies now preserve the explicit selection while leaving native trees editable.
The nose cones also exposed noninvariant OCC volume integration after STEP
surface reparameterization. If volume integrals disagree, the exporter requires
one-to-one solid matching with identical bounds and empty boolean differences
in both directions, instead of relaxing volume tolerance. Each STEP writes a
`.validation.json` sidecar recording the verification method and volume values.
The exchange regression verifies grouped-child selection and rejects deliberate
internal material removal despite unchanged bounds. Mesh volume probes further
support equivalent nose surfaces; native volume values are not validated mass data.

## Documented 1.11 CAD subset and rail-button patterns

The pinned OpenRocket format notes (`evals/cad/references/fileformat.txt`, source
and hash in `format-audit.json`) were reviewed before adding explicit 1.11 rigid
CAD support. Its new example tags concern deployment/separation metadata; these
are not treated as imported flight functionality. Version dispatch still rejects
1.12, 2.0 and malformed versions. This supersedes the earlier version-gate blocker.

All three reference examples now complete native import. Supported geometry:
simple model 10 solids, dual deployment 19 solids, high-power two-stage 50 solids.
Source coverage remains incomplete: respectively three, two and three recovery/
mass definitions are omitted. Stage definitions are recorded separately from
physical geometry; supported solid counts include fin and rail-button patterns.

The added rail-button importer uses the source spool dimensions, base-center
reference and linear spacing. It does not invent a fastener bore; nonzero source
screw-head height is rejected as unsupported. Six focused cases verify the native
pattern and invalid/unsupported inputs. The source-based checks also verify
rail-button volume, count, pitch, radial placement against its parent surface,
and top/middle axial references. The source reference implementation is pinned:
https://raw.githubusercontent.com/openrocket/openrocket/591c5e5f7b7377e48f2bd5e501cef25cdfbd61d8/core/src/main/java/info/openrocket/core/rocketcomponent/RailButton.java

The canonical native benchmark now passes 70 selected numerical checks, with
five detected negative controls: three +1 mm tube-length changes and two +1 mm
rail-spacing changes. Each baseline and negative run fingerprints the evaluator,
exchange layer and workbench sources. All three supported subsets export checked
AP242 STEP and GLB. Full coverage and the broader goal remain false.

All three local workbench patches reproduce the installed source bytes from the
pinned upstream revision (seven changed/added files), verified by
`evals/cad/verify_rocket_patches.py`. Reopening the saved high-power native document
preserves component volumes; its cutaway preview is generated from those saved
native features, with omitted recovery components and incomplete design scope
labelled explicitly.

## Agent access to existing live native models

The `cad_action` schema and Rust session now expose `open_document` for an empty
session. Native checkpoints are written under the run; source FCStd bytes remain
unchanged. Inspection lists supported editable parameters and assembly containers.
Python-backed features must have live recompute handlers, so missing workbenches
cannot be silently accepted as frozen editable geometry.

Supported Rocket body tubes, inner tubes and couplers expose Length, Thickness
and signed AxialOffset (with scaling/filled-component restrictions). Direct
property assignment was found to resize a solid without updating its assembly
position. The API now invokes native workbench setters. Tests verify the bottom
reference after length changes, movement after axial-offset edits, and an engine
block's automatic diameter following its parent tube's wall thickness. Invalid
wall edits roll back; direct arbitrary Placement edits on Rocket components are
rejected in favor of assembly-controlled parameters.

Native output selection distinguishes physical child membership from boolean
construction. It permits a body tube and its physical internal tube together,
but rejects aggregate Rocket/Stage containers as duplicate physical outputs.
All five Rust transport/session integration tests pass, including native import,
edit, export and checkpoint resume. The existing four primitive, five profile,
and seven connectivity/save cases also pass. These are tool integration checks;
they do not change the incomplete aircraft/payload/rocket completeness verdicts.

## Native inspector metadata and check freshness

Native GLB nodes now use unique FreeCAD object IDs, preserve human labels in
extras, and expose supported parameter values with exact object/property
bindings. Boolean outputs include parameters from geometric construction
sources; expression-driven properties are excluded. No material or mass values
are invented. Existing desktop parameter inspection can consume these fields.

Worker checks cache evidence against selected names and SHA-256 of cleaned
B-reps (excluding display triangulation). Export embeds check summaries and
per-part collision flags only when that fingerprint still matches. Direct
FreeCAD shape/placement edits and changes to the output selection invalidate the
result. Export also writes geometry-check.json. The desktop overlay distinguishes
current selected-geometry PASS/FAIL from checks that are not current; its scope
is not manufacturing or flight approval.

The native metadata regression verifies duplicate human labels, parameters,
expression filtering, collision flags, direct-edit invalidation, selection scope,
and saved evidence. Export regression, four primitive tests, connectivity cases,
and all five Rust native session tests pass. Desktop scene verification is pending: the first full desktop build failed with
missing compiled eframe/wright_agents crates; a retry is queued behind other
workspace builds. The actual-exporter test now covers parameter metadata and
edit-induced invalidation, but is not yet reported as passing.


## Parallel internal-CAD and training foundations

Three user-requested low-effort Astra workers delivered a sourced AI CAD research
report (`docs/cad-ai-research.md`), a native two-deck instrument fixture, and a
synthetic native edit corpus/evaluator. The fixture was independently rerun:
28 physical parts, 77 native objects, 378 pair checks, 36 required contacts,
zero clashes/unmet contacts and one contact group. Four explicit clearances,
parameter propagation, native reopen and collision/separation negative controls
pass. `out/freecad-internal-assembly/` contains FCStd, AP242 STEP, GLB, evidence
and a cutaway. This is provisional geometry, with threads/retention, lid fastening,
real component/cable geometry, tolerance and load/thermal checks still absent.

`evals/cad/training/` generates 40/53/58-part tray fixtures with a four-bolt
training family and six-bolt test family. The verified run is
`out/cad-training-verified/report.json`: 4,146 edited-state checks, native reopen
checks and all nine injected faults pass their intended assertions; five manifest
integrity tests pass. One reference training trajectory is exported, excluding
validation/test traces. These are deterministic reference executions, not model
accuracy. No weights were trained. Existing aircraft defects and missing payload
hardware remain open. The next evaluation must measure independently proposed
agent edits, then accumulate reviewed native success/failure/repair traces.

User authorized Google Docs and local Notes discovery for payload hardware.
Searches found the Google Doc “Videos of weather balloon” linking launch media,
but no architecture/BOM in the two connected Youwen accounts. Local Notes title
and hardware-term searches have not identified the architecture note. A note-title
and account clarification is pending; do not infer missing hardware dimensions.

## Shared dimensions through the agent API

The native worker, Rust session and agent action schema now expose
`make_parameters` and `bind_parameter`. These create bounded shared length inputs
and linear native expression bindings to supported dimensions/placement axes.
Inputs appear in inspection and downstream GLB parameter metadata; driven target
dimensions cannot be overwritten with set_parameter. Bounds and failed geometry
recomputes roll back atomically. Parameter tables are excluded from physical output
selection. This is not a native joint/DOF solver or arbitrary expression API.

The two-deck fixture now exposes LowerClearance and DeckSpacing as typed shared
controls. A controlled worker-only edit increases LowerClearance from 12 to 18 mm:
all 378 pairs and required contacts pass, the dependent stack moves 6 mm, housing
geometry stays unchanged, inter-deck clearance remains 10 mm and lid clearance
becomes 9 mm. Source FCStd bytes are preserved. Evidence and complete action trace:
`out/freecad-internal-layout-edit/`. Native layout/metadata/protocol checks and all
six Rust native session tests pass, including shared-dimension checkpoint resume.

An independent low-effort Astra attempt completed a new six-bolt stack edit,
without reference actions or scoring source. Three tray thicknesses changed from
2 to 4.25 mm through worker calls; all 18 bolt dependencies retained 1 mm
protrusion. The separate saved-output oracle passed 1,783 checks, target material
volume, unchanged shell/heads/washers and source preservation. Trace and score:
`out/cad-agent-trial/`. Four original disconnected groups remain. Two oversized
terminal-input retries were recorded, neither reaching the worker. This is one
synthetic task, not a broad model success rate or fine-tuned weights.


## User hardware diagram and current BOM received

The user supplied an architecture screenshot and a 12-line, 17-unit BOM. Recorded
verbatim model/quantity/dimension evidence in
`cad/projects/agents_in_space/hardware-bom.json` and routing/ambiguities in
`architecture.json`; screenshot retained in references. The latest table specifies
4S 6000 mAh, while the drawing says 5S2P; the table governs the current configuration.
The current camera is BRIO 4K, radio CR8 915 MHz ELRS, VTX GEPRC MATEN 1.2/1.3 GHz
2 W, GPS Matek M10, and there are two ESP32s. Earlier Pi/C270 and unknown Insta360
remain historical/optional rather than silently included.

`palloon_envelopes.py` exports seven user-dimensioned native solid envelopes,
plus a Jetson footprint sketch with unknown assembled height, under
`out/palloon-hardware-envelopes/`. Each full envelope matches the supplied dimensions;
21 display-layout pair checks pass and AP242 reimport validates. These are a
component catalogue, not a packaging solution or detailed board models. Ten units
lack complete 3D geometry (Jetson included). BOM per-unit mass ranges total
823.5–1014.5 g before housing/cables/antennas/retention/suspension.

Direct native inspection corrected an earlier inference about the aircraft pod:
it already has outer/inner lofts and a cavity. Current work targets its shell and
mount interfaces, not removal of a nonexistent solid interior.

## Conservative service-path screening

The worker/agent/Rust action schemas now expose `check_translation`. Straight
rigid translations use swept bounding boxes as conservative envelopes, intersected
against actual obstacle solids. Results distinguish clear, actual sampled collision,
and inconclusive; an obstacle between only two endpoint samples cannot yield a
false clear result. Geometry stays unchanged. Tests cover all three statuses and
invalid/self-overlapping selections. Numerical overlap tolerance is explicitly
reported as 1e-6 mm³; no cable, fastener removal, flex or general path-planning
claim is made.

The full desktop verification remains unverified: its second shared-target build
also encountered a missing dependency artifact (`once_cell.rmeta`) during compilation.
Do not claim the new overlay has passed desktop compilation. Native CAD execution
and the separate small Rust CAD target remain independently testable.

Saved-fixture service checks pass: upper component translation is blocked by the
closed lid, conservatively clear with the lid excluded, and lower component
translation is blocked by the upper deck even with the lid absent. Evidence:
`out/freecad-internal-service-access/result.json`. All six Rust native session
regressions pass after adding the motion action. The pod interface iteration is
still running in a confirmed live FreeCAD process; no completed result is claimed.


## Manufacturer mechanical references

NVIDIA SP-11324-001 v1.3, figures 4-1/4-2 (PDF pages 33/34), was downloaded
from its official endpoint, hashed, text-extracted and visually reviewed. Carrier
100×79 mm differs from assembled developer-kit 103×90.5×34.77 mm. The catalogue
now uses upper tolerance bounds 103.2×90.7×35.86 mm for the assembled envelope,
retaining a separate carrier outline. Manufacturer scope does not establish actual
user configuration, mounting-hole coordinates or plugged-cable/airflow keepouts.
Eight of seventeen units now have full envelope geometry; nine remain incomplete.
All eight catalogue dimensions and 28 pair checks pass.

GEPRC's official MATEN 1.2G 2W manual was also downloaded/hashed and visually
reviewed. It establishes a 30.5×30.5 mm M3 mounting pattern and MMCX antenna
connector, but does not resolve hole tolerances or connector/thermal keepouts.
Detailed provenance is in references/jetson-dimensions.json and
references/geprc-maten-dimensions.json. The user confirmed a custom battery and
asked Wright to infer a provisional pack. Capacity alone does not establish its
finished dimensions; the provisional envelope remains an assumption.

## Off-the-shelf CAD integration (24 October, afternoon)

The user clarified that Wright should prefer existing commercial components and
import their actual CAD, designing a custom PCB only if needed. No new VTOL PCB
is currently requested. `cad/components/catalog.json` records exact supplier
models, source hashes and verified mappings to aircraft BOM catalog IDs.
`component_catalog` and `import_component` are exposed through the native worker
and Rust CAD session. A registered model does not replace an unrelated selected
part, establish a mounting datum, or prove electrical compatibility.

The supplier MN2806 EVO motor and P12x4 propeller have native geometry and previews
in `out/vendor-components`. They are separate library demonstrations, not the
reference aircraft's AOS2807/Gemfan7042 parts. A real six-solid KiCad board fixture
has passed Rust checkpoint/reopen and checked exchange tests; it is explicitly a
Python-baseline PCB fixture, not the VTOL flight controller. The combined vendor import regression
now passes: motor, propeller and six PCB solids retain source geometry through
native save/reopen, with unit normalization, invalid-input rejection and rollback
checks. Evidence is `out/native-step-import/result.json`. A subscription-backed
Astra component study remains in progress; a successful preview alone is not an
independent model evaluation.

Aircraft builds now emit `component-cad.json`, an exact BOM-ID registry lookup
report. It retains missing models and unsupported import statuses, supplies runtime
catalog lookups, and makes no import or placement claim. Both focused Rust tests pass, covering
the EVO/non-EVO distinction, separate propeller hubs and ambiguous mappings.

The downloaded Holybro Pixhawk 6X Pro STEP contains 18 solids and 13 open shells.
The default solid-only mode rejects it; the explicit mixed-geometry mode now
retains its shells as reference geometry with visible mass/collision coverage
limitations (see verification below). The official Raspberry Pi 5 STEP and MIT notice are retained under
`cad/components/vendor/raspberrypi`; its 77,627,469-byte source exceeds the current
50 MiB import limit and has not been imported.

## Wing and rotor revisions

The smooth wing's two aerodynamic faces are genuinely bicubic native B-spline
surfaces with C2 interior continuity; this is not only display smoothing.
`out/tsung-vtol-smooth-aircraft/surface-continuity.json` records that limited claim.
The first smooth cavity protruded beyond the outer surface; the next cavity was
contained but measured only 0.247 mm minimum wall. Neither is approved. A cavity
derived from sections of the actual smooth outer surface is being rebuilt and
must pass wall, servo-fit and complete-aircraft rotor-envelope checks.

The separate rear-rotor revision moves the two rear motors forward 23.82 mm to
X=561.18 mm, producing 5.003614 mm nominal fin clearance. Physical pair, rotor
envelope and tool-access checks pass at the tested parameter states; export and
reopen completion is governed by `out/tsung-vtol-rear-rotor-layout/result.json`.
The aircraft still has three disconnected physical groups. Smooth-wing integration
must rerun the clearance checks; this revision does not establish flight readiness.

## Reusable capability checks

The corrected smooth wing now passes its 120-pair/24-contact audits before and
after native reopen and a 610 mm span edit. Minimum wall is 1.001658 mm initially
and 1.006862 mm after the edit; the static servo boundary gap is 0.774936 mm and
0.787878 mm respectively. The exterior remains unchanged. Evidence is
`out/tsung-vtol-smooth-wing-actual-sections/result.json`.

The typed spline-loft API now accepts a strict boolean `ruled` option, defaulting
to the previous ruled behavior. Smooth creation, profile edits and native
save/reopen pass the targeted tests. Mirror-plane bindings now accept `Base.x/y/z`
only on `Part::Mirroring`; regression tests include the actual rear-motor assembly
and rejection on unrelated object types.

`containment.py` supplies a reusable read-only exact-solid containment/boundary
clearance check, including construction solids. It passes tests for outside parts,
crossing parts, internal holes, insufficient wall, invalid input, source geometry
preservation and reopen. The same helper passes the real smooth-wing wall and
servo clearance checks: `out/freecad-containment/wing-result.json`. The
`check_containment` action is now registered in the worker, Rust session and agent
prompt. Its Rust integration test passes a 1 mm clearance and rejects 1.1 mm for
the same fixture. The action does not mutate geometry or create a checkpoint.

The integrated smooth-wing/rear-rotor assembly is complete for this geometry
revision: `out/tsung-vtol-smooth-aircraft-final/result.json`. Its 85 physical parts
pass 3,570 pair checks with no overlaps or missed required contacts; all 80
rotor-to-changed-wing checks pass. Native reopen and GLB export pass. STEP was
not exported or checked for this integrated revision. A stale verification
assertion expecting eight cavity sections was corrected to the source's sixteen;
the failed assertion and reopen-only completion evidence remain saved. No geometry
was changed to make that assertion pass. Three disconnected assembly groups and
unverified structural sizing remain explicit limitations.

## Commercial board and live agent evidence

The Holybro Pixhawk 6X Pro import now preserves all 18 solids and 13 reference
surfaces. An independent oracle checks every source entity exactly once, including
full BREP topology/coefficients and placement. Native reopen, explicit reference
rendering and the Rust session integration pass. Unmarked shells are rejected;
reference surfaces are excluded from mass and collision checks, with partial
coverage visible. Checked STEP export remains rejected for this mixed source.
Evidence: `out/native-mixed-step-import/result.json` and `preservation.json`.

Actual low-effort Astra study `out/astra-cots-component-study/run-25644` completed
six turns through the subscription-backed harness. The independent native oracle
verifies exact motor/propeller identity, source geometry and placements with
46.790127 mm separation. The model reported failed motor/combined STEP equivalence
checks honestly; only the propeller-only STEP export passed. Native checkpoints
retain both parts. This is a component-import study, not an autonomous aircraft
design demonstration.

The new rocket assembly screen checks all 1,441 solid pairs across three exported
fixtures and detects 0, 4 and 23 material overlaps. The first two-stage coupler and
bulkhead overlap is independently implied by the original `.ork` dimensions:
1,300.373918 mm³. It must not be hidden by changing imported source dimensions.
Recovery omissions and missing connection checks remain explicit. Evidence is
under `out/openrocket-assembly-fit`; commands and scope are in `evals/cad/README.md`.

### STEP mixed-unit normalization (2026-10-24)

The importer now audits per-context length units through `cad/freecad/step_units.py`: SI metre/mm/cm and graph-defined inch/foot. Inch/foot labels alone are insufficient; conversion measures, referenced base units and length dimensional exponents must agree. No caller scale override is accepted. `out/step-unit-normalization/result.json` proves Open CASCADE reads a 1×2×3 inch box as 25.4×50.8×76.2 mm and a mixed-context two-box STEP as volumes 6 and 98322.384 mm³. Five malformed conversion controls reject. NVIDIA official Jetson STEP has 132 audited unit contexts (`out/vendor-component-inventory/jetson-unit-audit.json`); this unit audit alone does not establish production import, validity or fit of all its entities. Large-source budgets and full native import remain separate work.

Compatibility regression after unit-audit integration also passed: real T-Motor motor/propeller and six-solid PCB fixture preserve source geometry, SI metre conversion, editable placement, transactional rollback and native save/reopen (`out/native-step-import/result.json`, rerun log `out/native-step-import-rerun.log`). Foot and centimetre native fixtures additionally pass.
The compatibility run loaded its test script before the result-field rename: `unsupported_inches_rejected` in that saved result refers specifically to a dangling `#999` conversion reference. Valid inch support is proven separately by the normalization fixtures; the next regression run writes the clearer `unresolved_conversion_unit_rejected` field.

### Explicit derived rocket bulkhead seat (2026-10-24)

`evals/cad/rocket_stepped_bulkhead.py` preserves the pinned two-stage OpenRocket source and reproduces its 1300.3739 mm³ coupler/bulkhead interference as a failing control. A separate proposed stepped bulkhead keeps the source rim radius and total thickness, with a 3 mm deep pilot and provisional 0.15 mm radial clearance. Typed Wright actions build and bind bore radius/axial placement. Nominal, edited-bore/length, restored and reopened states pass zero-interference, seat-face contact and analytic volume checks (`out/rocket-stepped-bulkhead/result.json`). This is a derived proposal, not an importer correction or a completed rocket: positive axial retention, recovery attachment, tolerance validation and load sizing remain absent.

### Exact native bounds cache (2026-10-24)

A measured slow path in repeated native descriptions was trimmed optimal bounding boxes. `native.shape_bounds` now caches six immutable coordinates by SHA-256 of cleaned exact BREP plus conservative-mode flag, with a bounded4096-entry LRU and fresh mutable return values. Geometry/placement edits therefore cannot reuse the old key. Tests cover trimmed curved geometry, exact/conservative separation, returned-box mutation, edits, native reopen and eviction. On the actual smooth wing, first call0.3175s, repeated call0.00406s, zero difference from uncached native bounds (`out/freecad-bounds-cache/actual-wing.json`). This one-shape measurement is not an overall assembly speedup claim; already-running workers retain their earlier code.

### Jetson carrier hole geometry audit (2026-10-24)

Official STEP solid275 is a100×79×1.57mm carrier at[-4,-17,0] to[96,62,1.57]. Native cylinder/depth/point-in-solid probes identify eightØ2.75mm through-holes with surrounding material, including92×58mm and86×58mm rectangular patterns. Coordinates/provenance are in `cad/projects/agents_in_space/references/jetson-cad-hole-candidates.json`, full94-feature audit in `out/vendor-component-inventory/jetson-mount-probe.json`, preview in `jetson-hole-map.png`. These are geometric candidates: mounting purpose, fastener specification, interference with installed components, service access and actual user hardware remain unverified. No standoff placement is accepted from hole diameter alone. Reproduction script `evals/cad/jetson_mount_probe.py` additionally pins the source hash; the completed initial probe ran its same geometry logic before that checksum guard was added.

### Jetson provisional mounting access (2026-10-24)

`evals/cad/jetson_mount_access.py` tested24native gauges across1393source solids and527reference surfaces, with a pinned source hash unchanged. No kernel errors occurred. All eightØ2.5mm shaft gauges and eightOD5/ID2.75mm underside-support gauges collide with source parts; only theØ5mm top-tool gauges at[92,0] and[92,58] clear. At[92,0], source solid1267 alone obstructs shaft/support. Its identity is not established; this may be existing mounting hardware, so neither removing it nor adding duplicate standoffs is justified. These provisional gauge sizes do not select actual fasteners/tools. Full evidence: `out/vendor-component-inventory/jetson-mount-access.json`.

### Atomic parameter bindings (2026-10-24)

New typed `bind_parameters` action accepts1..128unique binding targets with the existing named source/target, scale and offset contract. It prevalidates inputs, applies expressions in one transaction, recomputes/validates once, and rolls back the entire batch on failure. This avoids invalid intermediate states and repeated whole-document validation in dense assemblies. Native test changes a tube from outer6/bore5 to outer4/bore3 atomically, verifies invalid final geometry restores both expressions and shape, checks six invalid input batches, and verifies reopen/edit. Rust action test confirms successful checkpointing and unchanged checkpoint bytes after failure. Results: `out/freecad-bind-many/result.json`; Rust `atomic_parameter_bindings_checkpoint_and_rollback` passed1.46s. Tool schema/prompt exposes the action; this is not evidence of a live model choosing it.

### Closed reference enclosure checks and parser cost (2026-10-24)

`cad/freecad/reference_clearance.py` distinguishes surface separation from enclosure occupancy. A gauge wholly inside a closed reference shell can have positive surface distance; a temporary valid solid now checks enclosure overlap without changing the source reference role, assigning mass or exporting it as physical hardware. Native tests cover enclosed, outside, crossing and open-face cases, invalid tolerances and unchanged source geometry (`out/freecad-reference-clearance/result.json`). The Jetson gauge audit is rerun with this check; its prior surface-only evidence is preserved as `jetson-mount-access-surface-only.json`.

Unit-record parsing now consumes unquoted text in chunks while retaining STEP quoted semicolon/escaped-apostrophe behavior. The actual74MB Jetson audit completed in6.29s with the entire132-context result identical to the prior audit; a diagnostic of the preceding parser recorded31.07s. These are separate runs, not an overall import speedup. Native inch/foot/cm/mixed-unit and malformed conversion tests pass after the change (`out/vendor-component-inventory/jetson-unit-parser-benchmark.json`). The complete production import is still unverified following its300s timeout.

Completed enclosure-aware Jetson rerun:24gauges,2clear,zero kernel errors, source unchanged. The classifications agree with the earlier audit; no wholly-enclosed new obstruction was found in this particular dataset. The native synthetic negative control proves that the added enclosure check detects that failure mode when present.

### Source-frame payload cradle proposal (2026-10-24)

The obstruction at Jetson carrier hole[92,0] is source solid1267: a103×90.5mm frame extending below the board (a geometric description, not verified supplier part identity). A scoped two-solid diagnostic extract is saved at `out/jetson-obstruction-study/scoped-source-extract.FCStd`; its source remains unchanged. `cad/freecad/palloon_jetson_cradle.py` constructs an open-centre parametric cradle around this actual frame with0.3mm side clearance,2mm walls and3mm ledge/floor/lip assumptions. Its selected three-part check reports0overlaps and0unmet declared contacts. Native proposal: `out/palloon-jetson-cradle/proposal.FCStd`. Provenance was added without changing any shape; the report correctly marks1918missing vendor entities (1391solids+527references). This is not a complete Jetson or payload fit check; retention, other components, cables, thermal adequacy and strength remain unresolved.

The VTOL execution brief now references `cad/projects/vtol-demonstrator/hardware-package-inventory.json`:31required hardware/interface roles, catalog-candidate IDs and three exact-CAD candidates with source hashes. The package remains explicitly unfrozen/unready; no compatible hardware selection or availability claim is made.

### User architecture clarification (2026-10-24)

FreeCAD remains the actual CAD application and parametric kernel. Wright is a thin Rust overlay: primarily generate/run FreeCAD code and CLI jobs, with computer use for inspection or GUI-only operations. Do not build a replacement CAD platform or require a new wrapper action for each modeling feature. Reuse native FreeCAD capabilities/workbenches, prioritize assembled geometry, and separate expensive engineering audits from ordinary model creation without misrepresenting verification status. The VTOL execution prompt now states this explicitly.
