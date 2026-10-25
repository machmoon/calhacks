# Wright

Wright is an AI assistant for hardware engineers. Claude agents design
vehicles and boards that can actually be built: VTOL drones, student liquid
rockets down to the engine, high-altitude balloons, and now printed circuit
boards. Wright doesn't replace the engineer's tools. It runs as an overlay on
FreeCAD and KiCad and drives them the way an engineer would. The agents can
propose a design but can't accept one. Acceptance is a separate decision
made by checks the agents can't edit: Rust checks, KiCad's own `kicad-cli`,
an independent Haskell oracle and Lean proofs. None of these can pass a
design on missing evidence.

Built at Cal Hacks 2026 (from 2026-10-24).

![The VTOL servo power board from `out/demo-pcb`, rendered by kicad-cli](docs/img/demo-pcb-3d.png)

*A 6 V / 5 A servo power board for the VTOL (TPS5450 buck, 17 parts, 46.0 x 27.5 mm,
two layers). Wright made it from a one-line spec. KiCad 10.0.6 reports 0 ERC errors,
0 DRC errors, 0 unconnected items and 0 schematic/board parity errors. It does not pass
every one of Wright's own checks yet; see [Results](#results).*

## Quick example

```sh
cargo run --release -p wright-electronics --example design -- \
    evals/electronics/specs/servo_power.json OUT
```

The spec is the request, given as requirements only:

```json
{
  "request": "servo power board for the VTOL: 6 V 5 A from 4S",
  "template": {"kind": "servo_power", "vin_min_v": 14.0, "vin_max_v": 16.8,
               "vout_v": 6.0, "iout_a": 5.0, "servos": 5},
  "max_board_mm": [60.0, 40.0],
  "budget_usd": 25.0
}
```

`OUT/` ends up holding:

* `board.kicad_sch`, `board.kicad_pcb` and `board.kicad_pro`, which open in KiCad
* `kicad-verdicts.json` with the ERC, DRC and parity results from `kicad-cli`
* `checks.json`, all 15 electronics checks with their evidence
* `bom.csv`
* `cad/board.step` for the mechanical assembly
* SVG and PNG renders under `svg/`

This example uses no agents. To run the full agent team, use
`wright run electronics "<request>"`.

## Why use Wright?

* **Agents can't grade their own work.** Checks form a closed set. A check
  that never ran, or ran without evidence, fails. NaN fails. A missing
  rating fails. Lean proves these rules on a model of the checker
  (`lean/`); Rust enforces them (`wright-core/src/check.rs`).
* **The engineer's own tools are the authority.** For boards, a design is
  only clean when KiCad's own `kicad-cli` ERC, DRC (with zones refilled) and
  schematic/board parity report zero errors. Footprints come from KiCad's
  own library, never from re-drawn patterns. For mechanical work, solids,
  booleans, STEP export and interference checks all run through a
  persistent headless FreeCAD.
* **Independent recomputation.** A Haskell oracle (`haskell/`) re-derives
  the physics and the board geometry from scratch and is compared with the
  Rust under stated tolerances. For boards, that covers pad positions,
  per-net copper connectivity, clearance, IPC-2221 ampacity and the
  regulator setpoint.
* **The output can be built.** Printed parts are sliced with PrusaSlicer.
  Boards come out as KiCad projects with a STEP model. Every design comes
  with a priced BOM that links to each part, and anything unpriced is
  marked.
* **Honest comparisons.** Every eval compares Wright with a real reference
  design under the same checks, grading in the reference's favour where
  data is missing. The tables include the rows where the reference wins.

## Why not use Wright?

* It is a hackathon project, a weekend old. Electronics covers two-layer
  boards built from templates (servo power buck, LDO). There is no SPICE
  verification, no differential pairs and no fab-order package yet.
* KiCad 10 is required for the electronics checks. Without `kicad-cli`,
  `erc`, `drc` and `parity` fail as missing. They are never skipped.
* Neither the Lean proofs nor the Haskell oracle verify the Rust code
  itself. Lean pins down the acceptance rule. Haskell recomputes selected
  quantities differentially.
* Nothing is fabricated or purchased. Wright prepares the files and the
  BOM; a person reviews and orders.

## Results

The vehicle tables come from `evals/*/result.md`. Both sides of each
comparison are judged by the same checks with no budget, and cost is
reported for information only.

| eval | reference | Wright | source |
|---|---|---|---|
| VTOL quadplane | Tsung Xu's 2025 3D-printed VTOL, reproduced from its published parts at whichever unpublished wing chord flies it furthest. It fails 7 of 48 checks (3 real, 4 excused for missing data). | Passes 48/48. Range with a 15 % reserve: 218.9 vs 192.5 km (x1.137, or x1.132 pessimistic) with made-to-order parts allowed, as the reference's own pack is. With stock-listed parts only: 193.5 vs 192.5 km (x1.005, a dead heat; x1.036 pessimistic). | `evals/vtol/result.md`, `result_listed.md` |
| Student liquid rocket, engine included | Space Enterprise at Berkeley's Eureka-2 (LOX/propane), FAR-OUT score 588 from its recorded flight | All 8 propellant pairs pass 26/26 and score 836-839 under Eureka-2's own c* efficiency (0.71) and flight-calibrated dry mass. The 2-point spread is noise. Eureka-2's own LOX/propane scores 838. The pair search picks N2O/RP-1 (838, $10,110). | `evals/rocket/result.md` |
| High-altitude balloon | The author's team balloon plan, given the best catalog options: 20/26 checks | Passes 26/26. Burst at 37,819 vs 34,111 m (US76 model). Recovery probability 0.906 vs 0.828. Margins are thin: supply voltage +0.00 V and part temperature +0.15 K. | `evals/balloon/result.md` |

For electronics there is no `evals/electronics/result.md` yet. These numbers
are from the lane reports in `docs/TODO.md` and the run in `out/demo-pcb`.

* **Placement** (`wright-place`) was tested on the Python baseline's own
  footprints for its seven eval boards. All 7 beat the baseline on both
  board area and decoupling loop. On the ESP32 devboard the board is
  1225.5 mm² (Edge.Cuts) against the baseline's 1525.7 mm², and the
  decoupling loop is 24.4 mm against 60.9 mm. The comparison is graded in
  the baseline's favour, because its area excludes its 2 mm margin.
* **Routing** (`wright-route`) routes all 7 fixtures at 100 % with per-class
  track widths, and its clearance self-check is clean. The servo board's
  `+6V` and `SW` nets get 1.85 mm and `VBAT` gets 1.5 mm. The Python
  baseline routed every net at one width. It could finish this board only
  by setting every track to 1.0 mm, and that board then failed DRC
  (`docs/electronics-plan.md` §2).
* **The servo board above** passes 11 of 15 checks: KiCad ERC, DRC and
  parity at 0 errors, 11/11 nets routed or poured, 0 courtyard overlaps,
  and a regulator setpoint of 6.009 V. Four checks do not pass:
  * `trace_ampacity` fails. The narrowest GND track carries 4.03 A against
    a 5 A design current.
  * `decoupling_loop` fails. C3's loop is 12.9 mm against a 10 mm limit.
  * `part_ratings` lacks evidence: 8 of 17 ratings are not on file.
  * `budget` lacks evidence: none of the 12 BOM lines has a retrieved price.

Verification: `lake build` passes with no `sorry`, and the axiom gate
passes over 60 theorems. The Haskell cross-check makes 3928 comparisons,
with 0 over tolerance.

## Architecture at a glance

```
 engineer ──► Wright desktop app (app/, Tauri 2 + React) ──► `wright run` (child process)
                                                                 │
               Claude agents (wright-agents): orchestrator, specialists, hostile reviewer
                                                                 │  propose edits, call tools
               design model + domain crates (wright-core, air/rocket/balloon/electronics)
                                                                 │  bounded jobs, one revision each
   workers: FreeCAD · KiCad (kicad-cli) · PrusaSlicer · SU2 · AVL · gmsh + CalculiX · JSBSim · RocketPy · Cantera
                                                                 │  artifacts + evidence
   acceptance: Rust checks ── Haskell oracle (recompute) ── Lean (the acceptance rule, proved)
```

Runs are stored as files: JSON state, `events.jsonl`, stage manifests and
artifact directories under the run directory. A recorded run can be
replayed in the desktop app. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
for the lessons behind each rule, and
[`docs/electronics-plan.md`](docs/electronics-plan.md) for the PCB pipeline.

## Crates

| crate | what it does |
|---|---|
| `wright-core` | units as types, no-omission checks, BOM and provenance, the event stream, the `Domain` trait |
| `wright-geom` | the one geometric model: meshes, booleans (a port of csg.js), STL, all-pairs interference, the slicer driver |
| `wright-air` / `wright-rocket` / `wright-balloon` | the three vehicle domains and their evals |
| `wright-eda` | the shared electronics IR: circuit netlist, KiCad-library footprints, the placed and routed board, s-expressions. Integer nanometres, with one frame transform |
| `wright-kicad` | reads `.kicad_mod`/`.kicad_sym`; writes `.kicad_pcb`/`.kicad_pro`; runs `kicad-cli` ERC, DRC, parity and STEP export |
| `wright-sch` | `.kicad_sch` writer with embedded symbols and global net labels |
| `wright-place` | placement by simulated annealing (after KiCadRoutingTools), with a shelf-packing fallback and IPC-2221 net classes |
| `wright-route` | two-layer octilinear A* with rip-up, per-class widths, ground pour and stitching vias |
| `wright-electronics` | the electronics domain: circuit templates, the pipeline, the 15 checks, the BOM and the build |
| `wright-agents` | the Claude harness: plan graph, parallel specialists merged as JSON Patches, hostile reviewer, checkpoints, traces, spend ledger |
| `wright-cad` | the persistent headless FreeCAD worker: typed parts, joints, STEP/GLB/FCStd, interference, mass properties |
| `wright-cfd` / `wright-fea` | AVL (OptVL) and SU2 flow solves; CalculiX + gmsh static, buckling and modal runs |
| `wright-fab` | process plans, CAM toolpaths, removal simulation |
| `wright-creao` | the Build Desk: a Creao agent prices BOM lines from live listings, and Wright validates every quote |
| `wright-guide` | STL renderer and the printable build guide |
| `wright-train` | exports verified agent transcripts as training data (`wright train export`) |
| `wright-cli` | the `wright` binary |
| `app/` | the desktop app (Tauri 2 + React). Its Wright page drives `wright run` and shows plan, log, checks, BOM, renders and 3D, with pause, approve and replay |

## Building

Wright is written in Rust. You need a stable toolchain and the external
tools for the domains you use.

* KiCad 10 for electronics. The path comes from `WRIGHT_KICAD_CLI` and
  `WRIGHT_KICAD_SHARE`, falling back to `/Applications/KiCad/KiCad.app`.
* FreeCAD for CAD.
* PrusaSlicer for slicing.
* Optionally SU2, AVL, gmsh and CalculiX for the solvers. Their paths come
  from `scripts/env.sh`.

```sh
cargo build --release -p wright-cli
```

Some useful commands:

```sh
cargo run -p wright-cli -- balloon schema                     # what a balloon spec can ask for
cargo run -p wright-cli -- balloon design spec.json -o out/   # resolve, check, build, slice
cargo run -p wright-cli -- electronics design spec.json -o out/
cargo run -p wright-cli -- run aircraft "a VTOL that flies 200 km"   # the agent team, headless
(cd app && npm ci && npm run tauri dev)                       # the desktop app
```

Claude runs through a signed-in Claude Code subscription by default
(`WRIGHT_TRANSPORT=cli`). The other options are `api` (an Anthropic API
key) and `foundry` (Claude on Microsoft Foundry). Every call is priced and
recorded in a spend ledger. `WRIGHT_RUN_BUDGET_USD` and
`WRIGHT_GLOBAL_BUDGET_USD` add caps.

## Running tests

```sh
scripts/verify.sh --quick   # Rust tests + clippy
scripts/verify.sh           # also Lean proofs and axiom gate, Haskell oracle, cross-check, evals
```

Missing toolchains show up as `SKIP` with a reason, never as a silent pass.

## Credits

Wright follows the designs of these open-source projects. The source file
each one is followed from is named in a comment at the place it is used.

* [KiCadRoutingTools](https://github.com/drandyhaas/KiCadRoutingTools)
  (MIT): the placer (`simple_anneal.py`, `legality.py`, templates and
  grouping) and the router's obstacle map, proximity cost and seed judging.
* [Topola](https://codeberg.org/topola/topola) (MIT OR Apache-2.0): the
  net presort in the router (`planar_preconfigurer.rs`).
* [`kiutils_sexpr`](https://crates.io/crates/kiutils_sexpr) (MIT):
  the shape of the s-expression reader.
* [atopile](https://github.com/atopile/atopile) (MIT): the structure of the
  `.kicad_pcb` writer (`transformer.py`).
* [tscircuit circuit-json](https://github.com/tscircuit/circuit-json)
  (ISC): the split between the logical and physical IR.
* [KiCad](https://www.kicad.org/): the file formats, the footprint and
  symbol libraries, and `kicad-cli`, the electronics authority. Horizon
  EDA's copper clearance check and HOG2's octile grid were read for design
  only.
* [csg.js](https://github.com/evanw/csg.js) (MIT): ported for the
  fallback mesh booleans.
* [OpenRocket](https://github.com/openrocket/openrocket): nose pressure-drag
  scaling by fineness.
* [SWE-smith](https://github.com/SWE-bench/SWE-smith): the
  rejection-sampling format for training data.
* Solvers and tools run as subprocesses: FreeCAD, PrusaSlicer, SU2, AVL
  (OptVL), gmsh, CalculiX, JSBSim, RocketPy, Cantera, and Tawhiri for
  balloon flight prediction.
* The desktop app is a fork of [Pluely](https://github.com/iamsrikanthnani/pluely)
  (GPL-3.0). Its overlay pets follow the Codex pet format (openai/skills
  `codex-pet-contract.md`, Apache-2.0) and ship two MIT sprite sheets:
  "Corgi Companion" by cxian0928-afk and Baluc by Adarsh Jha. Details are
  in [`app/NOTICE.md`](app/NOTICE.md).

## License and disclosure

The Rust workspace is MIT. `app/` is GPL-3.0, inherited from Pluely; see
`app/NOTICE.md`.

Wright's code was written at Cal Hacks 2026. Some sourced data tables were
compiled earlier by the author for a prior Python prototype, the "Python
baseline" that the evals compare against: UIUC propeller and airfoil data,
ArduPilot parameter metadata, and a parts catalog with a URL per row. See
`data/*/README.md`. The PCB fixtures in `evals/electronics/fixtures/` are
that baseline's eval circuits. The baseline's code is not part of this
repository.
