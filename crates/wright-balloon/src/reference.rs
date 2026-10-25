//! The reference: Pat's team balloon as planned on 2026-09-19 (team plan
//! page claude.ai/artifact/Bpb4uLhdQDSMTVut1dxzKB and project notes), with
//! the open choices filled by stated assumptions. Every assumption is
//! listed in evals/balloon/reference.md.

use crate::model::{Bus, Design, Enclosure, Item, Site, Spec, Tracker, TrackerClass};

fn item(part: &str, qty: u32, bus: Option<usize>, rail: Option<usize>, after: bool) -> Item {
    Item { part: part.into(), qty, bus, rail, on_after_landing: after, until_burst: false }
}

/// The mission both designs answer: photos/video from near space and a
/// reliable recovery, launched inland in California (ASSUMPTION: the team is
/// at UC Santa Barbara; a common inland HAB launch is near Lost Hills, CA).
pub fn mission_spec(min_burst_m: f64, budget_usd: impl Into<Option<f64>>) -> Spec {
    Spec {
        mission: "photos and video from near space, reliable recovery".into(),
        launch: Site { name: "inland Central Valley, CA (assumed)".into(), lat_deg: 35.62, lon_deg: -119.69, elevation_m: 80.0 },
        min_burst_altitude_m: min_burst_m,
        budget_usd: budget_usd.into(),
        recovery_window_h: 4.0,
        min_recovery_probability: 0.90,
        needs_video: true,
        needs_photos: false,
        licensed_ham: true,
        allow_hydrogen: false,
        ground_temperature_c: Some(15.0),
        launch_temp_range_c: None,
        wind: vec![],
        owned_ground: vec!["aprs-ham-ht".into()],
        must_fly: vec![],
    }
}

/// Pat's design as planned. Parts on hand are from the plan page's parts
/// table; APRS primary per the 2026-09-19 recommendation (DRA818V on the
/// spare ESP32 sharing the M10); ELRS secondary; a phone as backup. The
/// 900 MHz ELRS transmitter is an open question, so no ground station is
/// attached to that link.
pub fn pat_design() -> Design {
    Design {
        label: "Pat's team balloon (as planned, 2026-09-19)".into(),
        balloon: "totex-1200".into(),
        gas: "he-uhp-300".into(),
        cylinders: 1,
        neck_lift_g: 0.0, // sized by the CUSF rule in the eval, as a team would fill it
        fill_target_mps: 5.0,
        parachute: "spherachute-36".into(),
        line: crate::design::LINE_ID.into(),
        line_m: 8.0,
        enclosure: Enclosure { insulation: "xps-foamular-150".into(), wall_mm: 25.4, inner_mm: [0.0; 3], printed_tray: false },
        items: vec![
            // generous to the plan: the MOSFET boards cut these at burst
            Item { until_burst: true, ..item("jetson-orin-nano-devkit", 1, Some(0), None, false) },
            Item { until_burst: true, ..item("logitech-brio", 1, Some(0), Some(0), false) },
            Item { until_burst: true, ..item("geprc-vtx-1g2-2w", 1, Some(0), None, false) },
            item("esp32-devkit", 2, Some(0), Some(1), true),
            item("matek-m10q", 1, Some(0), Some(1), true),
            item("stardrone-cr8-elrs", 1, Some(0), Some(1), true),
            // in the plan's favour: the "buck converter" on hand, set to
            // 4.0 V, feeds the DRA818V inside its 3.3-4.5 V range
            item("dra818v", 1, Some(0), Some(2), true),
            item("mpu6050", 1, Some(0), Some(1), false),
            item("sg90-servo", 1, Some(0), Some(1), false),
            item("mosfet-board", 2, None, None, false),
            item("phone-backup", 1, None, None, true),
        ],
        buses: vec![Bus {
            name: "4S LiPo (all loads)".into(),
            battery: "lipo-4s-6000".into(),
            series: 1,
            parallel: 1,
            rails: vec!["reg-5v-1a8".into(), "reg-5v-1a8".into(), "lm2596-buck-4v0".into()],
        }],
        trackers: vec![
            Tracker { name: "APRS (DRA818V + ESP32 + M10)".into(), class: TrackerClass::Primary, radio: "dra818v".into(), gps: "matek-m10q".into(), gps_airborne: true, ground: Some("aprs-ham-ht".into()) },
            Tracker { name: "ELRS telemetry (CR8 + ESP32 + M10)".into(), class: TrackerClass::Secondary, radio: "stardrone-cr8-elrs".into(), gps: "matek-m10q".into(), gps_airborne: true, ground: None },
            Tracker { name: "phone in the box".into(), class: TrackerClass::Backup, radio: "phone-backup".into(), gps: "phone-backup".into(), gps_airborne: false, ground: None },
        ],
        buy_ground: vec![],
        printed: None,
        printed_allowance_g: 0.0,
        // the plan names no heater and no launch temperature limit
        warmers_below_c: None,
        launch_window_c: None,
        notes: vec![
            "ASSUMPTION: Kaymont/Totex HAB-1200 latex (the plan names no balloon)".into(),
            "ASSUMPTION: 36 in Spherachute (the plan names no chute)".into(),
            "ASSUMPTION: 1 in XPS foam box sized to the parts, no printed rack".into(),
            "ASSUMPTION: one 4S 6000 mAh pack feeds everything (no second pack on hand)".into(),
        ],
    }
}
