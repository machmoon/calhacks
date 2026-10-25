# Rocket eval: a Berkeley rocket-club liquid rocket, down to the engine

Run: `cargo run --release -p wright-rocket --example rocket_eval` (about 7 CPU-minutes). It writes
`evals/rocket/result.json` and the builder packet in `evals/rocket/build/`. The 6-DOF flight of
the headline design is in `evals/rocket/build/sim/` (RocketPy; `manifest.json` lists the mp4,
plots and CSV). The propellant sweep is `cargo run --release -p wright-rocket --example
rocket_matrix`. `cargo test -p wright-rocket` runs 38 unit tests, 5 design_under tests and 4
propellant-matrix tests.

**Verdict: PASS.** The comparison is under Eureka-2's own engine and mass.

| | Wright (N2O/RP-1, chosen by the pair search) | Wright on LOX/propane (Eureka-2's pair) | Eureka-2 |
|---|---|---|---|
| FAR-OUT score, same model | **838** | **838** | **588** (its recorded flight) |
| Checks | **26/26** | **26/26** | n/a (reference) |
| Cost (information, no budget) | $10,110 | $14,154 + unpriced 4500 psi nitrogen | $10,673 + unpriced nitrogen |

**No budget (Pat, 2026-10-24: "ignore budget").**
- **Cost:** reported as information on both sides. The `budget` check now measures something only
  when the person's spec states a budget (`budget_usd` is optional, as in wright-air).
- **Without a budget:** the check records the cost and every unpriced line with its reason.
- **With a stated budget:** it is held as before, so an unpriced line fails (unit test).
- **Search order:** least check violation, then the higher FAR-OUT score (within 0.5 points is a
  tie), then the lower BOM (`domain::ranks_before`). This applies to the design search, the pair
  search and design_under alike.

**Pair search at SEB's c* efficiency 0.71 and dry mass × 1.86.** All eight schema pairs pass 26/26.
The scores are effectively tied; the 2-point spread is below what the 9-case dispersion mean
resolves:

| pair | score | BOM |
|---|---|---|
| N2O/propane | 839 | $10,547 |
| N2O/ethanol | 839 | $10,656 |
| N2O/IPA | 838 | $10,613 |
| N2O/RP-1 | 838 | $10,110 |
| LOX/propane | 838 | $14,154 |
| LOX/ethanol | 837 | $14,155 |
| LOX/IPA | 837 | $14,059 |
| LOX/RP-1 | 836 | $12,669 |

So the honest claim is about the requirements, not a propellant choice. Every pair Wright designs
to Eureka-2's requirements scores 836–839 against Eureka-2's 588. That includes Eureka-2's own
LOX/propane at 838: Pc 32.5 bar, 10.4 kN, 4.5 s, 122.7 kg at liftoff, T/W 8.7, 19,383 ft nominal.
The search keeps the least violation, then the higher score, then the lower BOM; with the four N2O
pairs within 0.4 points it picked N2O/RP-1, the cheapest (the previous run, with the FR 3 nose
drag, picked N2O/IPA at 842).

**Headline design: N2O/RP-1.** It runs Pc 20 bar, O/F 7.7, 11.4 kN and 155 s Isp at the site,
and burns 4.3 s for 43.4 kN·s. Liftoff mass is 115.2 kg (82.7 kg dry, 32.5 kg propellant) and the
vehicle is 6.11 m × 157 mm. It leaves the rail at T/W 10.1 and 109 ft/s, and reaches 19,369 ft
nominal (band 17,110–21,816).

**With Wright's a-priori c* efficiency of 0.90** (an unsourced estimate), the same pair scores 835
and weighs 92.4 kg. That is only a sensitivity: the headline uses SEB's demonstrated 0.71.

**Nose drag scaled to the nose's fineness (CFD worker follow-up).** The drag build-up used the
NASA TR-R-100 fineness-3 von Kármán pressure-drag table on our 5:1 noses. OpenRocket re-scales
that table to the nose's own fineness (`SymmetricComponentCalc.calculateNoseInterpolator`, branch
`unstable`, "Extrapolate for fineness ratio": `cd = stag · (cd₃ / stag)^(ln(f+1)/ln 4)`, `stag` its
stagnation CD, `f = length / (2 |r_aft − r_fore|)`); `aero::von_karman_pressure_cd` now does the
same, with a unit test that fineness 3 returns the data. On the 5:1 nose the term drops from
0.070 / 0.081 / 0.095 / 0.091 to 0.031 / 0.037 / 0.044 / 0.041 at M 1.1 / 1.2 / 1.4 / 2.0; SU2
on the same nose gives 0.032–0.036 over M 1.1–2.0 (the cfd stage), so the scaled curve is
within 0.01 of the CFD and still on the high side. A design can also carry a measured table:
`nose_cd_table` in the spec (`[[mach, cd], …]`, the cfd stage's `rocket_su2.json` `nose_points`
mach / cd_pressure) replaces the curve in the build-up, the same way `measured_dry` and
`eta_cstar` replace estimates. Effects of the re-run:
- **Every pair still passes 26/26; scores 842 → 836–839.** Less drag at the transonic peak lets
  the search size a smaller, lower-Pc vehicle (headline 115 kg, 20 bar against 123 kg, 22.5 bar),
  whose 9-case dispersion band is a little wider in score terms. The verdict is unchanged.
- **Eureka-2 reconstruction:** nominal apogee 27,933 → 28,942 ft (+3.6 %; max Mach 1.51 → 1.54),
  because that flight spends longer above Mach 1. The fitted dry-mass factor is 1.870 as before
  (the fit absorbs the change), and its recorded 588 is unchanged.
- **Open:** wright-cfd's `DragRow.cd_with_su2_nose` still subtracts the FR 3 table from
  `cd_wright`, which now holds the scaled term; it should subtract `aero.nose_pressure_cd(m)`
  (CFD worker's file, not changed here).

**Stock (review 5, F4).** A new check, `parts_orderable`, fails any BOM line whose catalog page
marks it out of stock. The search now buys only orderable parts:
- **GN2 regulator:** HK Army Turbine Pro, 350–650 psi, $129.95, in stock. Both Ninja Pro V3
  regulators are sold out, and the 650 psi ceiling caps LOX Pc at 32.5 bar.
- **Actuation second stage:** HK Hostile XLP, 300–800 psi in, 40–150 out, $109.95, stock
  unstated.
- **Main valves:** the ValveMan spring-return cryo ball valve.
- **Second altimeter:** Eggtimer Quasar.
- **Pressurant bottles:** HK HSTL 98/4500.
- **Drogue:** chosen from in-stock rows only.

The 2024 reference is exempt: whether its parts are sold today says nothing about the rocket.

**Tank walls judged on von Mises (FEA worker follow-up).** `tank_burst` and the wall part of
`tank_proof` now use the von Mises stress from Lamé's hoop, axial and radial stresses at the
bore, not hoop alone (`domain::lame_von_mises`; unit test against the closed form
√3·p·ro²/(ro²−ri²)). Hoop stays in the detail text.
- **Headline N2O run tank at 2 × MEOP 63.1 bar:** 268 MPa von Mises against 297 MPa hoop (axial
  142, radial −13). The burst margin rises from 4.5 % to 15.8 %.
- **LOX/propane design's fuel tank:** burst margin 53 %.
- **Unchanged:** proof margins, which the bulkhead and closure set (headline fuel tank: 12 ×
  1/4-20 screws, 10.2 kN against 12.2 kN in bearing, margin 20 %; LOX/propane: 16 screws, 23 %).

**Copy consistency split from physics (review 5, F1).** `apogee_accuracy` is now only the flown
apogee against the spec's contract. The check that the design's recorded contract, competition
and rail match the spec is now its own check, `spec_inputs`, listed in
`domain::CONSISTENCY_CHECKS` for the harness's consistency hook.

## 0. Reference vehicle

The reference is Space Enterprise at Berkeley's **Eureka-2**, powered by the **Lightbulb** engine
(details and sources in `reference.md`):

- **Engine:** LOX/propane, pressure-fed by GN2 through electronic regulators, pintle injector,
  aluminium chamber with a phenolic ablative liner.
- **Published operating point:** 24 bar chamber pressure, 1.14 + 0.49 kg/s of LOX and propane,
  3 kN thrust, 14 s burn.
- **Flight (2024-02-25):** 15,879 ft and Mach 0.88, against a 20,000 ft target.

It is the highest-flying Berkeley liquid rocket that has flown. STAR's Caldera went higher, but it
is a two-stage solid.

Rules applied: FAR-OUT 2026-27 v4.1, the competition at SEB's launch site. They set:

- thrust/weight > 8 (the 2027 rule);
- rail exit speed > 85 ft/s;
- stability ≥ 1.5 cal from rail exit to burnout;
- drogue descent 50–150 ft/s and main descent < 25 ft/s;
- burst safety factor 2 on metal vessels;
- altitude score = 1000 − 2000·|actual/contract − 1|.

The over-stability cap is borrowed from IREC DTEG §10.4.1 (4 cal), because FAR-OUT sets none.

## 1. Thermochemistry vs NASA CEA

The solver minimises Gibbs energy following the RP-1311 equations and CEA's matrix layout and
damping, taken from `source/equilibrium.f90` in github.com/nasa/cea at commit 2f79a64. It uses
NASA-9 polynomials from CEA's own `thermo.inp`.

The reference outputs come from NASA CEA 3.3.4, the official pip package (script:
`data/rocket/cea_reference.py`). That build reproduces the published RP-1311 Example 8 block to
every printed digit. Expansion is shifting equilibrium to 1 atm.

| case | Tc K (Wright / CEA) | M g/mol | γs | c* m/s | Isp 1 atm m/s | Ae/At | worst error |
|---|---|---|---|---|---|---|---|
| LOX/propane 24 bar O/F 2.33 (Lightbulb) | 3307.54 / 3307.54 | 20.238 / 20.239 | 1.1496 / 1.1495 | 1824.50 / 1824.50 | 2637.05 / 2637.06 | 4.191 / 4.191 | <0.01% |
| LOX/propane 30 bar O/F 2.6 | 3451.84 / 3451.83 | 21.356 / 21.356 | 1.1370 / 1.1370 | 1823.18 / 1823.18 | 2729.65 / 2729.66 | 5.184 / 5.184 | <0.01% |
| LOX/ethanol 20 bar O/F 1.3 | 2984.98 / 2984.98 | 20.888 / 20.888 | 1.1619 / 1.1618 | 1697.56 / 1697.56 | 2385.23 / 2385.23 | 3.587 / 3.587 | <0.01% |
| LOX/75% ethanol 25% water 68.9 bar | 3153.90 / 3153.89 | 23.376 / 23.376 | 1.1338 / 1.1338 | 1666.37 / 1666.37 | 2698.53 / 2698.54 | 9.391 / 9.391 | <0.01% |
| LOX/RP-1 68.9 bar O/F 2.4 | 3615.90 / 3616.47 | 22.721 / 22.736 | 1.1437 / 1.1436 | 1805.29 / 1804.89 | 2930.73 / 2930.29 | 9.465 / 9.469 | 0.07% (M) |
| LOX/CH4 30 bar O/F 3.0 | 3373.82 / 3373.82 | 20.265 / 20.265 | 1.1345 / 1.1345 | 1851.62 / 1851.62 | 2772.19 / 2772.20 | 5.181 / 5.181 | <0.01% |
| N2O/ethanol 30 bar O/F 4.0 | 3066.92 / 3066.91 | 24.689 / 24.689 | 1.1679 / 1.1679 | 1579.01 / 1579.01 | 2315.50 / 2315.51 | 4.611 / 4.611 | <0.01% |
| H2/O2 RP-1311 Ex. 8 (published block) | 3383.8 / 3383.845 | n/a | n/a | n/a / 2332.34 | 2967.4 / 2967.386 (pc/pe 10) | 2.35 / 2.350 | <0.3% (test) |

The tests enforce 0.1% on Tc, M, c* and Isp, and 0.2% on γs and Ae/At. The LOX/RP-1 residual is
there because Wright's product list (27 species) is shorter than CEA's.

### 1b. Audit against Cantera (2026-10-24)

Cantera 3.2.0 (pip, `cantera/cantera`) was run on the same nine cases, with the same reactant
enthalpies and every C/H/O/N gas species in its `nasa_gas.yaml` (146 species). Chamber: HP
equilibrium. Expansion: shifting SP. The throat is where the mass flux peaks. Script and output:
`evals/rocket/cantera_bench.py`, `cantera_bench.json`.

| worst error vs NASA CEA 3.3.4, all cases | Tc | M | c* | Isp 1 atm | Ae/At |
|---|---|---|---|---|---|
| Wright (`equilibrium.rs`) | 0.016 % | 0.067 % | 0.022 % | 0.015 % | 0.039 % |
| Cantera 3.2 | 0.114 % | 0.038 % | 0.050 % | 0.034 % | 0.105 % |

Wright matches CEA closer than Cantera does, on every quantity except the RP-1 molar mass. Its
polynomials are CEA's own NASA-9 set; Cantera's `nasa_gas.yaml` uses 7-coefficient fits. Cantera
would also put a Python call inside every search step. So `equilibrium.rs` stays. RocketCEA was
not benchmarked: it has no macOS wheel and needs gfortran, and NASA CEA 3.3.4 itself is already
the reference above. The Haskell oracle (`haskell/`) checks the ideal nozzle and the atmosphere,
not equilibrium, so nothing there changed.

## 2. Reproducing Eureka-2 from published inputs

Inputs used:

- **Engine operating point:** 24 bar, O/F 2.33, 3 kN, 14 s.
- **Body:** 7.7 in diameter, 19.3 ft long.
- **Pressures:** tank setpoints 600/550 psi and injector setpoint 480 psi, from SEB's eReg code.

Everything else comes from Wright's own models: mass build-up, tanks, GN2 bottle, fins sized to a
1.8 cal minimum, and drag. Neither the fins nor the masses are public.

| quantity | Wright | published | error | tolerance | result |
|---|---|---|---|---|---|
| mass flow for 3 kN (a priori c* efficiency 0.90) | 1.286 kg/s | 1.63 kg/s | −21% | 10% | MISS |
| delivered sea-level Isp (a priori) | 237.9 s | 187.7 s (derived) | +27% | 10% | MISS |
| apogee AGL (flying the published 3 kN at 1.63 kg/s) | 28,942 ft | 15,879 ft | +82% | 20% | MISS |
| max Mach | 1.54 | 0.88 | +75% | 15% | MISS |
| liftoff mass | 65.7 kg | 54.4 kg (Eureka-1) | +21% | 25% | ok |

The published thrust and flows imply a c* efficiency of **0.71**, which is very low; flight
engines run at 0.92–0.99. Wright assumes 0.90 a priori and says so. The flight row uses the
published thrust and flows directly.

**Why the flight misses.** A single unknown was fitted to the published apogee, and the other
published number, max Mach, was then checked against it. This tests each explanation; it does
not calibrate the model.

| hypothesis | fitted value | predicted max Mach (published 0.88) |
|---|---|---|
| Eureka-2's dry mass is higher than Wright's 42 kg build-up | **79 kg dry** | **0.93** |
| flight thrust below the stand's 3 kN at the same flows | 2,013 N (126 s Isp) | 1.00 |
| drag higher than the OpenRocket-style buildup | 2.8 × Cd | 1.05 |

Only the heavier vehicle reproduces both published numbers at once. So either Eureka-2 was much
heavier than its predecessor's published 120 lb, or it lost thrust in flight; Eureka-1's published
average thrust was 2.7 kN. Neither number is public, so the reproduction cannot be closed without
SEB's mass or flight data.

The model chain was checked separately:

- **Integrator:** with drag off, burnout speed and apogee match the closed-form rocket equation to
  1% (test `vacuum_burnout_matches_rocket_equation`).
- **Drag:** the drag curve (0.42 at M 0.3, peak 0.55 at M 1.0, 0.46 at M 1.5, with the
  fineness-scaled nose term) is in the range OpenRocket gives for such bodies.

Checks not applied to the 2024 reference, each with its reason:

- **thrust_to_weight:** the T/W > 8 rule is FAR-OUT 2027's, and Eureka-2 flew in 2024.
- **apogee_accuracy:** the reconstruction's +82% miss is the simulation's own error.
- **budget:** $10k is Wright's requirement. SEB's budget is not published.
- **pressurant_margin and the bottle's parts_fit:** Eureka-2's GN2 bottle is not published. The
  88 cu ft bottle is Wright's placeholder.

No other check fails on the reference.

### 2b. The same flights in a 6-DOF simulator (RocketPy 1.13)

`wright_rocket::sim` feeds the design to RocketPy (RocketPy-Team/RocketPy, pip 1.13.0). The
runner is `crates/wright-rocket/sim/rocketpy_sim.py`. The mapping follows RocketPy's liquid-motor
classes as installed:

- the liquid motor's two tanks drain at Wright's flows;
- the vacuum thrust curve is used with `reference_pressure = 0`, so RocketPy subtracts p_a·A_e
  itself;
- dry mass, CG and pitch inertia come from Wright's item list;
- drag is CD(M) from `aero::cd0`;
- the rocket has a von Kármán nose, a trapezoidal fin set and two parachutes;
- the site wind is the same 1/7 power law.

RocketPy computes several things independently: the 6-DOF equations, Barrowman lift and centre of
pressure with its own compressibility treatment, the time-varying inertia of the tanks, Earth
rotation, and the descent.

On the uncalibrated Eureka-2 reconstruction, the nine dispersion cases (output in
`evals/rocket/sim/eureka2_reconstruction/`):

| | RocketPy 6-DOF | trajectory.rs 3-DOF |
|---|---|---|
| nominal apogee (mid wind) | 28,695 ft | 28,942 ft (−0.9 %) |
| max Mach | 1.544 | 1.540 |
| rail exit | 110 ft/s | 111 ft/s |
| stability at rail exit | 1.80 cal | 1.80 cal |
| 9-case apogees | 22,319–35,440 ft | 23,118–35,163 ft (worst −3.5 %) |
| mean FAR-OUT score vs 20,000 ft | 238 | 213 |

Neither model flies the published inputs to the recorded 15,879 ft. Both overshoot by about
80 %. So the miss is in the inputs (mass, thrust, drag), not in the 3-DOF integrator, and RocketPy
is not more faithful on apogee. trajectory.rs stays as the model the checks use. It runs about
1 ms per flight against about 1 s, and the search flies thousands of them.

RocketPy is a cross-check, through the pipeline stage `sim` (`wright_rocket::sim::SimStage`). Its
manifest carries an mp4 of the flight, the trajectory, attitude, stability and dispersion plots, a
CSV, and three stage checks:

- 6-DOF apogee within 5 % of trajectory.rs;
- stability at rail exit;
- rail exit speed.

The headline N2O/RP-1 design (`evals/rocket/build/sim/`, mp4 included):

| | RocketPy | trajectory.rs |
|---|---|---|
| nominal apogee | 19,349 ft | 19,369 ft (−0.1 %) |
| max Mach | 1.14 | 1.14 |
| rail exit | 108 ft/s | 109 ft/s |
| stability at rail exit | 2.48 cal | 2.48 cal |
| 9-case apogees | 16,931–21,968 ft | 17,110–21,816 ft (worst 1.0 %) |
| 9-case mean FAR score | 828 | 838 |

All three stage checks pass. The 10-point score gap is 6-DOF dispersion against 3-DOF.

One real difference turned up. Through Mach 1 to 1.5, Wright's stability margin peaks at about
6 cal (5.9 on the previous run's N2O/IPA design), while RocketPy's peaks at 2.9. Wright blends the subsonic fin lift into supersonic linear theory
(OpenRocket's approach); RocketPy keeps the Diederich form with |1−M²|. No check reads the
transonic peak: the over-stability cap applies at rail exit. It is listed in the gaps.

## 3. Requirements-only design, same constraints (calibrated)

**Pair search** (FAR-OUT 2027 Group B, 20,000 ft, no budget, calibrated, c* efficiency 0.71;
the contraction ratio is fitted to stock within H&H's 2–5). This run has 26 checks with
`parts_orderable` and `spec_inputs`, and orderable parts only:

| pair | checks | FAR-OUT score | BOM (information) |
|---|---|---|---|
| N2O/propane | 26/26 | 839 | $10,547 |
| N2O/ethanol | 26/26 | 839 | $10,656 |
| N2O/IPA | 26/26 | 838 | $10,613 |
| **N2O/RP-1** | **26/26** | **838** | $10,110 |
| LOX/propane | 26/26 | 838 | $14,154 + GN2 quote |
| LOX/ethanol | 26/26 | 837 | $14,155 + GN2 quote |
| LOX/IPA | 26/26 | 837 | $14,059 + GN2 quote |
| LOX/RP-1 | 26/26 | 836 | $12,669 + GN2 quote |

N2O/ethanol's earlier parts_fit failure (141.8 mm liner blank) is gone. The Turbine Pro's 650 psi
ceiling moved the search to a lower-Pc N2O point whose liner fits the 5 in rod.

**On Eureka-2's own pair, LOX/propane:**


The spec contains only:

- **Competition:** FAR-OUT 2027, Group B.
- **Contract apogee:** 20,000 ft AGL.
- **Propellants:** LOX/propane.
- **Budget:** none stated (cost is information).

The design and checks use dry mass × 1.86.

The search:

1. Sweeps chamber pressure from 20 to 37.5 bar, with the nozzle exit at 1, 1.25, 1.5 or 2 × site
   pressure. An under-expanded exit keeps the liner within the largest priced phenolic rod.
2. Sets O/F for maximum Isp.
3. Sizes thrust for T/W and rail exit.
4. Bisects burn time to the contract apogee.
5. Bisects fin size to 1.8 cal, then takes the thinnest G10 sheet that clears flutter.
6. Takes the cheapest bottles that pass both the blowdown and the closed-form requirement.
7. Picks chutes.
8. Re-bisects burn time to the contract apogee.
9. Picks engine stock: the smallest priced phenolic rod and 6061 round that fit.
10. Keeps the least violation, then the higher FAR-OUT score, then the lowest BOM.

Table from the previous run (24 checks, $10k budget, Ninja regulator up to 750 psi). The current
LOX/propane numbers at 0.71 are in the summary at the top.

| | Eureka-2 (reconstructed, uncalibrated) | **Wright, calibrated** | calibrated, at SEB's c* efficiency 0.71 | Wright, uncalibrated |
|---|---|---|---|---|
| checks passed | 19/24 (all applicable pass) | **23/24** (budget) | 23/24 (budget) | 23/24 (budget) |
| Pc / O/F | 24 bar / 2.33 | 32.5 bar / 2.60 | 35 / 2.70 | 30 / 2.60 |
| thrust at site / Isp | 3.03 kN / 189 s | 9.24 kN / 247 s | 10.8 kN / 196 s | 3.68 kN / 248 s |
| burn / total impulse | 14 s / 42.4 kN·s | 4.7 s / 43.7 kN·s | 4.6 s / 49.3 kN·s | 6.0 s / 22.2 kN·s |
| liftoff / dry / propellant | 65.7 / 42.2 / 23.5 kg | 109.6 / 90.9 / 18.6 kg | 128 / 102 / 26 kg | 42.8 / 33.5 / 9.4 kg |
| T/W, rail exit | 4.69, 111 ft/s | 8.60, 101 ft/s | 8.63, 102 ft/s | 8.76, 101 ft/s |
| apogee nominal [band] | 27,933 ft | 19,311 ft [17,063–21,731] | 19,416 ft | 19,938 ft |
| BOM, priced lines (+ GN2 quote) | $10,650 | **$11,970** | $14,061 | $10,198 |
| FAR-OUT score | 588 as flown (= calibrated model) | **839** | 842 | 839 own model, 118 calibrated |

The calibrated design's checks, measured on the vehicle rebuilt and re-flown from its parameters:

- **Flutter:** 532 m/s against 1.2 × 377 m/s, with 3/16 in G10 and the TN 4197 constant.
- **Tank proof:**
  - wall, checked against the catalog tube;
  - bulkhead, checked with a finite-difference plate solution that agrees with Roark to under 1%;
  - closure, 16 × 1/4-20 screws per bulkhead, 10.0 kN against 12.2 kN in bearing.
- **Injector:** stiffness 21% on both circuits; the pintle momentum ratio is 1.71.
- **Start sequence:** the igniter is now an Estes E12-0. Its 2.6 s burn outlasts propellant arrival
  by 0.69 s.
- **Pressurant:** 2.02 kg of usable GN2 against 1.97 kg needed, from eight Ninja 68/4500 bottles
  with the gas at 174 K at burnout. Bottle selection now also requires this closed form, so the
  check cannot fail on a searched design; it still fails pinned and edited ones.
- **Engine stock:** a 4.5 in CE phenolic rod and a 5 in 6061 round.

## 3b. Every propellant pair, both competitions (calibrated, a stated $15k budget, Wright's a-priori c* efficiency 0.90)

`tests/propellant_matrix.rs` requires every pair to pass every check, or to fail only with a named
cause.

| | propane | ethanol | ipa | rp1 (priced as K-1 kerosene) |
|---|---|---|---|---|
| LOX, FAR-OUT | 23/24: GN2 unpriced; priced $11,970 | 23/24, $11,933 | 23/24, $11,827 | 23/24, $11,711 |
| LOX, IREC | 23/24: GN2 unpriced; $10,512 | 23/24, $10,531 | 23/24, $10,479 | 23/24, $10,405 |
| N2O, FAR-OUT | 23/24: liner 118.2 mm > 4.5 in rod | 23/24: 120.0 mm > rod | 23/24: 117.2 mm > rod | **24/24, $9,493** |
| N2O, IREC | **24/24, $8,315** | **24/24, $8,193** | **24/24, $8,366** | **24/24, $8,369** |

- **LOX pairs:** every one fails only because the nitrogen fill is unpriced.
- **N2O at FAR-OUT:** the heavier calibrated N2O engines need a 5 in phenolic rod. The eplastics 5 in
  and 6 in pages return 404, so no price could be read.
- **N2O at IREC:** all four pairs pass everything. Their fuel side and valve actuation use
  compressed air, labelled as such in the BOM; there is no LOX aboard.
- **Methane:** it stays out of the schema. LOX/methane fails budget with the reason, and N2O/methane
  is refused at design time.

**N2O model.** It is unchanged from the previous run, except the triplet check:

- **Properties:** NIST saturation table.
- **Run tank:** equilibrium blowdown.
- **Injector flow:** Dyer two-phase.
- **Flight days:** the design is flown at 5, 20 and 30 °C.
- **Triplet check:** the resultant-angle check now drills the paired N2O holes 0.05 mm over and
  under. On IREC N2O/ethanol the worst case is +1.45°, at 30 °C at ignition, against a 5° limit.

## 4. Gaps (stated, not hidden)

- **The calibration is one flight of one vehicle.**
  - The ×1.86 dry-mass factor (fitted ×1.870 this run) absorbs any thrust or drag error that the
    flight cannot separate from mass.
  - Eureka-1's published 54 kg liftoff suggests it is on the heavy side.
  - Weighing a built vehicle (`measured_dry`) replaces it.
- **Unpriced lines:**
  - nitrogen at 4500 psi (LOX designs);
  - liquid methane;
  - phenolic rod over 4.5 in.
  - The Canada Welding Supply 2000 psi cylinder ($370, counted as USD although the store is
    Canadian, pick-up only) is in the catalog but cannot reach the fill.
- **Stock and part assumptions:**
  - Out-of-stock rows now fail `parts_orderable`, and the search avoids them.
  - The Turbine Pro's inlet rating is not on its page; HK sells it as a tank regulator for its
    4500 psi tanks. It is a paintball regulator, and its flow capacity is not checked.
  - The Hostile XLP's stock is unstated (Evike blocks fetching), and its 800 psi inlet limit
    equals the 800 psi upstream.
  - The 4.5 in rod "may be up to 0.75 in under" its stated size.
- **6-DOF cross-check:** RocketPy agrees with trajectory.rs to within 0.1 % (headline) and
  0.9 % (Eureka-2 reconstruction) on the nominal apogee, and 3.5 % on the worst
  dispersion case. Wright's transonic stability peak (about 6 cal against RocketPy's 2.9 cal) comes from the subsonic-to-supersonic fin-lift blend. RocketPy runs only as
  a post-design stage (`WRIGHT_PYTHON` must point to a Python with rocketpy 1.13).
- **Checks that cannot fail on a searched design:**
  - pressurant_margin (bottle selection enforces it);
  - parts_fit engine stock (selection picks a rod that fits when one exists);
  - the plate term of tank_proof (the FD solution verifies the Roark sizing).
  - All three still fail on pinned or edited designs.
- **N2O model limits:**
  - equilibrium tank, no initial dip;
  - the vapour-phase tail is neglected;
  - the 5–30 °C fill band is a Wright assumption;
  - the ±0.05 mm orifice tolerance is unsourced.
- **Unsourced allowances:**
  - G10 shear 4 GPa (below NBSIR 83-1690's 6.8 GPa for G-11CR);
  - 15 g acceleration limit;
  - pintle momentum band;
  - 10% feed loss;
  - avionics and plumbing masses;
  - LOX at 2 × load and N2O at 1.5 × load;
  - 150 K LOX ullage;
  - regulator dropout at 1.15 ×;
  - A574 grade and 0.6 shear/tensile for the closure screws.
- **Other model limits:**
  - conical nozzle;
  - silica-phenolic liner properties;
  - no throat erosion;
  - planar 3-DOF trajectory (6-DOF RocketPy as a cross-check stage);
  - stiffness is the only stability proxy;
  - regulator flow capacity is not checked.
