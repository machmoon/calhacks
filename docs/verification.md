# Verification: what is proved, what is cross-checked

Two pieces, each with one job:

* `lean/` proves, in Lean 4, that the check semantics of
  `crates/wright-core/src/check.rs` cannot pass by omission.
* `haskell/` is an independent oracle: the load-bearing formulas the Rust
  domains use (US Standard Atmosphere 1976, ideal-nozzle relations, balloon
  lift and ascent) written again from the primary sources, tested against
  published tables, and run side by side with the Rust crates.

## How to run

```sh
# Lean (toolchain pinned in lean/lean-toolchain: v4.35.0, no Mathlib)
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build                     # must finish with no errors
lake env lean Wright/Axioms.lean          # every theorem: [propext, Quot.sound], no sorryAx

# Haskell (GHC 9.10.3, cabal 3.16.1.0, fully offline)
export PATH="$HOME/.ghcup/bin:$PATH"
cd haskell && cabal test                  # 31 table rows + 27 QuickCheck properties
echo '{"fn":"us76","z_m":11000}' | cabal run -v0 wright-oracle

# Rust vs oracle (from the repo root)
haskell/crosscheck.sh                     # KEEP=1 keeps the case/result JSON
```

## Lean: the check semantics

`lean/Wright/Check.lean` models `check.rs` line for line (the header of the
file maps every Rust item to its Lean counterpart):

| Rust | Lean model |
|---|---|
| `trait CheckId { const ALL }` | `class CheckId` with `all : List C` and a proof `complete : ∀ c, c ∈ all` |
| `Evidence::Margin { value: f64 }` / `Missing` | `Evidence.margin (m : Int)` / `.missing` |
| the `f64` given to `Report::margin` | `Value.finite (m : Int)` / `Value.nonfinite` (NaN, ±inf) |
| `Outcome::passed` | `Evidence.passed`: a margin with `0 ≤ m` |
| `Report::put` (worse outcome wins) | `keep` / `put`: the same two guards in the same order |
| `Report::finish`, `Finished::passed` | `Report.finish` (unevaluated becomes `missing`), `Finished.passed` |

Theorems (all in namespace `Wright.Check`, none uses `sorry`; `#print
axioms` shows only `propext` and `Quot.sound`, not even `Classical.choice`):

* **(a)** `finished_passes_iff_all_measured`: a finished report passes iff
  every declared check holds `margin m` with `0 ≤ m`.
  `build_passes_iff`: stated on the recordings: a report built from any
  sequence of recordings passes iff every declared check was recorded at
  least once and every recording of it was a measured non-negative margin.
* **(b)** `record_keeps_failure`: once a check holds a failing outcome, any
  further recording (passing or not) leaves it failing.
  `failure_is_permanent`: the same over any list of later recordings, for
  the slot and for the whole finished report.
* **(c)** `missing_fails`, `record_missing_fails`, `record_nonfinite_fails`
  (a NaN or infinite margin fails), `unevaluated_fails` (an id nobody
  recorded fails the report), `missing_slot_fails`.
* **(d)** `order_irrelevant`: two recording sequences that are permutations
  of each other (`List.Perm`) give the same pass/fail per check and for the
  report.
* Supporting lemmas: `keep_passed` (the kept outcome passes iff both
  did; this is the algebra everything else rests on), `keep_margin_min`
  (two measured margins keep the minimum: "the worse outcome wins"),
  `foldl_got`, `slotPassed_fold`, `build_slot`, `finish_passed_iff`.
* `lean/Wright/Example.lean` replays the three Rust unit tests on the model,
  checked by the kernel with `decide`.

Order does change *which* failing outcome is kept (a `Missing` recorded
after a negative margin replaces it, and vice versa); (d) is about pass/fail,
which is what the theorem states.

### The reward (`lean/Wright/Reward.lean`, 2026-10-24 night)

`wright-train` (docs/post-training.md) labels every recorded trajectory
with `checks_passed` / `checks_total` and keeps the `passed` ones for
rejection-sampling fine-tuning. `reward` is `checks_passed` on the model
(`List.countP` of passing outcomes over `finish`), and the theorems, in
namespace `Wright.Check.Report`, say what a policy can and cannot do to it
through the report (same axioms: `propext`, `Quot.sound`; listed in
`Axioms.lean`, checked by `scripts/lean_axioms.py`):

* `reward_eq_total_iff`, `reward_full_iff_all_measured`: the full reward is
  exactly a passing report, i.e. every declared check measured with a
  non-negative margin. No credit for a check nobody ran.
* `reward_lt_total_of_slot_fails`, `reward_lt_total_of_unevaluated`: one
  failing, missing or unevaluated check costs at least one point, whatever
  the rest did.
* `reward_le_recorded`: a report built from recordings scores at most one
  point per declared check that was recorded at all.
* `reward_record_le`, `reward_foldl_le`: measuring an already-measured
  check again, once or any number of times, never raises the reward (the
  slot keeps the worse outcome; `keep_passed`). Retrying checks buys
  nothing.
* `reward_perm`: recording order does not change the reward.

`Example.lean` runs the cases by `decide`: one measured pass and one
unevaluated check score 1 of 2; a pass recorded after a failure scores 0
for that check; ten recordings of one check score one point.

What this does not say: anything about the margins' values. A wrong number
that reaches the report as a non-negative margin is a pass here; that is
the oracle's territory below.

### The incumbent rule (`lean/Wright/Incumbent.lean`, 2026-10-24 late)

The harness keeps the best verified design (`crates/wright-agents/src/state.rs`
`Incumbent`, `better`, `DesignState::offer`: replace only on strictly
better, FunSearch's `_register_program_in_island`; fewest failing checks
first, then the objective, Optuna's feasibility-first `_get_best_trial`).
The model (`Wright.Incumbent`): `Score.failing : Nat`, `Score.objective :
Option Int` with `none` standing for both a missing and a NaN objective
(the Rust maps both to `-inf` before comparing, so they are one point of
the order), `better` written as in the Rust, `offer`, and `run` as a
`foldl` of offers. 12 theorems, no `sorry`, checked by the same gate:

* `better_failing_le`, `better_passing`: whatever beats an incumbent has
  no more failing checks than it; whatever beats a passing incumbent
  passes.
* `offer_keeps_passing`, `offer_failing_le`: one offer never replaces a
  passing design with a failing one and never raises the failing count.
* `run_failing_le`, `run_keeps_passing`: the same over any sequence of
  offers.
* `better_irrefl`, `offer_tie`, `better_asymm`: a design never beats
  itself (a tie keeps the earlier one), and two designs never both beat
  each other.
* `failing_zero_iff_passed`: in the check model, zero failing outcomes is
  exactly `Finished.passed`, so the harness's "0 failing" and
  `wright-train`'s `passed` label agree. This one theorem rests on
  `Classical.choice` as well (through the core list lemmas it uses); the
  gate allows it, the others use `propext` and `Quot.sound` only.

Not modelled: `rescore_best` (the incumbent re-scored under a changed
spec; the rule is then applied to the new scores, which is what the
theorems cover), `prefer_best` (whether the run's final design is the
incumbent), and the objective's value beyond its order.

### Where the model differs from the Rust, and what covers the gap

* **Numbers.** Margins are exact `Int`s in the model; Rust uses `f64`. On
  finite floats the only operations the semantics uses are `0 ≤ m` and
  `a < b`, and finite IEEE doubles are totally ordered under both, so the
  integer model is faithful for every finite margin (scale to a fine enough
  unit). Rounding happens before a margin reaches the report and is out of
  scope here.
* **NaN and infinity.** The model has no NaN inside the report, because
  Rust's `Report::margin` converts any non-finite value to `Missing` before
  `put` sees it (`Value.nonfinite` models exactly that conversion;
  `record_nonfinite_fails`). Inside `put`, `margin()` and `passed()` also
  test `is_finite`, so even a hand-built `Outcome` with a NaN margin fails.
  Covered in Rust by `missing_evidence_and_nan_fail`.
* **`CheckId::ALL` completeness.** The Lean class *requires* a proof that
  `all` lists every value. Rust cannot express that; it is a per-domain unit
  test. This matters: if a domain's `ALL` omits a variant, `finish` silently
  drops whatever was recorded for it, including a failure, and if `ALL`
  lists a variant twice, `finish` panics (`remove(..).unwrap()` on the second
  occurrence). Every domain must keep the "ALL lists every variant" test.
* **Not modelled:** names, units, details, `Source`, `Finished::violation`
  (a search heuristic, not a pass/fail rule), `Display`.

## Haskell: the independent oracle

`haskell/src/Oracle/` (base only; QuickCheck for tests), with the sources
in each module header:

| Module | What | Primary sources |
|---|---|---|
| `Atmosphere` | US76, 0-84.852 km geopotential: T, p, rho, mu, a | NOAA-S/T 76-1562 (US76) eqs. 18, 23, 33a/b, 42, 50, 51, Table 4; base pressures from ICAO Doc 7488/3 Table D as transcribed in `ambiance` (github.com/airinnova/ambiance, `src/ambiance/ambiance.py` `LAYER_SPEC_PROP`) |
| `Nozzle` | p/p0, T/T0, A/A* from Mach; Mach from A/A* (both branches) and from p/p0; Cf; c*; u_e | NACA Report 1135 eqs. 43, 44, 80 and Table I; Sutton & Biblarz, *Rocket Propulsion Elements* 9th ed. eqs. 3-16, 3-30, 3-32 |
| `Balloon` | gross/neck/free lift, steady ascent rate, fill for a target ascent rate, burst altitude | CUSF burst calculator (github.com/adamgreig/cusf-burst-calc, `js/calc.js` `calc_update`, defaults from `index.html`) restated from Archimedes + drag balance |
| `Beam` (2026-10-24 night) | hollow-circle I and A; cantilever tip P L³/3EI; M c/I; first bending mode (β₁L = 1.8751); Lame hoop/axial/radial/von Mises at the bore; circular plates, simply supported and clamped; classical axial shell buckling; SP-8007 bending knock-down; fin flutter; bulkhead thickness | Roark, *Formulas for Stress and Strain*, Table 8.1 case 1a, Table 11.2 cases 10a/10b, Table 13.5 case 1c; Blevins, *Formulas for Natural Frequency and Mode Shape*, Table 8-1; Timoshenko & Gere, *Theory of Elastic Stability*, eq. 11-13; NASA SP-8007 (1968) eq. 6; NACA TN 4197 with the constant as corrected in Apogee Peak of Flight #615 (the same source the Rust cites: a transcription check) |
| `Radio` (2026-10-24 night) | free-space loss; slant range, elevation and 4/3-Earth radio horizon over a spherical Earth; half-wave dipole gain toward an elevation | Friis / ITU-R P.525 eq. 4; law of cosines, with the elevation by the tangent of the ray's vertical and horizontal components where the Rust takes an arcsine over the slant range; Balanis, *Antenna Theory*, eq. 4-62 |
| `Radio`, GNSS coexistence (2026-10-24 late) | Friis held at the radian sphere λ/2π (6.02 dB) inside it; single-resonator patch rejection 10 log(1 + Q²(f/f₀ − f₀/f)²); kTB + NF floor; blocking limit P1dB − back-off; in-band limit floor + (signal − reacquisition); harmonic orders landing in L1; coupled power P_tx + G − FSPL | Pozar, *Microwave Engineering*, 6.1 (series RLC response); k T at 290 K = −173.98 dBm/Hz from the SI Boltzmann constant, rounded to the engineering −174 as the Rust does; the receiver constants (MAX-M10S data sheet, Infineon BGA524N6 P1dB, IS-GPS-200 L1) are the design's, passed in as case inputs with those defaults |
| `Thermal` (2026-10-24 late) | lumped-capacitance box: forward-Euler step, its closed form T_ss + (T₀ − T_ss)(1 − dt/RC)ⁿ, steady state; wall conduction + outer film in series with the geometric-mean wall area; film coefficient ∝ √ρ; iron hand-warmer power from the oxidation enthalpy; cell capacity vs temperature by table | Incropera & DeWitt, *Fundamentals of Heat and Mass Transfer*, 5.1-5.2, 3.1 eq. 3.19, 7.2 eq. 7.30; NIST-JANAF ΔHf(Fe₂O₃) −824.2 kJ/mol over 2 × 55.845 g; the packet mass and fractions (Red Rocks CC 2023, Kobayashi SDS) and the capacity points (LP503759, HTCNR18650, Energizer L91 data sheets) are the design's |
| `Aero` (2026-10-24 late) | von Karman (Haack C = 0) nose radius; blunt-face stagnation CD; TR-R-100 fineness-3 nose pressure drag scaled to another fineness; chute terminal descent; bulkhead closure screw capacity by mode and screw count | Haack 1941 / Crowell, *The Descriptive Geometry of Nose Cones* (1996); OpenRocket `core/src/net/sf/openrocket/aerodynamics/BarrowmanCalculator.java` `calculateStagnationCD` and `SymmetricComponentCalc.java` `calculateNoseInterpolator` (the Rust cites the same methods: transcription check); NASA TR-R-100 as OpenRocket tabulates it; Knacke, *Parachute Recovery Systems Design Manual*, eq. 5-1; Bruhn D1.6 and MMPDS e/D = 2 for bearing and tear-out, 1/4-20 UNC root area 0.0269 in² (Machinery's Handbook), A574 180 ksi |

Independence by method, not just by language: the oracle's atmosphere uses
the *published* layer base pressures, where wright-balloon and wright-rocket
integrate them upward from sea level, so an error in either the law or the
table would show. The balloon fill is solved by bisection on the monotone
residual, where Rust (and calc.js) use Cardano's formula.

**Tests** (`cabal test`): 13 US76 Table I rows from 0 to 50.4 km geometric
(T within 0.002 K; p, rho, mu within 1e-4 relative; rows from ambiance's
`tests/table_data.py`), 5 NACA 1135 Table I rows (gamma 1.4, M 0.5-3; p/p0
within 5e-4, A/A* within 1e-4), and 14 QuickCheck properties, 100 cases
each, all passing: `prop_layersContinuous` (each published base pressure
equals the layer below integrated to its top, within 2e-5),
`prop_hydrostatic` (dP/dH = -rho g0, 1e-5), `prop_pressureFalls`,
`prop_idealGas`, `prop_areaRoundTrip`, `prop_subsonicRoundTrip`,
`prop_throatIsMinimum`, `prop_cfIsUeOverCstar` (Cf from eq. 3-30 equals
u_e/c* from eqs. 3-16 and 3-32 to 1e-12), `prop_cfMaxAtMatched` (Cf is
largest at pe = pa), `prop_pressureMach`, `prop_fillRoundTrip`,
`prop_freeLiftIsDrag`, `prop_burstAtBurstVolume`, `prop_cliMatches`.

Added 2026-10-24 night, all passing: **13 table rows** that reproduce the
closed-form column of the FEA validation table below from the formulas
alone (cantilever tube 19.202 mm, 83.35 MPa, 80.05 Hz; Lame 50.01 and
24.50 MPa; clamped plate 0.1940 mm and 19.95 MPa; simply supported
0.7775 mm and 49.95 MPa; axial buckling 264.8 kN; all within 1e-3, with
6061-T6 at 2700 kg/m³ and "R 10 mm" read as the mid-wall radius, od 21 /
id 19 mm, which is what gives 19.202) and textbook free-space loss
(92.45 dB at 1 km and 1 GHz, 125.18 dB at 100 km and 433 MHz, 40.05 dB at
1 m and 2.4 GHz, within 2e-4); and **13 QuickCheck properties** (27 in
all): `prop_lameOrdering` (hoop > axial > 0 > radial),
`prop_lameVonMisesBounds` ((√3/2)(σ₁−σ₃) ≤ σ_vm ≤ σ₁−σ₃),
`prop_lameThinWall` (Lame over the thin-wall p R_m/t is exactly
1 + (t/2R_m)²), `prop_cantileverCubic` (δ ∝ L³, σ ∝ M), `prop_plateRatio`
(simply supported over clamped centre deflection is (5+ν)/(1+ν)),
`prop_bulkheadInverts` (the bulkhead thickness puts the Roark 10a stress
at sf × p on the allowable), `prop_knockdown` (0.269 < γ ≤ 1, falling with
R/t), `prop_fsplFriis` (the 32.45 + 20 log d_km + 20 log f_MHz form),
`prop_fsplDoubling` (+20 log 2 dB per doubling), `prop_geometryTriangle`
(the law-of-cosines slant range equals the hypotenuse of the ray's
components and sin el × slant is the vertical one),
`prop_geometrySymmetric`, `prop_horizonMonotone`, `prop_dipolePattern`
(peak broadside, never above peak, symmetric, on the floor along the
axis).

Added 2026-10-24 late (39 table rows and 41 properties in all, all
passing): **8 table rows**: kT at 290 K from the SI Boltzmann constant
(−173.98 dBm/Hz), the von Karman mid-length radius R/√2 exactly, the
TR-R-100 points at fineness 3 returned unchanged (M 1.4, M 2.0), the loss
on the radian sphere 20 log 2, the 1/4-20 A574 single-shear capacity from
the published root area, Fe → Fe₂O₃ 7.379 kJ/g, and the HotHands 1.37 W
that rules.rs derives; **14 properties**: `prop_rejectionSymmetric` (zero
at f₀, equal at f₀k and f₀/k, growing with detuning),
`prop_harmonicsInBand` (every order found is in band and none in band is
missed), `prop_coupledClamp` (coupled power ≤ P_tx + G − 6.02 dB and falling
with separation beyond the radian sphere), `prop_inbandLimit` (the limit is
exactly the reacquisition headroom above the floor; the floor is kTB + NF
within the 0.03 dB rounding), `prop_boxClosedForm` (the stepped Euler
model equals its closed form after 0-400 steps), `prop_boxSteady` (the
steady temperature is a fixed point and a step from either side moves
toward it without crossing when dt < RC), `prop_resistanceAdds`,
`prop_capacityMonotone` (fractions in [0, 1], non-decreasing in T, exact at
the table points), `prop_warmerInverse` (P ∝ 1/t_rated),
`prop_vonKarmanShape` (0 at the tip, R at the base, monotone, never over R),
`prop_stagnation` (the two brackets meet at M 1 within 1 %: 1.275 vs 1.281,
different approximations; the supersonic branch rises toward 0.85 × 1.84),
`prop_noseCd` (fineness 3 returns the data, slimmer is lower, zero below
M 0.9), `prop_descentDrag` (drag = weight at the terminal rate),
`prop_closureScrews` (even, ≥ 6, enough in bearing at 1.5 MEOP).

**CLI** (`wright-oracle`): a JSON case or array on stdin, the same with
outputs added on stdout. Functions: `us76`, `us76_geopotential`,
`nozzle_mach`, `nozzle_area`, `nozzle_cf`, `nozzle_cstar`, `balloon`,
`balloon_fill`, `tube_bending`, `tube_buckling`, `lame`, `flutter`,
`bulkhead`, `fspl`, `radio_geometry`, `dipole_gain`, `patch_rejection`,
`gnss_limits`, `gnss_harmonics`, `gnss_coexist`, `warmer_power`,
`capacity_fraction`, `box_thermal` (oracle only), `von_karman`,
`stagnation_cd`, `nose_cd`, `descent_rate`, `closure_capacity`,
`closure_screws` (inputs listed in `src/Oracle/Cases.hs`).

### Cross-check results (`haskell/crosscheck.sh`, 2026-10-24)

842 cases, 2125 comparisons, 0 over tolerance (433 cases until the
evening of 2026-10-24; the structures and radio rows were added that
night, 537 cases; the GNSS, thermal, rocket-aero and closure rows late
that night). The Rust side is `haskell/crosscheck-rs`, a standalone probe crate
(its own `[workspace]`, read-only path dependencies on the crates) that
answers the same JSON with the crates' public functions.

| Rust function | vs oracle | max relative difference |
|---|---|---|
| `wright_rocket::atmosphere::at`, 0-50 km every 500 m | `us76` | p 2.3e-6, rho 2.7e-6, a 3.5e-7, T and mu 0 |
| `wright_balloon::atmosphere::at`, 0-50 km | `us76` | p and rho 5.4e-6, T and mu < 1e-15 |
| `wright_air::atmosphere::isa`, 0-11 km geometric (after the fix below) | `us76` | p 1.1e-6, rho 7.0e-7, mu 4.6e-5 (different Sutherland form), T 0 |
| same, before the fix | `us76` | p 3.0e-3, rho 2.4e-3, T 5.7e-4 at 11 km |
| `wright_rocket::ideal::{area_ratio_from_mach, pressure_ratio_from_mach, supersonic_mach, exit_pressure_ratio, thrust_coefficient, c_star}` | `nozzle_*` | ≤ 3.3e-15 (round-off) |
| `wright_balloon::physics::{free_lift, ascent_rate, cusf_burst_altitude}` | `balloon` | ≤ 3.3e-15 |
| `wright_balloon::physics::cusf_fill_for_ascent` | `balloon_fill` (bisection) | volume 4.7e-12, neck lift 8.2e-12 |
| `wright_air::perf::bending_stress` on a catalog `Tube` (od 12-30 mm) | `tube_bending` (M c / I) | 7.9e-16 |
| `wright_air::perf::shell_buckling` (SP-8007 bending, nu 0.3) | `tube_buckling` | stress, knock-down and R/t all 0 |
| `wright_rocket::domain::lame_von_mises` (tank walls, 1-6 MPa) | `lame` | hoop, axial, radial, von Mises all 0 |
| `wright_rocket::domain::flutter_speed` (TN 4197, 3 fins x 3 pressures x 2 speeds of sound) | `flutter` | 3.7e-16 |
| `wright_rocket::vehicle::bulkhead_thickness` (Roark 10a) | `bulkhead` | 0 |
| `wright_balloon::radio::fspl_db` (1 m - 200 km, 144-2400 MHz) | `fspl` | 3.1e-16 |
| `wright_balloon::radio::geometry` (1-400 km, payload 0.1-30 km) | `radio_geometry` (elevation by arctangent, Rust by arcsine) | slant 0, horizon 0, elevation 4.5e-9 |
| `wright_balloon::radio::dipole_gain_dbi` | `dipole_gain` | 3.8e-16 |
| `wright_balloon::radio::patch_rejection_db` (144-5800 MHz) | `patch_rejection` | 2.9e-16 |
| `wright_balloon::radio::{gnss_blocking_limit_dbm, gnss_inband_limit_dbm}` | `gnss_limits` | 1.8e-16 |
| `wright_balloon::radio::harmonics_in_l1` (count and lowest order, 15 frequencies incl. L1/n) | `gnss_harmonics` | exact |
| `tx + gain - fspl_db(sep) - patch_rejection_db` and `- 60` as `analysis.rs` composes a `Coexist` row (separations 20 mm - 0.5 m, so the radian-sphere clamp is exercised at 144 MHz) | `gnss_coexist` | 7.8e-15 |
| `wright_balloon::rules::warmer_power_sl_w` (8-18 h) | `warmer_power` | 0 |
| `wright_balloon::analysis::capacity_fraction` (3 chemistries x 13 temperatures incl. the table points and the -20 C step) | `capacity_fraction` | 0 |
| `wright_rocket::aero::von_karman_radius` (7 stations x 3 noses) | `von_karman` | 0 |
| `wright_rocket::aero::stagnation_cd` (M 0.3-3.5) | `stagnation_cd` | 2.1e-16 |
| `wright_rocket::aero::von_karman_pressure_cd` (M 0.3-3.5 x fineness 3-6) | `nose_cd` | 0 |
| `wright_rocket::domain::descent_rate` (2-25 kg, 18-120 in chutes) | `descent_rate` | 1.3e-16 |
| `wright_rocket::vehicle::closure_capacity` (walls 2-6.35 mm) | `closure_capacity` | 0 |
| `wright_rocket::vehicle::closure_screws` (3 bores x 3 MEOPs x 4 walls) | `closure_screws` | exact |

The structures rows are transcription checks: the oracle and the Rust
restate the same closed forms from the same books, so agreement to
round-off says both copied them right, and the 13 table rows above say the
oracle's copy reproduces the published values. The independent evidence
on those formulas is the FEA below (CalculiX within 1-4 %), not the oracle.

### Findings

* **wright-air's ISA took geopotential altitude (fixed).** `isa(h)` applied
  the troposphere law to `h` directly: 0.3 % low in pressure at 11 km for a
  caller passing geometric height (under 0.1 % below 3 km, where a VTOL
  flies). Reported; the wright-air owner now converts z to H (US76 eq. 18)
  and the cross-check agrees to 1e-6.
* **calc.js's target-ascent-rate mode does not solve its own cubic.** In
  cusf-burst-calc `js/calc.js`, the Cardano term `g` is written
  `2b³/a³ - 9bc/a² + (27d/a)/27`: only the last term is divided by 27.
  Run verbatim (node) for a Totex 1000 g, 1000 g payload, 5 m/s target on
  helium, it returns a fill that actually rises at 5.58 m/s (+11.6 %).
  wright-balloon's `cusf_fill_for_ascent` divides the whole bracket, and
  agrees with the oracle's bisection to 1e-11.

### Gaps (said plainly)

* The ideal-nozzle cross-check compares two implementations of the *same*
  closed forms from the same textbook; it catches transcription errors, not
  model error. The equilibrium engine (`wright_rocket::performance`) is not
  cross-checked here; its own `validation_table()` compares it with NASA
  CEA. A useful next case: frozen-flow `rocket()` vs the ideal relations at
  the chamber gamma.
* The balloon cross-check covers the CUSF-style relations (constant
  rho_air 1.205, scale height 7238.3 m). wright-balloon's flight check
  (`flight::simulate`, ascent integrated through US76 with the gas
  expanding) is not compared.
* The box thermal model: `analysis::box_step(heat_cap, r_th, p, t_amb, dt, t)`
  is now the one Euler step `ThermalModel::case` uses, and the cross-check
  iterates it against `Oracle.Thermal.boxStep` (27 `box_thermal` rows, 3 box
  sizes x 3 power/ambient pairs x 3 step settings, tolerance 1e-9; 2152
  comparisons in all, 0 over tolerance). The oracle's closed-form and
  steady-state outputs stay oracle-only properties.
* The GNSS rows are transcription checks: both sides use the design's
  receiver constants and the same single-resonator patch model, and the
  "harmonics 60 dB under the carrier" figure is an assumption in
  `analysis.rs`, not a measured spectrum.
* The oracle uses double precision; tolerances are stated per row above.
* Vendored Haskell dependencies (QuickCheck 2.15.0.1, random 1.2.1.3,
  splitmix 0.1.1) come from Hackage over HTTPS; tarball SHA-256s match
  commercialhaskell/all-cabal-hashes (a3b2216d..., e9c81926..., d678c41a...).

## PCB: `Oracle.Pcb` and `Board.lean` (electronics Lane F, 2026-10-24)

Neither the Haskell oracle nor the Lean proofs verify the Rust code itself.
`lean/Wright/Board.lean` proves properties of a model of the electronics
acceptance rule (an unrouted attempted net, an unattempted net with two or
more pads, an undeclared floating pin, or an ERC/DRC/parity run that did
not happen or reported errors each mean "not accepted"; marking another net
unrouted never helps; with full coverage `completion = 1` iff nothing is
unrouted, and an empty `attempted` is never complete). The Rust `check` in
`wright-electronics` must implement that rule; its tests say whether it
does. `haskell/src/Oracle/Pcb.hs` recomputes the board geometry a second
way from the `(netlist, board, route)` serde JSON, and `haskell/crosscheck.sh`
compares it with the Rust under these tolerances: positions and extents
exact (integer nm), connectivity and counts exact, copper distance +-1 nm,
IPC-2221 current 1e-9 relative.

Cross-check on 7 SYNTHETIC boards (the real netlists of
`evals/electronics/fixtures`, with placement and copper made up by
`wright-crosscheck gen-pcb`; not a real routed board):

| Rust | vs oracle | comparisons | max difference |
|---|---|---|---|
| `Placed::pad_abs`, `Placed::pad_bbox_abs` | `pcb_pads` | 1326 | 0 |
| `Placed::courtyard_abs`, `Rect::overlaps` (all pairs; one forced overlap on `ldo`) | `pcb_courtyard` | 279 | 0 |
| `RouteReport.routed` / `.unrouted` (the router's claim) | `pcb_connectivity` (copper components) | 60 | 0 |
| `wright_electronics::copper::{min_gap_nm, clearance_violations}` (lane E) | `pcb_clearance` (+-1 nm) | 14 | 0 |
| `wright_electronics::copper::{narrowest_tracks, ampacity_at}` | `pcb_ampacity` | 94 | 0 |
| `wright_electronics::checks::regulator_vout` (servo board only) | `pcb_regulator` | 3 | 0 |

Pad layers (2026-10-24, after the first real servo board's DRC found a
+6V B.Cu track shorted to U1's B.Cu PowerPAD): the oracle uses
`wright_eda::Pad.layers` when it is non-empty (footprint frame, flipped
for a bottom-side part), else the old rule (SMD on the part's side, plated
hole on both). The Rust (`copper::source_pad_layers`) reads the layers from
the footprint's `.kicad_mod` text instead, so until lane A serialises
`layers` the synthetic boards above do not exercise a back-side pad on
either side; `cabal test` covers it directly. Re-run on lane C/D boards
once they exist.

## Toolchains (installed 2026-10-24)

* **Lean**: elan 4.2.4 from the GitHub release tarball, SHA-256 checked
  against the release's published asset digest; toolchain
  leanprover/lean4:v4.35.0. To save disk the toolchain's own oleans and
  static libraries for `Lean`, `Lake` and `Std` were deleted (only `Init` is
  imported here; `lakefile.toml` needs no `Lake` oleans): 2.8 GB -> 841 MB.
  `elan toolchain uninstall leanprover/lean4:v4.35.0 && elan toolchain
  install leanprover/lean4:v4.35.0` restores the full toolchain.
* **Haskell**: ghcup 0.2.6.2 binary checked against downloads.haskell.org
  SHA256SUMS (GPG signature not checked: no gpg on this machine); GHC
  9.10.3 and cabal 3.16.1.0 ("recommended"), whose tarballs ghcup checks
  against its metadata hashes. No HLS, no stack. GHC's HTML docs and
  profiling libraries were deleted: ~2.4 GB -> 1.0 GB. No Hackage index
  was downloaded (`active-repositories: :none`).

## Update (main, after this pass)

The gap noted above -- a check recorded but missing from a domain's `ALL` was
silently dropped by `Report::finish`, and a duplicate in `ALL` panicked -- is
closed in the Rust: `finish` now keeps any undeclared outcome (so a failure
still fails) and takes each declared id once
(`check::tests::a_check_missing_from_all_still_counts_and_duplicates_do_not_panic`).
The Lean model still assumes `ALL` is complete; the Rust behaviour is strictly
more conservative than the model, so the proved properties still hold.

## FEA (crates/wright-fea): CalculiX against closed form (2026-10-24)

`wright-fea validate DIR` (or `cargo test -p wright-fea -- --ignored`)
runs five closed-form cases through the same pipeline the part cases use:
Rust-written decks, gmsh solid meshes, CalculiX 2.23 (conda-forge), the
frd/dat readers. Every row is within 3 %. 6061-T6 (E 68.9 GPa, nu 0.33).

| case | quantity | FEA | closed form | error | reference | mesh |
|---|---|---|---|---|---|---|
| cantilever tube (L 0.5 m, R 10 mm, t 1 mm, 100 N) | tip deflection, mm | 19.234 | 19.202 | +0.17 % | Euler-Bernoulli P L^3 / 3EI | 2400 S8R |
| | bending stress at mid-span, MPa | 83.82 | 83.35 | +0.57 % | M c / I | 2400 S8R |
| | first bending mode, Hz | 79.90 | 80.05 | -0.19 % | (1.8751^2 / 2 pi) sqrt(EI / m L^4) | 2400 S8R |
| closed-end cylinder (R 100 mm, t 2 mm, 1 MPa) | hoop at the bore, MPa | 51.14 | 50.01 | +2.28 % | Lame thick-wall | 2880 S8R |
| | axial, MPa | 24.50 | 24.50 | -0.00 % | p ri^2 / (ro^2 - ri^2) | 2880 S8R |
| clamped plate (a 100 mm, t 5 mm, 0.1 MPa) | centre deflection, mm | 0.1953 | 0.1940 | +0.66 % | Roark 11.2 case 10b | 5252 C3D20R (gmsh) |
| | centre stress, MPa | 19.93 | 19.95 | -0.10 % | Roark 10b | 5252 C3D20R |
| simply supported plate (same) | centre deflection, mm | 0.7788 | 0.7775 | +0.18 % | Roark 11.2 case 10a | 5252 C3D20R |
| | centre stress, MPa | 49.99 | 49.95 | +0.08 % | Roark 10a | 5252 C3D20R |
| axially compressed cylinder (R 50 mm, t 1 mm, L 120 mm) | buckling load, kN | 257.7 | 264.8 | -2.66 % | E t / (R sqrt(3(1-nu^2))) (Timoshenko & Gere 11-13) | 6144 S8R, *BUCKLE |

The one systematic offset: an S8R shell reads the hoop stress at the bore
about R/t-dependently high (+2.3 % at R/t 50, +4.4 % at R/t 23 on the
rocket tanks), because a shell is not a thick cylinder. Lame is exact for
a tank wall; FEA does not replace it there (see the comparison below).
Runtime on this laptop: 3 min for all five (most of it rendering).

### FEA vs today's closed-form checks, on the eval designs

`wright-fea air evals/vtol/result.json OUT`, `rocket evals/rocket/build/design.json OUT`,
`balloon evals/balloon/build/design.json OUT`; full tables in `OUT/fea/comparison.csv`.

| part, load | closed form (check today) | FEA | FEA / closed | what FEA adds |
|---|---|---|---|---|
| VTOL boom, landing 2.67 g x 1.5 | 343 MPa (M c / I) | 342 MPa peak at the clamp | 1.00 | the whole boom, both clamps, both motors: confirms the cantilever idealisation |
| VTOL boom, full thrust x 1.5 | 154 MPa | 154 MPa | 1.00 | |
| VTOL boom, tail load | 61 MPa | 61 MPa | 1.00 | |
| VTOL boom, first mode | 18.38 Hz (tip mass + 0.24 beam) | 18.31 Hz | 1.00 | the whole boom's modes; the check's formula holds |
| VTOL boom, buckling at landing | none | factor 10.8 x SP-8007 0.89 = 9.6 | | a check that did not exist: local shell buckling of the thin tube |
| joiner, 4 g x 1.5 | 419 MPa at the root | 435 MPa peak (rigid clamp), 389 vs 389 MPa 21 mm out | 1.04 / 1.00 | clamp concentration +4 % |
| main spar, 4 g x 1.5 | 376 MPa | 381 MPa peak, 351 vs 347 MPa 15 mm out | 1.01 | |
| N2O tank wall, proof | hoop 222 MPa (Lame) | hoop 232 (shell, +4.4 %), von Mises 202 | 1.04 / 0.91 | the check compares hoop to yield; von Mises (the yield criterion) is 9 % lower |
| N2O bulkhead, proof | 218 MPa (FD plate) | 219 MPa | 1.00 | thick plate (a/t 4.3): stress unchanged, deflection +5.7 % (shear) |
| fuel bulkhead, proof | 210 MPa | 211 MPa | 1.00 | |
| fin modes | none (TN 4197 uses G, t/c) | 323 / 530 Hz | | the fin's real modes, for a mode-based flutter estimate |
| balloon rack shelf, landing 51 g | none | 7.4 MPa (9.6 at a post edge) vs PETG 20 MPa allowable | | a check that did not exist |

Rocket numbers are from design.json as of 13:20 on 10/24; the rocket worker changed the fuel tank and fins between runs. Reading: on these designs the closed-form numbers are right; FEA confirms
them within 1-4 %, and is conservative only where it idealises a clamp as
rigid. FEA adds three things the formulas do not have: shell buckling of
the boom, the rack shelf at landing, and the fin's modes.

## CFD (crates/wright-cfd): AVL and SU2 against published data (2026-10-24)

Tools (both logged in docs/TODO.md, Downloads log):

* **AVL 3.x through OptVL 2.5.0** (pip wheel, 1.4 MB): vortex-lattice
  lifting surfaces plus slender bodies. Driven by `crates/wright-cfd/py/avl_sweep.py`
  through `optvl/optvl_class.py` `OVLSolver` (`get_total_forces`,
  `get_stab_derivs`, `get_strip_forces`, `get_cp_data`). Used for the VTOL.
* **SU2 v8.5.0** (macOS binary, 54 MB): axisymmetric Euler (Roe/MUSCL,
  implicit). Case setup follows SU2's own `TestCases/axisymmetric_rans/air_nozzle/air_nozzle_restart.cfg`
  (Riemann total-condition inlet, static-pressure outlet, `AXISYMMETRIC= YES`).
  Used for the rocket nose and the nozzle.
* Rejected: OpenVSP/VSPAERO (about 150 MB with the GUI); AeroSandbox (about 400 MB with casadi);
  OpenFOAM (over 1 GB); PyFR (needs a GPU stack). A 2D RANS of the airfoil
  was not added, because NeuralFoil (trained on XFoil, so it captures transition) is better than
  fully turbulent RANS at this Reynolds number.

Run with `wright-cfd validate OUT`. The numbers are in `OUT/cfd/validation.json`.

| case | quantity | reference | Wright CFD | error |
|---|---|---|---|---|
| Warren-12 wing (AVL) | CL_alpha /rad | 2.743 | 2.7481 | +0.19 % |
| Warren-12 wing (AVL) | Cm_alpha /rad, apex | -3.10 | -3.0966 | +0.11 % |
| elliptic wing AR 6 (AVL) | span efficiency e | 1.000 | 0.9979 | -0.21 % |
| elliptic wing AR 10 (AVL) | span efficiency e | 1.000 | 0.9988 | -0.12 % |
| elliptic wing AR 6 (AVL) | CL_alpha /rad | 4.528 Helmbold (4.712 lifting line) | 4.383 | -3.2 % (-7.0 %) |
| elliptic wing AR 10 (AVL) | CL_alpha /rad | 5.151 Helmbold (5.236 lifting line) | 5.047 | -2.0 % (-3.6 %) |
| 15 deg cone M 1.5 (SU2) | surface Cp | 0.2400 Taylor-Maccoll | 0.2397 | -0.14 % |
| 15 deg cone M 2.0 (SU2) | surface Cp | 0.2022 | 0.2019 | -0.18 % |
| 15 deg cone M 3.0 (SU2) | surface Cp | 0.1731 | 0.1724 | -0.39 % |
| von Karman FR 3 nose M 1.1 (SU2) | nose pressure CD | 0.070 NASA TR-R-100 | 0.0866 | +23.7 % |
| von Karman FR 3 nose M 1.2 | | 0.081 | 0.0894 | +10.4 % |
| von Karman FR 3 nose M 1.4 | | 0.095 | 0.0871 | -8.4 % |
| von Karman FR 3 nose M 1.6 | | 0.097 | 0.0845 | -12.9 % |
| von Karman FR 3 nose M 2.0 | | 0.091 | 0.0801 | -12.0 % |
| von Karman FR 3 nose M 3.0 | | 0.083 | 0.0724 | -12.8 % |

Sources for the reference values:

* Warren-12: OpenVSP `examples/scripts/python_scripts/WarrenTest.py` (`m_Cl_alpha_expected = 2.743`, `m_Cm_alpha_expected = -3.10`).
* Taylor-Maccoll: `crates/wright-cfd/src/taylor_maccoll.rs` (Anderson, *Modern Compressible Flow*, sect. 10.4).
* TR-R-100 FR 3 table: as tabulated in OpenRocket `core/src/net/sf/openrocket/aerodynamics/barrowman/SymmetricComponentCalc.java`.

How to read these results:

* **AVL.** Span efficiency and the Warren-12 derivatives agree to within 0.2 %. The low elliptic CL_alpha is the known
  difference between a lifting-surface model and a lifting-line one (Helmbold's
  low-AR correction closes most of it).
* **SU2 cone.** The cone agrees with the exact solution to within 0.4 %.
* **SU2 von Karman nose.** The gap to TR-R-100 is mesh-converged: doubling the mesh (fine 1.6) moves
  M 1.4 from 0.0871 to 0.0870 and M 2.0 from 0.0801 to 0.0801. It is the difference between
  inviscid pressure drag and the free-flight data, which include
  boundary-layer and nose-tip effects. Near M 1 an inviscid solution
  overshoots, as expected.

### Nozzle (SU2, the eval rocket's engine)

The contour is the engine's own: a 1.5 r_t upstream throat arc, a 0.382 r_t
downstream arc, and cones at the design angles. The mesh throat radius is
1.00007 r_t. Frozen ideal gas, with gamma fitted to the equilibrium
pe/pc (1.161). The run converged to RMS density -10 in 490 iterations
(design.json as of 15:30 on 10/24: pc 2.0 MPa, area ratio 2.44).

| quantity | 1D ideal | Wright's engine model (lambda 0.983) | SU2 | SU2 / model |
|---|---|---|---|---|
| Cf vacuum | 1.5163 | 1.4944 | 1.4875 | -0.46 % |
| Cf at design ambient | | 1.2646 | 1.2575 | -0.56 % |
| discharge coefficient | 1 | 1 | 0.995 | |

Reading: the conical-divergence lambda and the 1D Cf in wright-rocket are
confirmed to within half a percent. The 2D loss is 1.9 % of ideal, against
1.7 % in the model. An earlier engine (pc 2.25 MPa, area ratio 2.66) gave
-0.34 %.

### CFD vs the analytic models, on the eval designs

VTOL: `wright-cfd vtol evals/vtol/result.json OUT` at the best-range speed
(18.3 m/s, trim CL 0.805).

| quantity | AVL | wright-air today |
|---|---|---|
| wing CL_alpha /rad | 4.174 | 4.497 (+7.7 %) |
| span efficiency (wing) | 0.997 | Oswald e 0.803 (Nita-Scholz) |
| CDi at trim | 0.0362 | 0.0468 |
| induced drag | 1.87 N | 2.41 N |
| neutral point | 89.8 mm | 85.3 mm |
| static margin | 12.1 % MAC | 10.0 % MAC |

Reading: Nita-Scholz e already contains the viscous lift-dependent drag
(their k_e,D0 term). wright-air also adds section profile drag at the strip
alpha, so that part is likely counted twice. The analytic model is
conservative by about 0.5 N, which is about 9 % of cruise drag. AVL puts the neutral point 4.5 mm further aft.

Rocket: `wright-cfd rocket evals/rocket/build/design.json OUT`, run on design.json as of 15:30 on 10/24
(von Karman nose, fineness 5; peak Mach 1.13). Nose pressure CD is on the
body reference area. wright-rocket now uses OpenRocket's fineness scaling of
the TR-R-100 FR 3 table (`aero::von_karman_pressure_cd`); the FR 3 table as-is
is shown for the record. Total CD is the wright-rocket build-up with its nose
term swapped for SU2's.

| Mach | FR 3 table as-is | wright-rocket nose term (OpenRocket fineness scaling) | SU2 (this nose) | total CD today | total CD with SU2 nose |
|---|---|---|---|---|---|
| 0.80 | 0 | 0 | 0.0016 | 0.6214 | 0.6230 |
| 0.95 | 0.0100 | 0.0026 | -0.0001 | 0.6607 | 0.6581 |
| 1.00 | 0.0270 | 0.0092 | 0.0096 | 0.6664 | 0.6668 |
| 1.05 | 0.0550 | 0.0228 | 0.0350 | 0.6588 | 0.6710 |
| 1.10 | 0.0700 | 0.0309 | 0.0358 | 0.6467 | 0.6516 |
| 1.20 | 0.0810 | 0.0369 | 0.0354 | 0.6239 | 0.6225 |
| 1.40 | 0.0950 | 0.0444 | 0.0345 | 0.5810 | 0.5710 |
| 1.60 | 0.0970 | 0.0451 | 0.0336 | 0.5381 | 0.5267 |
| 2.00 | 0.0910 | 0.0408 | 0.0323 | 0.4618 | 0.4533 |

Reading:

* Before the rocket worker's fix, wright-rocket applied the FR 3 table to an
  FR 5 nose, 2-3x too much wave drag above M 1.1 (total CD 3-11 % high).
  With OpenRocket's fineness scaling the build-up sits within 2 % of the
  SU2-informed CD everywhere: 1.8 % low at M 1.05, 0.7 % low at M 1.10, and
  1-2 % high from M 1.4 up.
* The flight peaks at M 1.13, in the band where the build-up is slightly
  optimistic. Scaling every drag term by the CD ratio there (x1.004) moves
  apogee from 5967 m to 5961 m, a 0.1 % change. The `cfd_drag_not_optimistic`
  stage check (limit -2 %) passes.
* Near M 1 SU2's inviscid value is the least trustworthy (see the FR 3
  validation above), so the M 1.05 gap is within the method's own error.
