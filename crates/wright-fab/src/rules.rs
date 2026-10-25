//! Every number that changes a toolpath or a time, with where it came from.
//! "UNSOURCED" marks a shop-practice value nobody here measured; they are
//! the first thing to replace with the actual machine's numbers.

// ---------- Machines ----------
/// 3-axis mill for metal: a Tormach PCNC 1100-class benchtop mill (the
/// common university-club machine). Max spindle 5140 rpm, rapids 2540
/// mm/min (100 ipm). UNSOURCED (recalled from Tormach's spec page, not
/// re-read): set to the real machine before trusting times.
pub const MILL_NAME: &str = "3-axis CNC mill (Tormach PCNC 1100 class), 4th-axis indexer for radial holes";
pub const MILL_MAX_RPM: f64 = 5140.0;
pub const MILL_RAPID_MM_MIN: f64 = 2540.0;
/// CNC router for sheet (G10 fins): Shapeoko/Onefinity class. UNSOURCED.
pub const ROUTER_NAME: &str = "CNC router (Shapeoko / Onefinity class) with dust shoe";
pub const ROUTER_MAX_RPM: f64 = 24000.0;
pub const ROUTER_RAPID_MM_MIN: f64 = 5000.0;
/// 2-axis CNC lathe running LinuxCNC in lathe mode (G7/G18). UNSOURCED.
pub const LATHE_NAME: &str = "2-axis CNC lathe (LinuxCNC lathe mode, G7 diameter, G18)";
pub const LATHE_MAX_RPM: f64 = 2500.0;
pub const LATHE_RAPID_MM_MIN: f64 = 3000.0;
/// Tool change + touch-off, per tool. UNSOURCED.
pub const TOOL_CHANGE_MIN: f64 = 0.5;
/// Fixturing a part (vise/chuck, indicate, zero), per setup. UNSOURCED.
pub const SETUP_MIN: f64 = 15.0;

// ---------- Cutting data ----------
// Surface speed Vc (m/min) and chip load / feed per rev. Aluminium and
// drilling values are in the ranges of Machinery's Handbook (speed tables
// for wrought aluminium alloys, carbide and HSS); picked mid-range for a
// light machine. Phenolic: linen/canvas phenolic machines with carbide at
// aluminium-like speeds and light feeds (dust, no coolant). UNSOURCED as
// single numbers.
pub const AL_MILL_VC: f64 = 250.0; // carbide end mill
pub const AL_CHIPLOAD_MM: f64 = 0.05; // per tooth, 1/4 in 3-flute
pub const AL_DOC_MM: f64 = 2.0; // axial depth, slotting/profiling
pub const AL_STEPOVER: f64 = 0.4; // of tool diameter, pocketing
pub const AL_TURN_VC: f64 = 300.0; // carbide insert
pub const PHENOLIC_TURN_VC: f64 = 150.0;
pub const TURN_ROUGH_FPR: f64 = 0.20; // mm/rev
pub const TURN_FINISH_FPR: f64 = 0.08;
pub const AL_TURN_DOC: f64 = 1.5; // radial, per roughing pass
pub const PHENOLIC_TURN_DOC: f64 = 1.0;
pub const FINISH_ALLOWANCE: f64 = 0.3; // radial, left for the finish pass
pub const DRILL_VC: f64 = 60.0; // HSS in 6061
pub const DRILL_FPR: f64 = 0.08;
pub const G10_ROUTER_RPM: f64 = 18000.0; // 1/8 in diamond-cut carbide
pub const G10_FEED_MM_MIN: f64 = 900.0;
pub const G10_DOC_MM: f64 = 1.6;

// ---------- Cutting tubes ----------
/// Kerf of an abrasive cut-off wheel / fine-tooth blade on carbon tube.
/// UNSOURCED (typical 1 mm wheel + squaring).
pub const CARBON_KERF_MM: f64 = 1.5;
/// Mark, cut, square and deburr one carbon/G12/aluminium tube. UNSOURCED.
pub const CUT_MIN: f64 = 4.0;
/// Bandsaw cut through aluminium tube or bar. UNSOURCED.
pub const SAW_KERF_MM: f64 = 2.0;

// ---------- Densities (g/cm^3) for grams of chips/parts ----------
/// 6061-T6: 2.70 g/cm^3 (ASM Aluminum Handbook / MatWeb 6061-T6).
pub const AL6061_DENSITY: f64 = 2.70;
/// Linen/canvas phenolic laminate ~1.35 g/cm^3 (NEMA LE/CE datasheets, 1.30-1.40).
pub const PHENOLIC_DENSITY: f64 = 1.35;
/// G10/FR4 1.85 g/cm^3 (NEMA G10 datasheets, 1.80-1.90).
pub const G10_DENSITY: f64 = 1.85;

pub fn rpm(vc_m_min: f64, dia_mm: f64, max: f64) -> f64 {
    (vc_m_min * 1000.0 / (std::f64::consts::PI * dia_mm.max(0.5))).min(max)
}
