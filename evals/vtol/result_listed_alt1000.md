# VTOL eval: Wright vs the 2025 130-mile 3D-printed VTOL

**Verdict: FAIL.** Wright's design passes every check (47 checks, 0 failing); range with 15 % reserve x0.975 nominal, x1.005 pessimistic.

Stock policy: `listed` (`listed`: bought parts in stock, or listed for sale with stock unstated; made to order, pre-order, backorder and out of stock refused. `lead_time` also accepts made to order / pre-order / backorder). Both aircraft are judged under it.

Run: `cargo run --release -p wright-air --example vtol_eval` (add `-- --build` for CAD, G-code, BOM, params, checklist).

## 1. Reference reproduced from its published parts

Graded against the author's report; the wing chord was never published, so it is swept. Range and endurance are to cutoff at the 19.4 m/s the pilot averaged (130 mi in 3 h).

| root chord | mass kg | hover throttle | endurance at flown speed | range at flown speed |
|---|---|---|---|---|
| 176 mm | 2.560 (+0 %) | 0.50 (+11 %) | 3.03 h (+1 %) | 129.7 mi (-0 %) |
| 220 mm | 2.649 (+4 %) | 0.51 (+13 %) | 2.88 h (-4 %) | 123.0 mi (-5 %) |
| 264 mm | 2.743 (+8 %) | 0.52 (+15 %) | 2.71 h (-10 %) | 115.6 mi (-11 %) |
| **reported** | 2.55 | 0.45 | 3.0 h | 130 mi |

## 2-3. Same model, same constraints, best-range speeds, 15 % reserve

The reference is taken at the swept chord that flies furthest in each model (176.0 mm nominal, 176.0 mm pessimistic), so its unpublished chord can only help it.

| | reference | Wright |
|---|---|---|
| range with reserve, km | 186.6 | 182.0 |
| range to cutoff, mi | 137.3 | 134.4 |
| endurance with reserve, min | 181 | 173 |
| best-range speed, m/s | 17.4 | 17.8 |
| cruise power from the pack, W | 116.9 | 207.0 |
| L/D | 7.74 | 7.14 |
| pusher prop efficiency | 0.664 | 0.675 |
| pusher motor efficiency | 0.814 | 0.869 |
| take-off mass, kg | 2.560 | 4.533 |
| battery energy, Wh | 451 | 778 |
| cost with battery (BOM), USD | 1998 | 1368 |
| hover throttle | 0.50 | 0.47 |
| predicted hover current per motor, A | 9.2 | 23.0 |
| stall, m/s | 13.4 | 13.7 |
| hover T/W (check: at least 1.8) | 1.96 | 2.01 |
| search allowance: T/W, pessimistic powertrain, less thrust-model error | 1.63 | 1.85 |
| search allowance: stall with the airfoil's measured CLmax error, m/s | 13.8 | 14.0 |
| **pessimistic** range with reserve, km | 148.6 | 149.4 |
| checks failing | 3 (+4 excused, below) | 0 |

Reference failures graded in its favour (not counted):

- `interference`: the pod and joiner shapes are Wright's reconstruction, not the author's parts
- `servo_fit`: the wing chord is unpublished; the servo fails only at the chord Wright swept
- `pusher_prop_mount`: the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop
- `lift_prop_mount`: the AOS 2807's shaft size is published inconsistently; the builder evidently mounted the prop

## Wright's design

- wing: 1200.0 mm span, 220.0/220.0 mm chords, "sd7062"
- lift: 4 x "brotherhobby_avenger_2806_5_1300kv" with "hqprop_mr_9x4_5_pair", ESC "holybro_tekko32_f4_45a_single_am32"
- pusher: "tmotor_mn3110_780kv" with "tmotor_mf1302_folding", ESC "holybro_tekko32_f4_45a_single_am32"
- battery: "hand-built 6S6P Huaian Tenpower Lithium INR21700-60XG"
- avionics: "corewing_f405_wing_v2", "speedybee_bzgnss_bz251", "radiomaster_rp1_v2", airspeed "holybro_ms4525do", servos "emax_es08ma_ii"
- structure: spar "cst_cs525_1m" + joiner "goodwinds_pct500024", booms "cst_cs525_1m", legs 11.0 mm

Search: 43659 whole-model evaluations in 275 s.

- battery Cells { id: "eve_58e", s: 6, p: 8 }: best -1229.5
- battery Cells { id: "tenpower_60xg", s: 6, p: 8 }: best -1348.7
- battery Cells { id: "tenpower_60xg", s: 6, p: 7 }: best -1074.2
- battery Cells { id: "eve_58e", s: 6, p: 7 }: best -1137.9
- battery Cells { id: "tenpower_60xg", s: 6, p: 6 }: best 182.0
- battery Pack { id: "tattu_uav_classic_6s_10000_30c_ec5", count: 3 }: best -2484.9
- battery Pack { id: "tattu_uav_classic_6s_10000_30c_ec5", count: 2 }: best -2150.6
- battery Pack { id: "tattu_uav_classic_6s_10000_30c_ec5", count: 1 }: best -1176.3
- best: 182.0 km -- brotherhobby_avenger_2806_5_1300kv / hqprop_mr_9x4_5_pair lift, tmotor_mn3110_780kv / tmotor_mf1302_folding pusher, Cells { id: "tenpower_60xg", s: 6, p: 6 }, sd7062 220/220 mm
- finalist 1 passes every check and keeps the model allowance

## Wright's checks

```
ok   spec_requirements              +0.000        the design carries the spec's requirements unchanged
ok   parts                          +0.000        every part resolved to a sourced catalog row
ok   parts_orderable                -0.000 parts  32 bought lines accepted under stock policy Listed; 19 in stock, 13 listed with stock unstated
ok   hover_thrust_to_weight         +0.145        T/W 1.95 (2.01 modelled, less 3.5 % thrust-model error), stopped by the ESC rating at 146 A / 19.5 V from the pack (hot day +15 K, low battery)
ok   hover_throttle                 +0.148        hover throttle 0.47 standard day full pack, 0.50 hot day low pack
ok   lift_motor_current             +0.400 A      23.0 A per motor in hover, 44.0 A at full throttle (ESC firmware limit if set, else none); rated 44 A
ok   lift_esc_current               +1.000 A      23.0 A hover / 44.0 A full throttle under the ESC firmware limit 44 A per motor (AM32 limits.current = 22, https://github.com/am32-firmware/AM32/blob/main/Src/main.c (use_current_limit: 1 kHz current PID on the ESC's own sensor caps duty; setting limits.current x 2 A)); ESC 45 A cont, 45 A burst
ok   pack_current                 +310.577 A      hover 49.4 A from the pack, rated 360 A
ok   pack_peak_current            +823.852 A      worst seconds 136 A (four lift motors flat out under the ESC firmware limit if set, else none; full pack); hand-built 6S6P Huaian Tenpower Lithium INR21700-60XG bursts 960 A
ok   voltage_ratings                +0.000 cells  6S pack; tightest lift motor brotherhobby_avenger_2806_5_1300kv 6-6S
ok   pusher_esc_current            +19.000 A      pusher 26.0 A at full throttle, ESC 45 A
ok   cruise                         +0.387        17.8 m/s at 207 W, L/D 7.1, prop 0.67, motor 0.87, throttle 0.51
ok   max_speed                      +0.150        top 25.8 m/s, best range 17.8 m/s
ok   stall_speed                    +0.013 m/s    stall 14.0 m/s EAS (13.7 modelled at CLmax 1.47, less 4.5 % measured CLmax error for sd7062; 14.0 m/s TAS at the 0 m field), at most 14 EAS
ok   static_margin                  +5.000 %MAC   10.0 % MAC: CG 68 mm, neutral point 90 mm
ok   tail_authority                 +0.559        tail CL +0.10 to trim at the stall; section CLmax 0.82
ok   spar_stress                  +158.156 MPa    main_spar in manoeuvre 4 g x 1.5: 342 MPa of 500 (19.8 N m where the joiner ends, 150 mm out)
ok   spar_joint                     +0.000 mm     sleeve goodwinds_pct500024 on cst_cs525_1m
ok   spar_fit                       +7.591 mm     rear spar: 7.6 mm inside the skin at its tightest, 0 mm out
ok   boom_stress                   +38.526 MPa    boom in landing 2.67 g x 1.5: 461 MPa of 500 (89 N leg load on a 300 mm overhang)
ok   boom_resonance                 +0.496        boom mode 38 Hz, hover rotation 184 Hz
ok   landing_legs                   +0.103        buckle at 580 N for 89 N; landing_leg in landing 2.67 g x 1.5: 18 MPa of 20 (89 N axial, 45 N side at the foot)
ok   load_paths                     -0.000 links  6 load paths, every link joined and touching
ok   interference                   +0.000 mm     50 solids, 1225 pairs, none overlapping
ok   attachments                    -0.000 joints 60 joints, all in contact
ok   prop_clearance                 +9.104 mm     closest: lift_prop_disc_RL to pusher_disc at 14.1 mm
ok   containment                    +4.167 mm     tightest battery: 4.2 mm inside the cavity
ok   servo_fit                      +7.430 mm     emax_es08ma_ii in the wing at 302 mm span: 7.9 mm inside the skins
ok   print_bed                     +30.002 mm     tightest wing_R: 220 x 200 x 31 mm piece in [256.0, 256.0, 256.0] less 3 mm margins
ok   ground_clearance               +5.000 mm     lowest non-leg point 20 mm above the feet
ok   thrust_line                   +15.000 mm     pusher +0 mm from the CG's height
ok   prop_mount                     +0.000        tmotor_mf1302_folding on tmotor_mn3110_780kv (short_stub shaft)
ok   lift_prop_pairs                +0.000        lift prop rotation Some("pair")
ok   pusher_prop_mount              +0.000        tmotor_mf1302_folding on tmotor_mn3110_780kv: 2x M3 on a 12 mm circle
ok   lift_prop_mount                +0.000        hqprop_mr_9x4_5_pair on brotherhobby_avenger_2806_5_1300kv: 5 mm bore on a 5 mm shaft/adapter
ok   wire_temperature              +28.432 C      lift ESC feed: 10 AWG, 115 C steady at 49.4 A, 172 C after 10 s at 145.9 A
ok   battery_connector             +13.852 A      AS150 70/150 A for 49.4 A hover, 136 A peak
ok   fc_outputs                     +2.000 outputs corewing_f405_wing_v2 has 12 outputs
ok   servo_power                    +9.000 A      servo BEC 14.0 A peak, five servos stall at 5.0 A
ok   ardupilot_params               -0.000 params 54 parameters, all inside ArduPlane's metadata
ok   budget                       +632.365 USD    BOM $1368 over 32 lines; budget $2000
ok   span                           +0.000 mm     span 1200 mm, allowed 1200
ok   range                          +2.020 km     182.0 km with 15 % reserve (216.3 km to cutoff)
ok   endurance                    +172.820 min    173 min with reserve
ok   airspeed_sensor                +0.000        on board
ok   print_time                     +0.000 h      27 pieces, 57.4 h on the bambu_a1 (no limit stated)
ok   printed_mass                  +14.554 g      bambu_pla_aero: sliced 270 g, mass model 285 g
47 checks, 0 failing
```

## The reference's checks (informational)

```
ok   spec_requirements              +0.000        the design carries the spec's requirements unchanged
ok   parts                          +0.000        every part resolved to a sourced catalog row
FAIL parts_orderable                -3.000 parts  refused under stock policy Listed: upgrade_energy_gold_v1_6s2p_22ah_sa08 (made to order), speedybee_f405_wing_app (discontinued), matek_m10q_5883 (out of stock)
FAIL hover_thrust_to_weight         -0.133        T/W 1.67 (1.96 modelled, less 17.5 % thrust-model error), stopped by the voltage at 88 A / 17.0 V from the pack (hot day +15 K, low battery)
ok   hover_throttle                 +0.126        hover throttle 0.50 standard day full pack, 0.52 hot day low pack
ok   lift_motor_current            +13.449 A      9.4 A per motor in hover, 26.6 A at full throttle (ESC firmware limit if set, else none); rated 40 A
ok   lift_esc_current              +33.449 A      9.4 A hover / 26.6 A full throttle, no current limit, per channel; ESC 50 A cont, 60 A burst
ok   pack_current                  +29.959 A      hover 20.0 A from the pack, rated 50 A
ok   pack_peak_current             +14.850 A      worst seconds 97 A (four lift motors flat out under the ESC firmware limit if set, else none; full pack); Upgrade Energy GOLD V1 6S2P 22.2Ah AMPRIUS SA08 (6S2P) bursts 112 A
ok   voltage_ratings                +0.000 cells  6S pack; tightest lift motor aos_supernova_2807_1400kv 4-6S
ok   pusher_esc_current             +5.000 A      pusher 40.0 A at full throttle, ESC 45 A
ok   cruise                         +0.435        17.4 m/s at 117 W, L/D 7.7, prop 0.66, motor 0.81, throttle 0.46
ok   max_speed                      +0.850        top 37.4 m/s, best range 17.4 m/s
ok   stall_speed                    +0.164 m/s    stall 13.8 m/s EAS (13.4 modelled at CLmax 1.26, less 7.0 % measured CLmax error for sd7037; 13.8 m/s TAS at the 0 m field), at most 14 EAS
ok   static_margin                  +5.000 %MAC   10.0 % MAC: CG 49 mm, neutral point 64 mm
ok   tail_authority                 +0.519        tail CL +0.14 to trim at the stall; section CLmax 0.82
ok   spar_stress                  +149.904 MPa    joiner in manoeuvre 4 g x 1.5: 350 MPa of 500 (20.3 N m at the root)
ok   spar_joint                     +0.000 mm     sleeve cst_cs525_1m on rockwest_t_rnd_314_l39
ok   spar_fit                       +1.505 mm     joiner: 1.5 mm inside the skin at its tightest, 150 mm out
ok   boom_stress                  +149.854 MPa    boom in landing 2.67 g x 1.5: 200 MPa of 350 (50 N leg load on a 224 mm overhang)
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
ok   budget                         +2.357 USD    BOM $1998 over 29 lines; budget $2000
ok   span                           +0.000 mm     span 1200 mm, allowed 1200
ok   range                        +186.638 km     186.6 km with 15 % reserve (221.0 km to cutoff)
ok   endurance                    +181.162 min    181 min with reserve
FAIL airspeed_sensor                -1.000        required, none on board
ok   print_time                     +0.000 h      27 pieces, 42.0 h on the bambu_a1 (no limit stated)
ok   printed_mass                  +15.782 g      bambu_pla_aero: sliced 178 g, mass model 193 g
47 checks, 7 failing
```
