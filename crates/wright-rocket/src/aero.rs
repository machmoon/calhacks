//! Aerodynamics: Barrowman normal-force/centre-of-pressure and an
//! OpenRocket-style component drag buildup.
//!
//! Prior art followed (OpenRocket, github.com/openrocket/openrocket, branch
//! `unstable`, read 2026-10-24):
//! * `core/.../aerodynamics/BarrowmanDragCalculator.java`: skin friction
//!   (`calculateFrictionCoefficient`, turbulent `1/(1.50 ln Re - 5.6)^2`
//!   with compressibility factors, roughness limit `0.032 (k/L)^0.2`,
//!   body correction `1 + 1/(2 f_B)`), base drag `0.12 + 0.13 M^2` /
//!   `0.25/M`, stagnation pressure `0.85 (1 + M^2/4 + M^4/40)`.
//! * `barrowman/FinSetCalc.java`: fin CN_alpha (subsonic Diederich form
//!   `2 pi s^2 / (1 + sqrt(1 + (1 - M^2)(s^2/(A cos G))^2)) / Aref`,
//!   supersonic `A K1 / Aref` with `K1 = 2/beta`), fin friction
//!   `Cf (1 + 2t/c) 2A/Aref`, rounded-LE fore drag, fin-fin factor for 3/4
//!   fins, body interference `1 + r/(s + r)` (Barrowman 1967 eq 3-46).
//! * `barrowman/SymmetricComponentCalc.java`: von Karman nose pressure drag
//!   from NASA TR-R-100 fineness-3 data (`vonKarmanInterpolator`), re-scaled
//!   to the nose's own fineness as `calculateNoseInterpolator` does:
//!   `cd = stag (cd_3 / stag)^(ln(f + 1) / ln 4)` with `stag` the stagnation
//!   CD and `f = length / (2 |r_aft - r_fore|)` (= L/D for a nose). Nose CP
//!   from volume.
//!
//! Deviation, stated: the transonic CN_alpha is a linear blend between
//! M 0.9 and 1.5 instead of OpenRocket's polynomial interpolator. Addition:
//! a design may carry a nose CD(M) table measured elsewhere (the CFD stage's
//! SU2 sweep), which then replaces the scaled TR-R-100 curve
//! (`nose_cd_table`; same idea as the spec's measured dry mass and c*).

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Axisymmetric body + one trapezoidal fin set. Lengths in metres from the
/// nose tip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AeroBody {
    pub diameter: f64,
    pub nose_length: f64,
    pub body_length: f64,
    pub fin_count: usize,
    pub fin_root: f64,
    pub fin_tip: f64,
    pub fin_span: f64,
    pub fin_sweep: f64,
    pub fin_thickness: f64,
    /// Fin root leading edge position from the nose tip.
    pub fin_le_x: f64,
    /// Surface roughness height, m (OpenRocket "smooth paint" 20 um).
    pub roughness: f64,
    /// Nose pressure (wave) drag on the body cross-section against Mach,
    /// `[mach, cd]` rows in rising Mach, from CFD on this nose (the cfd
    /// stage's `rocket_su2.json` `nose_points`). None: the fineness-scaled
    /// TR-R-100 von Karman curve.
    #[serde(default)]
    pub nose_cd_table: Option<Vec<[f64; 2]>>,
    /// Cached (nose volume m^3, nose wetted area m^2).
    #[serde(skip)]
    pub cache: std::sync::OnceLock<(f64, f64)>,
}

/// Von Karman (Haack C = 0) radius at x along a nose of length l, radius r.
pub fn von_karman_radius(x: f64, l: f64, r: f64) -> f64 {
    let th = (1.0 - 2.0 * x / l).clamp(-1.0, 1.0).acos();
    r * ((th - (2.0 * th).sin() / 2.0) / PI).sqrt()
}

/// NASA TR-R-100 (NTRS 19630004995, p. 16) free-flight pressure drag of a
/// fineness-3 von Karman nose, as OpenRocket tabulates it
/// (`SymmetricComponentCalc.vonKarmanInterpolator`).
pub const TR_R_100_VK_FR3: ([f64; 10], [f64; 10]) = ([0.9, 0.95, 1.0, 1.05, 1.1, 1.2, 1.4, 1.6, 2.0, 3.0], [0.0, 0.010, 0.027, 0.055, 0.070, 0.081, 0.095, 0.097, 0.091, 0.083]);

/// Stagnation-pressure CD of a blunt face
/// (OpenRocket `BarrowmanCalculator.calculateStagnationCD`).
pub fn stagnation_cd(m: f64) -> f64 {
    let pressure = if m <= 1.0 { 1.0 + m * m / 4.0 + m.powi(4) / 40.0 } else { 1.84 - 0.76 / (m * m) + 0.166 / m.powi(4) + 0.035 / m.powi(6) };
    0.85 * pressure
}

/// Von Karman nose pressure drag at Mach `m` for a nose of fineness
/// `fineness` (length / diameter): the TR-R-100 fineness-3 curve
/// extrapolated as OpenRocket does
/// (`SymmetricComponentCalc.calculateNoseInterpolator`, "Extrapolate for
/// fineness ratio"): `stag (cd_3 / stag)^(ln(f + 1) / ln 4)`, so fineness 3
/// returns the data and a slimmer nose a smaller fraction of the
/// stagnation CD. Zero below M 0.9 as in the data.
pub fn von_karman_pressure_cd(m: f64, fineness: f64) -> f64 {
    let (xs, ys) = &TR_R_100_VK_FR3;
    let cd3 = interp(xs, ys, m);
    if cd3 <= 0.0 {
        return 0.0;
    }
    let stag = stagnation_cd(m);
    stag * (cd3 / stag).powf((fineness + 1.0).ln() / 4f64.ln())
}

fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
            return ys[i - 1] + t * (ys[i] - ys[i - 1]);
        }
    }
    *ys.last().unwrap()
}

impl AeroBody {
    pub fn length(&self) -> f64 {
        self.nose_length + self.body_length
    }
    /// Nose fineness, length / diameter (OpenRocket
    /// `SymmetricComponentCalc`: `length / (2 |aftRadius - foreRadius|)`).
    pub fn nose_fineness(&self) -> f64 {
        self.nose_length / self.diameter
    }
    /// Nose pressure drag on the body cross-section at Mach `m`: the
    /// supplied CFD table when there is one, else the fineness-scaled
    /// von Karman curve.
    pub fn nose_pressure_cd(&self, m: f64) -> f64 {
        match &self.nose_cd_table {
            Some(t) if t.len() >= 2 => {
                let xs: Vec<f64> = t.iter().map(|r| r[0]).collect();
                let ys: Vec<f64> = t.iter().map(|r| r[1]).collect();
                interp(&xs, &ys, m).max(0.0)
            }
            _ => von_karman_pressure_cd(m, self.nose_fineness()),
        }
    }
    pub fn ref_area(&self) -> f64 {
        PI * self.diameter * self.diameter / 4.0
    }
    fn fin_area(&self) -> f64 {
        0.5 * (self.fin_root + self.fin_tip) * self.fin_span
    }
    /// Mid-chord sweep geometry: cos of mid-chord line angle.
    fn cos_gamma(&self) -> f64 {
        let dx = self.fin_sweep + self.fin_tip / 2.0 - self.fin_root / 2.0;
        self.fin_span / (dx * dx + self.fin_span * self.fin_span).sqrt()
    }
    fn mac(&self) -> f64 {
        let (cr, ct) = (self.fin_root, self.fin_tip);
        2.0 / 3.0 * (cr + ct - cr * ct / (cr + ct))
    }
    fn nose_volume(&self) -> f64 {
        self.nose_integrals().0
    }
    fn nose_wetted(&self) -> f64 {
        self.nose_integrals().1
    }
    fn nose_integrals(&self) -> (f64, f64) {
        *self.cache.get_or_init(|| (self.nose_volume_calc(), self.nose_wetted_calc()))
    }
    fn nose_wetted_calc(&self) -> f64 {
        let n = 100;
        let r = self.diameter / 2.0;
        (0..n)
            .map(|i| {
                let x0 = i as f64 / n as f64 * self.nose_length;
                let x1 = (i + 1) as f64 / n as f64 * self.nose_length;
                let r0 = von_karman_radius(x0, self.nose_length, r);
                let r1 = von_karman_radius(x1, self.nose_length, r);
                PI * (r0 + r1) * ((x1 - x0).powi(2) + (r1 - r0).powi(2)).sqrt()
            })
            .sum()
    }
    /// Nose volume by integrating the von Karman profile (for its CP).
    fn nose_volume_calc(&self) -> f64 {
        let n = 200;
        let r = self.diameter / 2.0;
        (0..n)
            .map(|i| {
                let x = (i as f64 + 0.5) / n as f64 * self.nose_length;
                PI * von_karman_radius(x, self.nose_length, r).powi(2) * self.nose_length / n as f64
            })
            .sum()
    }

    /// Normal-force slope (per rad) and CP from the tip at Mach `m`.
    pub fn cn_alpha_cp(&self, m: f64) -> (f64, f64) {
        // Nose: CN_alpha = 2 (Barrowman), CP = L - V/A_base.
        let cna_nose = 2.0;
        let x_nose = self.nose_length - self.nose_volume() / self.ref_area();
        // One fin, then the set.
        let s = self.fin_span;
        let a = self.fin_area();
        let aref = self.ref_area();
        let sub = |mm: f64| {
            2.0 * PI * s * s / (1.0 + (1.0 + (1.0 - mm * mm) * (s * s / (a * self.cos_gamma())).powi(2)).sqrt()) / aref
        };
        let sup = |mm: f64| a * 2.0 / (mm * mm - 1.0).sqrt() / aref;
        let cna1 = if m <= 0.9 {
            sub(m)
        } else if m >= 1.5 {
            sup(m)
        } else {
            let t = (m - 0.9) / 0.6;
            (1.0 - t) * sub(0.9) + t * sup(1.5)
        };
        let set_factor = match self.fin_count {
            3 => 1.5,
            4 => 2.0,
            n => n as f64 / 2.0,
        };
        let r = self.diameter / 2.0;
        let interference = 1.0 + r / (s + r);
        let cna_fins = cna1 * set_factor * interference;
        let (cr, ct, xr) = (self.fin_root, self.fin_tip, self.fin_sweep);
        let x_fin = self.fin_le_x + xr * (cr + 2.0 * ct) / (3.0 * (cr + ct)) + (cr + ct - cr * ct / (cr + ct)) / 6.0;
        let cna = cna_nose + cna_fins;
        (cna, (cna_nose * x_nose + cna_fins * x_fin) / cna)
    }

    /// Zero-lift drag coefficient (on the body cross-section) at Mach `m`,
    /// speed `v` and kinematic viscosity `nu`; `power_on` removes base drag
    /// over the nozzle exit area fraction `exit_frac` (OpenRocket subtracts
    /// the motor's area from the base while thrusting).
    pub fn cd0(&self, m: f64, v: f64, nu: f64, power_on: bool, exit_frac: f64) -> f64 {
        let l = self.length();
        let re = (v * l / nu).max(1.0);
        // Skin friction coefficient (turbulent, non-perfect finish branch).
        let mut cf = if re < 1e4 { 1.48e-2 } else { 1.0 / (1.50 * re.ln() - 5.6).powi(2) };
        let c1 = 1.0 - 0.1 * m * m;
        let c2 = 1.0 / (1.0 + 0.15 * m * m).powf(0.58);
        cf *= if m < 0.9 {
            c1
        } else if m < 1.1 {
            c2 * (m - 0.9) / 0.2 + c1 * (1.1 - m) / 0.2
        } else {
            c2
        };
        let rough_corr = if m < 0.9 {
            1.0 - 0.1 * m * m
        } else if m > 1.1 {
            1.0 / (1.0 + 0.18 * m * m)
        } else {
            let a = 1.0 - 0.1 * 0.81;
            let b = 1.0 / (1.0 + 0.18 * 1.21);
            b * (m - 0.9) / 0.2 + a * (1.1 - m) / 0.2
        };
        let cf = cf.max(0.032 * (self.roughness / l).powf(0.2) * rough_corr);
        let aref = self.ref_area();
        let d = self.diameter;
        // Wetted areas: nose (numerically), body cylinder, fins (both sides).
        let nose_wet = self.nose_wetted();
        let body_wet = PI * d * self.body_length;
        let fineness = l / d;
        let body_fric = cf * (nose_wet + body_wet) / aref * (1.0 + 1.0 / (2.0 * fineness));
        let fin_fric = cf * (1.0 + 2.0 * self.fin_thickness / self.mac()) * 2.0 * self.fin_area() * self.fin_count as f64 / aref;
        // Nose pressure drag: CFD table if given, else TR-R-100 von Karman
        // scaled to this nose's fineness (OpenRocket).
        let nose_p = self.nose_pressure_cd(m);
        // Fin fore drag, rounded leading edge, scaled by LE sweep cos^2.
        let le_cos = self.fin_span / (self.fin_sweep.powi(2) + self.fin_span.powi(2)).sqrt();
        let fin_fore = if m < 0.9 {
            (1.0 - m * m).powf(-0.417) - 1.0
        } else if m < 1.0 {
            1.0 - 1.785 * (m - 0.9)
        } else {
            1.214 - 0.502 / (m * m) + 0.1095 / m.powi(4)
        } * le_cos
            * le_cos
            * self.fin_span
            * self.fin_thickness
            * self.fin_count as f64
            / aref;
        // Fin trailing edge (rounded: half base drag) and body base.
        let base = if m <= 1.0 { 0.12 + 0.13 * m * m } else { 0.25 / m };
        let fin_base = base / 2.0 * self.fin_span * self.fin_thickness * self.fin_count as f64 / aref;
        let body_base = base * if power_on { (1.0 - exit_frac).max(0.0) } else { 1.0 };
        body_fric + fin_fric + nose_p + fin_fore + fin_base + body_base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AeroBody {
        AeroBody {
            diameter: 0.1524,
            nose_length: 0.76,
            body_length: 4.0,
            fin_count: 4,
            fin_root: 0.35,
            fin_tip: 0.12,
            fin_span: 0.15,
            fin_sweep: 0.20,
            fin_thickness: 0.005,
            fin_le_x: 4.76 - 0.36,
            roughness: 20e-6,
            nose_cd_table: None,
            cache: Default::default(),
        }
    }

    #[test]
    fn nose_pressure_drag_scales_with_fineness_like_openrocket() {
        // Fineness 3 is the TR-R-100 data itself.
        let (xs, ys) = &TR_R_100_VK_FR3;
        for (x, y) in xs.iter().zip(ys) {
            assert!((von_karman_pressure_cd(*x, 3.0) - y).abs() < 1e-12);
        }
        // Subsonic: nothing, as in the data.
        assert_eq!(von_karman_pressure_cd(0.8, 5.0), 0.0);
        // A 5:1 nose: OpenRocket's exponent ln 6 / ln 4 = 1.292 on the
        // fraction of stagnation CD. At M 1.2: stag 0.85 (1.84 - 0.76/1.44
        // + 0.166/2.0736 + 0.035/2.986) = 1.1934; 0.081/1.1934 = 0.06787;
        // ^1.2925 = 0.03089; x 1.1934 = 0.0369 (SU2 on this nose: 0.032-0.036).
        let cd = von_karman_pressure_cd(1.2, 5.0);
        assert!((cd - 0.0369).abs() < 0.0003, "{cd}");
        // Slimmer is lower, blunter is higher, monotone in fineness.
        assert!(von_karman_pressure_cd(1.2, 6.0) < cd && cd < von_karman_pressure_cd(1.2, 4.0));
        assert!(von_karman_pressure_cd(1.2, 2.0) > 0.081);
        // The sample body (0.76 m nose on 0.1524 m: fineness 4.99) uses the
        // scaled value; a CFD table replaces it.
        let b = sample();
        assert!((b.nose_fineness() - 4.987).abs() < 0.01);
        assert!((b.nose_pressure_cd(1.2) - von_karman_pressure_cd(1.2, b.nose_fineness())).abs() < 1e-12);
        let mut t = sample();
        t.nose_cd_table = Some(vec![[0.8, 0.0], [1.0, 0.020], [1.2, 0.034], [2.0, 0.036]]);
        assert!((t.nose_pressure_cd(1.1) - 0.027).abs() < 1e-12);
        assert!((t.nose_pressure_cd(3.0) - 0.036).abs() < 1e-12);
        let (d_scaled, d_table) = (b.cd0(1.2, 400.0, 1.5e-5, false, 0.0), t.cd0(1.2, 400.0, 1.5e-5, false, 0.0));
        assert!((d_scaled - d_table - (b.nose_pressure_cd(1.2) - 0.034)).abs() < 1e-12);
    }

    #[test]
    fn cp_sits_aft_and_cd_is_plausible() {
        let b = sample();
        let (cna, cp) = b.cn_alpha_cp(0.3);
        assert!(cna > 6.0 && cna < 16.0, "{cna}");
        assert!(cp > 3.0 && cp < b.length(), "{cp}");
        let cd = b.cd0(0.3, 100.0, 1.5e-5, false, 0.0);
        // Typical HPR 6-inch rocket power-off Cd 0.35-0.6.
        assert!(cd > 0.3 && cd < 0.7, "{cd}");
        // Transonic rise.
        assert!(b.cd0(1.05, 350.0, 1.5e-5, false, 0.0) > cd);
    }

    #[test]
    fn von_karman_profile_ends() {
        assert!(von_karman_radius(0.0, 1.0, 0.1).abs() < 1e-9);
        assert!((von_karman_radius(1.0, 1.0, 0.1) - 0.1).abs() < 1e-9);
    }
}
