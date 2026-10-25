# Package v2: separate-feed engineering candidate

This snapshot selects catalog parts and pin allocations. It is **not hardware-ready** and does not describe the currently running package-v1 CAD. No purchases were made. Source evidence was checked 2026-10-24. Original manufacturer CAD is reused; new envelopes are dimensional references, not modeled internals.

Changes from v1: four Holybro Tekko32 F4 AM32 lift ESCs replace fixed-wing Skywalkers; a separate stock Gens Ace Adventure 4S2200 XT60 pack supplies cruise/avionics; the stock CNHL4S5000 XT90 pack supplies lift. The positive rails are never joined. This adds83.2g in known battery/ESC mass before the PDB and harness. The revised3kg/8-minute cruise budget is an assumption, not achieved endurance or mass.

The selected Holybro18138 top-entry PDB now has actual manufacturer STEP and the correct drawing:60×80×23.5mm,50mm-square M3 mounting centers. The earlier80×80×20/70mm-pitch attribution was incorrect: that image describes the side-entry board. The previous JSON snapshot is preserved under history/2026-10-24-before-cad-acquisition. Native top-entry CAD independently measures59.987×79.987×23.0mm and fourØ3.300005mm bores on50.0001mm pitch; retain the published23.5mm height reservation. PDB current rating does not upgrade stock XT90/10AWG45A continuous or XT60/12AWG30A continuous limits.

BOM and wiring are exported in JSON and CSV. All11 formerly unnamed roles now have a selected catalog family or explicit fabricated-part definition. Several are not fabrication-ready: a generic fastener family is not a per-joint screw schedule; fabricated skids are not a purchased complete landing gear; harness routes are not known cut lengths. Procurement statuses distinguish catalog presence from confirmed stock and compatibility.

Remaining release work:

- MN2806EVO drawing confirms M3 threads and manufacturer package includes fourM3×5 screws. Neither source specifies permitted penetration or the applicable mounting stack. Native blind-hole bounds constrain geometry but do not establish thread strength.
- P12 manufacturer's CW/CCW product selection is known, but the reused single STEP has unidentified hand; shaft centering/adapter and screw retention remain unverified.
- HS65MG now replacesHS5065MG after actual FreeCAD inspection of its officialSTEP and current datasheet. The two central Ø2mm bores have28.6mm pitch and2mm ear thickness.25 spline arcs agree withM25TØ5; use the modeled/supplied horn. CurrentPDF mass12.5g overrides product-page11.9g. See servo-interface.json; installed torque adequacy still needs verification.
- DU-BRO116 hinges and105 horns have catalog lengths but no complete mounting/slot geometry. Their exact leaf thickness, barrel and hole patterns still need manufacturer CAD or measurement; approximate slots are not released.
- PDB mass/input-terminal torque, extension-cable stock and route lengths, strain relief, connector orientation, fuse coordination and thermal duty remain unverified. The10A Littelfuse part applies only to the UBEC branch and is not a propulsion-fault protection claim.
- Mass/CG, installed low-voltage thrust, separate energy reserves, wing and joint loads, landing impact and control torque remain pending. Foam has no demonstrated vibration isolation performance.

The official HS65MG model has now been inspected. The next interface improvement is the manufacturer-supplied motor/prop hardware. Replacing a motor with another undocumented mounting depth would not close the same gap.


## Manufacturer CAD acquisition

Actual current Pixhawk6C MiniModelA, shared M8N/M9N/M10 GPS housing assembly, and selected18138 PDB STEP files are in sources/holybro-cad. All three passed FreeCAD native validity; respectively2/7/27solids. Full geometry results and SHA256 are in holybro-geometry-inspection.json and cad-acquisition.json. The GPS legacyfilename is explicitly published under the manufacturer's shared M8N/M9N/M10 heading; it establishes housing geometry, not exact M10 electronic internals. The FC download is explicitly currentModelA, not legacy.

PM02D and SiK1W models were found but rejected as substitutes for selected analogPM02V3 and100mW radio. The exact radio mechanical drawing and TekkoESC2D drawing were retrieved and inspected. PM02V3 retains its published25×25×10mm module reference; no exact manufacturerSTEP was located in the official downloads. RP3 remains manufacturer-dimension based. No approximate electronics were passed off as imported CAD.


Topology correction: recursive native ownership inspection of HS65MG proves12 maximal solids and zero free reference surfaces. Its13 constituent shells include an inner shell inside a solid. The prior free-shell inference was incorrect; source geometry is unchanged. The previous provenance snapshot is retained under history/2026-10-24-before-servo-topology-correction.
