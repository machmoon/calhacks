# Reference vehicle: UC Berkeley student liquid rocket

Researched 2026-10-24 from live web sources. Every number below has a URL. "Derived" means
computed here from published numbers, not published by the team.

## 1. UC Berkeley rocketry teams surveyed

| Team | Propulsion | Flown highlights | Sources |
|---|---|---|---|
| **Space Enterprise at Berkeley (SEB)**, founded Oct 2016 | Liquid bipropellant (LOX/propane on Eureka-1/2; N2O/IPA for Eureka-3) plus solid demonstrators (LAD/VLAD/HAD) | Eureka-1 (Dec 3 2022, 11,024 ft, lost); Eureka-2 (Feb 25 2024, 15,879 ft, not recovered); Eureka-3 is in development; spaceshot planned for 2028 | https://berkeleyse.org/ , https://www.berkeleyse.org/about , https://berkeleyse.org/rockets , GitHub org https://github.com/Space-Enterprise-at-Berkeley |
| **STAR (Space Technologies and Rocketry)**, also called CalSTAR (LinkedIn handle `calstar`) | Mainly solids. Liquid engines: ELLIE/LE1 (GOX/ethanol, 6 s test fire, 2022), ALULA/LE2 (first liquid vehicle), LE3 "Diablo" (regen, 2024-26) | ALULA: 6,126 ft in 2023, the first recovered Berkeley liquid, and won FAR's Dollar-Per-Foot contest. Caldera: two-stage solid, 27,326 ft at IREC 2024 (the Berkeley apogee record). CalVistor: 10,023 ft, 4th at IREC 2023. Bear Force One: 1st at FAR 1030 in 2021 | Site https://stars.studentorg.berkeley.edu/ (a JS app; the project data was read from its bundle at https://stars.studentorg.berkeley.edu/assets/index-s6M1jRYE.js), https://crowdfund.berkeley.edu/project/38919 , https://crowdfund.berkeley.edu/project/47409 , https://crowdfund.berkeley.edu/project/27511 |
| Other Berkeley space clubs | Not rocketry vehicle teams: STAC (CubeSat/balloon), SESB, SPACE at Berkeley (Artemis challenges), SpaceForm, NewSpace@Berkeley | none | https://aero.berkeley.edu/clubs/ |

No separate team called "CalSTAR" exists. That name is STAR's social-media handle, per the STAR site bundle
(`linkedin.com/company/calstar`).

## 2. Chosen vehicle: SEB Eureka-2, powered by the Lightbulb engine

**Why Eureka-2:**

- It is the highest-flying UC Berkeley liquid rocket that has actually flown: 15,879 ft, against ALULA's 6,126 ft and Eureka-1's 11,024 ft.
- It is a pressure-fed cryogenic LOX/propane bipropellant with closed-loop electronic regulation. That suits a first-principles reproduction: chemical-equilibrium engine, then a pressure-fed feed system, then the vehicle, then the trajectory.
- Its engine has a peer-reviewed-venue paper (IAC 2023, arXiv 2401.07444) giving chamber pressure, mass flows, thrust and feed pressures. That is rare for student teams.

**Higher-apogee Berkeley vehicles, and why they weren't chosen:**

- Caldera (27,326 ft) is a two-stage *solid*, so it doesn't fit a liquid-engine eval.
- Eureka-3 (Heavybulb: N2O/IPA, regen, Inconel-718, 8 kN design and 11 kN measured) is more ambitious but **has not flown**. Its published numbers are thin.

**Caveat on the airframe:** the only published dimensions and mass are for **Eureka-1**. Eureka-2 reused the
Lightbulb engine but had "lighter aero-structures, and larger propellant tanks"
(https://www.youtube.com/watch?v=jMwTMRFgS2k, description). Treat the Eureka-1 airframe numbers as the
baseline, and Eureka-2's apogee, Mach and burn time as the flight targets to reproduce.

### 2a. Engine: Lightbulb (flew on both Eureka-1 and Eureka-2)

| Quantity | Value | Unit | Source | Confidence |
|---|---|---|---|---|
| Propellants | LOX / liquid propane | - | https://berkeleyse.org/eureka1 ; https://crowdfund.berkeley.edu/project/36887 ; E-2: https://www.youtube.com/watch?v=YwI34dbE7J4 ("Fueled by LOX and Propane") | High |
| Nominal thrust (100% design) | 3 | kN | arXiv 2401.07444 §2.1, https://arxiv.org/abs/2401.07444 | High (test stand, sea level) |
| Average thrust, Eureka-1 | 2.7 | kN | https://berkeleyse.org/eureka1 | High (team site) |
| Thrust, other team phrasings | "700 lb./force" (3.1 kN); "more than 600 pounds"; "over 650lbs" | lbf | https://www.protolabs.com/resources/blog/space-enterprise-berkeley-rocket-to-reach-record-heights-with-3d-printed-parts-from-protolabs/ ; https://crowdfund.berkeley.edu/project/36887 ; https://crowdfund.berkeley.edu/project/30023 | Medium (rounded marketing figures) |
| Chamber pressure | 24 | bar | arXiv 2401.07444 §2.1 | High |
| Oxidizer mass flow at 100% | 1.14 | kg/s | arXiv 2401.07444 §2.1 | High |
| Fuel mass flow at 100% | 0.49 | kg/s | arXiv 2401.07444 §2.1 | High |
| O/F (mass) | **2.33** | - | Derived: 1.14/0.49 | Derived |
| Total mass flow | 1.63 | kg/s | Derived | Derived |
| Delivered Isp, sea level | **~188** | s | Derived: 3000 N / (1.63 kg/s × 9.80665) | Derived (implies modest c* efficiency) |
| Throttle range demonstrated | 3.0 kN down to 2.1 kN (70%) | - | arXiv 2401.07444 §4 ; https://berkeleyse.org/ereg (60-100% throttle test, Nov 2022) | High |
| Max static-fire thrust | 3.0 | kN | https://berkeleyse.org/ereg (Oct 2022 5 s hotfire) | High |
| Combustion temperature | ~2800 | °C | https://berkeleyse.org/lightbulb-ablative-engine | Low (team page, rounded) |
| Cooling | Ablative: aluminium chamber with two layers of phenolic ablative liner; graphite nozzle on Eureka-1 (phenolic nozzles also tested) | - | https://berkeleyse.org/lightbulb-ablative-engine ; https://berkeleyse.org/eureka1 | High |
| Injector | Pintle. LOX runs down the pintle shaft and exits radially through the pintle cap; fuel exits through an annulus after a diffusion plate. Copper pintle cap (steel failed) | - | https://berkeleyse.org/lightbulb-ablative-engine ; https://engineering.berkeley.edu/news/2023/05/a-eureka-1-moment/ | High |
| Ignition | External solid-propellant igniter | - | arXiv 2401.07444 §2.1 | High |
| Test history | 23 hotfires and 2 launches, 2021-2024 | - | https://berkeleyse.org/lightbulb-ablative-engine | High |
| Throat/exit diameter, expansion ratio, L* | **Not public** | - | - | - |

### 2b. Feed system

| Quantity | Value | Unit | Source | Confidence |
|---|---|---|---|---|
| Type | Pressure-fed. A GN2 pressurant tank pressurizes the LOX and propane tanks | - | https://berkeleyse.org/eureka1 ; Protolabs article (above) | High |
| Regulation, Eureka-1 | Two mechanical dome-loaded regulators, one per propellant tank. Max tank pressure was "severely limited and inconsistent" | - | https://berkeleyse.org/eureka1 ; https://berkeleyse.org/ereg | High |
| Regulation, Eureka-2 | In-house electronic regulators (eReg/E-Reg): motor-driven ball valves, PID on ESP32. "Quad E-Reg": 2 on tank pressurant, 2 on injector flow | - | https://berkeleyse.org/eureka-2 ; https://berkeleyse.org/ereg ; arXiv 2401.07444 | High |
| Pressurant | Gaseous nitrogen in a commercial Type 3 COPV (firefighting bottle) | - | arXiv 2401.07444 §2.1 | High (test stand; flight pressurant tank is "composite" per https://berkeleyse.org/eureka1) |
| Pressurant upstream pressure | 310 | bar | arXiv 2401.07444 abstract, §2.2 | High (test stand) |
| Gas-side regulator valve rating | 415 | bar | arXiv 2401.07444 §2.2 | High |
| Injector-side regulator valve MEOP / rating | 42 / 78 | bar | arXiv 2401.07444 §2.2 | High |
| Regulation accuracy | Tanks within ~0.5 bar; injector within ~1 bar | bar | arXiv 2401.07444 | High |
| eReg pressurant flow capacity | 0.39 | kg/s GN2 | arXiv 2401.07444 | High |
| Test-stand eReg setpoints (Feb 2023 code) | LOX tank 600 psi (41.4 bar); fuel tank 550 psi (37.9 bar), "hotfire 7 was 495"; LOX injector 480 psi; fuel injector 460 psi; flow profile LOX injector 440 psi; flowDuration 18 s | psi | https://github.com/Space-Enterprise-at-Berkeley/AutomaticElectronicPressureRegulator (commit 6db6c318, `src/EReg/Config/*.h`, `src/EReg/FlowProfiles.cpp`, `src/EReg/Config.h`) | Medium (test-stand code, not flight config) |
| Propellant tanks | Seamless aluminium tanks (test stand) | - | arXiv 2401.07444 §2.1 | Medium (stand; flight tank material not stated) |
| Flight burn time, Eureka-2 | 14 ("nominal, 14-second burn") | s | https://berkeleyse.org/ereg | High |
| Hotfire durations, Eureka-1 era | 14 s (Feb 2022), plus "full-duration" burns in Apr and Sep 2022 | s | https://berkeleyse.org/eureka1 | High |
| Total impulse, Eureka-2 | **~42** | kN·s | Derived: 3 kN × 14 s (roughly an N-to-O class boundary; actual thrust curve not public) | Derived |
| Propellant mass, Eureka-2 | **~23** | kg | Derived: 1.63 kg/s × 14 s (ignores residuals and start transient) | Derived |
| Flight tank pressures, tank volumes, flight COPV size | **Not public** | - | - | - |

### 2c. Vehicle and flights

| Quantity | Eureka-1 | Eureka-2 | Unit | Source | Confidence |
|---|---|---|---|---|---|
| Launch date | 2022-12-03 | 2024-02-25 | - | https://berkeleyse.org/eureka1 ; https://www.youtube.com/watch?v=jMwTMRFgS2k | High |
| Launch site | FAR site, Mojave | Not stated (FAR most likely, but unconfirmed) | - | https://berkeleyse.org/eureka1 | High / unknown |
| Competition | None (independent flight) | None (independent flight; FAR-OUT runs in late May) | - | team pages mention no competition | High |
| Height | 19.3 ft (5.88 m); also "19 ft", "around 19.5 ft", "20 feet" | not public | ft | https://berkeleyse.org/eureka1 ; Protolabs ; https://crowdfund.berkeley.edu/project/36887 | High (E-1) |
| Diameter | 7.7 in (195.6 mm) | not public (presumably same) | in | https://berkeleyse.org/eureka1 ; Protolabs | High (E-1) |
| Liftoff mass | 120 lb (54.4 kg) "fully fueled" | not public | lb | Protolabs (pre-launch article, Nov 2022) | Medium (pre-flight figure) |
| Dry mass | not public | not public | - | - | - |
| Airframe | Mostly fiberglass; SLS PA12 40% glass-filled nylon parts (plenum, parachute bulkheads) | "lighter aero-structures" | - | Protolabs ; https://www.machinedesign.com/3d-printing-cad/article/21271796/protolabs-eureka-berkeley-space-team-rocket-flies-high-with-3d-printed-components | High |
| Target apogee | 25-30,000 ft (Protolabs, Nov 2022); 40,000 ft (an older crowdfund, 2021-22) | 20,000 ft (Berkeley Engineering, May 2023) | ft | Protolabs ; https://crowdfund.berkeley.edu/project/30023 ; https://engineering.berkeley.edu/news/2023/05/a-eureka-1-moment/ | Medium (targets shifted) |
| Achieved apogee | 11,024 ft (3,360 m) | **15,879 ft (4,840 m)**; also rounded to "16,000 feet" | ft | https://crowdfund.berkeley.edu/project/36887 ; https://www.youtube.com/watch?v=jMwTMRFgS2k ; https://www.berkeleyse.org/about | High. AGL vs MSL is not stated; FAR is about 2,000 ft MSL |
| Max speed | Mach 0.95; "over 700 miles per hour" | **Mach 0.88** | - | https://berkeleyse.org/eureka1 ; https://crowdfund.berkeley.edu/project/36887 ; https://www.youtube.com/watch?v=jMwTMRFgS2k | High |
| Recovery | Drogue + main; lost to an apogee-detection error (faulty deployment) | Drogue anomaly; not recovered | - | https://berkeleyse.org/eureka1 ; https://www.youtube.com/watch?v=jMwTMRFgS2k | High |
| Avionics | Custom FC (PTs, TCs, capacitive fill sensors), IMU, GPS, baro, custom radio with flash "black box" | Custom FC; firmware at https://github.com/Space-Enterprise-at-Berkeley/E2-Firmware | - | https://berkeleyse.org/eureka1 | High |
| Cost | $65-75k development; $8-10k to rebuild from final design | not public | USD | https://engineering.berkeley.edu/news/2023/05/a-eureka-1-moment/ | Medium (quoted by students) |

**Derived sanity numbers (for the eval, not published):**

- Liftoff thrust-to-weight at 54.4 kg and 3 kN is about 5.6. A reproduced Eureka-2 at a similar or slightly higher mass would sit near T/W 5. That is below FAR-OUT 2027's minimum TWR of 8 (see section 4), so this vehicle **would not meet current FAR-OUT rules**. This is a useful check for the eval.
- Propellant mass fraction is roughly 23 / 54 ≈ 0.4. This is rough because the 120 lb figure is for Eureka-1, whose tanks were smaller.

### 2d. Not public (say so plainly)

- Dry mass and propellant load for either Eureka; any Eureka-2 dimensions or mass.
- Nozzle throat and exit diameters, expansion ratio, chamber L*, contraction ratio, and measured c* or Isp (the ~188 s figure is derived).
- Flight tank pressures, tank volumes and materials (the stand used seamless aluminium), and the flight COPV size and pressure.
- Thrust curve and total impulse (derived only), drag model, and fin geometry.
- Parachute sizes and descent rates; rail length used at FAR.
- Whether reported apogees are AGL or MSL.
- An SA Cup or IREC technical report: SEB has not flown at IREC/SA Cup with Eureka. An older crowdfund page (https://crowdfund.berkeley.edu/project/7570) says SEB once competed at SA Cup, but no report was found.

### 2e. Alternative (if the eval wants regen + N2O): Eureka-3 / Heavybulb, unflown

| Quantity | Value | Source |
|---|---|---|
| Propellants | Nitrous oxide / isopropyl alcohol, blowdown feed | https://berkeleyse.org/eureka3 ; https://berkeleyse.org/heavybulb-regen-engine |
| Design thrust | 8 kN (2000 lbf) | https://berkeleyse.org/heavybulb-regen-engine |
| Measured thrust, first static fire 2024-04-20 | 11 kN (2600 lbf) | same |
| Cooling / build | Regenerative (N2O coolant), 3D-printed Inconel-718 | https://berkeleyse.org/eureka3 |
| Goal | Kármán line (100 km); spaceshot planned for 2028; crowdfund says ">3000 lbf" | https://www.berkeleyse.org/about ; https://crowdfund.berkeley.edu/project/36887 |
| Test tanks | Sized for an 8 s full-scale burn | https://berkeleyse.org/eureka3 |

Chamber pressure, O/F, Isp and burn time for Heavybulb are **not public**.

## 3. Competition rules: ESRA IREC (formerly Spaceport America Cup), 2025 edition (latest posted)

Documents (index: https://www.soundingrocket.org/irec-documents--forms.html):

- DTEG: https://www.soundingrocket.org/uploads/9/0/6/4/9064598/2025-irec_dteg_v1.1.6_02-14-25.pdf (2025 V1.1.6, effective 02-14-25)
- Rules & Requirements: https://www.soundingrocket.org/uploads/9/0/6/4/9064598/irec_rules_and_requirements_document_v_1.6_.pdf (2025 v1.6, effective 03/04/2025)

| Rule | Value | Doc / section (page) |
|---|---|---|
| Target apogee categories | 10,000 ft AGL or 30,000 ft AGL (SRAD hybrid **or liquid** has its own 10k and 30k categories); also 30k and 45k two-stage | Rules §2.0 (p.11) |
| Minimum payload | ≥ 4.4 lb (2.0 kg) | Rules §2.0 (p.11) |
| Flight performance scoring | 500 of 1000 pts. Apogee accuracy 350 pts: `Points = 350 − (350 / (0.3·Target))·|Target − Actual|`, scored within ±30%. Recovery 150 pts (pass/fail) | Rules §2.8.1.4 (p.31) |
| Rail departure velocity | Should be ≥ 30 m/s (100 ft/s); never < 15 m/s (50 ft/s), and lower values need analysis | DTEG §10.2.1, §10.2.2.1 (p.33) |
| Min stability | Dynamic stability margin ≥ 1.5 cal from launch to first deployment | DTEG §10.3.1 (p.33) |
| Max stability (over-stable) | Static margin > 4 cal or dynamic > 6 cal is not allowed | DTEG §10.4.1 (p.33) |
| Launch elevation angle | 84° ± 2° (staged 87° ± 2°) | DTEG §10.1.1, §10.1.3 (p.33) |
| Launch rail | ≥ 5.2 m (17 ft), 1515 80/20 aluminium | DTEG §11.1.2 (p.34) |
| Drogue descent rate | 20-40 m/s (65-131 ft/s). Note: not 75-150 ft/s | DTEG §6.3.3 (p.21) |
| Main deployment altitude | ≤ 450 m (1,500 ft) AGL | DTEG §6.4.1 (p.21) |
| Landing descent rate | < 11 m/s (36 ft/s). Note: not 30 ft/s | DTEG §6.4.3 (p.21) |
| Descent-rate reference elevation | 1,400 m (4,600 ft) MSL | DTEG §6.3.2, §6.4.2 (p.20-21) |
| Pressure vessel burst, metallic | ≥ 2 × MEOP | DTEG §6.17.4.1 (p.25) |
| Pressure vessel burst, composite/COPV | ≥ 3 × MEOP | DTEG §6.17.5.1 (p.25) |
| Proof test | 1.5 × MEOP, held ≥ 2 × max system working time, on the flight article | DTEG §6.17.6.1 (p.25-26) |
| Relief device | Required on SRAD vessels, set ≤ proof pressure (combustion chambers exempt) | DTEG §6.17.3 (p.25) |
| Liquid engine definition / propellants | Stored liquid fuel + oxidizer; non-toxic only. LOX, propane, alcohol, kerosene and N2O are listed as non-toxic. Liquids must be static fired | DTEG §5.2.3, §5.4.2 (p.14-15) |
| Liquid pad ops | Remote vent/offload of all propellants; vent/detank in ≤ 30 min | DTEG §5.10.1, §5.18.3 |
| Insurance | Tripoli insurance does NOT cover liquids; teams must provide their own | DTEG p.~50 ; Rules §2.x (insurance) |

## 4. Competition rules: FAR-OUT (Friends of Amateur Rocketry, Mojave), the competition at SEB's launch site

Rules v4.1 for 2026-27, last updated 2026-09-09. Landing page: https://www.faroutlaunch.org/rules ; PDF:
https://static1.squarespace.com/static/64cc93ec537b6a2fa74f96ba/t/6aa1ff516daf985f5ebb7ae4/1789001554847/FAR-OUT_Rules_Requirements_2026-09-09.pdf
(Overview: https://friendsofamateurrocketry.org/far-out-competition/ .) There is no FAR-Mars or FAR 1030 rulebook
online for current years. FAR-OUT is the current FAR collegiate hybrid/liquid competition. FAR 1030 (won by STAR in
2021) is older.

| Rule | Value | Section (PDF page) |
|---|---|---|
| Allowed propulsion | Hybrid or liquid only. Oxidizer must be N2O, oxygen, or air. Fuel can be any liquid that isn't mutagenic, teratogenic, carcinogenic or illegal to release in CA, so LOX/propane qualifies | 5.2.1 (p.32) |
| Groups (contract altitude, AGL) | A: 3,000-15,000 ft; B: 20,000-50,000 ft; C: ≥ 50,000 up to 250,000 ft (the overview page says 5-15k / 20-40k / 50-110k) | 7.3 Altitude Accuracy (p.75) |
| Altitude scoring | `1000 − 2000·|Actual/Contract − 1|`, floored at 0 (points within ±50%) | 7.3 (p.75) |
| Simulation precision | `1000 − 10000·|Actual/Projected − 1|`, floored at 0 (within ±10%) | 7.4 (p.75) |
| Total points | 5,250 (also stated as 5,650 in the same paragraph): motor performance 1000, altitude 1000, sim precision 1000, payload 500, video 500, recovery 500, and others | 7.1 General Scoring (p.74) |
| Rail exit velocity | Must exceed 85 ft/s; design for > 100 ft/s | 5.1.4 (p.30) |
| Thrust-to-weight | FAR-OUT 2027: TWR > 8 minimum (> 10 recommended); 2028+: > 10 | 5.1.4 (p.30) |
| Stability | Static margin ≥ 1.5 cal from rail exit to burnout; damping ratio 0.03-0.5 (0.05-0.3 suggested); no stated max caliber | 5.1.5 (p.30) |
| Launch winds / angle | No launches above 20 mph; design for 30 mph gusts; team launchers 0-5° from vertical | 5.1.5 (p.30); p.47 |
| Pressure vessels, burst | Design burst SF ≥ 2 (welded 2.4); ≥ 4 if pressurized near people (welded 4.8); composite vessels/chambers SF 4 | 5.2.2 (p.33) |
| Hydrostatic proof | ≥ 1.5 × MEOP (2 × if people are present at fill) | 5.7.1 Hydrostatic Testing |
| Relief | Relief device at or below proof pressure on pressure-fed and cryogenic tanks; PRV set at MEOP | 5.2.2.3-5.2.2.4; 7.3 PRV (p.43) |
| Drogue | Two-phase recovery required. Drogue keeps descent at 50-150 ft/s below 30,000 ft, deployed at apogee | 5.6 Recovery (p.53) |
| Main | Descent < 25 ft/s, reached somewhere below 1/3 of target altitude and above 1,000 ft AGL. Main opening above 1/3 of target counts as premature | 5.6 (p.53) |
| Touchdown | ≤ 30 ft/s (≤ 20 ft/s recommended) | 5.6.2.5 (p.~54) |
| Launch rails at FAR | 4× 10 ft 1010; 2× 10 ft 1515; 2× 20 ft 1515; 4× 50 ft 1515; 3× 60 ft T-slot; one adjustable 20 ft tower | 5.4.13 (p.46) |

## 5. Launch site parameters

| Site | Parameter | Value | Source |
|---|---|---|---|
| FAR site, Mojave CA (where Eureka-1 flew) | Elevation | ~2,000 ft MSL (~610 m). FAR-OUT applies "a MSL correction of 2000'" (AGL = MSL − 2 kft) | FAR-OUT rules PDF p.49 (5.5 Electronics) |
| FAR site | Rails | 10-60 ft rails/towers (see above); teams are encouraged to bring their own | FAR-OUT rules 5.4.13 (p.46) |
| Spaceport America, NM (historic IREC/SA Cup) | Elevation for design | 1,400 m (4,600 ft) MSL | IREC DTEG §6.3.2 (p.20) |
| IREC rail | Length | ≥ 5.2 m (17 ft), 84° elevation | IREC DTEG §11.1.2, §10.1.1 |
| IREC 2025 venue | Moved to Midland, Texas | Elevation not checked in this research | https://atomicaggies.nmsu.edu/about-us-drop-down/about-us.html |

## 6. Suggested reproduction targets (summary)

- Engine: LOX/propane, Pc = 24 bar, O/F ≈ 2.33, mdot = 1.63 kg/s, F = 3.0 kN at sea level, pintle injector, ablative chamber. Delivered Isp is about 188 s (derived).
- Feed: GN2 at 310 bar, regulated to tanks at about 38-41 bar (test-stand setpoints, 550-600 psi). Injector-side MEOP is 42 bar.
- Vehicle: 7.7 in diameter, about 19.3 ft long. Liftoff mass about 54 kg (Eureka-1). Burn 14 s.
- Flight to match: apogee 15,879 ft (4,840 m), max Mach 0.88 (Eureka-2). Cross-check against Eureka-1: 11,024 ft, Mach 0.95. The lower Mach at the higher apogee is consistent with a longer, lower-acceleration burn.

## 7. Mass calibration (Wright's default since 2026-10-24)

Wright designs and checks every rocket with its dry-mass build-up × **1.86**
(`rules::EUREKA2_DRY_MASS_FACTOR`; a spec can set `dry_mass_factor`, and a weighed
vehicle's `measured_dry` replaces it).

- **Where it comes from:** `eval::run` flies the Eureka-2 reconstruction (section 6 inputs,
  Wright's build-up 42.4 kg dry) and bisects the dry mass until Wright's trajectory model reaches
  the recorded 15,879 ft. The answer is 79.0 kg, so the factor is 79.0 / 42.4 = 1.86. The eval
  prints the fitted value each run as `mass_calibration_fitted`.
- **Why mass, not thrust or drag:** with the fitted mass, the model also predicts the recorded max
  Mach, 0.93 against 0.88. The thrust hypothesis (×0.67) predicts 1.00, and drag (×2.8) predicts 1.05.
- **Limits:**
  - It rests on one flight of one vehicle. It also absorbs any thrust or drag error that the
    flight data cannot separate from mass.
  - The 120 lb liftoff mass published for Eureka-1 (54 kg) suggests 1.86 is on the heavy side for
    that vehicle family.
  - It is a calibration, not a measurement. Weigh the built vehicle and enter `measured_dry`.
