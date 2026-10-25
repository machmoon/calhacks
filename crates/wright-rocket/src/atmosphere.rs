//! U.S. Standard Atmosphere 1976 (NOAA-S/T 76-1562), geopotential layers
//! to 84.852 km, the same model OpenRocket ships as its default
//! (`core/.../models/atmosphere/ExtendedISAModel.java`).

const R_AIR: f64 = 287.052_87; // J/(kg K), US76 R*/M0
const G0: f64 = 9.806_65;
const R_EARTH: f64 = 6_356_766.0; // m, US76 effective radius for geopotential

// (base geopotential altitude m, base T K, lapse K/m)
const LAYERS: [(f64, f64, f64); 7] = [
    (0.0, 288.15, -0.0065),
    (11_000.0, 216.65, 0.0),
    (20_000.0, 216.65, 0.001),
    (32_000.0, 228.65, 0.0028),
    (47_000.0, 270.65, 0.0),
    (51_000.0, 270.65, -0.0028),
    (71_000.0, 214.65, -0.002),
];

#[derive(Debug, Clone, Copy)]
pub struct Air {
    pub t: f64,
    pub p: f64,
    pub rho: f64,
    pub a: f64,
    /// Dynamic viscosity, Pa s (Sutherland, US76 eq 51).
    pub mu: f64,
}

/// Air at geometric altitude `z` metres above mean sea level.
pub fn at(z: f64) -> Air {
    let h = (R_EARTH * z / (R_EARTH + z)).clamp(-5000.0, 84_000.0);
    let mut p = 101_325.0;
    let mut t = 288.15;
    for (i, &(hb, tb, l)) in LAYERS.iter().enumerate() {
        let top = LAYERS.get(i + 1).map(|x| x.0).unwrap_or(f64::INFINITY);
        let hh = h.min(top) - hb;
        let pb = p;
        t = tb + l * hh;
        p = if l == 0.0 { pb * (-G0 * hh / (R_AIR * tb)).exp() } else { pb * (tb / t).powf(G0 / (R_AIR * l)) };
        if h <= top {
            break;
        }
    }
    let rho = p / (R_AIR * t);
    let a = (1.4 * R_AIR * t).sqrt();
    let mu = 1.458e-6 * t.powf(1.5) / (t + 110.4);
    Air { t, p, rho, a, mu }
}

#[cfg(test)]
mod tests {
    #[test]
    fn us76_table_points() {
        // US76 Table I: 11 km 22632 Pa / 216.65 K; 20 km 5474.9 Pa.
        let a = super::at(11_019.0); // geometric for 11 km geopotential
        assert!((a.p - 22_632.0).abs() < 30.0, "{}", a.p);
        let b = super::at(20_063.0);
        assert!((b.p - 5_474.9).abs() < 10.0, "{}", b.p);
        assert!((super::at(0.0).rho - 1.225).abs() < 1e-3);
    }
}
