# Analysis from native wing CAD

Wright now extracts the current editable FreeCAD spline-loft sections and runs open-source wing analysis against that geometry. This complements the existing `wright-cfd` abstract-aircraft AVL/SU2 stages; it does not replace their full-aircraft model or acceptance checks.

## Integrated tools

| Tool | Purpose here | Upstream |
|---|---|---|
| AeroSandbox 4.2.10 | 3D vortex-lattice lift, induced drag, panel forces; coarse/fine comparison | https://github.com/peterdsharpe/AeroSandbox |
| OptVL 2.5.0 / AVL | Separate solver comparison of the same exported sections | https://github.com/joanibal/OptVL |
| NeuralFoil 0.3.3 | Separate root/mid/tip 2D section polars and model confidence | https://github.com/peterdsharpe/NeuralFoil |

Adapter follows AeroSandbox `aerosandbox/aerodynamics/aero_3D/vortex_lattice_method.py`, `aerosandbox/geometry/wing.py`, and OptVL's `OVLSolver` interface. AeroSandbox and NeuralFoil publish MIT licenses. OptVL's upstream `LICENSE.txt` contains GPL and bundled-library notices; retain upstream licensing when packaging. These are subprocess dependencies, not copied solvers.

OpenVSP/VSPAERO (https://github.com/OpenVSP/OpenVSP) is a candidate for richer aircraft/propeller analysis, not integrated in this change. Its NOSA license differs from MIT. VortexLattice.jl (https://github.com/byuflowlab/VortexLattice.jl) is another candidate independent implementation; adding a Julia runtime is deferred until a concrete validation need justifies it.

## Run

Use a Python environment with `crates/wright-cfd/py/requirements-native-wing.txt`; those are tested direct-version pins, not a complete dependency lock. The existing solver environment already had these packages, so this change did not install another environment.

Set `WRIGHT_ANALYSIS_PYTHON` to that environment's Python, or use `WRIGHT_PYTHON` from `scripts/env.sh`. FreeCAD remains the separate native geometry worker. Agent action:

```json
{"action":"analyze_wing","args":{"object_name":"RightOuterContour","velocity_m_s":20,"alphas_deg":[0,4,8],"altitude_m":0,"samples_per_branch":65,"symmetric":true}}
```

Use the actual loft object name reported by inspection. Symmetry explicitly assumes the other half matches. The managed output folder contains input geometry, solver report, PNG plots, panel-load CSV and an AVL deck. Direct runner replay:

```sh
"$WRIGHT_ANALYSIS_PYTHON" -I crates/wright-cfd/py/native_wing_analysis.py out/native-wing-analysis/input.json out/native-wing-analysis-replay
```

`cad/freecad/aero_sections.py` converts millimetres to metres, reads native profile curves/placement, records a geometry fingerprint, rejects unsupported nested frames and invalid budgets. Smooth spanwise lofts are reduced to interpolation between their original sections; this is not a full surface-panel discretization. The runner rejects sampled coordinate-frame deviations above 0.25 mm.

## Evidence and limits

`out/native-wing-analysis/analysis.json` records the actual 11-section smooth wing, 1.2 m span and 0.2251 m² reference area. At 20 m/s, alpha 4°, AeroSandbox predicts CL 0.5399 versus AVL 0.5616; the aerodynamic right-root vertical bending estimate is 3.8014 N m. Fixed coarse/fine and solver-comparison criteria pass at 0°, 4°, 8°. These comparisons do not validate shared assumptions.

`cad/freecad/test_aero_sections.py` checks native units, placement, edited sections, invalid inputs and save/reopen. `crates/wright-cfd/py/test_native_wing_analysis.py` checks symmetric zero lift, sign symmetry, left/right loads, speed-squared force scaling, positive induced drag, and rectangular AR10 lift slope against the classical finite-wing estimate (12% tolerance). Actual result: 4.893 vs 5.236 per radian. Four invalid-input controls also pass. Evidence: `out/freecad-aero-sections/result.json` and `out/native-wing-analysis/regression.json`.

The 3D model is steady incompressible inviscid wing-only analysis. CD is induced drag, not total aircraft drag. NeuralFoil's reported confidence is a model diagnostic, not a calibrated probability of correctness; especially the tiny tip's low Reynolds number requires further validation. Section polars do not include finite-wing downwash and are not added into 3D total drag. No fuselage/tail, rotor wash, stall validation, aircraft stability, structural strength, weight/inertia, attachment flexibility, or flight approval is supplied. CSV loads are inputs for later structural analysis, not a structural pass. Geometry changes require rerunning analysis; these artifacts do not grant design acceptance.

The native tool fixture also exercised all three real solvers through `wing_analysis.analyze`: source file and native geometry unchanged, report/PNG/CSV present, invalid options rejected (`out/native-wing-tool-test/test-result.json`). Rust `typed_wing_analysis_is_readonly_and_returns_scoped_artifacts` passed with managed-output enforcement and unchanged checkpoint bytes.

Offline agent-harness test `wing_analysis_emits_json_plot_and_loads_project_artifacts` passed: the real solver action emits all three artifacts into the project event stream. This verifies tool integration, not a live model choosing the action.

The actual typed action also passed on `out/tsung-vtol-smooth-aircraft-final/assembly.FCStd`, `RightOuterContour` (11 sections), at 20 m/s and 0°, 4°, 8°. `out/native-wing-tool-actual/test-result.json` records unchanged source-file/native-wing fingerprints, solver versions, numerical criteria and managed JSON/PNG/CSV artifact paths. This closes the native geometry → typed tool → real solvers path; the independent runner regression and fixture/harness tests remain distinct evidence. The wrapper retains input/log artifacts on a mocked 240-second timeout instead of returning a success claim (`out/native-wing-tool-test/test-result.json`).
