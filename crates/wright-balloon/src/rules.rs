//! Every number that changes an answer, with its source. Numbers marked
//! ASSUMPTION have no published source; each says why it is what it is.

// ---------------------------------------------------------------- atmosphere
// NOAA-S/T 76-1562, U.S. Standard Atmosphere 1976.
/// Sea-level temperature, K (US76 Table 4).
pub const US76_T0_K: f64 = 288.15;
/// Sea-level pressure, Pa (US76 Table 4).
pub const US76_P0_PA: f64 = 101_325.0;
/// Specific gas constant of air R*/M0 = 8314.32/28.9644, J/(kg K) (US76 eqs. 1, 23).
pub const US76_R_AIR: f64 = 8314.32 / 28.9644;
/// Effective Earth radius for geopotential, m (US76 eq. 18).
pub const US76_R0_M: f64 = 6_356_766.0;
/// Sutherland constants for air viscosity (US76 eq. 51): beta, kg/(s m K^0.5); S, K.
pub const SUTHERLAND_BETA: f64 = 1.458e-6;
pub const SUTHERLAND_S: f64 = 110.4;
/// Universal gas constant, J/(mol K) (US76 Table 2).
pub const R_UNIVERSAL: f64 = 8.31432;
/// Molar mass of air, kg/mol (US76 Table 2).
pub const M_AIR: f64 = 0.0289644;
/// Molar masses, kg/mol (IUPAC 2021 standard atomic weights: He 4.002602, H 1.00794).
pub const M_HE: f64 = 0.004002602;
pub const M_H2: f64 = 0.00201588;
/// Reference state for cylinder gas volumes (CGA P-11 / Airgas "cubic feet":
/// 70 F = 21.1 C and 1 atm).
pub const CYL_REF_T_K: f64 = 294.26;
pub const CYL_REF_P_PA: f64 = 101_325.0;

// ------------------------------------------------- the CUSF sizing rule
// cuspaceflight/cusf-burst-calc @ a9c4d175d6f82100fd9c749f34f580db116bfc2b,
// js/calc.js `calc_update` (lines 292-328) and index.html defaults (lines 207-215).
/// Air density the CUSF calculator sizes at, kg/m^3 (index.html `rho_a`).
pub const CUSF_RHO_AIR: f64 = 1.2050;
/// Scale height of the CUSF exponential air-density model, m (index.html `adm`).
pub const CUSF_ADM_M: f64 = 7238.3;
/// Gas densities used by CUSF, kg/m^3 (calc.js `find_rho_g`).
pub const CUSF_RHO_HE: f64 = 0.1786;
pub const CUSF_RHO_H2: f64 = 0.0899;

/// Reynolds range of the Gallice et al. (2011) drag fit (their Sect. 2.3.1:
/// ~8-9e5 at the ground to ~6-9e4 at 30 km).
pub const GALLICE_RE_MIN: f64 = 6.0e4;
pub const GALLICE_RE_MAX: f64 = 9.0e5;

// --------------------------------------------------------------- requirements
/// Ascent-rate band, m/s (the user's requirement for this domain, 2026-10-24).
pub const ASCENT_MIN_MPS: f64 = 4.0;
pub const ASCENT_MAX_MPS: f64 = 6.0;
/// Target ascent rate the fill is sized for: the middle of the band.
pub const ASCENT_TARGET_MPS: f64 = 5.0;
/// Landing speed limit, m/s (the user's requirement).
pub const LANDING_MAX_MPS: f64 = 5.0;

// --------------------------------------------------------- FAA 14 CFR 101.1
// 14 CFR 101.1(a)(4), eCFR (read via law.cornell.edu/cfr/text/14/101.1, 2026-10-24).
/// (i): a package over 4 lb is regulated if its weight/size ratio exceeds 3 oz/in^2.
pub const FAA_PKG_LB: f64 = 4.0;
pub const FAA_PKG_OZ_PER_IN2: f64 = 3.0;
/// (ii): any single package over 6 lb is regulated.
pub const FAA_PKG_MAX_LB: f64 = 6.0;
/// (iii): two or more packages totalling over 12 lb are regulated.
pub const FAA_TOTAL_LB: f64 = 12.0;
/// (iv): a suspension needing more than 50 lb of impact force to separate is regulated.
pub const FAA_LINE_LBF: f64 = 50.0;
/// ASSUMPTION: a line's "lb test" is its rated minimum break; the actual
/// break of a given spool may exceed it, and no maker of the catalog lines
/// publishes the spread. The 101.1(a)(4)(iv) check therefore judges the
/// rated break times this factor (a 30 lb test line at up to 45 lbf stays
/// exempt; a 50 lb test line does not), while the strength check uses the
/// rated minimum.
pub const LINE_OVERSTRENGTH: f64 = 1.5;

// -------------------------------------------------------------------- GNSS
/// u-blox M10 altitude limit in the Airborne <4g platform, m (MAX-M10S data
/// sheet UBX-20035208 R02, p. 4, "Operational limits ... Altitude 80,000 m,
/// 1 Assuming Airborne 4 g platform").
pub const M10_ALT_AIRBORNE_M: f64 = 80_000.0;
/// u-blox default (portable) platform altitude limit, m: u-blox M8 receiver
/// description UBX-13003221 "Dynamic platform model" table (portable: 12000 m),
/// as reproduced in the Paparazzi wiki u-blox page; not restated in the M10 docs read.
pub const UBLOX_ALT_PORTABLE_M: f64 = 12_000.0;
/// u-blox M8 airborne platform altitude limit, m (same table: 50000 m).
pub const M8_ALT_AIRBORNE_M: f64 = 50_000.0;
/// GNSS L1 centre, MHz (IS-GPS-200).
pub const GPS_L1_MHZ: f64 = 1575.42;
/// Receiver chain noise figure, dB (MAX-M10S DS Table 11 `NFtot`).
pub const GNSS_NF_DB: f64 = 1.5;
/// Satellite signal level the M10 is specified at, dBm (MAX-M10S DS Table 1 note 3).
pub const GNSS_SIGNAL_DBM: f64 = -130.0;
/// Reacquisition sensitivity, dBm (MAX-M10S DS Table 1: -160 dBm). The
/// difference to `GNSS_SIGNAL_DBM` is the noise rise the receiver tolerates
/// and still reacquires after a loss.
pub const GNSS_REACQ_DBM: f64 = -160.0;
/// GNSS LNA input 1 dB compression, dBm: Infineon BGA524N6 (a common GNSS
/// front-end LNA) product page "Input 1 dB compression point: -12 dBm". The
/// M10's own LNA P1dB is not published.
pub const GNSS_LNA_P1DB_DBM: f64 = -12.0;
/// ASSUMPTION: an interferer must stay 10 dB under P1dB for gain compression
/// to be negligible (the usual linear-operation back-off).
pub const GNSS_BLOCK_BACKOFF_DB: f64 = 10.0;
/// ASSUMPTION: GNSS patch -10 dB bandwidth 20 MHz (18-25 mm ceramic L1
/// patches), modelled as a single resonator of Q = f0/BW for out-of-band rejection.
pub const GNSS_PATCH_BW_MHZ: f64 = 20.0;
/// GNSS signal bandwidth for the in-band noise floor, MHz (C/A 2.046 MHz main lobe).
pub const GNSS_BW_MHZ: f64 = 2.046;

// --------------------------------------------------------------------- radio
/// ASSUMPTION: fade margin a link must keep beyond sensitivity for a swinging
/// payload with polarisation mismatch, dB.
pub const LINK_FADE_MARGIN_DB: f64 = 6.0;
/// ASSUMPTION: deepest null of a hanging dipole once swing is averaged, dBi.
pub const DIPOLE_FLOOR_DBI: f64 = -10.0;
/// 4/3-Earth effective radius factor for radio horizon (ITU-R P.453 median refractivity).
pub const K_FACTOR: f64 = 4.0 / 3.0;
pub const EARTH_R_M: f64 = 6_371_000.0;
/// ASSUMPTION: payload antenna height once landed, m.
pub const LANDED_ANTENNA_M: f64 = 0.3;
/// Amateur 23 cm band, MHz (47 CFR 97.301(a)): where a 1.2 GHz VTX may legally transmit.
pub const HAM_23CM_MHZ: (f64, f64) = (1240.0, 1300.0);

// ------------------------------------------------------------------- thermal
/// ASSUMPTION: outside film coefficient at sea level, W/(m^2 K), scaled by
/// sqrt(rho/rho0) for forced convection at the ascent speed (Nu ~ Re^0.5).
pub const H_EXT_SL: f64 = 10.0;
/// ASSUMPTION: lumped specific heat of the payload contents, J/(kg K)
/// (electronics and Li cells are 800-1100).
pub const CP_PAYLOAD: f64 = 1000.0;
/// ASSUMPTION: wiring, connectors, tape, zip ties as a fraction of part mass.
pub const WIRING_FRACTION: f64 = 0.10;
/// Camera view port in the +x wall: radius at the inner face, mm
/// (ASSUMPTION: a 30 mm hole clears action-camera lens housings), flared
/// outward as a cone so the wall does not crop the picture.
pub const PORT_R_MM: f64 = 15.0;
/// ASSUMPTION: half-angle of the clear cone a camera must see through its
/// port, deg (60 deg full cone: the horizon and the black sky in one frame;
/// narrower than the RunCam 5 / GoPro / BRIO fields of view, so the port,
/// not the lens, is what this bounds).
pub const VIEW_HALF_DEG: f64 = 30.0;
/// ASSUMPTION: clearance kept around each part in the box, mm.
pub const PACK_GAP_MM: f64 = 8.0;

/// ASSUMPTION: payload powered and sealed on the ground before release, min.
pub const PRELAUNCH_MIN: f64 = 30.0;
/// ASSUMPTION: the payload is assembled, powered and sealed in the heated or
/// air-conditioned chase vehicle (or a building), so the box starts at room
/// temperature, C, then sits `PRELAUNCH_MIN` in the open air before release.
/// The checklist makes this a procedure. (The Red Rocks CC 2023 HotHands
/// flight activated its warmers 1 h 10 min before launch; reference below.)
pub const PACK_TEMP_C: f64 = 20.0;

// ------------------------------------------------------- launch temperature
/// Default ground-temperature range a design must be launchable over, C:
/// the user's requirement for this domain (2026-10-24: "a sensible default
/// range, e.g. -10..+35 C ground"). A spec narrows or widens it with
/// `launch_temp_range_c`.
pub const LAUNCH_TEMP_RANGE_C: [f64; 2] = [-10.0, 35.0];
/// Where the launch-window sweep looks, C, and its step before the edges
/// are bisected to `WINDOW_TOL_C`.
pub const WINDOW_SWEEP_C: [f64; 2] = [-40.0, 50.0];
pub const WINDOW_STEP_C: f64 = 1.0;
pub const WINDOW_TOL_C: f64 = 0.1;

// ------------------------------------------------- chemical hand warmers
// Air-activated iron warmers (HotHands / Grabber, Kobayashi Healthcare).
// Heat output is not published; it is derived here from the composition:
//   P = m_packet * f_contents * f_Fe * eta * dH_Fe / t_rated
//     = 29.7 g * 0.90 * 0.50 * 0.50 * 7.379 kJ/g / 10 h = 1.37 W per warmer
/// Mass of one unused HotHands hand warmer, g: Betz, Lewis, Smith, Zumtobel,
/// "Bringing the Heat to 100,000 Feet: The Capacity of HotHands to Facilitate
/// High-Altitude Research", Red Rocks CC, 2023 COSGC Undergraduate Space
/// Research Symposium, p. 6: "One HotHand packet weighs 29.7g".
pub const WARMER_PACKET_G: f64 = 29.7;
/// ASSUMPTION: share of the packet mass that is the heating mix (the rest is
/// the non-woven pouch); the SDS states no pouch mass.
pub const WARMER_CONTENTS_FRACTION: f64 = 0.90;
/// Iron powder share of the heating mix: Kobayashi Healthcare SDS
/// "Air-activated heating packs of various styles" (HotHands, Grabber),
/// rev. 2021-12-08, Section 3: "Iron powder 7439-89-6 50-60" wt %. The
/// lower bound is used (less heat is the conservative side for the cold end,
/// the only end where warmers are opened).
pub const WARMER_IRON_FRACTION: f64 = 0.50;
/// Heat of oxidising iron to Fe2O3, J per g Fe: dHf(Fe2O3, s) = -824.2
/// kJ/mol (NIST-JANAF / CRC standard value) over 2 mol Fe x 55.845 g/mol.
pub const FE_OXIDATION_J_PER_G: f64 = 824.2e3 / (2.0 * 55.845);
/// ASSUMPTION: half of the iron oxidises within the rated duration (the
/// pack is still warm, not spent, at its rated end); no published figure.
pub const WARMER_CONVERSION: f64 = 0.50;
/// Average heat output of one warmer at sea-level pressure, W (derived above
/// with the catalog row's rated duration).
pub fn warmer_power_sl_w(rated_h: f64) -> f64 {
    WARMER_PACKET_G * WARMER_CONTENTS_FRACTION * WARMER_IRON_FRACTION * WARMER_CONVERSION * FE_OXIDATION_J_PER_G / (rated_h * 3600.0)
}
/// ASSUMPTION: output scales with the oxygen partial pressure, i.e. with
/// p/p0 (oxygen reaches the iron by diffusion through the air-permeable
/// film, Fick's law with a partial-pressure driving gradient). Supported
/// qualitatively, not in magnitude, by Red Rocks CC 2023 p. 4: in a vacuum
/// chamber at "roughly equivalent to the altitude of 65,000ft" the HotHands
/// temperature "dropped 16 C" in 30 min while the control held within 2 C;
/// and p. 11: a sealed box with warmers fell to ~5 % O2 on the ground, so
/// the box must stay vented (a checklist item).
pub const WARMER_PRESSURE_EXPONENT: f64 = 1.0;

// ------------------------------------------------------------------- power
/// ASSUMPTION: usable depth of discharge of a LiPo / Li-ion pack.
pub const LIPO_USABLE: f64 = 0.80;
/// Capacity fraction vs cell temperature (C) by chemistry. Sources in
/// `capacity_fraction`.
/// LiPo / Li-ion: LP503759 LiPo data sheet (sites.units.it ... lp503759is3.pdf):
/// "-10 C: 70 % (minimum)", "0 C: 80 %", discharge rated -20..60 C;
/// HTCNR18650 data sheet (altronics S4736A): "-20 C >= 60 %". Below the
/// rated -20 C nothing is counted.
pub const LIPO_CAPACITY: &[(f64, f64)] = &[(-20.01, 0.0), (-20.0, 0.60), (-10.0, 0.70), (0.0, 0.80), (25.0, 1.0)];
/// Energizer L91 (data.energizer.com/pdfs/l91.pdf) capacity-vs-temperature
/// curve read at 250 mA: ~1600 mAh at -40 C vs ~3550 mAh at 21 C (0.45);
/// rated -40..60 C. Linear between (ASSUMPTION for the shape).
pub const L91_CAPACITY: &[(f64, f64)] = &[(-40.0, 0.45), (21.0, 1.0)];

// ---------------------------------------------------------------- recovery
/// ASSUMPTION: share of landings in a recoverable spot (not water, not a
/// canopy out of reach) for an inland US launch.
pub const P_SITE: f64 = 0.92;
/// ASSUMPTION: chance one tracker chain works end to end when every check
/// on it passes, by kind (a dedicated APRS tracker vs a DIY ELRS telemetry
/// bridge vs a phone that needs cell coverage at the landing site).
pub const P_TRACKER_APRS: f64 = 0.90;
pub const P_TRACKER_LORA: f64 = 0.85;
pub const P_TRACKER_ELRS: f64 = 0.80;
pub const P_TRACKER_PHONE: f64 = 0.55;
/// ASSUMPTION: chase vehicle distance from the predicted landing at touchdown, km.
pub const CHASE_OFFSET_KM: f64 = 10.0;
/// ASSUMPTION: fraction of the latex that stays on the neck after burst and
/// descends with the payload (shreds and neck; flight predictors such as
/// the habhub/sondehub predictor ignore it entirely).
pub const BALLOON_REMNANT: f64 = 0.5;
/// ASSUMPTION: cylinder residual left unusable, fraction.
pub const CYL_UNUSABLE: f64 = 0.08;
/// ASSUMPTION: suspension line must hold this multiple of the payload weight
/// (chute opening shock after burst).
pub const LINE_SHOCK_FACTOR: f64 = 5.0;

// ---------------------------------------------------- spec envelope
// The range each spec field may take (the schema's minimum/maximum and
// `Spec::validate`): an attack or a request outside it is not a condition a
// builder meets.
/// Ground temperature extremes on record for the assumed launch region (the
/// San Joaquin Valley floor, CA), C: NWS Hanford, "Coldest Temperatures ever
/// at BFL" (weather.gov/media/hnx/Coldest%20Temperatures%20ever%20at%20BFL.pdf:
/// Bakersfield 12 F, 1908-01-03 = -11.1 C) and "Top 15 Weather Events"
/// (weather.gov/media/hnx/Top15WxEvents.pdf: Bakersfield 118 F, 1908-07-28,
/// "the highest temperature recorded in the central/southern San Joaquin
/// Valley" = 47.8 C). Another region needs its own record here.
pub const GROUND_TEMP_RECORD_C: [f64; 2] = [-11.1, 47.8];
/// Burst targets the CUSF burst calculator accepts, m:
/// cuspaceflight/cusf-burst-calc js/calc.js `sanity_check_inputs`
/// ("tba < 10000" and "tba > 40000" are rejected).
pub const BURST_TARGET_M: [f64; 2] = [10_000.0, 40_000.0];
/// Launch elevations, m: Badwater Basin -86 m to Mount Whitney 4421 m, the
/// lowest and highest points of California and the contiguous US (USGS,
/// "Elevations and Distances in the United States").
pub const LAUNCH_ELEVATION_M: [f64; 2] = [-86.0, 4421.0];
/// Wind speed at any altitude, m/s: NOAA JetStream, "The Jet Stream":
/// jet streams "can reach speeds of more than 275 mph (239 kts)" = 123 m/s;
/// 125 m/s is the bound.
pub const WIND_MAX_MPS: f64 = 125.0;
/// Wind profile altitudes, m: the ground to the CUSF calculator's highest
/// burst target (`BURST_TARGET_M`) plus 10 km.
pub const WIND_ALT_MAX_M: f64 = 50_000.0;
/// ASSUMPTION (Wright's own allowance, no source): recovery windows up to
/// 72 h (three days of searching). Budgets have no ceiling.
pub const RECOVERY_WINDOW_MAX_H: f64 = 72.0;

// ---------------------------------------------------- rack shelf at landing
/// PETG allowable for the printed rack: wright-air `rules::PETG` (Bambu
/// PETG HF TDS, tensile 30-33 MPa printed XY; 20 MPa allowable for
/// layer-line weakness), the same row wright-fea's CalculiX case uses.
pub const PETG_ALLOWABLE_MPA: f64 = 20.0;
/// Stopping distance at touchdown on soft ground (a ploughed or grass
/// field), as a fraction of the foam wall: ASSUMPTION, the case wright-fea
/// runs (`cases/balloon.rs`): the box stops over the full wall thickness.
pub const LANDING_STOP_WALL_FRACTION: f64 = 1.0;
/// XPS crush stress on hard ground (asphalt, a dirt road): FOAMULAR 150 is
/// sold as "15 psi" compressive strength (catalog row
/// `xps-foamular-150`: "Owens Corning R-5 15 psi FOAMULAR NGX 150") = 103.4
/// kPa. Landing flat on rigid ground, the bottom wall crushes at this stress
/// over the box footprint, so the deceleration is that force over the
/// package mass and the stopping distance follows from the energy.
pub const XPS_CRUSH_KPA: f64 = 103.4;
/// Peak over section-average bending stress in a shelf held at its four
/// corner posts. The section average is exact statics (cut the shelf at
/// mid-span: M = W L / 8); the peak sits near the free edges. ASSUMPTION
/// 1.5: Roark's corner-supported table was not available to read;
/// wright-fea's CalculiX run of the eval shelf gives 7.4 MPa against the
/// 5.35 MPa section average, a ratio of 1.38.
pub const SHELF_PEAK_FACTOR: f64 = 1.5;
