# Before release

- GO / NO-GO: measure the ground air temperature at the launch site. Launch only between -12 C and 35 C (10 F to 95 F); outside that range the payload box leaves a part's rated temperature (the `launch_window` check), so scrub
- Assemble, power and seal the payload in the heated or air-conditioned car (about 20 C) and carry it out no more than 30 min before release; the thermal model starts the box there (rules::PACK_TEMP_C, rules::PRELAUNCH_MIN)
- Fill the totex-3000 with he-uhp-300 (1 cylinder) until the fill scale reads 2485 g of neck lift (sized for 5.5 m/s ascent); tie off, then weigh again
- Set every u-blox GPS to airborne mode (UBX-CFG-NAV5 dynModel 6, Airborne <1g, good to 50 km) and confirm it on the bench; in the default mode many receivers stop reporting above about 12 km
- Power every tracker from fresh lithium cells and watch a full position report reach the ground station before release
- Rig the spherachute-72 parachute on 8 m of ashaway-dacron-30lb line (30 lb test) and the payload under it on the swivel harness; the line is the weak link of the train, so the payload separates at under 50 lbf (14 CFR 101.1(a)(4)(iv))
- Weigh each payload package at the launch site: under 4 lb each and 12 lb total keep the flight exempt from Part 101 Subpart D (101.1(a)(4))
- Run a landing prediction for the day (e.g. the CUSF/SondeHub predictor) and do not launch if it lands in water, in a city or near an airport
