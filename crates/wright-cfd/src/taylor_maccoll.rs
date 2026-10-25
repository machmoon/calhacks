//! Exact supersonic flow over a sharp cone at zero incidence (Taylor and
//! Maccoll 1933), the reference SU2's axisymmetric Euler is checked against.
//!
//! Equations as in Anderson, *Modern Compressible Flow* 3rd ed., sect. 10.4
//! (eq. 10.15, the Taylor-Maccoll equation in V' = V / V_max, and the
//! oblique-shock start, eqs. 4.7-4.12); the same theory as the cone charts
//! of NACA Report 1135 (Ames Research Staff, 1953, charts 5-6). This is a
//! reference value for validation only; nothing in a check reads it.

fn oblique(m1: f64, beta: f64, g: f64) -> (f64, f64, f64) {
    // returns (deflection delta, M2, p2/p1)
    let mn1 = m1 * beta.sin();
    let tan_d = 2.0 / beta.tan() * (mn1 * mn1 - 1.0) / (m1 * m1 * (g + (2.0 * beta).cos()) + 2.0);
    let delta = tan_d.atan();
    let mn2 = ((1.0 + (g - 1.0) / 2.0 * mn1 * mn1) / (g * mn1 * mn1 - (g - 1.0) / 2.0)).sqrt();
    let m2 = mn2 / (beta - delta).sin();
    let p21 = 1.0 + 2.0 * g / (g + 1.0) * (mn1 * mn1 - 1.0);
    (delta, m2, p21)
}

fn rhs(th: f64, vr: f64, vt: f64, g: f64) -> (f64, f64) {
    let a = (g - 1.0) / 2.0 * (1.0 - vr * vr - vt * vt);
    let num = vt * vt * vr - a * (2.0 * vr + vt / th.tan());
    let den = a - vt * vt;
    (vt, num / den)
}

/// Integrate from the shock at `beta` towards the axis until V_theta = 0.
/// Returns (cone angle, surface V').
fn march(m1: f64, beta: f64, g: f64) -> Option<(f64, f64)> {
    let (delta, m2, _) = oblique(m1, beta, g);
    if delta.is_nan() || delta <= 0.0 || !m2.is_finite() {
        return None;
    }
    let v = (2.0 / ((g - 1.0) * m2 * m2) + 1.0).powf(-0.5);
    let (mut vr, mut vt) = (v * (beta - delta).cos(), -v * (beta - delta).sin());
    let mut th = beta;
    let h = -1e-5;
    while vt < 0.0 {
        // RK4 in theta
        let (k1a, k1b) = rhs(th, vr, vt, g);
        let (k2a, k2b) = rhs(th + h / 2.0, vr + h / 2.0 * k1a, vt + h / 2.0 * k1b, g);
        let (k3a, k3b) = rhs(th + h / 2.0, vr + h / 2.0 * k2a, vt + h / 2.0 * k2b, g);
        let (k4a, k4b) = rhs(th + h, vr + h * k3a, vt + h * k3b, g);
        let (nvr, nvt) = (vr + h / 6.0 * (k1a + 2.0 * k2a + 2.0 * k3a + k4a), vt + h / 6.0 * (k1b + 2.0 * k2b + 2.0 * k3b + k4b));
        if nvt >= 0.0 {
            let t = vt / (vt - nvt);
            return Some((th + t * h, vr + t * (nvr - vr)));
        }
        vr = nvr;
        vt = nvt;
        th += h;
        if th <= 1e-4 {
            return None;
        }
    }
    Some((th, vr))
}

#[derive(Debug, Clone, Copy)]
pub struct Cone {
    pub shock_angle_deg: f64,
    pub surface_mach: f64,
    /// Surface pressure over free-stream pressure.
    pub p_ratio: f64,
    /// Surface pressure coefficient = the cone's pressure drag coefficient
    /// on its base area.
    pub cp: f64,
}

/// Attached-shock cone solution for free-stream Mach `m1`, half-angle
/// `theta_c_deg`, gamma `g`; None if the shock detaches.
pub fn cone(m1: f64, theta_c_deg: f64, g: f64) -> Option<Cone> {
    let tc = theta_c_deg.to_radians();
    let mu = (1.0 / m1).asin();
    // weak branch: cone angle grows with beta from the Mach angle up to the
    // detachment maximum; bisect on [mu, beta_max]
    let mut grid = vec![];
    let n = 400;
    for k in 1..n {
        let t = k as f64 / n as f64;
        let b = mu + (std::f64::consts::FRAC_PI_2 - mu) * t * t;
        if let Some((c, _)) = march(m1, b, g) {
            grid.push((b, c));
        }
    }
    let k = grid.windows(2).position(|w| (w[0].1 - tc) * (w[1].1 - tc) <= 0.0)?;
    let (mut lo, mut hi) = (grid[k].0, grid[k + 1].0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let c = march(m1, mid, g)?.0;
        if (c - tc) * (march(m1, lo, g)?.0 - tc) <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let beta = 0.5 * (lo + hi);
    let (_, vc) = march(m1, beta, g)?;
    let (_, m2, p21) = oblique(m1, beta, g);
    let mc = (2.0 / (g - 1.0) * vc * vc / (1.0 - vc * vc)).sqrt();
    let iso = |m: f64| (1.0 + (g - 1.0) / 2.0 * m * m).powf(g / (g - 1.0));
    let p_ratio = p21 * iso(m2) / iso(mc);
    Some(Cone { shock_angle_deg: beta.to_degrees(), surface_mach: mc, p_ratio, cp: (p_ratio - 1.0) / (0.5 * g * m1 * m1) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thin_cone_tends_to_the_mach_angle_and_known_values() {
        // a thin cone's shock sits just outside the Mach wave (30 deg at M 2)
        let c = cone(2.0, 3.0, 1.4).unwrap();
        assert!(c.shock_angle_deg > 30.0 && c.shock_angle_deg < 31.0, "{c:?}");
        // M 2, 10 deg cone: shock ~31.2 deg, surface Mach ~1.8 (NACA 1135 chart 5/6 range)
        let c = cone(2.0, 10.0, 1.4).unwrap();
        assert!(c.shock_angle_deg > 30.5 && c.shock_angle_deg < 32.5, "{c:?}");
        assert!(c.cp > 0.07 && c.cp < 0.12, "{c:?}");
        // a 40 deg cone at M 1.5 detaches
        assert!(cone(1.5, 40.0, 1.4).is_none());
    }
}
