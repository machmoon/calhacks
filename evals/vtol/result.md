# VTOL eval: Wright vs the 2025 130-mile 3D-printed VTOL

**Verdict: PASS.** Wright's design passes every check (48 checks, 0 failing); range with 15 % reserve x1.137 nominal, x1.132 pessimistic.

Stock policy: `lead_time` (`listed`: bought parts in stock, or listed for sale with stock unstated; made to order, pre-order, backorder and out of stock refused. `lead_time` also accepts made to order / pre-order / backorder). Both aircraft are judged under it.

Run: `cargo run --release -p wright-air --example vtol_eval` (add `-- --build` for CAD, G-code, BOM, params, checklist).

## 1. Reference reproduced from its published parts

Graded against the author's report; the wing chord was never published, so it is swept. Range and endurance are to cutoff at the 19.4 m/s the pilot averaged (130 mi in 3 h).

| root chord | mass kg | hover throttle | endurance at flown speed | range at flown speed |
|---|---|---|---|---|
| 176 mm | 2.560 (+0 %) | 0.50 (+11 %) | 3.14 h (+5 %) | 134.5 mi (+3 %) |
| 220 mm | 2.649 (+4 %) | 0.51 (+13 %) | 2.99 h (-0 %) | 127.9 mi (-2 %) |
| 264 mm | 2.743 (+8 %) | 0.52 (+15 %) | 2.82 h (-6 %) | 120.6 mi (-7 %) |
| **reported** | 2.55 | 0.45 | 3.0 h | 130 mi |

## 2-3. Same model, same constraints, best-range speeds, 15 % reserve

The reference is taken at the swept chord that flies furthest in each model (176.0 mm nominal, 220.0 mm pessimistic), so its unpublished chord can only help it.

| | reference | Wright |
|---|---|---|
| range with reserve, km | 192.5 | 218.9 |
| range to cutoff, mi | 141.7 | 161.2 |
| endurance with reserve, min | 181 | 205 |
| best-range speed, m/s | 18.0 | 18.0 |
| cruise power from the pack, W | 117.3 | 205.0 |
| L/D | 7.92 | 7.27 |
| pusher prop efficiency | 0.670 | 0.638 |
| pusher motor efficiency | 0.814 | 0.859 |
| take-off mass, kg | 2.560 | 4.235 |
| battery energy, Wh | 451 | 902 |
| cost with battery (BOM), USD (information: no budget on either side) | 1998 | 3153 |
| hover throttle | 0.50 | 0.41 |
| predicted hover current per motor, A | 9.2 | 23.5 |
| stall, m/s | 13.8 | 13.8 |
| hover T/W (check: at least 1.8) | 1.96 | 1.97 |
| search allowance: T/W, pessimistic powertrain, less thrust-model error | 1.63 | 1.90 |
| search allowance: stall with the airfoil's measured CLmax error, m/s | 13.8 | 13.8 |
| **pessimistic** range with reserve, km | 152.3 | 172.4 |
| checks failing | 3 (+4 excused, below) | 0 |

Reference failures graded in its favour (not counted):

- `interference`: the pod and joiner shapes are Wright's reconstruction, not the author's parts
- `servo_fit`: the wing chord is unpublished; the servo fails only at the chord Wright swept
- `pusher_prop_mount`: the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop
- `lift_prop_mount`: the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop

## Wright's design

- wing: 1200.0 mm span, 240.0/180.0 mm chords, "sd7062"
- lift: 4 x "brotherhobby_avenger_2806_5_1300kv" with "hqprop_mr_10x4_5_pair", ESC "holybro_tekko32_f4_45a_single_am32"
- pusher: "brotherhobby_avenger_2806_5_1300kv" with "gemfan_flash_7042_2blade", ESC "holybro_tekko32_f4_45a_single_am32"
- battery: "2 x Upgrade Energy GOLD V1 6S2P 22.2Ah AMPRIUS SA08 (6S4P)"
- avionics: "corewing_f405_wing_v2", "hglrc_m100_mini", "radiomaster_rp1_v2", airspeed "holybro_ms4525do", servos "emax_es08ma_ii"
- structure: spar "cst_cs525_1m" + joiner "goodwinds_pct500024", booms "cst_cs525_1m", legs 12.0 mm

Search: 59739 whole-model evaluations in 1453 s.

- battery Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08", count: 3 }: best -1289.1
- battery Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08", count: 2 }: best 218.9
- battery Pack { id: "upgrade_energy_gold_v1_6s1p_11ah_sa08", count: 3 }: best 210.7
- battery Cells { id: "eve_58e", s: 6, p: 8 }: best -1192.3
- battery Cells { id: "tenpower_60xg", s: 6, p: 8 }: best -1273.4
- battery Cells { id: "tenpower_60xg", s: 6, p: 7 }: best -1049.1
- battery Cells { id: "eve_58e", s: 6, p: 7 }: best -1103.0
- battery Cells { id: "tenpower_60xg", s: 6, p: 6 }: best 194.1
- battery Pack { id: "upgrade_energy_gold_v1_6s1p_11ah_sa08", count: 2 }: best 214.3
- battery Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08", count: 1 }: best 215.2
- best: 218.9 km -- brotherhobby_avenger_2806_5_1300kv / hqprop_mr_10x4_5_pair lift, brotherhobby_avenger_2806_5_1300kv / gemfan_flash_7042_2blade pusher, Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08", count: 2 }, sd7062 240/180 mm
- finalist 1 passes every check and keeps the model allowance

## Wright's checks

```
ok   spec_requirements              +0.000        the design carries the spec's requirements unchanged
ok   parts                          +0.000        every part resolved to a sourced catalog row
ok   parts_orderable                -0.000 parts  30 bought lines accepted under stock policy LeadTime; 18 in stock, 12 listed with stock unstated
ok   hover_thrust_to_weight         +0.104        T/W 1.90 (1.97 modelled, less 3.5 % thrust-model error), stopped by the ESC rating at 133 A / 17.6 V from the pack (hot day +15 K, low battery)
ok   hover_throttle                 +0.205        hover throttle 0.41 standard day full pack, 0.44 hot day low pack
ok   lift_motor_current             +0.400 A      23.5 A per motor in hover, 44.0 A at full throttle (ESC firmware limit if set, else none); rated 44 A
ok   lift_esc_current               +1.000 A      23.5 A hover / 44.0 A full throttle under the ESC firmware limit 44 A per motor (AM32 limits.current = 22, https://github.com/am32-firmware/AM32/blob/main/Src/main.c (use_current_limit: 1 kHz current PID on the ESC's own sensor caps duty; setting limits.current x 2 A)); ESC 45 A cont, 45 A burst
ok   pack_current                  +55.249 A      hover 44.8 A from the pack, rated 100 A
ok   pack_peak_current            +100.416 A      worst seconds 124 A (four lift motors flat out under the ESC firmware limit if set, else none; full pack); 2 x Upgrade Energy GOLD V1 6S2P 22.2Ah AMPRIUS SA08 (6S4P) bursts 224 A
ok   voltage_ratings                +0.000 cells  6S pack; tightest lift motor brotherhobby_avenger_2806_5_1300kv 6-6S
ok   pusher_esc_current             +0.600 A      pusher 44.4 A at full throttle, ESC 45 A
ok   cruise                         +0.299        18.0 m/s at 205 W, L/D 7.3, prop 0.64, motor 0.86, throttle 0.60
ok   max_speed                      +0.508        top 32.5 m/s, best range 18.0 m/s
ok   stall_speed                    +0.189 m/s    stall 13.8 m/s EAS at CLmax 1.41 (1.47 modelled, less 4.5 % measured CLmax error for sd7062; 13.8 m/s TAS at the 0 m field), at most 14 EAS
ok   static_margin                  +5.000 %MAC   10.0 % MAC: CG 64 mm, neutral point 85 mm
ok   tail_authority                 +0.571        tail CL +0.09 to trim at the stall; section CLmax 0.83
ok   spar_stress                  +193.593 MPa    main_spar in manoeuvre 4 g x 1.5: 306 MPa of 500 (17.8 N m where the joiner ends, 150 mm out)
ok   spar_joint                     +0.000 mm     sleeve goodwinds_pct500024 on cst_cs525_1m
ok   spar_fit                       +7.257 mm     rear spar: 7.3 mm inside the skin at its tightest, 480 mm out
ok   boom_stress                  +132.410 MPa    boom in landing 2.67 g x 1.5: 368 MPa of 500 (83 N leg load on a 256 mm overhang)
ok   boom_buckling                 +40.584        buckles at 15286 MPa (SP-8007, R/t 4.5, knock-down 0.91) under 368 MPa in landing 2.67 g x 1.5: 41.6 x
ok   boom_resonance                 +0.376        boom mode 47 Hz, hover rotation 145 Hz
ok   landing_legs                   +0.154        buckle at 474 N for 83 N; landing_leg in landing 2.67 g x 1.5: 17 MPa of 20 (83 N axial, 42 N side at the foot)
ok   load_paths                     -0.000 links  6 load paths, every link joined and touching
ok   interference                   +0.000 mm     50 solids, 1225 pairs, none overlapping
ok   attachments                    -0.000 joints 60 joints, all in contact
ok   prop_clearance                +19.000 mm     closest: lift_prop_disc_RR to fin_R at 24.0 mm
ok   containment                    +3.920 mm     tightest battery: 3.9 mm inside the cavity
ok   servo_fit                      +7.134 mm     emax_es08ma_ii in the wing at 302 mm span: 7.6 mm inside the skins
ok   print_bed                     +10.002 mm     tightest wing_R: 240 x 200 x 34 mm piece in [256.0, 256.0, 256.0] less 3 mm margins
ok   ground_clearance               +5.000 mm     lowest non-leg point 20 mm above the feet
ok   thrust_line                   +15.000 mm     pusher -0 mm from the CG's height
ok   prop_mount                     +0.000        gemfan_flash_7042_2blade on brotherhobby_avenger_2806_5_1300kv (threaded shaft)
ok   lift_prop_pairs                +0.000        lift prop rotation Some("pair")
ok   pusher_prop_mount              +0.000        gemfan_flash_7042_2blade on brotherhobby_avenger_2806_5_1300kv: 5 mm bore on a 5 mm shaft/adapter
ok   lift_prop_mount                +0.000        hqprop_mr_10x4_5_pair on brotherhobby_avenger_2806_5_1300kv: 5 mm bore on a 5 mm shaft/adapter
ok   wire_temperature              +55.331 C      lift ESC feed: 10 AWG, 100 C steady at 44.8 A, 145 C after 10 s at 133.5 A
ok   battery_connector             +25.249 A      AS150 70/150 A for 44.8 A hover, 124 A peak
ok   fc_outputs                     +2.000 outputs corewing_f405_wing_v2 has 12 outputs
ok   servo_power                    +9.000 A      servo BEC 14.0 A peak, five servos stall at 5.0 A
ok   ardupilot_params               -0.000 params 54 parameters, all inside ArduPlane's metadata
ok   budget                         +0.000 USD    BOM $3153 over 30 lines; budget none stated
ok   span                           +0.000 mm     span 1200 mm, allowed 1200
ok   range                        +218.935 km     218.9 km with 15 % reserve (259.4 km to cutoff)
ok   endurance                    +205.497 min    205 min with reserve
ok   airspeed_sensor                +0.000        on board
ok   print_time                     +0.000 h      27 pieces, 58.8 h on the bambu_a1 (no limit stated)
ok   printed_mass                  +18.634 g      bambu_pla_aero: sliced 264 g, mass model 283 g
48 checks, 0 failing
```

## The reference's checks (informational)

```
ok   spec_requirements              +0.000        the design carries the spec's requirements unchanged
ok   parts                          +0.000        every part resolved to a sourced catalog row
FAIL parts_orderable                -2.000 parts  refused under stock policy LeadTime: speedybee_f405_wing_app (discontinued), matek_m10q_5883 (out of stock)
FAIL hover_thrust_to_weight         -0.133        T/W 1.67 (1.96 modelled, less 17.5 % thrust-model error), stopped by the voltage at 88 A / 17.0 V from the pack (hot day +15 K, low battery)
ok   hover_throttle                 +0.126        hover throttle 0.50 standard day full pack, 0.52 hot day low pack
ok   lift_motor_current            +13.449 A      9.4 A per motor in hover, 26.6 A at full throttle (ESC firmware limit if set, else none); rated 40 A
ok   lift_esc_current              +33.449 A      9.4 A hover / 26.6 A full throttle, no current limit, per channel; ESC 50 A cont, 60 A burst
ok   pack_current                  +29.959 A      hover 20.0 A from the pack, rated 50 A
ok   pack_peak_current             +14.850 A      worst seconds 97 A (four lift motors flat out under the ESC firmware limit if set, else none; full pack); Upgrade Energy GOLD V1 6S2P 22.2Ah AMPRIUS SA08 (6S2P) bursts 112 A
ok   voltage_ratings                +0.000 cells  6S pack; tightest lift motor aos_supernova_2807_1400kv 4-6S
ok   pusher_esc_current             +5.000 A      pusher 40.0 A at full throttle, ESC 45 A
ok   cruise                         +0.431        18.0 m/s at 117 W, L/D 7.9, prop 0.67, motor 0.81, throttle 0.47
ok   max_speed                      +0.756        top 37.0 m/s, best range 18.0 m/s
ok   stall_speed                    +0.164 m/s    stall 13.8 m/s EAS at CLmax 1.17 (1.26 modelled, less 7.0 % measured CLmax error for sd7037; 13.8 m/s TAS at the 0 m field), at most 14 EAS
ok   static_margin                  +5.000 %MAC   10.0 % MAC: CG 49 mm, neutral point 64 mm
ok   tail_authority                 +0.519        tail CL +0.14 to trim at the stall; section CLmax 0.82
ok   spar_stress                  +149.904 MPa    joiner in manoeuvre 4 g x 1.5: 350 MPa of 500 (20.3 N m at the root)
ok   spar_joint                     +0.000 mm     sleeve cst_cs525_1m on rockwest_t_rnd_314_l39
ok   spar_fit                       +1.505 mm     joiner: 1.5 mm inside the skin at its tightest, 150 mm out
ok   boom_stress                  +149.854 MPa    boom in landing 2.67 g x 1.5: 200 MPa of 350 (50 N leg load on a 224 mm overhang)
ok   boom_buckling                 +23.447        buckles at 4893 MPa (SP-8007, R/t 6.9, knock-down 0.89) under 200 MPa in landing 2.67 g x 1.5: 24.4 x
ok   boom_resonance                 +0.491        boom mode 44 Hz, hover rotation 213 Hz
ok   landing_legs                   +0.282        buckle at 347 N for 50 N; landing_leg in landing 2.67 g x 1.5: 14 MPa of 20 (50 N axial, 25 N side at the foot)
ok   load_paths                     -0.000 links  6 load paths, every link joined and touching
FAIL interference                   -2.973 mm     joiner_R x pod 26 mm^3; joiner_L x pod 26 mm^3
ok   attachments                    -0.000 joints 60 joints, all in contact
ok   prop_clearance                 +9.075 mm     closest: lift_prop_disc_RR to pusher_disc at 14.1 mm
ok   containment                    +3.988 mm     tightest battery: 4.0 mm inside the cavity
FAIL servo_fit                      -0.252 mm     emax_es08ma_ii in the wing at 302 mm span: 0.2 mm inside the skins
ok   print_bed                     +50.000 mm     tightest wing_R: 200 x 144 x 14 mm piece in [256.0, 256.0, 256.0] less 3 mm margins
ok   ground_clearance               +5.000 mm     lowest non-leg point 20 mm above the feet
ok   thrust_line                   +15.000 mm     pusher -0 mm from the CG's height
ok   prop_mount                     +0.000        gemfan_flash_7042_2blade on aos_supernova_2807_1400kv (unstated shaft)
ok   lift_prop_pairs                +0.000        lift prop rotation Some("pair")
FAIL pusher_prop_mount              -1.000        gemfan_flash_7042_2blade on aos_supernova_2807_1400kv: aos_supernova_2807_1400kv publishes no prop mount
FAIL lift_prop_mount                -1.000        gemfan_flash_7042_2blade on aos_supernova_2807_1400kv: aos_supernova_2807_1400kv publishes no prop mount
ok   wire_temperature             +118.417 C      lift motor FR phases: 16 AWG, 62 C steady at 9.4 A, 82 C after 10 s at 23.9 A
ok   battery_connector             +49.959 A      AS150 70/150 A for 20.0 A hover, 97 A peak
ok   fc_outputs                     +2.000 outputs speedybee_f405_wing_app has 12 outputs
ok   servo_power                    +0.500 A      servo BEC 5.5 A peak, five servos stall at 5.0 A
ok   ardupilot_params               -0.000 params 54 parameters, all inside ArduPlane's metadata
ok   budget                         +0.000 USD    BOM $1998 over 29 lines; budget none stated
ok   span                           +0.000 mm     span 1200 mm, allowed 1200
ok   range                        +192.533 km     192.5 km with 15 % reserve (228.0 km to cutoff)
ok   endurance                    +180.674 min    181 min with reserve
FAIL airspeed_sensor                -1.000        required, none on board
ok   print_time                     +0.000 h      27 pieces, 42.0 h on the bambu_a1 (no limit stated)
ok   printed_mass                  +15.782 g      bambu_pla_aero: sliced 178 g, mass model 193 g
48 checks, 7 failing
```

## Read this first (latest, 2026-10-24, after the induced-drag fix)

The tables above were generated by the eval itself. Both runs grade the two aircraft the same way: same model, same checks, best-range speeds, 15 % reserve. The reference is taken at its most favourable unpublished chord. No budget applies to either side (Pat, 10/24); cost is shown for information only.

| run | Wright | reference | ratio | pessimistic |
|---|---|---|---|---|
| headline (`lead_time` stock policy) | 218.9 km | 192.5 km | x1.137 | 172.4 vs 152.3 km (x1.132) |
| stock-listed only (`result_listed.md`) | 193.5 km | 192.5 km | x1.005 | 157.7 vs 152.3 km (x1.036) |

- **Checks.** Wright passes 48 of 48 in both runs. The reference fails 7 of 48: 3 real failures and 4 excused (for example `airspeed_sensor`, since none is on board).
- **Headline design.** 4.235 kg, 902 Wh, $3153 (information only). Made-to-order Amprius SA08 packs, which `listed` refuses.
- **Stock-listed design.** 60XG 6S6P packs, 4.532 kg, $1366. It is a dead heat with the reference nominally and 3.6 % ahead pessimistically.
- **Induced-drag fix (wright-cfd's AVL run, this date).** AVL gave 1.87 N of induced drag at 18.3 m/s against our 2.41 N. Our port of AeroBuildup used Nita and Scholz's *total* Oswald factor, k_e,D0 = 0.836 included, while the wing strips already read lift-dependent profile drag from the section polar. Nita and Scholz define k_e,D0 as exactly that viscous term, so it was counted twice. `aero::trim` now uses the inviscid e_theo x k_e,F (0.96 for this wing). Induced drag at 18 m/s is 2.07 N, total drag 6.7 % lower. The residual 10 % above AVL is the trim: our wing carries CL 0.845 with a tail download, AVL's whole-aircraft CL is 0.805. AeroBuildup itself keeps the double count (`aerosandbox/library/aerodynamics/inviscid.py`, `aero_buildup.py` line 915), so the sim's tables and the earlier "within 1-3 %" audit were agreeing with the same error; the sim will now read a few percent above the analytic model.
  - Before the fix: headline 203.2 vs 184.2 km (x1.103), listed 172.3 vs 184.2 km (x0.935).
  - The reference reproduction moved too: range at the flown speed is now +3 % over the pilot's 130 mi and endurance +5 % over 3.0 h (was -0 % and +1 %). The model is now slightly optimistic against the one logged flight, where before the double count was hiding that.
  - Not changed: the wing lift slope (ours 4.50/rad from Helmbold's AR/(2+sqrt(4+AR^2)), AVL 4.17/rad, +7.7 %). It sets the trim angle, not the drag at the trimmed CL. The neutral point (ours 85.3 mm, AVL 89.8 mm; 10.0 % vs 12.1 % MAC) is left analytic because the search cannot run AVL and the layout places the battery by it; the `cfd` stage already re-judges the same band from AVL's Xnp as `cfd_static_margin`.
- **One corrected stall** (`cl_max/(1+clmax_over)`, review 5 F7). The stall check, the 1.3x cruise floor, AIRSPEED_MIN, Q_ASSIST_SPEED and the checklist all use the same value.
- **6-DOF sim** (JSBSim 1.3.3 + AeroSandbox AeroBuildup tables, ArduPlane QuadPlane transition logic; the stage is `sim`). It ran on the earlier 182 km listed design and completed the full hover, transition, cruise, back-transition and landing mission.
  - Cruise battery power: 217 W in the sim vs 207 W analytic (+4.9 % before the drag fix; expect roughly +10 % after it, tolerance 15 %).
  - Hover shaft power: 708 W vs 747 W.
  - The stage needs `WRIGHT_PYTHON` pointing at a Python with jsbsim and aerosandbox installed.
- **`boom_buckling`** (NASA SP-8007 knock-down, ultimate landing load). Margin 33-41x on the cs525 booms; on the FEA's design (goodwinds_wct375048, R/t 6.9) the closed form gives 14.3x against 9.6x from FEA, possibly load introduction at the clamp. It passes either way.
- **Envelope.** Every requirement field has a sourced minimum and maximum. The hot-day limit depends on altitude (MIL-HDBK-310, 1 % high), so the 3500 m/+45 K and 3000 m/+35 K attacks are rejected.
