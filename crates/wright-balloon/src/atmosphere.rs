//! U.S. Standard Atmosphere 1976 from 0 to 84.852 km geopotential.
//!
//! Layer table, gas constant and Earth radius from NOAA-S/T 76-1562
//! (U.S. Standard Atmosphere, 1976), Table 4 and eqs. 18, 33a/33b;
//! viscosity from the same document's Sutherland form (eq. 51).

use crate::rules;
use wright_core::units::{Density, Length, Pressure, Temperature};

/// Base geopotential height (m'), base temperature (K) and lapse rate
/// (K/m') of each layer (US76 Table 4). Base pressures are integrated from
/// the sea-level value at load, so the table cannot drift from the law.
const LAYERS: [(f64, f64); 7] = [
    (0.0, -0.0065),
    (11_000.0, 0.0),
    (20_000.0, 0.0010),
    (32_000.0, 0.0028),
    (47_000.0, 0.0),
    (51_000.0, -0.0028),
    (71_000.0, -0.0020),
];
const TOP_H: f64 = 84_852.0;

/// The state of the air at one geometric altitude.
#[derive(Debug, Clone, Copy)]
pub struct Air {
    pub temperature: Temperature,
    pub pressure: Pressure,
    pub density: Density,
    /// Dynamic viscosity, Pa*s.
    pub viscosity: f64,
}

/// Geometric -> geopotential altitude (US76 eq. 18).
fn geopotential(z: f64) -> f64 {
    rules::US76_R0_M * z / (rules::US76_R0_M + z)
}

/// The air at geometric altitude `z` (clamped to 0..84.8 km).
pub fn at(z: Length) -> Air {
    let h = geopotential(z.as_m().max(0.0)).min(TOP_H);
    let g0 = wright_core::units::G0;
    let r = rules::US76_R_AIR;
    let mut t_b = rules::US76_T0_K;
    let mut p_b = rules::US76_P0_PA;
    let mut i = 0;
    loop {
        let (h_b, lapse) = LAYERS[i];
        let h_top = if i + 1 < LAYERS.len() { LAYERS[i + 1].0 } else { TOP_H };
        let dh = (h.min(h_top)) - h_b;
        let (t, p) = if lapse == 0.0 {
            (t_b, p_b * (-g0 * dh / (r * t_b)).exp())
        } else {
            let t = t_b + lapse * dh;
            (t, p_b * (t_b / t).powf(g0 / (r * lapse)))
        };
        if h <= h_top || i + 1 == LAYERS.len() {
            let rho = p / (r * t);
            let mu = rules::SUTHERLAND_BETA * t.powf(1.5) / (t + rules::SUTHERLAND_S);
            return Air {
                temperature: Temperature::k(t),
                pressure: Pressure::pa(p),
                density: Density::kg_m3(rho),
                viscosity: mu,
            };
        }
        t_b = t;
        p_b = p;
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spot values from US76 Table I (geometric altitude).
    #[test]
    fn matches_us76_tables() {
        let cases = [
            // z m, T K, P Pa, rho kg/m3
            (0.0, 288.15, 101_325.0, 1.2250),
            (11_000.0, 216.774, 22_699.9, 0.36480),
            (20_000.0, 216.65, 5_529.3, 0.088910),
            (32_000.0, 228.49, 889.06, 0.013555),
            // 47 km geopotential = 47 350 m geometric (stratopause base)
            (47_350.1, 270.65, 110.91, 0.0014275),
        ];
        for (z, t, p, rho) in cases {
            let a = at(Length::m(z));
            assert!((a.temperature.as_k() - t).abs() < 0.05, "T at {z}: {}", a.temperature.as_k());
            assert!((a.pressure.as_pa() / p - 1.0).abs() < 2e-3, "P at {z}: {}", a.pressure.as_pa());
            assert!((a.density.as_kg_m3() / rho - 1.0).abs() < 2e-3, "rho at {z}");
        }
        // Sea-level viscosity 1.7894e-5 Pa s (US76 Table III).
        assert!((at(Length::ZERO).viscosity - 1.7894e-5).abs() < 2e-8);
    }
}
