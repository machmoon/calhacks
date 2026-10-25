//! Link budgets over the whole trajectory and GNSS coexistence with the
//! payload's own transmitters.

use crate::rules;
use std::f64::consts::PI;

/// Free-space path loss, dB: 20 log10(4 pi d f / c).
pub fn fspl_db(d_m: f64, f_mhz: f64) -> f64 {
    let lambda = 299_792_458.0 / (f_mhz * 1e6);
    20.0 * (4.0 * PI * d_m.max(lambda / (2.0 * PI)) / lambda).log10()
}

/// Geometry from a ground station to the payload over a spherical Earth:
/// (slant range m, elevation angle rad at the station, within the 4/3-Earth
/// radio horizon).
pub fn geometry(ground_dist_m: f64, station_alt_m: f64, payload_alt_m: f64) -> (f64, f64, bool) {
    let r = rules::EARTH_R_M;
    let th = ground_dist_m / r;
    let (a, b) = (r + station_alt_m, r + payload_alt_m);
    let slant = (a * a + b * b - 2.0 * a * b * th.cos()).max(0.0).sqrt();
    // elevation at the station: angle between local horizontal and the ray
    let el = if slant > 0.0 { ((b * th.cos() - a) / slant).asin() } else { PI / 2.0 };
    let re = rules::K_FACTOR * r;
    // heights above the local ground for the horizon test
    let h1 = station_alt_m.max(0.0);
    let h2 = payload_alt_m.max(0.0);
    let horizon = (2.0 * re * h1).sqrt() + (2.0 * re * h2).sqrt();
    (slant, el, ground_dist_m <= horizon)
}

/// Gain of a vertical half-wave dipole toward elevation `el` (rad), dBi,
/// floored at the swing-averaged null depth.
pub fn dipole_gain_dbi(peak_dbi: f64, el: f64) -> f64 {
    let theta = PI / 2.0 - el.abs(); // angle from the dipole axis
    let s = theta.sin();
    let g = if s < 1e-6 { 0.0 } else { ((PI / 2.0 * theta.cos()).cos() / s).abs() };
    (peak_dbi + 20.0 * g.max(1e-9).log10()).max(rules::DIPOLE_FLOOR_DBI)
}

/// Out-of-band rejection of a GNSS patch at `f_mhz`, dB, as a single
/// resonator of Q = f0/BW: 10 log10(1 + Q^2 (f/f0 - f0/f)^2).
pub fn patch_rejection_db(f_mhz: f64) -> f64 {
    let f0 = rules::GPS_L1_MHZ;
    let q = f0 / rules::GNSS_PATCH_BW_MHZ;
    let x = f_mhz / f0 - f0 / f_mhz;
    10.0 * (1.0 + (q * x).powi(2)).log10()
}

/// The largest interferer the GNSS front end takes before compressing, dBm.
pub fn gnss_blocking_limit_dbm() -> f64 {
    rules::GNSS_LNA_P1DB_DBM - rules::GNSS_BLOCK_BACKOFF_DB
}

/// The largest in-band interferer that still lets the receiver reacquire,
/// dBm: thermal floor kTB+NF raised by the receiver's own reacquisition
/// headroom (signal level minus reacquisition sensitivity).
pub fn gnss_inband_limit_dbm() -> f64 {
    let floor = -174.0 + 10.0 * (rules::GNSS_BW_MHZ * 1e6).log10() + rules::GNSS_NF_DB;
    floor + (rules::GNSS_SIGNAL_DBM - rules::GNSS_REACQ_DBM)
}

/// Harmonic orders of `f_mhz` that land inside the GNSS L1 signal band.
pub fn harmonics_in_l1(f_mhz: f64) -> Vec<u32> {
    (2..=20)
        .filter(|n| ((*n as f64) * f_mhz - rules::GPS_L1_MHZ).abs() <= rules::GNSS_BW_MHZ / 2.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fspl_matches_the_textbook_formula() {
        // 32.44 + 20 log10(km) + 20 log10(MHz)
        let d = fspl_db(100_000.0, 144.39);
        assert!((d - (32.45 + 40.0 + 20.0 * 144.39_f64.log10())).abs() < 0.05, "{d}");
    }

    #[test]
    fn horizon_blocks_a_landed_payload_far_away() {
        let (_, _, vis) = geometry(50_000.0, 2.0, 0.3);
        assert!(!vis);
        let (_, el, vis) = geometry(100_000.0, 2.0, 30_000.0);
        assert!(vis && el > 0.0);
    }

    /// Audit against pylink (github.com/harrison-caudill/pylink @9eac09a):
    /// `tributaries/channel.py::_unity_gain_propagation_loss_db`
    /// (`2 * to_db(4 pi d / lambda)`, Sklar eq. 5.10) and
    /// `tributaries/geometry.py::_slant_range_km`
    /// (`R (sqrt(h^2/R^2 - cos^2 e) - sin e)`), ported verbatim.
    #[test]
    fn link_geometry_and_loss_match_pylink() {
        let pylink_loss = |d_m: f64, f_mhz: f64| 2.0 * 10.0 * (4.0 * PI * d_m / (299_792_458.0 / (f_mhz * 1e6))).log10();
        let pylink_slant = |r: f64, alt: f64, e: f64| {
            let h = alt + r;
            r * ((h * h / (r * r) - e.cos().powi(2)).sqrt() - e.sin())
        };
        for (ground, alt, f) in [(10_000.0, 30_000.0, 144.39), (95_000.0, 20_000.0, 915.0), (130_000.0, 35_000.0, 144.39)] {
            let (slant, el, _) = geometry(ground, 0.0, alt);
            let ps = pylink_slant(rules::EARTH_R_M, alt, el);
            assert!((slant - ps).abs() < 1.0, "slant {slant} vs pylink {ps}");
            let (a, b) = (fspl_db(slant, f), pylink_loss(slant, f));
            assert!((a - b).abs() < 0.01, "loss {a} vs pylink {b}");
        }
    }

    #[test]
    fn limits_are_ordered() {
        assert!(gnss_inband_limit_dbm() < gnss_blocking_limit_dbm());
        assert!(patch_rejection_db(144.39) > patch_rejection_db(1250.0));
    }
}
