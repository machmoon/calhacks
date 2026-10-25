# Electronics plan: the PCB engine in Rust (2026-10-24 22:40)

Pat, 10/24 21:50: redo the Python PCB engine completely; rewrite it in Rust, Haskell or Lean.
Deadline: Cal Hacks submission, Sun 2026-10-25 08:50 PT.

The skeleton is in the tree and compiles. These all pass:

- `cargo check`
- `cargo test -p wright-eda -p wright-electronics` (11 tests)
- `cargo clippy --all-targets -D warnings`

The skeleton crates are `wright-eda` (shared IR, implemented) and five lane crates holding stubs with frozen signatures: `wright-kicad`, `wright-sch`, `wright-place`, `wright-route` and `wright-electronics`. The Python baseline's eval circuits are exported as JSON in `evals/electronics/fixtures/` (7 files, including the VTOL servo board), and a test checks that every one validates.

## 1. What in the Python baseline is real, and what to drop

The Python baseline is about 71k lines of Python in `engine/silkscreen`, plus front ends. It sorts into three groups.

**Real and measured.** The baseline keeps these and so do we:

| Baseline module | What it does | Evidence |
|---|---|---|
| `netlist.py` (580 lines) | Validated circuit IR. Every failure is batched for one repair prompt (`:88`, `:501`). | The 7 fixtures. |
| `kicadlib/` (resolve, index, footprint) | Exact symbol resolution against KiCad's library and pin-number correction. | `resolve.py` docstring: the "555 rejected" bug, measured 2026-09-14. |
| `footprints.py` (1507) | Generated land patterns: `silkscreen:R_0603`, `SOIC-8`, `ti_powerpad_so8`. | Fresh boards use `silkscreen:*` for most parts. Only SOT-223, SOIC-16 and one SOIC-8 come from KiCad's library. |
| `packing.py` (845) | CP-SAT placer. | `docs/measurements/placer-2026-10-24-cpsat.json` |
| `simple/` (about 1.5k) | Annealing placer, ported from KiCadRoutingTools. | `placer-2026-10-24-simple.json` |
| `routing.py` (2190) | Octilinear A* on a 0.25 mm grid with rip-up. | `routing.py:75,79,168` |
| `board.py` (2776) | Build, emit `.kicad_pcb`, ground pour, fan-out, stitching. | |
| `schematic.py` (1831) | `.kicad_sch` with embedded `lib_symbols` and labels. | |
| `verify/kicad.py` | kicad-cli ERC, DRC and parity. | |
| `spice/` | ngspice clauses with signed margins. | |
| `aircraft/electronics.py` | Power budget, harness and the TPS5450 servo board (`:257`, `:406`). | |
| `scripts/board_eval.py` | The PCBWorld-style scoreboard. | |

**Aspirational or unverified live.** These are self-declared:

- `kicadcollab/`, `meetings/`, `zoombot/`, `teamsbot/`, `alexa` (see `docs/collab.md:8`, `docs/alexa.md:9`).
- `billing/`, which has never run against real Stripe (`docs/billing.md:263`).
- The `service/` deploy, whose URL answered 500 (baseline CLAUDE.md known issue 8).
- `placement/` Ollama and Tinker adapters.
- `agents/` stages: `drc` and `parity` are "not yet on a stage" (`docs/agent-harness.md:232`).

**Drop.** These are front-end glue or belong to another product:

- `service/`, `slackbot/`, `meetings/`, `zoombot/`, `teamsbot/`, `googleapps/`, `billing/`
- `kicadcollab/`, `frontend/`, `mcp/`, `audit/judgment.py`, `specreview.py`, `onboard.py`
- `placement/*_policy.py`, `prior_art.py`, `web_research.py`, and the Gemini `agents/` layer. Wright's harness replaces that layer.
- `enclosure/` and `aircraft/cad.py`. CAD belongs to Astra, so we hand STEP to them.
- `aircraft/` physics apart from the electronics file. `wright-air` already replaces it.

**Keep the behaviour, rewrite in Rust:**

- netlist validation and library resolution
- footprints (taken from the KiCad library, not generated)
- placement, routing and pour
- the pcb, sch and pro writers
- kicad-cli verification
- the servo-board template
- board_eval as a Wright eval

SPICE is a stretch goal. It would run ngspice as a subprocess against the same netlist.

## 2. Why it is bad (measured tonight, 10/24 22:05)

I re-ran `scripts/board_eval.py` on the baseline at `3710578` with KiCad 10.0.6. All six fixtures are KiCad-clean, so the failure is not DRC on the toy fixtures. These are the problems:

1. **Too slow.** One eval pass took 3 min 50 s for six boards with at most 18 parts. The ESP32 board alone took 100.5 s with CP-SAT and 324 s with the annealer (`placer-2026-10-24-simple.json`). Two servo-board builds took 218 s.
2. **Boards are oversized and decoupling is poor.**
   - The ESP32 devboard is 1525.7 mm² for 18 parts. That is 39×39 mm, where a hand layout is roughly 25×30.
   - Its decoupling loop totals 60.9 mm, and `two_chip_usb` is 38.6 mm.
   - The placer weighs ground at 0.02 (`simple/objective.py`), and CP-SAT minimises only HPWL plus area (`packing.py`).
3. **One track width for every net** (`routing.py:79`, `DEFAULT_TRACK_WIDTH_NM = 0.2 mm`). On the boards I checked (LDO, blinker), every track was 0.2 mm, VIN and GND included.
   - The VTOL servo board can only be routed by turning the whole board up to 1.0 mm. The docstring of `aircraft/electronics.py:406` says 1.5 mm left VBAT, SW and +6V unrouted.
   - I re-measured tonight. At 1.5 mm: 7/10 nets routed, unrouted `+6V`, `SW`, `VBAT`.
   - At 1.0 mm the board reports 10/10 routed, but **DRC fails**: `starved_thermal` on J3 pad 3 (GND) in the B.Cu pour. The docstring says that build was KiCad-clean.
   - 1.0 mm carries 3.2 A at a 20 °C rise (IPC-2221, `electronics.py:390`), but five servos stall at 5 A. The check excuses this as a "transient".
4. **Its own footprints instead of KiCad's.** Every multi-pin pattern was mirrored until 2026-09-05 and no rule caught it (baseline CLAUDE.md, "Verifying against real KiCad"). SOT-223 pads sat 0.1 mm apart (known issue 11).
5. **Coordinate-frame bug class.** The solver is Y-up and KiCad is Y-down, flipped at two boundaries (`kicad.py` "Coordinate frames"). The schematic has the same problem (`_pin_on_sheet`). Known issue 9 (rotated anchors) came from this. "The run reports success, and the opened board has parts stacked."
6. **Validation gaps.**
   - Only 6 of 18 design-quality trials were scored on function (`design-quality-2026-09-08-after.json`: `scored_on_function 6`, `unscored 12`).
   - DRC and parity are not agent stages (`docs/agent-harness.md:232`).
   - ERC, DRC and parity cannot run in CI (baseline CLAUDE.md).
   - The router admits "not a competitive autorouter … a uniform grid cannot reach every pin of a fine-pitch package" (CLAUDE.md, `routing.py`).
7. **Non-determinism and weight.** CP-SAT is deterministic only with `workers=1` (`packing.py`). The OR-Tools C++ dependency is too heavy to rebuild here.
8. **Breadth over depth.** Roughly 30k lines are meeting bots, billing and service glue around a 10k-line engine.

## 3. Prior art (source read; licences checked)

A research agent read the source tonight. Clones are in the session scratchpad.

| Need | Follow | Licence | Use |
|---|---|---|---|
| s-expression format | dev-docs.kicad.org sexpr-intro and sexpr-pcb. KiCad `common/richio.cpp` `OUTPUTFORMATTER::Quotes` for quoting. | spec / GPL (design only) | `wright-eda/src/sexpr.rs` (done). |
| s-expression reader shape | `kiutils_sexpr` `crates/kiutils_sexpr/src/parser.rs` (355 lines, has a depth limit) | MIT | Design followed. Not a dependency: we need exact output control. `kiutils_kicad` 0.3 could be a dev-dependency parse check later. |
| Logical vs physical IR | tscircuit circuit-json `src/source/source_port.ts`, `src/pcb/pcb_component.ts`, `pcb_trace.ts` | ISC | `ir.rs` is the `source_*` half, `board.rs` the `pcb_*` half. |
| PCB writer | atopile `src/faebryk/exporters/pcb/kicad/transformer.py` (`insert_footprint`, `insert_track`, `insert_via`, `insert_zone`, `insert_net`) | MIT | Lane A structure. |
| Placement | KiCadRoutingTools `py_placer/placement/simple_anneal.py` and `simple.py` (@4cf6d29). Overlap weight ramps from `LAM_START` to `LAM_END`; moves are shift, turn, swap, jump; `legality.py` does the final check. | MIT | Lane C (the baseline's `simple/` ported it, minus halo and congestion). |
| Router | The baseline's `routing.py` (our own: octile A*, HOG2 no-corner-cutting, FreeRouting 45° restriction) for the algorithm. KiCadRoutingTools `rust_router/{router.rs,obstacle_map.rs}` as a Rust precedent. Topola `src/router/navmesh.rs`, `thetastar.rs` (MIT OR Apache-2.0) to read for a later topological router. | MIT / MIT-Apache | Lane D. Topola is not a dependency: it is 0.1.0, its I/O is DSN only, it has 16 dependencies, and the repo uses a petgraph fork. |
| Rules, clearance | Horizon EDA `src/board/board_rules_check_clearance_copper.cpp` | GPL (design only) | Lane E `CopperClearance`. |
| Ampacity | IPC-2221 external curve, k = 0.048 (baseline `electronics.py:386`) | standard | Lanes E and F. |

Rejected:

- OR-Tools: a C++ build.
- `good_lp` + HiGHS: `highs-sys` builds HiGHS with cmake and bindgen.
- `geo` (11 dependencies) and `geo-clipper` (C++).
- FreeRouting via DSN: kicad-cli 10.0.6 has **no DSN export**. `pcb import` has no Specctra.
- `rstar` and `i_overlay` are fine licences (MIT/Apache, 3 and 5 dependencies) but are *not needed*. KiCad fills zones itself (`drc --refill-zones`), and the slice boards are small enough for an O(n²) clearance check. **The new crates add zero dependencies** beyond `serde`, `serde_json`, `anyhow`, `thiserror`.

kicad-cli 10.0.6 surface used:

- `sch erc --format json --severity-error`
- `pcb drc --format json --refill-zones --severity-error [--schematic-parity] [--all-track-errors]`
- `pcb export step|glb|gerbers|drill|pos`
- `sch export bom|netlist`

The binary path comes from `WRIGHT_KICAD_CLI`, falling back to `/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli`. It is not on PATH. The library path comes from `WRIGHT_KICAD_SHARE`, falling back to `…/Contents/SharedSupport`, which holds 155 `.pretty` libraries in format 20260206. Both are in `wright_kicad::{kicad_cli, kicad_share}`.

## 4. Verification design

**Rust** (`wright-electronics/src/checks.rs`, done): a closed set of 15 checks. Each is unevaluated means fail; `Missing` fails; NaN fails.

`netlist_valid`, `pins_accounted`, `footprints_resolved`, `courtyard_clearance`, `routed`, `copper_clearance`, `trace_ampacity`, `regulator_setpoint`, `part_ratings`, `decoupling_loop`, `board_fits_bay`, `erc`, `drc`, `parity`, `budget`.

Rules:

- `erc`, `drc` and `parity` are kicad-cli's verdicts. `CliVerdict.ran == false` (tool missing or crashed) becomes `Missing`, never a pass. `check()` re-emits the design into a temp directory and runs kicad-cli, so the evidence belongs to this design and not to a cached file.
- `routed` fails, naming every `RouteReport.unrouted` entry with its reason. `RouteReport.attempted` must cover every net with two or more pads.

**Haskell oracle** (`haskell/src/Oracle/Pcb.hs`, base-only like the rest; ghc 9.10.3 and cabal 3.16 are in `~/.ghcup/bin`). It reads the design's board JSON, which is the serde output of `wright_eda::Board` plus `Netlist`, and independently recomputes:

1. Absolute pad positions, with its own rotation and flip code.
2. Courtyard overlap over all pairs.
3. **Copper connectivity per net.** Union-find over pads, track endpoints, track–pad overlap and vias, layer-aware. Each net must be one component. The result is compared with the router's `routed` claim. Zones are treated as connecting their net's pads on that layer only when KiCad's DRC says 0 unconnected; otherwise they count as unverified.
4. Minimum different-net copper distance: segment–segment, segment–rectangle and circle distances, minus half-widths.
5. IPC-2221 ampacity of each net's narrowest track.
6. TPS5450 setpoint, Vout = 1.221 × (1 + R1/R2) (SLVS757).

It plugs into `haskell/crosscheck.sh` as a new `pcb` function group, with tolerances: positions exact (integer nm), distances ±1 nm, current 1e-9 relative. The probe is a `crosscheck-rs` feature `electronics`.

**Lean** (`lean/Wright/Board.lean`, Lean 4.35 via `~/.elan/bin/lake`), modelled on `Check.lean`. Definitions: `NetStatus := routed | poured | unrouted reason`, `ToolVerdict := ran n | didNotRun`, and `accept board := (∀ net ∈ attempted, status ≠ unrouted) ∧ (∀ pin, onNet ∨ declaredNC) ∧ erc = ran 0 ∧ drc = ran 0 ∧ parity = ran 0`. Theorems:

1. `didNotRun` on any tool means not accepted ("unverified never green").
2. Any unrouted net means not accepted.
3. Monotonicity: marking one more net unrouted can't turn rejected into accepted.
4. An undeclared floating pin means not accepted.
5. Completeness: `attempted` ⊇ nets with ≥ 2 pads → `completion = 1` ↔ no unrouted. The empty-attempted case is ruled out, which is the baseline's "an empty result reads as 100 %" bug.

The axiom gate is the one in `scripts/verify.sh`. Neither Lean nor Haskell verifies the Rust code by itself. Lean pins the acceptance rule that Rust's `check` must implement; Haskell recomputes the geometry differentially.

## 5. Crate layout (skeleton in tree)

| Crate / path | Status | Replaces (baseline) | Follows (OSS) |
|---|---|---|---|
| `crates/wright-eda`: `units.rs` (Nm, Pt, Rect, exact 90° rotation), `ir.rs` (Circuit ↔ baseline CIRCUIT-SPEC, `validate` → Netlist, refdes, net roles), `board.rs` (Footprint, Pad, Placed with `to_board` as the **only** transform, Track, Via, Zone, NetClass, Rules (JLCPCB defaults), Board, RouteReport/Unrouted), `sexpr.rs` (reader, writer, deterministic uuid) | **implemented, 10 tests** | `units.py`, `netlist.py`, frames in `kicad.py` | tscircuit circuit-json split; kiutils_sexpr parser; KiCad sexpr spec |
| `crates/wright-kicad`: `library.rs`, `pcb.rs`, `project.rs`, `cli.rs` | stub with frozen API | `kicadlib/`, `footprints.py`, `kicad.py`, `board.py emit_kicad_pcb`, `models3d.py`, `verify/kicad.py` | atopile transformer.py; KiCad sexpr-pcb |
| `crates/wright-sch` | stub | `schematic.py` | KiCad sexpr-schematic spec; the baseline's emitter logic |
| `crates/wright-place` | stub | `packing.py`, `simple/` | KiCadRoutingTools `simple_anneal.py`, `simple.py`, `legality.py` |
| `crates/wright-route` | stub (reports every net unrouted, honestly) | `routing.py`, `diffpair.py` (stretch), `board.py` fan-out, pour, stitch | baseline routing.py; HOG2 Map2DEnvironment; KiCadRoutingTools rust_router |
| `crates/wright-electronics`: `checks.rs` (done), `templates.rs`, `pipeline.rs` (wiring done), `lib.rs` (`impl Domain for Electronics`) | skeleton | `agents/pipeline.py`, `aircraft/electronics.py`, `sourcing.py`, `scripts/board_eval.py` | `wright-balloon` as the Domain pattern |
| `haskell/src/Oracle/Pcb.hs`, `lean/Wright/Board.lean` | to write | baseline `audit/geometry.py` (second reader), `test_kicad.py` overlap truth | `Oracle/Beam.hs`, `Check.lean` patterns |
| `evals/electronics/` | fixtures done | `docs/measurements/board-eval-*.json` | |

### Data contracts (frozen; lanes may add, never change)

These are all in `wright-eda` (see the source). Lane functions:

```rust
// wright-kicad (lane A)
pub fn resolve_footprints(netlist: &Netlist, share: &Path) -> anyhow::Result<Vec<Footprint>>; // part order
pub fn write_pcb(netlist: &Netlist, board: &Board, title: &str) -> anyhow::Result<String>;
pub fn write_project(netlist: &Netlist, board: &Board, name: &str) -> anyhow::Result<String>;
pub fn verify(pcb: &Path, sch: &Path) -> Vec<CliVerdict>;   // always [erc, drc, parity]
pub fn export_step(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf>;
// wright-sch (lane B)
pub fn write_schematic(netlist: &Netlist, footprints: &[Footprint], title: &str) -> anyhow::Result<String>;
// wright-place (lane C)
pub fn place(netlist: &Netlist, footprints: &[Footprint], rules: &Rules, opts: &PlaceOptions) -> anyhow::Result<(Board, PlaceReport)>;
// wright-route (lane D)
pub fn route(netlist: &Netlist, board: &mut Board, opts: &RouteOptions) -> RouteReport;
```

Shared conventions:

- **Frame.** KiCad board frame, Y down, rotation as quarter turns counter-clockwise. A bottom-side part is mirrored in X before rotation. `Placed::to_board` is the only place this is computed. Lane F re-derives it independently in Haskell.
- **Net index.** Index into `Netlist::nets`. KiCad net numbers in the file are `index + 1`; 0 is the unconnected net. **Board net names equal schematic label names exactly**, with no `/` prefix (global labels).
- **Pad numbers.** `Pin.pad` must equal the library pad number. A device's `package` may be a KiCad footprint id (`Lib:Name`) or a baseline package name, and lane A maps the latter. Passives use pads `1`/`2`; a diode's pin 1 is the cathode (K), matching KiCad's Device:D symbols and `D_*` footprints, where pad 1 is the band (corrected 10/24 by lane E; the original line said pin 2, and fixture servo_power_6v5a had the catch diode reversed).
- **Net classes.** `Board.classes[0]` is `Default`. Lane C creates classes from `NetRole` and per-net current. Width comes from the IPC-2221 inverse at a 20 °C rise, clamped at `Rules.min_track`. Lane D must route each net at its class width and may neck down only inside a pad's own area.
- **Determinism.** Same input gives the same bytes. Budgets are counted in moves and expansions, not seconds. UUIDs come from `sexpr::uuid_for(seed)`.
- **Board JSON.** `serde_json::to_string(&(netlist, board, route))` is the oracle's input format.

## 6. Lanes (one agent each; disjoint files)

Shared-file rule: only lane E edits shared files, and only additively after re-reading them. Those files are `crates/wright-cli/*`, `crates/wright-agents/src/prompts.rs`, `merge.rs`, the root `Cargo.toml` and `docs/TODO.md`.

Every lane must:

- check `df -h /System/Volumes/Data` before building and stop under 2 GB
- never run `cargo clean`
- leave `crates/wright-cad` and `cad/` alone
- not commit

When done, each lane passes `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings`.

### Lane A: `wright-kicad` (library, pcb, project, cli). **Slice-critical.**

**Files:**
- `crates/wright-kicad/**`
- `evals/electronics/packages.json`: baseline package name → KiCad footprint id, plus a device-name → footprint table. TPS5450DDA → `Package_SO:TI_SO-PowerPAD-8_ThermalVias`. KiCad has no TPS5450 symbol, so pins come from the template.

**Work:**

- **library.rs**
  - Parse `.kicad_mod` with `wright_eda::sexpr`: pads (`at` with angle, `size`, shape incl. `roundrect_rratio`, `drill`, `layers`, `thru_hole`/`smd`/`np_thru_hole`), `F.CrtYd` union (`fp_rect`/`fp_line`/`fp_poly`/`fp_circle`/`fp_arc`, as in the baseline's `kicad.py _courtyard_points`) and `model`.
  - Passive sizes → `Resistor_SMD:R_0603_1608Metric` etc.
  - Resolve an IC by exact symbol name in `symbols/*.kicad_sym` → `Footprint` property (baseline `kicadlib/resolve.py` rules: exact match or KiCad's `x` placeholder, never fuzzy).
  - Verify every pin's pad exists, with errors batched.
- **pcb.rs**
  - Header `(kicad_pcb (version 20240108) (generator "wright"))`.
  - Layers, setup, nets, then each footprint: the library sexpr with `at`/rot, Reference/Value substituted, `(net N "name")` on pads, and `${KICAD10_3DMODEL_DIR}` model kept.
  - Then segments, vias, zones (unfilled), and the `Edge.Cuts` rectangle.
- **project.rs:** `.kicad_pro` JSON with `net_settings.classes` and assignments.
- **cli.rs:** run erc/drc/parity with `--format json` and parse `violations`, `unconnected_items`, `schematic_parity` (baseline `board_eval.py:127 kicad_counts`). Version from `kicad-cli version`. `export_step` runs `pcb export step --subst-models -o`.

**Done when:**
1. All 7 fixtures resolve.
2. A hand-placed R_0603 pair board written by `write_pcb` passes `kicad-cli pcb drc` with 0 errors. This is a gated test that is skipped when KiCad is missing.
3. Re-parsing written output with `sexpr::parse` works.
4. A STEP file exists for that board.

**Order:** first, since B, C, D and E use real footprints. C and D can start on synthetic footprints right away.

### Lane B: `wright-sch`. **Slice-critical.**

**Files:** `crates/wright-sch/**`

**Work:** port `schematic.py`:

- generated symbols in `lib_symbols` (rectangle body, pins left and right by name, pin numbers equal to the pad numbers)
- one symbol instance per part, with `Reference`, `Value`, `Footprint = footprints[i].lib_id`
- a wire stub plus a **global_label** per connected pin, and `no_connect` flags on NC pins
- `PWR_FLAG` on power-input nets driven only by connectors (else ERC `power_pin_not_driven`)
- `sheet_instances`/`instances` so refs stick

**Done when:** every fixture's `.kicad_sch` (footprints from lane A, or `lib_id` strings in tests) passes `kicad-cli sch erc --severity-error` with 0 errors, and parity with lane A's pcb is 0 (joint test lives in lane E).

**Order:** independent. It needs only `Netlist` and `Footprint.lib_id`.

### Lane C: `wright-place`. **Slice-critical**, shelf fallback first.

**Files:** `crates/wright-place/**`

**Work:**

1. A deterministic shelf or row packer that always returns a legal placement. This is the slice minimum.
2. Simulated annealing per KiCadRoutingTools `simple_anneal.py`:
   - cost: MST per net (signal 1 / power 0.3 / ground 0.02), decap-to-supply-pin pull (baseline `board.py:1039 decoupling_caps` rule: a cap sharing a power net and a ground net with an IC), overlap penalty ramped 1 → 1e4, edge hints for connectors
   - moves: shift, rotate 90, swap, jump
   - keep the best legal state, then an exact legality pass
3. Shrink the outline to the courtyard bbox + edge clearance, at a 0.5 mm grid.
4. Build `Board.classes` from net roles and currents (helper `classes_for(netlist, currents)`).

**Done when:**
1. 0 courtyard overlaps on all fixtures (property test over random seeds).
2. Same seed gives an identical board.
3. Area and decoupling loop on `esp32_devboard` beat the baseline (1525.7 mm², 60.9 mm) in `cargo test --release`.
4. Each fixture under 2 s.

**Order:** independent, using synthetic footprints. Lane A's footprints are swapped in once they land.

### Lane D: `wright-route`. **Slice-critical.**

**Files:** `crates/wright-route/**`

**Work:**

- Port `routing.py`: two layers, octile A* (ortho 12 / diagonal 17, no corner cutting, ≤ 90° heading change), an occupancy grid with clearance discs computed **per net class width**, vias with their own disc, and the expansion budgets.
- Rip-up-and-retry with lifted nets replayed from the base state, never un-marked (`routing.py` docstring).
- Pad escape: snap the pad centre to the nearest grid node with a straight stub (baseline `_MAX_ESCAPE_STEPS = 8`).
- Ground: a B.Cu `Zone` over the outline. Every GND pad that is an SMD on F.Cu gets a stitching via next to it, checked against clearance (baseline `board.py:2263 stitch_filled_nets`). Use **solid zone connection for THT** pads to avoid the baseline's `starved_thermal`.
- Route the remaining nets in order of fewest pins and shortest span first.

**Done when:**
1. Every fixture routes at 100 % with class widths. The servo board's VBAT, SW and +6V must be ≥ 1.5 mm, or whatever class width carries 5 A; this is what the baseline failed.
2. An internal clearance self-check passes.
3. `RouteReport.attempted` covers every net with two or more pads.
4. Each fixture under 5 s in release.

**Order:** independent, starting on synthetic boards. Real boards come from C + A.

### Lane E: `wright-electronics` + harness/CLI wiring + eval. **Slice-critical** (integrator).

**Files:**
- `crates/wright-electronics/**`
- `evals/electronics/{run.sh,result.md}`
- additive edits:
  - `crates/wright-cli/{Cargo.toml,src/main.rs,src/bench.rs}` (the `"electronics"` arms beside `"balloon"` at `main.rs:59,178,282`)
  - `crates/wright-agents/src/prompts.rs` `roles("electronics")`: `circuit` (topology, part values and ratings, no-connects), `layout` (placement and routing constraints, board size vs bay), `verification` (ERC/DRC/parity repair), `sourcing`
  - `merge.rs depends_on`
  - `docs/TODO.md`

**Work:**

- **templates.rs**
  - `ServoPower`: port `servo_board_spec`. R2 = 10k·1.221/(Vout−1.221). Input caps rated ≥ 1.5 × Vin max. The XT30 and inductor are rated for the current.
  - `Ldo`.
  - Per-net current table.
- **checks.rs** `check()`:
  - all 15 checks
  - kicad-cli is run in a temp directory
  - `budget` uses a small priced catalog in `evals/electronics/catalog.json` with url and retrieved fields, or Creao later
- **bom()**
- **build()**:
  - writes pcb, sch and pro
  - runs `wright_kicad::verify`
  - writes `export_step` → `<out>/cad/board.step` with a `Manifest` of kind `cad`, for Astra
  - writes the BOM CSV
- **headlines:** board mm, routed %, DRC errors, cost.
- **eval:** `evals/electronics/run.sh` runs `wright electronics design` on all 7 fixtures. `result.md` has a table of the Python baseline (numbers from §2, re-run tonight) against Wright: routed %, ERC/DRC/unconnected/parity, vias, copper mm, area, decoupling loop and seconds, including where the baseline wins.

**Done when:** `wright run electronics "servo power board for the VTOL: 6 V 5 A from 4S"` (and `--demo` offline via `wright_agents::fake`) produces:
- a `.kicad_pcb`, `.kicad_sch` and `.kicad_pro` with ERC = 0, DRC = 0 (zones refilled), unconnected = 0 and parity = 0
- `board.step`
- a BOM
- all 15 checks with evidence

**Order:** wire it against the stubs now. The pipeline goes green as A, B, C and D land.

### Lane F: Haskell `Oracle.Pcb` + Lean `Board.lean`. Slice: the Lean theorems plus the oracle connectivity and clearance checks.

**Files:**
- `haskell/src/Oracle/Pcb.hs`
- the `wright-oracle.cabal` module list (one line)
- `haskell/test/Props.hs` (append)
- `haskell/crosscheck-rs/` (an `electronics` feature)
- `haskell/crosscheck.sh` (add a `grep` gate line)
- `lean/Wright/Board.lean`
- the `lean/Wright.lean` import if one exists, otherwise the lakefile globs

**Work:** as §4.

**Done when:**
1. `cabal test` passes, including the new props: connectivity is invariant under translation and under 90° rotation of the whole board, and the clearance oracle is symmetric.
2. `lake build` passes with no `sorry` and the axiom gate passes.
3. The cross-check on lane E's fixture boards shows 0 over tolerance.

**Order:** Lean can start now. Haskell can start now on synthetic JSON written from `wright_eda::Board`; real boards come once C and D land.

## 7. Minimum vertical slice vs stretch

**Slice (by 08:50):**
- A: library, pcb, project, cli
- B: sch
- C: shelf placer plus annealing
- D: router with class widths and GND pour
- E: servo template, all checks, build plus STEP, the eval table on the 7 fixtures, and `wright run electronics`
- F: Lean acceptance theorems plus Haskell connectivity and clearance

**Stretch:**
- SPICE verification of the buck through ngspice (`/opt/homebrew/bin/ngspice`, baseline `spice/`)
- differential pairs (baseline `diffpair.py`) for USB
- an agent-proposed circuit path with the baseline's repair loop
- `pcb export glb` for the desktop view
- gerbers, drill and pos for a JLCPCB order package
- 4-layer boards
- a Topola-style topological router
- a power budget and harness (baseline `electronics.py:94,149`) folded into `wright-air` electronics

**Timeline:** 22:45 lanes start → 02:00 A, B, C, D first green on fixtures → 04:00 E end-to-end green → 06:00 eval table and oracle cross-check → 08:00 freeze, re-run evals, write result.md.
