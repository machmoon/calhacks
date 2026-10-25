# Reference: Pat's team balloon (as planned, 2026-09-19)

The reference model is `wright_balloon::reference::pat_design()`. It is
judged by the same checks as Wright's design (`cargo run --release -p
wright-balloon --example eval`). Results are in [result.md](result.md).

## What is known

From the team's plan page (claude.ai/artifact/Bpb4uLhdQDSMTVut1dxzKB, read
2026-10-24) and the project notes of 2026-09-19:

| item | known | source |
|---|---|---|
| hardware on hand | Jetson Orin Nano, 4S 6000 mAh LiPo, Matek M10 GPS, 2x ESP32 DevKit, 5 V regulators (4x "5 V 1.8 A"), 2x MOSFET boards, buck converter, MPU6050, GEPRC 1.2 GHz 2 W VTX, SG90 servo (cutdown), STARDRONE CR8 915 MHz ELRS RX, Logitech BRIO | notes; plan parts table |
| part masses | Jetson 175 g, LiPo 575 g, M10 9 g, 2x ESP32 20 g, MOSFET 2x 4.5 g, MPU 5 g, VTX 14.5 g, SG90 10 g, CR8 7 g, BRIO 65 g ("listed parts ~920 g") | plan parts table (the catalog uses maker figures where published: M10 8 g, SG90 9 g) |
| tracking | APRS 144.39 MHz primary (licensed teammate; DRA818V/SA818 recommended), ELRS 915 MHz GPS telemetry via ESP32 -> CR8 secondary, phone in the box as backup | notes 2026-09-19 |
| cutdown | SG90 driven by the ESP32 from ELRS channels | plan |
| open questions | whether they own a 900 MHz ELRS transmitter; whether the 1.2 GHz VTX flies (GPS interference); "5 V regulator separate from the Jetson's power, with its own battery if possible" | plan "Answer these before building" |
| still missing in the plan | balloon, helium, parachute, foam box, pressure sensor, temperature probe, Jetson storage, recovery phone | plan |

## What is assumed (each is a stated choice, not a fact)

| assumption | value | why |
|---|---|---|
| balloon, chute, foam wall, fill target | chosen in the plan's favour: every catalog combination (8 balloons x 5 chutes x 3 walls x 5 fill targets = 600) is checked and the one failing the fewest checks (then highest burst, then cheapest) is kept; the result is in result.md | the plan names none of them; earlier versions assumed HAB-1200 / 36 in chute / 1 in wall / 5 m/s, which a reviewer showed was not the plan's best case (with the 60 in chute it lands under 5 m/s) |
| fill | CUSF neck lift (calc.js @a9c4d17) for the best of the same five targets Wright may use | the plan states no ascent rate |
| gas | helium UHP, one size-300 cylinder (292 ft^3), $295 | Westchester County 2026 contract price; balloon-grade price not published by a US seller |
| line | 8 m of 30 lb Ashaway braided Dacron (Into The Wind #1319) | same as Wright; a 50 lb test line can break above 50 lbf (rules::LINE_OVERSTRENGTH) |
| box | Owens Corning FOAMULAR XPS (wall chosen as above), sized to the packed parts, no printed rack; the camera is packed against its port wall | "foam box" is listed as missing |
| power | everything on the single 4S 6000 mAh pack: Jetson and VTX straight off the pack, BRIO on one 5 V 1.8 A regulator, ESP32s, M10, CR8, MPU6050, SG90 on the other; the DRA818V on the plan's on-hand "buck converter" (modelled as an LM2596 module) set to 4.0 V | only one pack is on hand; the plan does not say how the DRA818V (3.3-4.5 V) is fed, so it is fed in range (earlier versions wired it to 5 V) |
| loads | Jetson in its 7 W mode (the lowest NVIDIA lists), BRIO 2.5 W (unpublished), VTX 6 W DC for 2 W RF (unpublished), DRA818V 1 W beacon every 60 s | catalog notes |
| shutdown | Jetson, BRIO and VTX switched off at burst by the MOSFET boards | generous to the plan |
| APRS radio | DRA818V on the spare ESP32, sharing the M10 | the 2026-09-19 recommendation |
| ELRS ground TX | none | open question; the check reports missing evidence |
| phone | a $100 used phone, 170 g, 3.3 Ah, 0-35 C rating (Apple's iPhone ambient rating as the reference) | backup tracker named in the plan |
| VTX channel | 1280 MHz (inside the 1240-1300 MHz amateur band) | legal choice for the licensed teammate |
| launch | inland Central Valley, CA, 80 m elevation, 15 C on the ground, typical westerly wind profile with a 30 m/s jet | team at UCSB; no forecast wired in |
| burst goal | 30 km, the same requirement for both designs | not stated in the notes; earlier versions judged both on Pat's own burst + 500 m, which failed his plan by construction |
| cost (information only; no budget in the spec) | his plan's BOM at market prices with the open choices above, including the fill regulator, cylinder rental, chute harness and microSD that every flight buys (parts he owns are priced, so the comparison is like for like) | "his budget scale" |
| launch window | the window the model gives his design is declared for him, as for Wright's (none exists: his box leaves the phone's 0-35 C rating at mid-range) | in his favour |

## Every number's source

All constants are in `crates/wright-balloon/src/rules.rs`, each with its
source or the word ASSUMPTION; every catalog row in
`data/balloon/catalog.json` carries its URL, the date read and a note
saying which fields are published and which are assumed. The main sources:

* atmosphere: U.S. Standard Atmosphere 1976 (NOAA-S/T 76-1562), layers to 84.852 km.
* sizing rule: CUSF burst calculator, `cuspaceflight/cusf-burst-calc` @ a9c4d175d6f82100fd9c749f34f580db116bfc2b, `js/calc.js` lines 292-328 (cubic for the launch radius, constant Cd per balloon from Kaymont Totex data, scale height 7238.3 m, rho_air 1.205).
* check model drag: Gallice et al. 2011, Atmos. Meas. Tech. 4, 2235-2253, eq. (6), Cd(Re) fitted on Totex TX1200 night flights.
* burst diameters: Kaymont HAB-600..3000 spec sheets (kaymont.com/balloons), equal to the CUSF table.
* chutes: Spherachutes descent-rate chart, "Cd Rate: 0.75"; product pages for mass and price.
* FAA: 14 CFR 101.1(a)(4)(i)-(iv) (law.cornell.edu/cfr/text/14/101.1, eCFR).
* GNSS: u-blox MAX-M10S data sheet UBX-20035208 R02 (80 000 m, airborne 4 g; NF 1.5 dB; sensitivities; +15 dBm RF abs max), M10 SPG 5.00 interface description UBX-20053845 (dynModel 6/7/8 airborne), Infineon BGA524N6 (GNSS LNA input P1dB -12 dBm); portable-model 12 km limit from the u-blox M8 receiver description table.
* radios: Dorji DRA818V data sheet (VCC 3.3-4.5 V, 1 W, -122 dBm), ExpressLRS `src/src/common.cpp` @36c1bf2 (900 MHz 25 Hz: SF9/BW500, -123 dBm), STARDRONE CR8 listing (20 dBm telemetry), Kenwood TM-D710GE (0.16 uV = -123 dBm), Heltec LoRa 32 V3 page, QRP Labs LightAPRS 2.0 page.
* batteries: Energizer L91 data sheet (-40..60 C; capacity curve read at 250 mA), LP503759 LiPo and HTCNR18650 data sheets for cold capacity.
* temperatures: NVIDIA Orin Nano carrier board spec (0-35 C, 9-20 V), Logitech BRIO data sheet (0-40 C), Matek M10Q (-20..80 C), TowerPro SG90 (0-55 C), GoPro HERO13 (-10..35 C).

Research downloads (data sheets, product pages) were read on 2026-10-24.
