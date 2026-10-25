# Devpost draft (fill the bracketed numbers only from evals/*/result.md)

## Name
Wright

## Tagline (2-3 sentences, the submission form's pitch)
Describe a drone, a student rocket or a weather balloon; Wright's team of
Claude agents designs one you can actually build -- sliced prints, a priced
parts list with links, wiring and flight settings. The agents can only act
through deterministic engineering tools, and every claim must pass a check
that fails when its evidence is missing.

## Inspiration
Builders lose drones, rockets and balloons on the first flight to things no
calculator checked: a boom that flexes, a centre of gravity 2 cm off, a GPS
that stops at 12 km (docs/product/brief.md has the forum threads). Our own
earlier prototype "beat" a real 130-mile VTOL on paper -- until an adversarial
review found thirteen bugs that let impossible designs pass. Wright is the
rebuild, with every one of those lessons turned into a rule.

## What it does
* Takes a request in plain words and a requirements-only spec.
* An orchestrator (Claude Fable 5.1) delegates to specialist agents
  (Claude Opus 5.5): aero, structures, electronics, sourcing, fabrication;
  propulsion and engine for rockets; envelope and tracking for balloons.
* Their only actions are tools: design, check, sweep, catalog search,
  build, slice. The lead keeps an explicit plan graph; independent
  specialists run in parallel and their edits merge as JSON Patches; a
  strict part-id gate refuses any part not in the catalog. A hostile
  reviewer agent attacks every design; each confirmed attack becomes a
  permanent regression check, and lessons from earlier runs are retrieved
  into the next one. Runs checkpoint after every step and can be paused,
  approved step by step, or resumed after a crash. `wright bench` replays
  11 adversarial scenarios (hallucinated parts, a reviewer attacking the
  request, a looping lead, a 429ing API) against the team offline.
* The desktop app (Tauri + React) shows the plan, the agents' log, every check
  with its margin, renders and the 3D assembly live, and lets you pause the
  team, approve it step by step, or draw on the design and send the sketch back.
* Output: STL + sliced G-code (PrusaSlicer 2.9.6) with real print time and
  grams, a priced BOM, wiring, ArduPilot parameters / engine drawings /
  fill instructions, and a check report with margins.

> **Re-run after three hostile reviews and a CFD audit (10/24).** Earlier wins were withdrawn: the reviews found checks that couldn't fail and grading that favoured Wright, and AVL found a double-counted drag term in our own model (and in AeroSandbox's). Both sides are now judged by the same strict checks with no budget; the stock-only VTOL is a dead heat and the table says so.

## Results (strict checks, same model for both sides)
| vehicle | reference | Wright |
|---|---|---|
| VTOL quadplane | Tsung Xu's 2025 3D-printed VTOL (flew 130 mi), graded at its best unpublished chord | 48/48 strict checks, no budget, 15 % reserve, drag model audited against AVL: 218.9 vs 192.5 km (x1.14) with the same made-to-order pack class the reference flies; stock-listed parts only: 193.5 vs 192.5 km (x1.005, a dead heat); the reference fails 7 checks; JSBSim 6-DOF flies the full mission |
| student liquid rocket incl. engine | Space Enterprise at Berkeley's Eureka-2 (flew 15,879 ft) | with SEB's own flight-calibrated mass and engine efficiency on both sides and OpenRocket's nose-drag scaling (SU2-checked): all 8 propellant pairs pass 26/26 and score 836-839 vs 588 (even SEB's own LOX/propane: 838); trajectory checked against RocketPy 6-DOF (0.1 % on apogee) |
| weather balloon | our team's plan, given the best of 600 balloon/chute/wall/fill choices (fails 6 of 26 on a 30 km goal; 2 of those are evidence it never declared) | 26/26 checks; burst 37.8 vs 34.1 km; recovery 0.91 vs 0.83; passes on live forecast winds (Tawhiri); thin margins on supply voltage and an assumed camera temperature rating |

Each reference is graded in the reference's favour where data is missing
(e.g. the VTOL's unpublished chord is set to whatever flies it furthest).

## How we built it
Rust workspace (core, geometry, three vehicle domains, agent harness, CLI) with
the desktop UI in `app/` (Tauri 2 + React, built on Pat's earlier desktop shell); Lean 4 proofs that no check can pass on missing
evidence; an independent Haskell implementation of the atmosphere, nozzle and
balloon formulas as a cross-check oracle. Wright is the overlay; the heavy
tools are maintained open source run as subprocesses, never reimplemented:
FreeCAD headless for CAD (typed parts, joints, STEP/GLB), PrusaSlicer for
slicing, AVL and SU2 for CFD, CalculiX + gmsh for FEA, JSBSim and RocketPy
for flight simulation, Cantera to audit the engine chemistry, and FreeCAD's
CAM post-processors for the toolpaths.

## Challenges
Our wins kept evaporating, and that turned out to be the product working.

* Our prototype claimed x1.23 the range of a real 130-mile VTOL. Once
  missing evidence counted as a failure, that dropped to 180 vs 181 km.
* Two hostile review agents then attacked Wright itself and found 29 more
  defects, including:
  * a stall check using a lift coefficient its own data said was 4.5 % high;
  * hover thrust computed with the battery sagged below its cell minimum;
  * a rocket tank rated below nitrous oxide's own vapour pressure;
  * fin flutter overstated by sqrt 2;
  * an agent that could lower the person's range requirement by editing the
    design instead of the spec;
  * grading that quietly favoured Wright over each reference.
  We withdrew every win, fixed the checks, and re-ran both sides under the
  same rules.
* The rocket went from "837 vs 588" to "122 vs 588" once both sides used a
  mass model calibrated on Eureka-2's real flight, and back to 836-839 only after
  the engine was redesigned at SEB's demonstrated efficiency and SU2 showed our
  nose drag was a fineness-3 table on a fineness-5 nose.
* Our first recorded agent runs showed the specialists guessing part numbers.
  They also showed the team throwing away a passing design after a fair
  reviewer attack. We added a catalog lookup tool and kept the best verified
  design, following Optuna's best-trial and FunSearch's program database.
* AVL (via the new CFD stage) showed our induced drag was double-counted: the
  Nita-Scholz Oswald factor's viscous term on top of airfoil-polar profile drag.
  AeroSandbox's AeroBuildup has the same double count, which is why an audit
  against it had "agreed". Fixing it moved the stock-only VTOL from a loss
  (x0.935) to a dead heat (x1.005).
* A disk-filling bug: the aircraft search cached 8 GB of sliced G-code in a
  temp directory.

## Built with
rust, lean4, haskell, claude (claude code cli + anthropic api), prusaslicer, egui,
freecad (open cascade), su2, avl, gmsh, calculix, jsbsim, rocketpy, cantera, openusd

## Tracks
Best Hardware Hack / Overall; Anthropic -- Claude for Public Benefit
for first-time university high-altitude-balloon and student rocketry teams, starting with our own UCSB balloon team; Microsoft -- Built for the Real
World; Creao -- One-Person Company [agent id + run share link required].

## Disclosure
Code written at Cal Hacks 2026. Sourced data tables (UIUC propeller and
airfoil data, ArduPilot parameter metadata, a parts catalog with a URL per
row) were compiled earlier by the author for a prior project (the Python baseline).
