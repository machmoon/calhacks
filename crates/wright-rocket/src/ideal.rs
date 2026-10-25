//! Calorically perfect (constant-gamma) nozzle relations, Sutton & Biblarz,
//! Rocket Propulsion Elements 9th ed., ch. 3 (eqs 3-14, 3-25, 3-30). Used
//! for the conical-nozzle Mach number along the contour and as an
//! independent cross-check of the equilibrium expansion; never for the
//! performance numbers themselves (those come from `performance`).

/// Area ratio A/A* at Mach `m` for ratio of specific heats `g` (Sutton eq 3-14).
pub fn area_ratio_from_mach(m: f64, g: f64) -> f64 {
    let t = 2.0 / (g + 1.0) * (1.0 + (g - 1.0) / 2.0 * m * m);
    t.powf((g + 1.0) / (2.0 * (g - 1.0))) / m
}

/// Static/total pressure ratio p/p0 at Mach `m` (isentropic).
pub fn pressure_ratio_from_mach(m: f64, g: f64) -> f64 {
    (1.0 + (g - 1.0) / 2.0 * m * m).powf(-g / (g - 1.0))
}

/// Supersonic Mach number at area ratio `eps` (> 1), by bisection on [1, 50].
pub fn supersonic_mach(eps: f64, g: f64) -> f64 {
    let (mut lo, mut hi) = (1.0, 50.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if area_ratio_from_mach(mid, g) < eps { lo = mid } else { hi = mid }
    }
    0.5 * (lo + hi)
}

/// Exit-to-chamber pressure ratio pe/pc for a supersonic nozzle of area ratio `eps`.
pub fn exit_pressure_ratio(eps: f64, g: f64) -> f64 {
    pressure_ratio_from_mach(supersonic_mach(eps, g), g)
}

/// Thrust coefficient (Sutton eq 3-30): pe/pc and pa/pc dimensionless, `eps` = Ae/At.
pub fn thrust_coefficient(g: f64, pe_pc: f64, pa_pc: f64, eps: f64) -> f64 {
    let a = 2.0 * g * g / (g - 1.0) * (2.0 / (g + 1.0)).powf((g + 1.0) / (g - 1.0));
    (a * (1.0 - pe_pc.powf((g - 1.0) / g))).sqrt() + (pe_pc - pa_pc) * eps
}

/// Characteristic velocity c* (m/s) for chamber temperature `tc` (K), molar
/// mass `m_kg_mol` (kg/mol) and gamma `g` (Sutton eq 3-32).
pub fn c_star(tc: f64, m_kg_mol: f64, g: f64) -> f64 {
    let r = crate::thermo::R / m_kg_mol;
    (g * r * tc).sqrt() / (g * (2.0 / (g + 1.0)).powf((g + 1.0) / (g - 1.0)).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sutton_table_points() {
        // gamma 1.4: A/A* = 1.6875 at M 2 (NACA 1135 table).
        assert!((area_ratio_from_mach(2.0, 1.4) - 1.6875).abs() < 1e-4);
        assert!((supersonic_mach(1.6875, 1.4) - 2.0).abs() < 1e-6);
        // Vacuum-optimum Cf limit for g 1.2 with eps 10 ~ 1.6 (Sutton Fig 3-6 range).
        let pe = exit_pressure_ratio(10.0, 1.2);
        let cf = thrust_coefficient(1.2, pe, pe, 10.0);
        assert!(cf > 1.5 && cf < 1.7, "{cf}");
    }
}
