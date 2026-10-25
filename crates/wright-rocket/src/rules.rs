//! Every number that changes an answer, with its source. Competition values
//! are from the documents listed in `evals/rocket/reference.md`.
//!
//! FAR-OUT = Friends of Amateur Rocketry FAR-OUT Rules & Requirements v4.1
//! (2026-09-09), https://static1.squarespace.com/static/64cc93ec537b6a2fa74f96ba/t/6aa1ff516daf985f5ebb7ae4/1789001554847/FAR-OUT_Rules_Requirements_2026-09-09.pdf
//! IREC = ESRA IREC Design, Test & Evaluation Guide 2025 v1.1.6,
//! https://www.soundingrocket.org/uploads/9/0/6/4/9064598/2025-irec_dteg_v1.1.6_02-14-25.pdf

/// ft/s -> m/s.
pub const FT: f64 = 0.3048;

// ---------- Competition: FAR-OUT 2026-27 ----------
/// Rail exit speed must exceed 85 ft/s (FAR-OUT 5.1.4, p.30).
pub const FAR_RAIL_EXIT_MIN_MPS: f64 = 85.0 * FT;
/// Liftoff thrust-to-weight > 8 for FAR-OUT 2027 (5.1.4, p.30).
pub const FAR_TWR_MIN: f64 = 8.0;
/// Static margin >= 1.5 cal from rail exit to burnout (5.1.5, p.30).
pub const FAR_STABILITY_MIN_CAL: f64 = 1.5;
/// FAR-OUT states no maximum; IREC DTEG 10.4.1 forbids static > 4 cal. We
/// use IREC's 4 cal at rail exit as the over-stability limit (stated
/// borrowing: weathercocking into wind grows with margin).
pub const OVERSTABLE_MAX_CAL: f64 = 4.0;
/// Burst safety factor, metallic unwelded vessels (5.2.2, p.33).
pub const FAR_BURST_SF_METAL: f64 = 2.0;
/// Burst safety factor, composite vessels and chambers (5.2.2, p.33).
pub const FAR_BURST_SF_COMPOSITE: f64 = 4.0;
/// Drogue descent 50-150 ft/s (5.6, p.53).
pub const FAR_DROGUE_MIN_MPS: f64 = 50.0 * FT;
pub const FAR_DROGUE_MAX_MPS: f64 = 150.0 * FT;
/// Main descent < 25 ft/s (5.6, p.53); touchdown <= 30 ft/s (5.6.2.5).
pub const FAR_MAIN_MAX_MPS: f64 = 25.0 * FT;
/// Launch winds up to 20 mph (5.1.5); the eval flies 0-20 mph.
pub const FAR_WIND_MAX_MPS: f64 = 20.0 * 0.44704;
/// FAR site elevation ~2000 ft MSL; FAR-OUT applies a 2000 ft MSL
/// correction (5.5, p.49).
pub const FAR_SITE_ELEVATION_M: f64 = 2000.0 * FT;
/// FAR rail options include 20 ft 1515 rails (5.4.13, p.46).
pub const FAR_RAIL_20FT_M: f64 = 20.0 * FT;
/// Team launchers 0-5 deg from vertical (p.47): we fly 5 deg.
pub const FAR_LAUNCH_ANGLE_FROM_VERTICAL_DEG: f64 = 5.0;
/// Altitude score = 1000 - 2000 |actual/contract - 1|, floored at 0 (7.3, p.75).
pub fn far_altitude_score(actual: f64, contract: f64) -> f64 {
    (1000.0 - 2000.0 * (actual / contract - 1.0).abs()).max(0.0)
}

// ---------- Engine design guidelines ----------
/// Injector pressure drop as a fraction of Pc: "the rule-of-thumb design
/// value ... varies from 15 to 20 percent of the chamber-nozzle stagnation
/// pressure" (Huzel & Huang, NASA SP-125, 1967, eq 4-40, p.128; p.123
/// gives combustion stability as the reason for a minimum drop; NTRS
/// 19710019929). Design uses 20 %; the check requires >= 15 % at the worst
/// operating point.
pub const INJ_DP_DESIGN: f64 = 0.20;
pub const INJ_DP_MIN: f64 = 0.15;
/// Discharge coefficient of a sharp-edged orifice below 2.5 mm: ~0.65
/// (Sutton & Biblarz RPE 9th ed. 2017, Table 8-2 "Injector Discharge
/// Coefficients", p.279: 0.61 above 2.5 mm, ~0.65 below).
pub const ORIFICE_CD: f64 = 0.65;
/// Pintle total momentum ratio TMR = (mdot v)_radial / (mdot v)_axial.
/// No primary source gives a number (Dressler & Bauer AIAA 2000-3871
/// discuss "relative momentums" without one); the 0.5-2 band is Wright's,
/// unsourced, around the commonly taught value of ~1.
pub const PINTLE_TMR_RANGE: (f64, f64) = (0.5, 2.0);
/// Characteristic length L* for LOX/RP-1: 40-50 in = 1.02-1.27 m (Huzel &
/// Huang SP-125 Table 4-1, p.87). No table lists LOX/propane; the RP-1 row
/// is used as a proxy (stated).
pub const LSTAR_RANGE_M: (f64, f64) = (1.02, 1.27);
pub const LSTAR_DESIGN_M: f64 = 1.10;
/// c* efficiency, small uncooled/ablative student engines: 0.90 assumed
/// (Sutton RPE 9th ed. section 3.5 quotes 0.92-0.99 for flight engines;
/// student pintle engines run lower -- unsourced estimate).
pub const ETA_CSTAR: f64 = 0.90;
/// Conical nozzle half angle 15 deg; divergence factor (1 + cos a)/2
/// (Sutton & Biblarz RPE 9th ed. eq 3-34, p.75). No large open-source Rao
/// bell implementation exists to follow (GitHub search 2026-10-24 finds
/// only single-author repos), so Wright uses a conical nozzle.
pub const NOZZLE_HALF_ANGLE_DEG: f64 = 15.0;
/// Converging half angle 30 deg (Wright choice; common practice 20-45 deg).
pub const CONV_HALF_ANGLE_DEG: f64 = 30.0;
/// Contraction ratio rule of thumb Ac/At = 8.0 Dt^-0.6 + 1.25, Dt in cm,
/// valid 0.7-200 cm (Humble, Henry & Larson, Space Propulsion Analysis and
/// Design, 1995, as quoted in USU MAE 5540 sec. 6.1 slide 37,
/// mae-nas.eng.usu.edu/MAE_5540_Web/propulsion_systems/section6/section.6.1.pdf).
pub fn contraction_ratio(dt_m: f64) -> f64 {
    8.0 * (dt_m * 100.0).powf(-0.6) + 1.25
}

// ---------- Materials ----------
/// 6061-T6 aluminium: yield 276 MPa, ultimate 310 MPa at room temperature
/// (ASM Aerospace Specification Metals, asm.matweb.com 6061-T6).
pub const AL6061_YIELD_PA: f64 = 276e6;
pub const AL6061_ULT_PA: f64 = 310e6;
pub const AL6061_DENSITY: f64 = 2700.0;
/// 6061-T6 keeps 100 % of room-temperature Ftu/Fty after 100 h at 150 C
/// (MMPDS-01 Fig 3.6.2.2.1(c),(d), as quoted in LIGO T1000564); strength
/// *at* temperature falls (~290 MPa at 100 C). Wright caps the case at
/// 120 C so room-temperature allowables stay conservative.
pub const AL6061_MAX_CASE_K: f64 = 393.15;
/// Silica phenolic liner: density 1.45-1.76 g/cm^3, k 0.37-0.43 W/m K,
/// cp 1.13-1.38 kJ/kg K (NASA SP-8124 "Liquid Rocket Engine Self-Cooled
/// Combustion Chambers", 1977, Table II p.17, NTRS 19780013268). Wright
/// takes the conductive end (k 0.43) and mid density/cp. Pyrolysis onset
/// 600 K is an unsourced Wright assumption (phenolic resins decompose from
/// ~300-400 C).
pub const PHENOLIC_DENSITY: f64 = 1600.0;
pub const PHENOLIC_K: f64 = 0.43;
pub const PHENOLIC_CP: f64 = 1250.0;
pub const PHENOLIC_PYROLYSIS_K: f64 = 600.0;
/// Char surface temperature of a charring phenolic under a hot gas, ~1650 K
/// (assumed; surface runs below Taw because pyrolysis gas blows the boundary layer).
pub const PHENOLIC_SURFACE_K: f64 = 1650.0;
/// Liner design margin on predicted char depth.
pub const ABLATIVE_SF: f64 = 1.5;
/// Fibreglass (G10/FR4) airframe: density 1850 kg/m^3 (matweb G10).
pub const FIBERGLASS_DENSITY: f64 = 1850.0;
/// Carbon-fibre-overwrapped GN2 bottle burst factor already in its rating;
/// we read service pressure from the catalog and require catalog burst >=
/// 4 x MEOP (FAR composite SF).
pub const GN2_GAMMA: f64 = 1.4;
pub const GN2_R: f64 = 296.8;

// ---------- Recovery ----------
/// Main deploy altitude, metres AGL (FAR: below 1/3 of contract altitude
/// and above 1000 ft AGL; we pick 1500 ft).
pub const MAIN_DEPLOY_AGL_M: f64 = 1500.0 * FT;

// ---------- Mass build-up (unsourced Wright allowances, stated) ----------
/// Avionics + wiring + batteries + GPS for a liquid rocket with valves, kg.
pub const AVIONICS_KG: f64 = 2.0;
/// Plumbing (lines, fittings, valves, regulator) per propellant circuit, kg.
pub const PLUMBING_PER_CIRCUIT_KG: f64 = 1.5;
/// Thrust structure + engine mount, kg per kN of thrust.
pub const THRUST_STRUCTURE_KG_PER_KN: f64 = 0.6;
/// Ullage fraction of tank volume.
pub const ULLAGE: f64 = 0.10;
/// Unusable residual propellant fraction.
pub const RESIDUAL: f64 = 0.03;
/// Regulator mass when the catalog lists none (Ninja Pro V3 pages give no
/// mass): 0.3 kg allowance, Wright assumption.
pub const REGULATOR_MASS_ALLOWANCE_KG: f64 = 0.3;

// ---------- Self-pressurised nitrous oxide (see nitrous.rs) ----------
/// Fraction of tank pressure lost in lines and valves before the injector
/// (Wright allowance, 10 %, unsourced; the same value `VehicleParams::feed_loss`
/// carries for the regulated circuits).
pub const FEED_LOSS: f64 = 0.10;
/// N2O run-tank bulk temperature at launch. Vapour pressure follows the
/// temperature (NIST: 35.4 bar at 5 C, 50.5 bar at 20 C, 63.1 bar at 30 C),
/// so the design assumes the team holds the tank in a band with a heater
/// blanket and checks both ends: nominal 20 C, a cold desert morning 5 C,
/// a hot pad 30 C. The band is a Wright assumption (unsourced); the
/// pressures at each end come from data/rocket/n2o_saturation.json.
pub const N2O_FILL_NOMINAL_K: f64 = 293.15;
pub const N2O_FILL_COLD_K: f64 = 278.15;
pub const N2O_FILL_HOT_K: f64 = 303.15;
/// Liquid discharge coefficient for sharp-edged N2O orifices, 0.63 (Nino &
/// Razavi AIAA 2019-4154 sec. IV.I: Cd = 1/sqrt(4 Cf L/D + K), K = 2.28,
/// "approximately 0.63 for all cases" on Waxman's 0.79-1.93 mm orifices),
/// applied to the Dyer model as they do.
pub const N2O_ORIFICE_CD: f64 = 0.63;
/// Largest N2O orifice Wright drills: the Dyer model is validated on
/// Waxman's 0.79-1.93 mm orifices (Nino & Razavi Table 2); 2.0 mm cap.
pub const N2O_HOLE_MAX_M: f64 = 2.0e-3;
/// Impingement angle of each N2O jet on the axial fuel jet of the O-F-O
/// triplet: "The satisfactory design value for the included angle is
/// usually found to be between 20 and 45 deg" (Huzel & Huang SP-125, 1967,
/// p.125). Wright uses 30 deg.
pub const IMPINGEMENT_ANGLE_DEG: f64 = 30.0;
/// Resultant (beta) angle of an element's momentum from the chamber axis
/// (Huzel & Huang p.128-129, eq 4-41): positive beta (toward the wall) of
/// 2-5 deg is the most H&H recommend, and only for hypergolics. Wright
/// requires |beta| <= 5 deg over every flown blowdown.
pub const INJ_BETA_MAX_DEG: f64 = 5.0;

// ---------- Bulkhead closure (radial screws through the tank wall) ----------
/// Each tank bulkhead is retained by radial 1/4-20 socket head cap screws
/// through the 6061 tube wall into the bulkhead rim (the common student
/// closure). Screw: 1/4 in nominal, thread root area 0.0269 in^2 (ASME
/// B1.1 UNC tables, 1/4-20), ASTM A574 minimum tensile 180 ksi for sizes
/// <= 1/2 in; shear ultimate taken as 0.6 x tensile (common ratio for
/// steel fasteners, assumption). The supplier page does not state the
/// grade: A574 is the standard for alloy-steel SHCS (assumption).
pub const CLOSURE_SCREW_D_M: f64 = 0.25 * 0.0254;
pub const CLOSURE_SCREW_ROOT_AREA_M2: f64 = 0.0269 * 0.0254 * 0.0254;
pub const A574_TENSILE_PA: f64 = 180.0 * 6.894_757e6;
pub const STEEL_SHEAR_OVER_TENSILE: f64 = 0.6;
/// 6061-T6 bearing ultimate / yield at e/D = 2 and shear ultimate (ASM
/// Aerospace Specification Metals 6061-T6 data sheet, the source of
/// AL6061_YIELD_PA: 607 / 386 / 207 MPa).
pub const AL6061_BEARING_ULT_PA: f64 = 607e6;
pub const AL6061_BEARING_YIELD_PA: f64 = 386e6;
pub const AL6061_SHEAR_ULT_PA: f64 = 207e6;
/// Screw centre to tube end: 2 diameters (the e/D the bearing values are for).
pub const CLOSURE_EDGE_OVER_D: f64 = 2.0;

// ---------- Mass calibration ----------
/// Wright's dry-mass build-up times this is the dry mass a vehicle is
/// designed and checked with. 1.86 = 79.0 kg / 42.4 kg: the dry mass that
/// makes Wright's trajectory model fly its Eureka-2 reconstruction to the
/// recorded 15,879 ft (and then also predicts the recorded max Mach, 0.93
/// vs 0.88), over Wright's build-up of the same vehicle. One flight, one
/// vehicle; see evals/rocket/reference.md "Mass calibration". A spec may
/// override it (`dry_mass_factor`), a weighed vehicle (`measured_dry`)
/// replaces it.
pub const EUREKA2_DRY_MASS_FACTOR: f64 = 1.86;

/// Drilled-orifice diameter tolerance for the injector, +/- m. The two N2O
/// jets of an O-F-O triplet cancel only if their holes match; Wright checks
/// the resultant angle with one hole 0.05 mm over and its partner 0.05 mm
/// under (Wright assumption for a drilled-and-reamed small hole, unsourced).
pub const ORIFICE_TOL_M: f64 = 0.05e-3;

// ---------- Pressurant fill ----------
/// LOX designs: nitrogen only (compressed air over LOX is a hazard), filled
/// to the bottles' 4500 psi rating. No 4500 psi nitrogen source had a
/// readable price on 2026-10-24 (6K cylinders and boosters are quote-only;
/// paintball "nitrogen (HPA)" is compressed air): the BOM carries the fill
/// as an unpriced line with that reason, so the budget check fails until a
/// quote is entered. The only priced nitrogen, a 2000 psi 80 cu ft cylinder
/// (catalog gn2_cylinder_80cf_full_cws), cascade-fills a bottle to well
/// under 2000 psi, where a calibrated LOX design would need 25-30 bottles.
pub const GN2_FILL_PSI: f64 = 4500.0;
/// N2O designs pressurise only the fuel tank, with compressed air from a
/// paintball HPA fill station at the bottles' 4500 psi rating (labelled as
/// air in the BOM; no LOX on board).
pub const HPA_FILL_PSI: f64 = 4500.0;
/// N2 compressibility at 293 K, linear between NIST WebBook isotherm points
/// 100 bar (114.91 kg/m^3, Z = 1.001) and 310 bar (309.93 kg/m^3, Z = 1.150);
/// 1.0 below 100 bar. (Was a flat 1.10.) Used for air as well (stated).
pub fn n2_z(p_pa: f64) -> f64 {
    let p = p_pa / 1e5;
    if p <= 100.0 { 1.0 } else { 1.001 + (p - 100.0) / 210.0 * (1.150 - 1.001) }
}

/// Lowest chamber contraction ratio Wright will use when fitting the liner
/// to stock: the bottom of Huzel & Huang's 2-5 range for pressure-fed
/// low-thrust engines (SP-125 p.88). Lower ratios raise chamber Mach and
/// with it Taw and Bartz h, which the thermal checks recompute; the
/// stagnation-pressure loss (Sutton & Biblarz sec. 3.3, Table 3-2: a few
/// tenths of a percent at Ac/At >= 2) is not modelled.
pub const CONTRACTION_MIN: f64 = 2.0;

// ---------- Spec envelope (what a person or a reviewer may ask for) ----------
// The launch site's elevation, wind and temperatures are not spec fields:
// the competition fixes them (FAR site 2,000 ft MSL, winds to 20 mph,
// FAR-OUT 5.1.5; IREC 1,400 m, DTEG 6.3.2; N2O fill 5-30 C above), so they
// cannot be attacked through the spec. The fields that can:
pub const IREC_RULES_URL: &str = "https://www.soundingrocket.org/uploads/9/0/6/4/9064598/irec_rules_and_requirements_document_v_1.6_.pdf";
/// FAR-OUT contract altitude groups, ft AGL: A 3,000-15,000 and B
/// 20,000-50,000 (FAR-OUT 7.3, p.75). Group C (50,000-250,000 ft) is
/// outside Wright's validated envelope (planar 3-DOF, no upper-atmosphere
/// recovery model) and refused.
pub const FAR_GROUP_A_FT: (f64, f64) = (3_000.0, 15_000.0);
pub const FAR_GROUP_B_FT: (f64, f64) = (20_000.0, 50_000.0);
/// IREC liquid categories: 10,000 or 30,000 ft AGL (IREC Rules v1.6 sec.
/// 2.0, p.11).
pub const IREC_CATEGORIES_FT: [f64; 2] = [10_000.0, 30_000.0];
/// Rail lengths, m: FAR's pad has 10-60 ft rails (FAR-OUT 5.4.13, p.46);
/// IREC requires >= 5.2 m (17 ft), DTEG 11.1.2, and Wright caps it at the
/// longest FAR rail.
pub const FAR_RAIL_RANGE_M: (f64, f64) = (10.0 * FT, 60.0 * FT);
pub const IREC_RAIL_RANGE_M: (f64, f64) = (5.2, 60.0 * FT);
/// Budget, USD: positive, and at most $100,000 (above the largest published
/// student liquid-program cost Wright found: SEB's Eureka-1 at $65-75k
/// development, https://engineering.berkeley.edu/news/2023/05/a-eureka-1-moment/).
pub const BUDGET_MAX_USD: f64 = 100_000.0;
/// Payload, kg: 0-20 (IREC's minimum payload is 2.0 kg, Rules sec. 2.0
/// p.11; 20 kg is Wright's envelope, unsourced -- heavier payloads are
/// outside the mass build-up's range).
pub const PAYLOAD_RANGE_KG: (f64, f64) = (0.0, 20.0);
/// c* efficiency: 0.5-1.0. Above 1 is impossible; SEB's demonstrated 0.71
/// (evals/rocket) is the lowest published student value Wright has; 0.5 is
/// a Wright floor (unsourced).
pub const ETA_CSTAR_RANGE: (f64, f64) = (0.5, 1.0);
/// Dry-mass calibration: 0.5-3.0 (the Eureka-2 fit is 1.86; outside this a
/// calibration says more about the model than the vehicle; unsourced).
pub const DRY_MASS_FACTOR_RANGE: (f64, f64) = (0.5, 3.0);
/// A spec's nose CD(M) table entries: 0 (no wave drag) to the stagnation
/// CD of a flat face at high Mach (OpenRocket `calculateStagnationCD`,
/// 0.85 x 1.84 = 1.56 as M -> inf; 1.2 covers M 2.5).
pub const NOSE_CD_RANGE: (f64, f64) = (0.0, 1.2);
/// Weighed dry mass, kg, and CG from the nose, m: 5-500 kg, 0-20 m
/// (Wright's envelope; Eureka-2 reconstructs at 42-79 kg, 5.9 m).
pub const MEASURED_DRY_KG: (f64, f64) = (5.0, 500.0);
pub const MEASURED_CG_M: (f64, f64) = (0.0, 20.0);
