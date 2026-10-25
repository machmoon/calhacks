# Original VTOL demonstrator — provisional native layout

This project implements the supplied BUILD_PROMPT in staged native FreeCAD work. `package-v1` freezes evidence, **not a compatible hardware kit**. The current builder is an original editable layout, not a complete or flight-validated aircraft and not Tsung Xu's original CAD.

Run with the installed FreeCAD Python runtime:

```
cad/freecad/run.sh cad/projects/vtol-demonstrator/build.py --output-dir /absolute/output --package /absolute/path/package-v1
```

Optional `--design /absolute/design.json` accepts millimetre-valued keys `Span`, `RootChord`, `BoomHalfSpacing`, `MotorFrontX`, `MotorRearX`, `BatteryLength`, `BatteryWidth`, `BatteryHeight`, `ServoLength`. Unknown, nonfinite, Boolean and out-of-range inputs fail. The ranges are input limits, not validated engineering envelopes. Battery and servo controls edit packaging reservations, not manufacturer parts.

The model uses native FreeCAD lofts, booleans, mirrors, primitive features and expressions. Airfoil/ellipse section features use the small shipped `vtol_features.py` FreeCAD Python proxy module. **Open the generated `open.FCMacro`** to establish its module path and display the intended physical objects. For headless reopening, put this project directory on `sys.path` and import `vtol_features` before `App.openDocument`. A bare FCStd alone is not a self-contained scripted-feature runtime.

Coordinate convention: X aft, Y right, Z up. The wing uses pinned UIUC S3024 points from the existing project with its final trailing-edge point closed to the initial point (original X discrepancy0.00001 chord). It uses two interpolating B-splines per section and a non-ruled native loft. The cavity reduces profile thickness and chord; it is not a certified constant-wall offset. Existing smooth-wing work informed this construction, but neither old walls nor old fit evidence transfer to this larger aircraft.

Exact manufacturer motor/propeller BReps are explicitly *candidate references*. Displaying one handed prop model four times does not establish correct CW/CCW selection. Controller, battery, power module, payload, servo, cruise and harness shapes are nonphysical reservations. Motor mounting geometry inspection is recorded separately; screw engagement and detailed mounts await a compatible hardware freeze.

Outputs include native FCStd, actual model renders, proposed airframe STEP and artifact manifest. STEP omits candidate COTS and nonphysical reservations; equivalence remains a separate check. `reopen_edit.py` exercises saved native span, root chord, boom spacing, battery and servo reservation edits. An edit propagation pass does not certify collisions, wall thickness, service paths, structure, electrical compatibility, mass/CG or flight performance.

Known incomplete details include airframe joints/retainers, motor/prop fasteners, control hinges/linkages/servos, electronics standoffs, harness termination, landing gear, payload retention and assembly/tool access. Every missing requirement stays open; neither a rendered model nor valid BReps closes the build brief.

## Revision2 saddle detail

Four original split boom saddles and connecting webs now carry motor plates. The manufacturer drawing confirms two M3 holes on19mm and two on25mm diametral patterns. CAD-derived rotation is preserved. A9mm central relief clears the actual motor base boss: an initial5mm relief was rejected after a7.4608mm³ interference. Corrected native local checks report zero intersection volume and zero contact distance for motor/plate and both saddle halves/boom; split gap1mm. Propeller flange alignment and45° rotation also report zero overlap and zero gap for the single supplied handed propeller model.

Clamp bolts/nuts are explicitly simplified provisional envelopes, not sourced exact fastener models or helical thread/load validation. The motor manufacturer drawing has no allowable engagement annotation; motor-to-plate and propeller screws remain unresolved. Thus plates now bear physically on the selected motor geometry, but the motor retention chain is not released. Tall web stiffness, boom compression, clamp preload and all four assembly contacts require independent assessment.

## Revision5 retained interfaces (independent audit pending)

`build_r5.py` preserves the earlier candidates and adds internal strap windows with outer tray edge bridges, side-loaded captive tray nuts, boom-to-spar split bridges and an integrated pod frame/spar clamp. A nominal8mm central joiner bridges the two hollow spar ends. Each manufactured output is required to be one valid connected solid; a disconnected part stops the build and saves an invalid candidate for diagnosis.

The spar remains its original full length and diameter, with its axis moved down3mm to fit the actual taper. Native checks on revision4 established2.1312mm spar-to-aerodynamic-surface clearance and1.9302mm after its dedicated passage; revision5 deliberately adds service apertures around the new clamp assemblies. These apertures are not sealed aerodynamic hatches. Control-surface hinges, battery straps, motor/propeller screw qualification and structure/preload remain unresolved.

Use `--skip-renders` during geometry iteration to save the native candidate and preview STL before spending time on diagnostic views. This does not skip geometry validity or the single-solid gate. Full assembly interference, contact, service and export audits remain separate. No revision is a complete VTOL merely because it passes a local or whole-assembly geometric check.
