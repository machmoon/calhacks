//! Airfoil sections: coordinates and polars from `data/air/airfoils.json`
//! (UIUC coordinates, NeuralFoil polars; see `data/air/README.md`).
//!
//! Read the way polar tables are read: linear in angle of attack, linear in
//! log Reynolds number. A query outside the table's Reynolds range is held at
//! the edge and reported by [`Airfoil::clamped`], never extrapolated.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct Raw {
    airfoils: BTreeMap<String, RawFoil>,
}

#[derive(Debug, Deserialize)]
struct RawFoil {
    t_max: f64,
    x_t_max: f64,
    camber: f64,
    coordinates: Vec<[f64; 2]>,
    re: Vec<f64>,
    alpha: Vec<f64>,
    cl: Vec<Vec<f64>>,
    cd: Vec<Vec<f64>>,
    cm: Vec<Vec<f64>>,
}

#[derive(Debug)]
pub struct Airfoil {
    pub name: String,
    pub t_max: f64,
    pub x_t_max: f64,
    pub camber: f64,
    /// Selig order: trailing edge, upper surface, leading edge, lower surface.
    pub coordinates: Vec<[f64; 2]>,
    log_re: Vec<f64>,
    re: Vec<f64>,
    alpha: Vec<f64>,
    cl: Vec<Vec<f64>>,
    cd: Vec<Vec<f64>>,
    cm: Vec<Vec<f64>>,
    upper: Vec<[f64; 2]>,
    lower: Vec<[f64; 2]>,
}

fn table() -> &'static BTreeMap<String, Airfoil> {
    static T: OnceLock<BTreeMap<String, Airfoil>> = OnceLock::new();
    T.get_or_init(|| {
        let raw: Raw = serde_json::from_str(include_str!("../../../data/air/airfoils.json"))
            .expect("data/air/airfoils.json parses");
        raw.airfoils
            .into_iter()
            .map(|(name, f)| {
                let le = f
                    .coordinates
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1[0].total_cmp(&b.1[0]))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let mut upper: Vec<[f64; 2]> = f.coordinates[..=le].to_vec();
                upper.sort_by(|a, b| a[0].total_cmp(&b[0]));
                let mut lower: Vec<[f64; 2]> = f.coordinates[le..].to_vec();
                lower.sort_by(|a, b| a[0].total_cmp(&b[0]));
                let a = Airfoil {
                    name: name.clone(),
                    t_max: f.t_max,
                    x_t_max: f.x_t_max,
                    camber: f.camber,
                    coordinates: f.coordinates,
                    log_re: f.re.iter().map(|r| r.ln()).collect(),
                    re: f.re,
                    alpha: f.alpha,
                    cl: f.cl,
                    cd: f.cd,
                    cm: f.cm,
                    upper,
                    lower,
                };
                (name, a)
            })
            .collect()
    })
}

pub fn names() -> Vec<&'static str> {
    table().keys().map(|s| s.as_str()).collect()
}

pub fn get(name: &str) -> Option<&'static Airfoil> {
    table().get(name)
}

fn bracket(xs: &[f64], x: f64) -> (usize, usize, f64) {
    let n = xs.len();
    if x <= xs[0] {
        return (0, 0, 0.0);
    }
    if x >= xs[n - 1] {
        return (n - 1, n - 1, 0.0);
    }
    let j = xs.partition_point(|&v| v <= x);
    let i = j - 1;
    (i, j, (x - xs[i]) / (xs[j] - xs[i]))
}

fn interp_curve(curve: &[[f64; 2]], x: f64) -> f64 {
    let j = curve.partition_point(|p| p[0] < x).clamp(1, curve.len() - 1);
    let (a, b) = (curve[j - 1], curve[j]);
    if b[0] == a[0] {
        a[1]
    } else {
        a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0])
    }
}

impl Airfoil {
    fn lookup(&self, t: &[Vec<f64>], alpha_deg: f64, re: f64) -> f64 {
        let (i, j, w) = bracket(&self.log_re, re.max(1.0).ln());
        let (p, q, s) = bracket(&self.alpha, alpha_deg);
        let at = |k: usize| t[k][p] + (t[k][q] - t[k][p]) * s;
        at(i) + (at(j) - at(i)) * w
    }
    pub fn cl(&self, alpha_deg: f64, re: f64) -> f64 {
        self.lookup(&self.cl, alpha_deg, re)
    }
    pub fn cd(&self, alpha_deg: f64, re: f64) -> f64 {
        self.lookup(&self.cd, alpha_deg, re)
    }
    pub fn cm(&self, alpha_deg: f64, re: f64) -> f64 {
        self.lookup(&self.cm, alpha_deg, re)
    }
    /// True when `re` lies outside the table and was held at its edge.
    pub fn clamped(&self, re: f64) -> bool {
        re < self.re[0] * 0.999 || re > self.re[self.re.len() - 1] * 1.001
    }
    /// (CLmax, alpha at CLmax) over the table's angles.
    pub fn cl_max(&self, re: f64) -> (f64, f64) {
        let mut best = (f64::NEG_INFINITY, 0.0);
        for &a in &self.alpha {
            let c = self.cl(a, re);
            if c > best.0 {
                best = (c, a);
            }
        }
        best
    }
    /// Lowest angle on the attached branch giving `cl`; None past CLmax.
    pub fn alpha_for_cl(&self, cl: f64, re: f64) -> Option<f64> {
        let (cmax, a_max) = self.cl_max(re);
        if cl > cmax {
            return None;
        }
        let mut lo = self.alpha[0];
        if self.cl(lo, re) > cl {
            return Some(lo);
        }
        let mut hi = a_max;
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if self.cl(mid, re) < cl {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }
    /// Section lift slope from the linear range (-2..+4 deg), PER RADIAN.
    pub fn lift_slope_per_rad(&self, re: f64) -> f64 {
        (self.cl(4.0, re) - self.cl(-2.0, re)) / 6.0_f64.to_radians()
    }
    /// (upper, lower) surface heights over chord at chordwise station `x_c`.
    pub fn surfaces_at(&self, x_c: f64) -> (f64, f64) {
        (interp_curve(&self.upper, x_c), interp_curve(&self.lower, x_c))
    }
    pub fn thickness_at(&self, x_c: f64) -> f64 {
        let (u, l) = self.surfaces_at(x_c);
        u - l
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sd7037_is_a_sensible_section() {
        let a = get("sd7037").unwrap();
        let re = 2e5;
        let slope = a.lift_slope_per_rad(re);
        assert!(slope > 5.0 && slope < 7.5, "slope {slope}");
        let (cmax, _) = a.cl_max(re);
        assert!(cmax > 1.0 && cmax < 1.6, "clmax {cmax}");
        assert!((a.thickness_at(0.275) - a.t_max).abs() < 0.01);
        assert!(a.cd(2.0, re) > 0.005 && a.cd(2.0, re) < 0.02);
        assert!(a.clamped(1e4) && !a.clamped(1e5));
    }
}

#[cfg(test)]
mod clmax_allowance {
    /// Each `rules::CLMAX_OVER` entry is the table's CLmax over the
    /// wind-tunnel value at the Re named in its comment (UIUC LSAT / TM-4062).
    #[test]
    fn allowances_match_the_table_and_the_tunnel() {
        for (name, re, measured) in [
            ("e387", 1e5, 1.208),
            ("sd7032", 3e5, 1.391),
            ("sd7037", 1e5, 1.235),
            ("sd7062", 1e5, 1.516),
            ("sd7080", 2e5, 1.123),
            ("s4083", 2e5, 1.302),
            ("mh32", 2e5, 1.177),
            ("rg15", 2e5, 1.103),
        ] {
            let model = super::get(name).unwrap().cl_max(re).0;
            let over = model / measured - 1.0;
            assert!((over - crate::rules::clmax_over(name)).abs() < 0.003, "{name}: model {model:.3} vs {measured}: {over:+.3}");
        }
        assert_eq!(crate::rules::clmax_over("clarky"), crate::rules::CLMAX_OVER_UNMEASURED);
    }
}
