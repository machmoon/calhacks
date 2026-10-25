//! What a person asks for (`Spec`) and what Wright resolves (`Design`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub name: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub elevation_m: f64,
}

/// Wind at one altitude: speed and the compass direction it blows toward.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindPoint {
    pub alt_m: f64,
    pub speed_mps: f64,
    pub toward_deg: f64,
}

/// A requirements-only request. Every field has a default except the
/// mission's numbers a person must choose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub mission: String,
    pub launch: Site,
    /// Burst altitude the design must reach, m.
    pub min_burst_altitude_m: f64,
    /// Total spend allowed, USD; none stated means cost is information only.
    #[serde(default)]
    pub budget_usd: Option<f64>,
    /// Hours the trackers must keep working on the ground after landing.
    #[serde(default = "d_window")]
    pub recovery_window_h: f64,
    #[serde(default = "d_prob")]
    pub min_recovery_probability: f64,
    #[serde(default = "d_true")]
    pub needs_video: bool,
    #[serde(default)]
    pub needs_photos: bool,
    /// Someone on the team holds a US amateur licence (APRS, 23 cm video).
    #[serde(default)]
    pub licensed_ham: bool,
    #[serde(default)]
    pub allow_hydrogen: bool,
    /// Forecast ground temperature at launch and landing, C (ISA 15 C if
    /// absent). Judged with the range below when it lies inside it; a
    /// forecast outside the declared range is a no-go day, not a design case.
    #[serde(default)]
    pub ground_temperature_c: Option<f64>,
    /// Ground temperatures the payload must be launchable at, C [low, high]
    /// (default `rules::LAUNCH_TEMP_RANGE_C`). Every thermal check is
    /// evaluated at both ends.
    #[serde(default)]
    pub launch_temp_range_c: Option<[f64; 2]>,
    /// Wind profile; a typical mid-latitude profile if empty.
    #[serde(default)]
    pub wind: Vec<WindPoint>,
    /// Ground stations the team owns (catalog ids); others must be bought.
    #[serde(default)]
    pub owned_ground: Vec<String>,
    /// Parts that must fly (catalog ids).
    #[serde(default)]
    pub must_fly: Vec<String>,
}

impl Spec {
    /// Reject a spec outside the envelope of `rules` (each bound sourced
    /// there) or with fields that contradict each other, saying which.
    pub fn validate(&self) -> anyhow::Result<()> {
        use crate::rules as r;
        let mut bad: Vec<String> = Vec::new();
        fn within(bad: &mut Vec<String>, name: &str, x: f64, lo: f64, hi: f64, why: &str) {
            if !(x.is_finite() && x >= lo && x <= hi) {
                bad.push(format!("{name} = {x} is outside [{lo}, {hi}] ({why})"));
            }
        }
        within(&mut bad, "launch.lat_deg", self.launch.lat_deg, -90.0, 90.0, "latitude");
        within(&mut bad, "launch.lon_deg", self.launch.lon_deg, -180.0, 180.0, "longitude");
        within(&mut bad, "launch.elevation_m", self.launch.elevation_m, r::LAUNCH_ELEVATION_M[0], r::LAUNCH_ELEVATION_M[1], "lowest and highest US ground, USGS");
        within(&mut bad, "min_burst_altitude_m", self.min_burst_altitude_m, r::BURST_TARGET_M[0], r::BURST_TARGET_M[1], "the CUSF burst calculator's accepted targets");
        if let Some(b) = self.budget_usd {
            if !(b.is_finite() && b > 0.0) {
                bad.push(format!("budget_usd = {b} must be a positive number (or omitted/null for no budget)"));
            }
        }
        within(&mut bad, "recovery_window_h", self.recovery_window_h, 0.0, r::RECOVERY_WINDOW_MAX_H, "Wright's allowance");
        within(&mut bad, "min_recovery_probability", self.min_recovery_probability, 0.0, r::P_SITE, "the recovery estimate is capped at rules::P_SITE");
        let [tlo, thi] = r::GROUND_TEMP_RECORD_C;
        let rec = "recorded extremes of the San Joaquin Valley, NWS Hanford";
        if let Some(g) = self.ground_temperature_c {
            within(&mut bad, "ground_temperature_c", g, tlo, thi, rec);
        }
        if let Some([a, b]) = self.launch_temp_range_c {
            within(&mut bad, "launch_temp_range_c[0]", a, tlo, thi, rec);
            within(&mut bad, "launch_temp_range_c[1]", b, tlo, thi, rec);
            if a > b {
                bad.push(format!("launch_temp_range_c [{a}, {b}] has its low end above its high end"));
            }
        }
        for (k, w) in self.wind.iter().enumerate() {
            within(&mut bad, &format!("wind[{k}].alt_m"), w.alt_m, 0.0, r::WIND_ALT_MAX_M, "the ground to 10 km over the highest CUSF burst target");
            within(&mut bad, &format!("wind[{k}].speed_mps"), w.speed_mps, 0.0, r::WIND_MAX_MPS, "NOAA: jet streams exceed 275 mph");
            within(&mut bad, &format!("wind[{k}].toward_deg"), w.toward_deg, 0.0, 360.0, "compass bearing");
        }
        if self.wind.windows(2).any(|p| p[1].alt_m <= p[0].alt_m) {
            bad.push("wind points must be in increasing altitude".into());
        }
        // combinations
        if self.min_burst_altitude_m.is_finite() && self.launch.elevation_m.is_finite() && self.min_burst_altitude_m < self.launch.elevation_m + 1000.0 {
            bad.push(format!("min_burst_altitude_m {} is not at least 1 km above the launch elevation {} m", self.min_burst_altitude_m, self.launch.elevation_m));
        }
        if bad.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("implausible balloon spec: {}", bad.join("; "))
        }
    }
}

fn d_window() -> f64 {
    4.0
}
fn d_prob() -> f64 {
    // reachable by construction: the recovery estimate is capped at
    // rules::P_SITE (0.92), so a 0.95 default made every spec unsatisfiable
    0.90
}
fn d_true() -> bool {
    true
}

/// One part instance in the design.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub part: String,
    #[serde(default = "one")]
    pub qty: u32,
    /// Index into `Design::buses`; None for passive or self-powered parts.
    pub bus: Option<usize>,
    /// Index into that bus's `rails` (a regulator output); None: straight off the battery.
    #[serde(default)]
    pub rail: Option<usize>,
    /// On during the recovery window too (trackers), not only in flight.
    #[serde(default)]
    pub on_after_landing: bool,
    /// Switched off at burst (a camera whose job is the ascent and the burst).
    #[serde(default)]
    pub until_burst: bool,
}

fn five() -> f64 {
    5.0
}

fn one() -> u32 {
    1
}

/// A battery feeding loads directly and through regulator rails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bus {
    pub name: String,
    /// Catalog battery id and how many in series / parallel.
    pub battery: String,
    pub series: u32,
    pub parallel: u32,
    /// Regulators hanging off the battery (catalog ids).
    #[serde(default)]
    pub rails: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerClass {
    Primary,
    Secondary,
    Backup,
}

/// A position-reporting chain: a GPS, a radio, and who listens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tracker {
    pub name: String,
    pub class: TrackerClass,
    /// The radio part (catalog id, must be an `Item`).
    pub radio: String,
    /// The GPS part feeding it.
    pub gps: String,
    /// Set to an airborne dynamic platform model before flight.
    pub gps_airborne: bool,
    /// Ground station (catalog id); None when nobody has one yet.
    pub ground: Option<String>,
}

/// The payload enclosure: insulation walls around a packed interior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enclosure {
    pub insulation: String,
    pub wall_mm: f64,
    /// Inner size, mm (x, y, z).
    pub inner_mm: [f64; 3],
    /// A printed PETG tray with standoffs holds the boards.
    pub printed_tray: bool,
}

/// Slicer output for the printed parts (PrusaSlicer via wright-geom).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Printed {
    pub grams: f64,
    pub seconds: f64,
    pub printer: String,
}

/// A fully resolved balloon: every part named, the fill set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Design {
    pub label: String,
    pub balloon: String,
    pub gas: String,
    pub cylinders: u32,
    /// Neck lift the team fills to, g (what the fill scale reads).
    pub neck_lift_g: f64,
    /// Ascent rate the CUSF rule sized the fill for, m/s.
    #[serde(default = "five")]
    pub fill_target_mps: f64,
    pub parachute: String,
    pub line: String,
    pub line_m: f64,
    pub enclosure: Enclosure,
    pub items: Vec<Item>,
    pub buses: Vec<Bus>,
    pub trackers: Vec<Tracker>,
    /// Ground gear that must be bought for this design (catalog ids).
    #[serde(default)]
    pub buy_ground: Vec<String>,
    /// Slicer result for the printed tray; None until sliced.
    #[serde(default)]
    pub printed: Option<Printed>,
    /// Upper bound on the printed tray mass used before slicing, g.
    #[serde(default)]
    pub printed_allowance_g: f64,
    /// Chemical hand warmers in the box are activated (opened) at sealing
    /// only when the ground temperature is below this, C; None: always.
    #[serde(default)]
    pub warmers_below_c: Option<f64>,
    /// The go/no-go launch criterion this design declares: the ground
    /// temperatures, C [low, high], it may be released at. Printed in the
    /// checklist; the `launch_window` check verifies it against the model.
    #[serde(default)]
    pub launch_window_c: Option<[f64; 2]>,
    #[serde(default)]
    pub notes: Vec<String>,
}
