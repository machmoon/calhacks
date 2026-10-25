//! Species thermodynamics from NASA polynomials.
//!
//! Data: `data/rocket/thermo_products.inp`, a verbatim subset of NASA CEA's
//! `data/thermo.inp` (github.com/nasa/cea, commit 2f79a647e857, read
//! 2026-10-24). Those records are the NASA 9-coefficient form of McBride,
//! Zehe & Gordon, NASA/TP-2002-211556; the 7-coefficient (CHEMKIN / GRI-Mech
//! `thermo30.dat`) form is the special case a1 = a2 = 0. We use the 9-term
//! records, not GRI-Mech's 7-term ones, because GRI-Mech's fits stop at
//! 3500 K for H2O, CO2 and OH, below a LOX/RP-1 chamber (3616 K), and
//! because CEA itself reads these exact records -- validating against CEA
//! then tests the solver, not two different data sets.
//!
//! Formulas (RP-1311 Part I eqs 4.9-4.11, as implemented in CEA
//! `source/thermo.f90`):
//!   cp/R = a1 T^-2 + a2 T^-1 + a3 + a4 T + a5 T^2 + a6 T^3 + a7 T^4
//!   H/RT = -a1 T^-2 + a2 ln(T)/T + a3 + a4 T/2 + a5 T^2/3 + a6 T^3/4 + a7 T^4/5 + b1/T
//!   S/R  = -a1 T^-2/2 - a2/T + a3 ln T + a4 T + a5 T^2/2 + a6 T^3/3 + a7 T^4/4 + b2

use std::collections::BTreeMap;

/// Universal gas constant, J/(mol K) (CODATA 2018, exact; the value CEA
/// uses in `source/param.f90.in`).
pub const R: f64 = 8.314_462_618;
/// Standard-state pressure of the polynomials, Pa (1 bar).
pub const P_REF: f64 = 1.0e5;

pub const PRODUCTS_INP: &str = include_str!("../../../data/rocket/thermo_products.inp");

#[derive(Debug, Clone)]
struct Interval {
    t_lo: f64,
    t_hi: f64,
    a: [f64; 7],
    b1: f64,
    b2: f64,
}

#[derive(Debug, Clone)]
pub struct Species {
    pub name: String,
    /// Element symbol -> atoms per molecule.
    pub elements: BTreeMap<String, f64>,
    /// kg/mol.
    pub molar_mass: f64,
    intervals: Vec<Interval>,
}

impl Species {
    fn interval(&self, t: f64) -> &Interval {
        // Outside the fitted range, extrapolate the nearest interval (CEA
        // does the same up to its 20000 K header limit).
        self.intervals
            .iter()
            .find(|iv| t >= iv.t_lo && t <= iv.t_hi)
            .unwrap_or_else(|| {
                if t < self.intervals[0].t_lo {
                    &self.intervals[0]
                } else {
                    self.intervals.last().unwrap()
                }
            })
    }
    /// cp/R (dimensionless).
    pub fn cp_r(&self, t: f64) -> f64 {
        let iv = self.interval(t);
        let a = &iv.a;
        a[0] / (t * t) + a[1] / t + a[2] + a[3] * t + a[4] * t * t + a[5] * t.powi(3) + a[6] * t.powi(4)
    }
    /// H/(RT) (dimensionless), H including the heat of formation.
    pub fn h_rt(&self, t: f64) -> f64 {
        let iv = self.interval(t);
        let a = &iv.a;
        -a[0] / (t * t) + a[1] * t.ln() / t + a[2] + a[3] * t / 2.0 + a[4] * t * t / 3.0
            + a[5] * t.powi(3) / 4.0
            + a[6] * t.powi(4) / 5.0
            + iv.b1 / t
    }
    /// S°/R at the 1-bar standard state (dimensionless).
    pub fn s_r(&self, t: f64) -> f64 {
        let iv = self.interval(t);
        let a = &iv.a;
        -a[0] / (2.0 * t * t) - a[1] / t + a[2] * t.ln() + a[3] * t + a[4] * t * t / 2.0
            + a[5] * t.powi(3) / 3.0
            + a[6] * t.powi(4) / 4.0
            + iv.b2
    }
}

fn fnum(s: &str) -> f64 {
    s.trim().replace(['D', 'd'], "E").parse::<f64>().unwrap_or(0.0)
}

fn field(line: &str, a: usize, b: usize) -> &str {
    let b = b.min(line.len());
    if a >= b { "" } else { &line[a..b] }
}

/// Parse NASA-9 records (the thermo.inp layout, RP-1311 Part II App. A).
pub fn parse_nasa9(text: &str) -> Vec<Species> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.is_empty() || l.starts_with('!') || l.starts_with(' ') {
            i += 1;
            continue;
        }
        let name = field(l, 0, 18).trim().to_string();
        let h = lines[i + 1];
        let nint: usize = field(h, 0, 2).trim().parse().unwrap_or(0);
        let mut elements = BTreeMap::new();
        for k in 0..5 {
            let sym = field(h, 10 + 8 * k, 12 + 8 * k).trim();
            let cnt = fnum(field(h, 12 + 8 * k, 18 + 8 * k));
            if !sym.is_empty() && cnt != 0.0 {
                elements.insert(sym.to_string(), cnt);
            }
        }
        let molar_mass = fnum(field(h, 52, 65)) * 1e-3;
        let mut intervals = Vec::new();
        for k in 0..nint {
            let t = lines[i + 2 + 3 * k];
            let c1 = lines[i + 3 + 3 * k];
            let c2 = lines[i + 4 + 3 * k];
            let mut a = [0.0; 7];
            for (j, slot) in a.iter_mut().enumerate().take(5) {
                *slot = fnum(field(c1, 16 * j, 16 * j + 16));
            }
            a[5] = fnum(field(c2, 0, 16));
            a[6] = fnum(field(c2, 16, 32));
            intervals.push(Interval {
                t_lo: fnum(field(t, 0, 11)),
                t_hi: fnum(field(t, 11, 22)),
                a,
                b1: fnum(field(c2, 48, 64)),
                b2: fnum(field(c2, 64, 80)),
            });
        }
        if nint > 0 {
            out.push(Species { name, elements, molar_mass, intervals });
        }
        i += 2 + if nint == 0 { 1 } else { 3 * nint };
    }
    out
}

/// The gas-phase product species Wright's equilibrium solver may form.
pub fn products() -> Vec<Species> {
    parse_nasa9(PRODUCTS_INP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_matches_janaf_heat_of_formation() {
        let sp = products();
        let h2o = sp.iter().find(|s| s.name == "H2O").unwrap();
        // H(298.15) = Hf = -241.826 kJ/mol (thermo.inp header; JANAF).
        let h = h2o.h_rt(298.15) * R * 298.15;
        assert!((h + 241_826.0).abs() < 5.0, "{h}");
        // S°(298.15) = 188.834 J/mol K (JANAF H2O(g)).
        assert!((h2o.s_r(298.15) * R - 188.834).abs() < 0.05);
        assert!((h2o.molar_mass - 0.018_015_28).abs() < 1e-9);
        assert_eq!(h2o.elements.get("H"), Some(&2.0));
    }

    #[test]
    fn every_record_parsed() {
        assert!(products().len() >= 25);
    }
}
