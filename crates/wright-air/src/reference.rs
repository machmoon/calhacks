//! The build to beat: Tsung Xu's 3D-printed quadplane (June 2025).
//!
//! Sources, read 2026-10-24 (the prototype's `reference.py`):
//! * "I made a 3D printed VTOL drone that can fly 130 miles",
//!   https://www.tsungxu.com/p/i-made-a-3d-printed-vtol-that-can (2025-06-10):
//!   130 miles, 3 hours on one charge, Bambu A1.
//! * Show HN, https://news.ycombinator.com/item?id=44241278: 1200 mm span,
//!   770 mm long, 2.55 kg; foaming PLA and PETG, CFRP booms and spars;
//!   Amprius SA08 6S2P pack (Upgrade Energy); 2807 AOS motors and Gemfan
//!   7042 props for lift AND cruise ("unoptimized"); Holybro Tekko32 F4 45A
//!   4-in-1 (lift) and Flycolor Raptor 5 45A (cruise); SpeedyBee F405 Wing;
//!   M10 GPS; ArduPilot 4.6.0; hover at about 45 % throttle; $2000 with the
//!   battery.
//!
//! Simplified surrogate inputs the eval varies: the wing's chord and
//! airfoil (220/160 mm SD7037; NOT the author's reported Selig 3024), the
//! boom tube, the servos and the receiver. The Tekko32 Mini 45A has no
//! published mass, so its 50A successor stands in. The author clarified in
//! the Reddit discussion that a crash prevented the full endurance test.
//! 130 mi / 3 h implies 19.4 m/s; this is not a measured flight speed.
//! https://www.reddit.com/r/diydrones/comments/1l89dcr/

use crate::spec::*;

pub struct Reported {
    pub span_mm: f64,
    pub mass_kg: f64,
    pub hover_throttle: f64,
    pub endurance_h: f64,
    pub range_miles: f64,
    pub cost_usd: f64,
}

pub const REPORTED: Reported =
    Reported { span_mm: 1200.0, mass_kg: 2.55, hover_throttle: 0.45, endurance_h: 3.0, range_miles: 130.0, cost_usd: 2000.0 };

pub const MILE_KM: f64 = 1.609_344;

/// Speed implied by the claimed range/endurance, not measured flight data.
/// Legacy function name retained for existing callers.
pub fn flown_speed_mps() -> f64 {
    REPORTED.range_miles * MILE_KM * 1000.0 / (REPORTED.endurance_h * 3600.0)
}

/// The constraints the reference was built to, as a requirements-only spec.
/// No budget: the comparison is the best aircraft under every other strict
/// check (Pat, 10/24); cost is reported as information (the reference's
/// reported ~$2000 stays in `REPORTED`).
pub fn requirements() -> Requirements {
    Requirements { max_span_mm: 1200.0, printer: "bambu_a1".into(), cells_s: 6, budget_usd: None, ..Requirements::default() }
}

/// The reference's published parts; structure placeholders the sizing
/// rules replace (`sizing::size`), exactly as for any design.
pub fn design(root_chord_mm: f64) -> Design {
    let tip = (root_chord_mm * 160.0 / 220.0).round();
    Design {
        name: format!("reference: Tsung Xu 3D-printed VTOL (2025), root chord {root_chord_mm:.0} mm"),
        requirements: requirements(),
        wing: Wing {
            span_mm: 1200.0,
            root_chord_mm,
            tip_chord_mm: tip,
            airfoil: "sd7037".into(),
            incidence_deg: 2.0,
            washout_deg: 2.0,
            dihedral_deg: 0.0,
            aileron_chord_frac: 0.25,
        },
        tail: Tail { airfoil: "naca0009".into(), vh: crate::rules::TAIL_VH, vv: crate::rules::TAIL_VV, aspect_ratio_v: 1.6 },
        propulsion: Propulsion {
            lift_motor: "aos_supernova_2807_1400kv".into(),
            lift_prop: "gemfan_flash_7042_2blade".into(),
            lift_esc: "holybro_tekko32_f4_4in1_50a_am32".into(),
            cruise_motor: "aos_supernova_2807_1400kv".into(),
            cruise_prop: "gemfan_flash_7042_2blade".into(),
            cruise_esc: "flycolor_raptor5_blheli32_45a".into(),
            lift_tilt_deg: 3.0,
            lift_current_limit_a: None,
        },
        battery: BatteryChoice::Pack { id: "upgrade_energy_gold_v1_6s2p_22ah_sa08".into(), count: 1 },
        avionics: Avionics {
            fc: "speedybee_f405_wing_app".into(),
            gps: "matek_m10q_5883".into(),
            receiver: "radiomaster_rp1_v2".into(),
            airspeed: None,
            servo: "emax_es08ma_ii".into(),
        },
        structure: Structure {
            main_spar: "rockwest_t_rnd_314_l39".into(),
            rear_spar: "cst_cs328_1m_rod".into(),
            joiner: "cst_cs525_1m".into(),
            boom_tube: "goodwinds_wct375048".into(),
            leg_thickness_mm: 10.0,
            shell_filament: "bambu_pla_aero".into(),
            structural_filament: "bambu_petg_basic".into(),
        },
        hardware: Default::default()
    }
}

/// The reference with its structure sized by rule (its ESCs are published
/// and kept).
pub fn sized(root_chord_mm: f64) -> Result<Design, String> {
    let d = design(root_chord_mm);
    let (le, ce) = (d.propulsion.lift_esc.clone(), d.propulsion.cruise_esc.clone());
    let mut d = crate::sizing::size_and_repair(d)?;
    d.propulsion.lift_esc = le;
    d.propulsion.cruise_esc = ce;
    // the published build sets no ESC current limit (its 4-in-1 has no
    // per-motor limit); the sizing rule's limit is Wright's, not the author's
    d.propulsion.lift_current_limit_a = None;
    Ok(d)
}
