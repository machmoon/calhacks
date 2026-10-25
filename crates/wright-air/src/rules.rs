//! Every number that changes an answer, with where it came from.
//!
//! A constant here is a claim and names its source so a reviewer can check it
//! and a measurement can replace it. Assumptions nobody published say so
//! ("ASSUMPTION"). Values are SI unless the name carries the unit.
//!
//! Most of these were collected for the Python baseline
//! (`silkscreen/aircraft/rules.py`) and re-checked here; the rest are new
//! with the checks that use them.

// ---------------------------------------------------------------------------
// Atmosphere: ISA troposphere (ICAO Doc 7488/3; U.S. Standard Atmosphere 1976).

pub const ISA_T0_K: f64 = 288.15;
pub const ISA_P0_PA: f64 = 101_325.0;
pub const ISA_LAPSE_K_PER_M: f64 = 0.0065;
pub const ISA_R_J_PER_KG_K: f64 = 287.052_87;
pub const ISA_GAMMA: f64 = 1.4;
/// Sutherland's law for air (White, Viscous Fluid Flow, 3rd ed., table 1-2).
pub const SUTHERLAND_MU_REF: f64 = 1.716e-5;
/// ISA sea-level density, kg/m^3 (EAS is referred to it).
pub const ISA_RHO0_KG_M3: f64 = 1.225;
pub const SUTHERLAND_T_REF_K: f64 = 273.15;
pub const SUTHERLAND_S_K: f64 = 110.4;

// ---------------------------------------------------------------------------
// Materials.

#[derive(Debug, Clone, Copy)]
pub struct Material {
    pub name: &'static str,
    pub density_kg_m3: f64,
    pub modulus_pa: f64,
    /// Allowable stress at ultimate load: strength over the material's own
    /// knock-down, stated per material.
    pub allowable_pa: f64,
    pub source: &'static str,
}

/// Foaming PLA printed single-wall (skins only; never counted as structure).
pub const LW_PLA: Material = Material {
    name: "foaming PLA",
    density_kg_m3: 500.0,
    modulus_pa: 1.1e9,
    allowable_pa: 10e6,
    source: "reference author measured ~0.45 g/cm3 single wall (Show HN 44241278); maker TDS 0.5-0.8",
};
/// PETG printed fittings: Bambu PETG HF TDS 1.25 g/cm3, tensile 30-33 MPa
/// printed XY; 20 MPa allowable for layer-line weakness.
pub const PETG: Material = Material {
    name: "PETG",
    density_kg_m3: 1250.0,
    modulus_pa: 1.9e9,
    allowable_pa: 20e6,
    source: "Bambu PETG HF TDS",
};
/// Pultruded carbon tube or rod: E 125-140 GPa, compressive strength about
/// 1.2 GPa (Rock West T-RND rows in the catalog: 133.8 GPa, 2372 MPa tensile).
/// Allowable 500 MPa: compression governs in bending and thin walls buckle
/// locally first; about 0.4 x the compressive coupon strength, hobby-UAV
/// practice.
pub const CF_PULTRUDED: Material = Material {
    name: "pultruded carbon",
    density_kg_m3: 1550.0,
    modulus_pa: 125e9,
    allowable_pa: 500e6,
    source: "Rock West T-RND-314 page (catalog); 0.4 x compressive strength",
};
/// Roll-wrapped carbon tube: Rock West 46704 page 63.3 GPa, 435 MPa;
/// Easy Composites CFT-WF 64 GPa, 650 MPa. Allowable 350 MPa.
pub const CF_ROLLED: Material = Material {
    name: "roll-wrapped carbon",
    density_kg_m3: 1550.0,
    modulus_pa: 63e9,
    allowable_pa: 350e6,
    source: "Rock West 46704 / Easy Composites CFT-WF pages (catalog)",
};

/// A single-perimeter foaming-PLA skin: one 0.4 mm nozzle line widened by
/// foaming (Bambu PLA-Aero guide: line width about 0.45 mm).
pub const SKIN_WALL_M: f64 = 0.45e-3;
/// Ribs, spar sleeves and servo bays as a multiple of skin mass. Eclipson /
/// Flightory published part masses for single-wall geodesic wings run 30-40 %
/// over the skin alone.
pub const WING_INTERNAL_FACTOR: f64 = 1.35;
/// Pod wall: two foamed perimeters, m (ASSUMPTION, the prototype's).
pub const POD_WALL_M: f64 = 1.2e-3;
/// Pod internal frames over its skin mass (ASSUMPTION: bulkheads at the
/// wing saddle and the pusher mount, 25 %).
pub const POD_INTERNAL_FACTOR: f64 = 1.25;

// ---------------------------------------------------------------------------
// Printers: build volume, mm (manufacturers' specifications).

pub const PRINTERS: &[(&str, [f64; 3])] = &[
    ("bambu_a1", [256.0, 256.0, 256.0]),
    ("bambu_a1_mini", [180.0, 180.0, 180.0]),
    ("bambu_p1s", [256.0, 256.0, 256.0]),
    ("bambu_x1c", [256.0, 256.0, 256.0]),
    ("prusa_mk4", [250.0, 210.0, 220.0]),
    ("prusa_core_one", [250.0, 220.0, 270.0]),
    ("creality_k1", [220.0, 220.0, 250.0]),
    ("voron_350", [350.0, 350.0, 340.0]),
];
/// Margin each printed piece keeps inside the bed on every side, mm (brim and
/// the purge line: Bambu's default exclusion strip is 2-5 mm).
pub const BED_MARGIN_MM: f64 = 3.0;

// ---------------------------------------------------------------------------
// Loads and margins.

/// Limit manoeuvre load factor. 14 CFR 23.337 (pre-amendment 64) normal
/// category is 3.8; small electric UAVs are designed to 4, which also covers
/// the 23.333 gust lines at these wing loadings.
pub const LIMIT_LOAD_FACTOR: f64 = 4.0;
/// Ultimate = 1.5 x limit (14 CFR 23.2230 / old 23.303).
pub const ULTIMATE_FACTOR: f64 = 1.5;
/// Static margin band, fraction of MAC: Raymer, Aircraft Design, 6th ed.,
/// sect. 16.3 (5-15 % for stable, controllable small aircraft).
pub const STATIC_MARGIN_MIN: f64 = 0.05;
pub const STATIC_MARGIN_MAX: f64 = 0.15;
pub const STATIC_MARGIN_TARGET: f64 = 0.10;
/// Tail volume coefficients: Raymer table 6.4 (sailplane V_H 0.50, V_V 0.02;
/// homebuilt 0.50 / 0.04); a long-range UAV sits between.
pub const TAIL_VH: f64 = 0.45;
pub const TAIL_VV: f64 = 0.030;
/// The horizontal tail must trim the aircraft at the stall with at most this
/// fraction of its section's CLmax (elevator deflected): the rest is the
/// authority left to recover. ASSUMPTION (common practice 0.6-0.8).
pub const TAIL_CL_USE_MAX: f64 = 0.8;
/// Hover thrust-to-weight minimum, all four motors at full throttle:
/// ArduPilot QuadPlane guidance (hover at no more than about 50 % throttle,
/// ardupilot.org/plane/docs/quadplane-frame-setup.html).
pub const HOVER_TW_MIN: f64 = 1.8;
/// Highest acceptable hover throttle, hot day, low battery: ArduPilot's
/// Q_M_THST_HOVER range tops at 0.6875 (apm.pdef.json); 0.65 keeps authority.
pub const HOVER_THROTTLE_MAX: f64 = 0.65;
/// `max_stall_ms` (default 14 m/s; ASSUMPTION carried from the prototype,
/// no published source) is an EQUIVALENT airspeed. The aircraft takes off
/// and lands vertically, so the stall limit's job is the transition and the
/// autopilot's airspeed floor, and ArduPilot holds those in EAS: AP_TECS
/// `_update_speed()` sets `_TASmin = aparm.airspeed_min * EAS2TAS` and, on
/// landing, `aparm.airspeed_stall * EAS2TAS` (ArduPilot master,
/// libraries/AP_TECS/AP_TECS.cpp). A wing stalls at a fixed EAS for a given
/// CLmax, so the check is density-independent except through CLmax's
/// Reynolds number, which is taken at the field's density. The TAS stall at
/// the field is reported as an Info headline. (No dead-stick belly landing is
/// planned, the one case where touchdown TAS energy would matter.)
pub const STALL_IS_EAS: bool = true;
/// Cruise no slower than 1.3 x stall (14 CFR 25.107-style V2/Vs).
pub const CRUISE_OVER_STALL_MIN: f64 = 1.3;
/// Top speed at least 1.3 x best-range speed: headroom to penetrate wind.
pub const MAX_OVER_CRUISE_MIN: f64 = 1.3;
/// Pusher throttle at best-range cruise at most this (headroom to climb).
pub const CRUISE_THROTTLE_MAX: f64 = 0.9;
/// A boom's first bending mode must sit at least 30 % away from the hover
/// rotation rate (resonance avoidance, multirotor arm practice).
pub const BOOM_FREQUENCY_SEPARATION: f64 = 0.3;
/// Clearance a spinning prop disc keeps from anything else, m.
pub const PROP_CLEARANCE_M: f64 = 0.005;
/// Clearance two solids that are not joined keep, m (print tolerance).
pub const SOLID_CLEARANCE_M: f64 = 0.0;
/// Two joined parts must come within this of each other, m: a part declared
/// attached and floating free fails.
pub const JOINT_CONTACT_M: f64 = 0.0005;
/// Wing depth an aileron servo must leave over itself, m.
pub const SERVO_SPARE_M: f64 = 0.0005;
/// The pusher's thrust line within this of the CG's height, m: a low thrust
/// line pitched the reference's predecessor up in transition (tsungxu.com,
/// "First VTOL Design, Build and Flight Success").
pub const THRUST_LINE_MAX_M: f64 = 0.015;
/// The feet below every other point by at least this, m.
pub const GROUND_CLEARANCE_M: f64 = 0.015;

// ---------------------------------------------------------------------------
// Mission.

/// Energy left at landing, fraction of usable (ArduPilot BATT_LOW_MAH
/// guidance; a long-range UAV plans 15-20 %).
pub const ENERGY_RESERVE: f64 = 0.15;
/// Usable fraction of rated energy to cutoff at under 1C: Molicel and
/// Amprius datasheet discharge curves at 0.2-0.5C about 0.95; LiPo 0.90.
pub const USABLE_FRACTION_LIION: f64 = 0.95;
pub const USABLE_FRACTION_LIPO: f64 = 0.90;
/// Pack over cells: strip, wire, connector, wrap. The reference pack is
/// 1350 g for twelve SA08 cells of 106.5 g = 1.056; a hand-built 21700 pack
/// with holders and strip runs higher: 1.08.
pub const PACK_OVERHEAD: f64 = 1.08;
/// Hover seconds per take-off and landing (ArduPilot QuadPlane: 20-40 s in
/// practice; 60 s budgets a go-around).
pub const HOVER_TAKEOFF_S: f64 = 45.0;
pub const HOVER_LANDING_S: f64 = 60.0;
/// Transition with quad assist, s, each way.
pub const TRANSITION_S: f64 = 15.0;
/// Climb to cruise altitude.
pub const CLIMB_ALTITUDE_M: f64 = 100.0;
/// Overall efficiency of the climb (potential energy over battery energy).
/// ASSUMPTION: prop x motor x ESC at climb power about 0.5.
pub const CLIMB_EFFICIENCY: f64 = 0.5;
/// Lift motors' thrust over weight/4 in hover: booms, motors and the pod's
/// edge in the wash ("download"). The discs clear the wing in plan view, so
/// 3 % (ASSUMPTION, the prototype's).
pub const HOVER_DOWNLOAD: f64 = 1.03;

// ---------------------------------------------------------------------------
// Propulsion and electrical.

/// ESC efficiency at part load (BLHeli_32/AM32 bench data are 0.92-0.97).
pub const ESC_EFFICIENCY: f64 = 0.94;
/// Avionics draw, W: FC + GPS + receiver + airspeed about 3-5 W; servos in
/// cruise about 1-2 W.
pub const AVIONICS_POWER_W: f64 = 6.0;
/// A 9-12 g servo's stall current at 6 V (EMAX/KST/Corona datasheets put
/// 0.8-1.2 A), used where the catalog row states none.
pub const SERVO_STALL_A: f64 = 1.0;
pub const SERVO_AVERAGE_A: f64 = 0.15;
pub const SERVO_RAIL_V: f64 = 6.0;
/// Seconds a full-throttle burst lasts (the pack makers' burst ratings are
/// for 10 s: Upgrade Energy GOLD page "burst 112 A for 10 s").
pub const BURST_S: f64 = 10.0;
/// Copper resistivity at 20 C, ohm m (IEC 60028); temperature coefficient
/// 0.00393 /K (IEC 60028); density 8960 kg/m3; specific heat 385 J/(kg K).
pub const COPPER_RESISTIVITY: f64 = 1.724e-8;
pub const COPPER_ALPHA: f64 = 0.003_93;
pub const COPPER_DENSITY: f64 = 8960.0;
pub const COPPER_CP: f64 = 385.0;
/// Silicone insulation's continuous rating, C (sellers' 200 C silicone wire).
pub const SILICONE_MAX_C: f64 = 200.0;
/// Hot-day ambient inside a sun-heated pod, C (ASSUMPTION).
pub const AMBIENT_HOT_C: f64 = 45.0;
/// Heat transfer from a wire in still air, natural convection plus
/// radiation, W/(m^2 K): 10-15 for small cylinders (Incropera, ch. 9,
/// horizontal cylinders at D = 2-6 mm). 10 used: conservative, no airflow.
pub const WIRE_H_W_M2K: f64 = 10.0;
/// Most voltage a run may drop at its continuous current, of pack nominal.
pub const WIRE_DROP_MAX: f64 = 0.02;
/// Silicone wire by AWG: (conductor area mm^2, sellers' continuous rating A,
/// mass g/m, outer diameter mm). Areas from the AWG definition; ratings and
/// masses from Turnigy/BNTECHGO silicone-wire listings; ODs from BNTECHGO's
/// listing (10 AWG 5.0, 12 AWG 4.0 ... 22 AWG 1.5 mm). The 8 AWG rating is
/// the 10 AWG one scaled by area^0.725 (sellers claim 190-200 A, so it is low).
pub const WIRE_AWG: &[(u32, f64, f64, f64, f64)] = &[
    (8, 8.37, 125.0, 97.0, 6.5),
    (10, 5.26, 90.0, 61.0, 5.0),
    (12, 3.31, 60.0, 39.0, 4.0),
    (14, 2.08, 40.0, 25.0, 3.2),
    (16, 1.31, 25.0, 16.0, 2.6),
    (18, 0.82, 16.0, 10.0, 2.2),
    (20, 0.52, 10.0, 6.5, 1.8),
    (22, 0.33, 6.0, 4.2, 1.5),
];
/// Battery connectors: (name, continuous A, burst A, catalog hardware id).
/// AMASS XT60 30 A cont / 60 A burst; XT90 45 A / 90 A (AMASS datasheets);
/// AS150 70 A / 150 A (Rotorama's AMASS AS150 listing, catalog row).
pub const CONNECTORS: &[(&str, f64, f64, &str)] = &[
    ("XT60", 30.0, 60.0, "amass_xt60h_connector_piece"),
    ("XT90", 45.0, 90.0, "amass_xt90s_connector_piece"),
    ("AS150", 70.0, 150.0, "amass_as150_connector_set"),
];
/// Routing allowance over the Manhattan path for every wire run.
pub const WIRE_ROUTING: f64 = 1.15;

// ---------------------------------------------------------------------------
// Drag bookkeeping the component buildup cannot derive from geometry.

/// A stopped two-blade prop: blades near their pitch angle to the flow,
/// sections 15-25 deg past stall, CD about 0.3-0.5 on blade planform
/// (Sheridan et al., AIAA 2020-2731, stopped-rotor drag). 0.4.
pub const STOPPED_BLADE_CD: f64 = 0.4;
/// A stopped folding prop trails its blades: friction on both faces plus root
/// and thickness pressure drag, 0.06 on planform (conservative; 2 x Cf is
/// about 0.012 at blade Re 5e4-1e5, AeroSandbox Cf_flat_plate).
pub const FOLDED_BLADE_CD: f64 = 0.06;
/// Blade planform over disc area for a two-blade prop (solidity 0.08-0.10).
pub const PROP_SOLIDITY_2BLADE: f64 = 0.09;
/// A bare motor bell in crossflow: a short cylinder, CD about 0.8 on frontal
/// area (Hoerner, Fluid-Dynamic Drag, ch. 3, finite cylinders).
pub const MOTOR_BELL_CD: f64 = 0.8;
/// A motor whose bell length is unpublished is drawn and dragged at this
/// length, m: an envelope longer than any 28xx-31xx bell, never a claim.
pub const UNKNOWN_BELL_LENGTH_M: f64 = 0.030;
/// Faired landing strut, CD on frontal area (Hoerner ch. 6, streamlined
/// struts at thickness ratio 0.3-0.4: about 0.1).
pub const FAIRED_STRUT_CD: f64 = 0.10;
/// Excrescence and interference: gaps, horns, seams, pitot, antennas.
/// Raymer sect. 12.5.5: 5-15 % on propeller aircraft; 10 %.
pub const EXCRESCENCE_FACTOR: f64 = 1.10;
/// Horizontal tail span efficiency for its induced drag (ASSUMPTION 0.9).
pub const TAIL_SPAN_EFFICIENCY: f64 = 0.9;
/// Tail efficiency (dynamic pressure ratio) in the neutral point: Etkin and
/// Reid, sect. 2.3, typical 0.9.
pub const TAIL_ETA: f64 = 0.9;
/// Raymer eq. 16.25's fuselage pitching-moment factor K_fus, PER DEGREE,
/// for a pod whose wing quarter-chord root sits near 35 % of its length
/// (Raymer fig. 16.14). ASSUMPTION: read off the figure at one point.
pub const K_FUS_PER_DEG: f64 = 0.010;

// ---------------------------------------------------------------------------
// Wing joint.

/// Each half of the spar joiner runs this fraction of the semi-span into its
/// wing half. ASSUMPTION: RC joiners run a fifth to a third of the semi-span;
/// the spar is checked where the joiner ends, so a short joiner shows up as
/// spar stress, never as a silent pass.
pub const JOINER_HALF_FRAC: f64 = 0.25;
/// Largest diametral clearance between joiner and spar that epoxy still
/// bonds, m.
pub const JOINER_FIT_M: f64 = 0.4e-3;
/// Main and rear spar stations as fractions of the TIP chord (each spar is
/// straight, at constant x), and how far each runs out (fraction of the
/// semi-span). The prototype's layout; builders' practice.
pub const MAIN_SPAR_TIP_FRAC: f64 = 0.28;
pub const REAR_SPAR_TIP_FRAC: f64 = 0.62;
pub const MAIN_SPAR_REACH: f64 = 0.9;
pub const REAR_SPAR_REACH: f64 = 0.8;

// ---------------------------------------------------------------------------
// Landing.

/// Limit landing load factor: 14 CFR 23.473(g) (amdt 23-48 and earlier):
/// not less than 2.67; ultimate 1.5 x.
pub const LANDING_LOAD_FACTOR: f64 = 2.67;
/// Legs that take a tilted touchdown's load.
pub const LANDING_LEGS_LOADED: f64 = 2.0;
/// Side load at the foot over the vertical (Raymer sect. 17.5: 0.5 for
/// unprepared ground).
pub const FOOT_FRICTION: f64 = 0.5;

// ---------------------------------------------------------------------------
// Layout (geometry the CAD and the physics share).

/// A prop disc's clearance target from structure when placing parts, m.
pub const LAYOUT_GAP_M: f64 = 0.015;
/// Rear lift discs to the tail's leading edge, m.
pub const TAIL_GAP_M: f64 = 0.025;
/// Pod nose ahead of the bay and cone behind the battery, m.
pub const NOSE_LENGTH_M: f64 = 0.06;
pub const TAIL_CONE_M: f64 = 0.07;
/// Battery to the pod's inner wall, m.
pub const BATTERY_CLEARANCE_M: f64 = 0.004;
/// Bay stack spacing, m.
pub const BAY_GAP_M: f64 = 0.004;
/// Pod cross-section superellipse exponent: n = 5 is the rounded box
/// printed fuselages are; a box of half-sizes (w, h) fits in (w, h) x 2^(1/n).
pub const POD_EXPONENT: f64 = 5.0;
/// Lift motor mount above the boom top, prop plane above the motor top, m.
pub const MOUNT_ABOVE_BOOM_M: f64 = 0.005;
pub const PROP_ABOVE_MOTOR_M: f64 = 0.004;
/// Disc slab thickness drawn for a spinning prop, m (blade sweep at the hub).
pub const LIFT_DISC_THICK_M: f64 = 0.010;
pub const PUSHER_DISC_THICK_M: f64 = 0.012;
/// Boom axis below the wing's lowest point, m (saddle clamp material).
pub const BOOM_BELOW_WING_M: f64 = 0.004;
/// Legs: faired PETG struts, chord = 2.75 x thickness (NACA 0036-ish).
pub const LEG_CHORD_OVER_THICKNESS: f64 = 2.75;
/// A lift-motor mount's mass, kg, for the boom's tip mass in the resonance
/// check (the layout weighs the drawn mount; comparable parts 8-14 g).
pub const MOTOR_MOUNT_KG: f64 = 0.012;
/// Glue, screws, inserts, horns, pushrods, kg (ASSUMPTION).
pub const HARDWARE_KG: f64 = 0.045;
/// Filament wasted in printing: purge, brim, a failed panel (ASSUMPTION 25 %).
pub const PRINT_WASTE: f64 = 0.25;
/// A folding prop's spinner and hub, kg (Aeronaut Z-spinner 42 mm with its
/// collet weighs about 15-20 g; ASSUMPTION 0.018).
pub const FOLDING_HUB_KG: f64 = 0.018;

// ---------------------------------------------------------------------------
// Model uncertainty the SEARCH keeps in hand (the checks keep their limits).

/// The lift thrust model against every maker thrust table in the catalog
/// whose prop is also in the catalog (11 motor/prop pairs, 12 rows at 100 %
/// throttle; `powertrain::thrust_tables` re-derives both numbers):
/// at the maker's measured current the model's thrust is -12 % .. +3.0 %
/// of the table (worst +3.0 %), so where a current rating (pack burst, ESC, motor) stops
/// the lift motors the model over-predicts thrust by at most this;
pub const THRUST_AT_CURRENT_OVER: f64 = 0.035;
/// ... but at the maker's voltage with the current left to the model, it
/// over-predicts by up to +97 % (T-Motor MN3110 470 kV on a 13x4.4 CF: 15.0
/// N vs 7.6 N), because it spins the prop faster than the maker measured.
/// A lift unit stopped by its voltage is credited with its thrust over
/// (1 + this) in the search.
pub const THRUST_VOLTAGE_LIMITED_OVER: f64 = 0.97;
/// ... and for a motor whose supply voltage is calibrated on one of those
/// rows (`catalog::calibrate`), the worst error on the rows the fit did NOT
/// use (9 rows: other props, other voltages of the same motors): +17.0 %
/// (T-Motor MN3110 700 kV on the P12x4 at 14.8 V).
pub const THRUST_CALIBRATED_OVER: f64 = 0.175;
/// Section CLmax over-prediction of the polar table (NeuralFoil, data/air
/// /airfoils.json) against wind-tunnel CLmax, per airfoil: the worst of Re
/// 100k, 200k and 300k. Measured: UIUC LSAT clean-model lift runs
/// (LIFT0x.TXT in https://m-selig.ae.illinois.edu/pd/pub/lsat/volume01.zip,
/// volume02.zip, volume03.zip; e387_lift.dat in volume05.zip; several
/// models of one airfoil averaged) and, for E387, NASA TM-4062 table B1
/// (https://ntrs.nasa.gov/api/citations/19890001471/downloads/
/// 19890001471.pdf). Model higher in 24 of 27 cases, mean +5.8 %. The
/// search takes the stall at sqrt(1 + this) of the model's.
pub const CLMAX_OVER: &[(&str, f64)] = &[
    ("e387", 0.089),   // Re 100k: 1.315 vs 1.208
    ("sd7032", 0.091), // Re 300k: 1.518 vs 1.391
    ("sd7037", 0.070), // Re 100k: 1.321 vs 1.235
    ("sd7062", 0.045), // Re 100k: 1.584 vs 1.516
    ("sd7080", 0.165), // Re 200k: 1.308 vs 1.123
    ("s4083", 0.055),  // Re 200k: 1.374 vs 1.302
    ("mh32", 0.028),   // Re 200k: 1.210 vs 1.177
    ("rg15", 0.058),   // Re 200k: 1.167 vs 1.103
];
/// An airfoil nobody measured here gets the worst measured over-prediction.
pub const CLMAX_OVER_UNMEASURED: f64 = 0.165;

pub fn clmax_over(airfoil: &str) -> f64 {
    CLMAX_OVER.iter().find(|x| x.0 == airfoil).map(|x| x.1).unwrap_or(CLMAX_OVER_UNMEASURED)
}

/// The requirement envelope: `(field, minimum, maximum, source)`. The spec
/// schema states each as `minimum`/`maximum` with its source, and
/// `Requirements::validate` enforces it, so an attack or stored regression
/// outside it is refused rather than designed against. Bounds marked
/// "Wright scope" have no outside source: they are the range this domain
/// claims to design for, stated as such.
pub const REQUIREMENT_BOUNDS: &[(&str, f64, Option<f64>, &str)] = &[
    ("altitude_m", -378.0, Some(4411.0), "lowest airfield (Bar Yehuda, -378 m) to highest airport (Daocheng Yading, 4411 m)"),
    ("hot_day_dk", 0.0, Some(34.0), "MIL-HDBK-310 5.3.1.1.2 worldwide 1 % high temperature, 49 C at the surface, less ISA 15 C; the limit falls with altitude (validated per field altitude)"),
    ("payload_g", 0.0, Some(24_947.0), "14 CFR 107.3: a small unmanned aircraft weighs under 55 lb (24.95 kg) at takeoff, payload included"),
    ("max_span_mm", 300.0, Some(3000.0), "Wright scope (unsourced): spans the printed-rib wing and catalog tubes are laid out for"),
    ("range_km", 0.0, Some(1000.0), "Wright scope (unsourced): beyond any electric quadplane under the 14 CFR 107.3 mass limit"),
    ("endurance_min", 0.0, Some(600.0), "Wright scope (unsourced): beyond any electric quadplane under the 14 CFR 107.3 mass limit"),
    ("budget_usd", 0.0, None, "a cost cannot be negative; no upper limit"),
    ("max_stall_ms", 5.0, Some(75.0), "ArduPlane Parameters.cpp AIRSPEED_STALL @Range 5 75 m/s"),
    ("cells_s", 2.0, Some(6.0), "every catalog ESC's cells_min..cells_max lies within 2-6S (data/air/catalog.json escs)"),
    ("max_print_hours", 0.0, Some(1000.0), "Wright scope (unsourced): about six weeks of one printer"),
];

/// MIL-HDBK-310 (1997) 5.3.1.1.2, worldwide air environment: 1-percent high
/// temperature at GEOMETRIC altitude, (m, C). Below 0 m the surface value
/// holds; linear between rows (the handbook's own interpolation, as the
/// MathWorks Non-Standard Day 310 block does).
pub const MIL_HDBK_310_HOT_1PCT_C: &[(f64, f64)] = &[(0.0, 49.0), (1000.0, 40.0), (2000.0, 30.0), (4000.0, 17.0), (6000.0, 6.0)];

/// The 1 % hot-day temperature at a geometric altitude, C.
pub fn hot_1pct_c(altitude_m: f64) -> f64 {
    let t = MIL_HDBK_310_HOT_1PCT_C;
    if altitude_m <= t[0].0 {
        return t[0].1;
    }
    for w in t.windows(2) {
        if altitude_m <= w[1].0 {
            let f = (altitude_m - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + f * (w[1].1 - w[0].1);
        }
    }
    t[t.len() - 1].1
}

/// Bought packs in parallel: 1 to this many (the search's range; a harness
/// beyond three is not one a builder solders).
pub const PACK_COUNT_MAX: u32 = 3;
/// Hand-built packs: 1 to this many cells in parallel (the search's range).
pub const CELLS_P_MAX: u32 = 8;
