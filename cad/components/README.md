# Exact manufacturer CAD components

`component_catalog {query}` discovers local manufacturer files by CAD catalog ID,
model/category or explicitly verified aircraft BOM catalog ID. `import_component`
accepts the returned exact CAD ID and preserves the manufacturer BREP solids at
an explicit rigid placement. It does not select a replacement BOM part.

Aircraft builds write `component-cad.json` with exact BOM-ID candidate matches,
recorded importer status and catalog lookup requests. This is availability
evidence, not an import, placement or compatibility result.

Current mappings are `tmotor_mn2806_evo_650kv` → `tmotor_mn2806_evo` and
`tmotor_p12x4_cf` → `tmotor_p12x4`. Registry evidence records the manufacturer
product pages. These models do not match the reference aircraft's AOS2807 or
Gemfan7042. Propeller handedness is not independently established by the mapping.
The Holybro Pixhawk 6X Pro source has 18 solids and 13 non-solid shells.
Default solid-only import rejects it as `unsupported_mixed_geometry`. The verified
explicit `preserve_reference_geometry:true` path retains reference surfaces
separately and never promotes them to physical solids.
Checked STEP export of a selection from that mixed import instance is rejected.

Source hashes, identity, declared STEP units, labels where recoverable, solid
indices and snapshot limitations persist in each native object's
`WrightStepImport` property and appear in scoped inspection. The native reader
normalizes file-declared SI mm/cm/m and audited inch/foot conversion graphs
to millimetres, including mixed per-representation contexts. Callers cannot
declare a scale. Unknown, missing, cyclic or inconsistent unit graphs reject.
The complete unit-context audit remains in native provenance.
Supplier coordinates are preserved: `mounting_frame_verified:false` means no
mounting datum or automatic snap/alignment has been established. Editable rigid
placement does not imply editable vendor feature history or associative updates
to the original manufacturer file.

The STEP importer defaults to valid solid-only geometry. Explicit reference
preservation additionally accepts valid shells/faces and classifies them as
reference-only. It creates one native object per retained entity, retains label
candidates without duplicating imported
aggregate containers, and validates the entire document transactionally.
Imports and source files are bounded; name collisions and invalid inputs roll
back. Manufacturer files retain their source metadata and attribution; no
redistribution license is inferred from public download availability.

Run `cad/freecad/run.sh cad/freecad/test_step_import.py` for actual motor,
propeller and explicit PCB-fixture geometry, unit conversion, placement and
FCStd reopen coverage. The PCB source/export hashes and model references are in
`out/step-import-fixture/pcb-source.json`; that test board is not the reference
VTOL's flight controller. `test_component_catalog.py` tests exact lookup and
unsupported-file rejection. `test_brep_compare.py` tests the independent full
serialized BREP comparison (strict topology/structure except the documented OCCT Checked cache flag, absolute 1e-10 coefficient
roundoff limit), including displacement and topology negative controls. A native
coincident Boolean self-difference failed on the supplier motor; its evidence is
retained in `out/native-step-import/boolean-comparison-failure.json`, rather than
using volume equality as a substitute for preservation checks.

The ignored harness test `live_astra_cots_component_study` is an explicit
subscription-backed, bounded model evaluation. It records requests, native tool
results and the report in `out/astra-cots-component-study/`. Execution alone is
not a pass: a fresh native oracle must verify saved geometry, separation,
placement and provenance. A failed checked STEP export must remain distinct
from a valid native checkpoint. No compatibility, structural or flight approval
is implied by an import study.

Actual subscription evaluation `out/astra-cots-component-study/run-25644` used
six Astra turns. Independent native BREP/provenance verification passed for
both exact parts, with 46.790127 mm separation and zero overlap. The model
reported combined and motor-only STEP equivalence failures honestly; only its
propeller-only checked export passed. This demonstrates native import and
placement with an unresolved motor exchange limitation, not a complete combined
export or hardware compatibility. Usage was 124830 input tokens including
24832 cached, and 1632 output tokens; one task-specific run, not an efficiency
benchmark.

The actual Holybro mixed import passed `test_mixed_step_import.py` and a fresh
`check_import_preservation.py` replay: all 31 entities exactly once, full BREP
coefficient/topology comparison, original placements, source hashes and native
reopen. Evidence: `out/native-mixed-step-import/{result,preservation}.json`.
Summary/scoped inspection expose reference roles; checks report excluded
reference surfaces and unselected solids from the same source instance.
`check_containment` is a typed read-only operation with quantitative outside
volume, all-boundary distance, clearance and geometry coverage; a distance alone
is not clearance when containment fails.

`save_preview {objects, reference_objects}` writes `preview.FCStd` and
`preview.glb` through Wright's managed CAD artifact pipeline. The controller's
verified example is `out/native-mixed-step-import/preview/`: 18 physical and
13 reference mesh nodes, native reopen and source preservation verified.
Reference-role strings and exclusion flags are retained in GLB `params`, which
Wright's existing part inspector displays read-only, and in node/asset extras.
`preview_coverage` names omitted source geometry; missing source entities do not
restore a completeness claim. `step_export.status` explicitly states that STEP
is unavailable in this preview contract, and no `roundtrip_valid` field is
returned. The strict `save` operation is unchanged.

Larger imports require explicit integer budgets: `max_source_mib` defaults to
50 (hard maximum 200), and `max_entities` defaults to 512 (hard maximum 4096).
The 100000-face cap remains. `summary:true` preserves full validation, source
counts/physical and reference names/provenance, but omits detailed object
records; the response is bounded to 512 KiB and scoped inspection remains
available. Requested budgets are recorded. Preview `max_objects` defaults to
512 and can explicitly reach 4096. Native rendering defaults to 128 objects and
20000 source faces; explicit `max_objects`/`max_source_faces` allow up to
4096/100000 while triangle/pixel/work caps remain unchanged. None of these
options promises that a complete large model can render within those caps.

The explicit budgets pass a 513-solid native import/preview regression
(`out/native-component-budgets/result.json`). They do not yet make the full
NVIDIA P3766/P3768/P3767 supplier assembly usable within the 300-second typed
operation deadline. The actual 74 MB, 1920-entity attempt timed out without a
checkpoint (`out/native-jetson-import/run-41698/attempt.json`). A separate,
read-only diagnostic reached 1613 leaf validity checks before its own deadline:
STEP reading took 148.13 seconds, aggregate validity 75.48 seconds, and completed
leaf validity and measures another 73.13 seconds. Native stack samples and
phase logs are in `out/native-jetson-import/diagnostic-2/`; these timings are
host-load dependent. No source geometry was dropped or validation bypassed to
claim a successful import. `profile_step_validation.py SOURCE OUTDIR` reproduces
the diagnostic phases; run it with an external process timeout, as it is not a
typed import action and does not enforce its own deadline.

Import validation now checks the actual placed aggregate once, then derives its
leaves internally and records only their exact serialized BREP hashes in the
bounded native validity cache. Ordinary document validation still runs; changed
geometry or placements miss that cache. This relies on the recursive contract
of [OCCT 7.8.1 BRepCheck_Analyzer](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/src/BRepCheck/BRepCheck_Analyzer.hxx#L107-L130),
not imported metadata. Mass/area, type, source hash and transactional checks are
unchanged. Native tests reject nested invalid solids and verify foreign/mutated
geometry receives no cached proof. This is kernel validity, not interference,
manufacturing, or structural certification.

Both imports accept strict boolean `preserve_labels:false` to omit the optional
second STEP read used for label matching. The default remains true. Source entity
indices, geometry, hash and units remain recorded; `labels_status:not_requested`
makes absent recovered labels explicit. No NVIDIA performance improvement is
claimed until a fresh complete typed import passes.
