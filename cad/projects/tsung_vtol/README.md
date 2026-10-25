# Tsung Xu June 2025 VTOL reference

This project targets the specific printed VTOL shown in the author's June 2025
video, not his earlier foam aircraft or a generic quadplane. `reference-audit.json`
separates author-published specifications, observed visual features and unknowns.
`build_reference_audit.py` reproduces comparisons against the prior structural
CAD model; it does not modify that model.

The published span is 1200 mm and length is 770 mm. The author identifies Selig
3024 and a near-elliptical planform in his Reddit reply. UIUC coordinates are
pinned in `references/s3024.dat` with a hash in the audit. Author-supplied
hardware identities are provenance records, not exact component models.

`cad/freecad/tsung_vtol_reference.py` builds a separate native reconstruction at
`out/tsung-vtol-reference/`. X runs nose to tail, Y spans left/right, Z points up.
The wing chord/leading-edge distribution, pod stations, top opening and nominal
wall reduction, boom dimensions/spacing, motor envelopes and tail dimensions
are explicitly inferred. Wing and tail solids represent exterior envelopes;
they do not represent completed manufactured skins or internal assemblies.
Five blue disks are separate swept envelopes, not physical propeller blades.

The tail follows the observed inverted-U topology: fins cant inward toward a
horizontal stabilizer at their tops. The pod includes an inferred oval top
opening matching the qualitative video observation; no exact wall thickness
or internal battery/electronics locations are claimed from the image.

The saved native document exposes functional shared span, boom-spacing,
tail-top-spacing and tail-height controls. Detailed sections remain native
constrained sketches. The source audit records that no public original CAD
release was found in the checked author sources. This is an approximate
reconstruction and must not be labelled the author's exact CAD.

The earlier model was 973.953 mm long despite its correct 1200 mm span, had a
constant-chord SD7062 wing and a low horizontal stabilizer with uncanted fins.
Those are major silhouette differences. Their correction is separate from
validating joints, internals, fabrication or flight performance.
