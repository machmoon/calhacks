//! The International Standard Atmosphere, troposphere (ICAO Doc 7488/3).

use crate::rules::*;
use wright_core::units::{Density, Length, Pressure, Speed, Temperature, G0};

#[derive(Debug, Clone, Copy)]
pub struct Air {
    pub altitude: Length,
    pub temperature: Temperature,
    pub pressure: Pressure,
    pub density: Density,
    /// Dynamic viscosity, Pa s.
    pub viscosity_pa_s: f64,
}

impl Air {
    /// Reynolds number for `speed` over `length`.
    pub fn reynolds(&self, speed: Speed, length: Length) -> f64 {
        self.density.value() * speed.value() * length.value() / self.viscosity_pa_s
    }
    /// q = rho V^2 / 2, Pa.
    pub fn q(&self, speed: Speed) -> Pressure {
        Pressure::pa(0.5 * self.density.value() * speed.value() * speed.value())
    }
}

/// ISA at GEOMETRIC `altitude` (converted to geopotential, US 1976 eq. 18,
/// r0 = 6356766 m), below 11 km geopotential; `delta_t_k` hotter than
/// standard at the same pressure: a hot day is the case that sizes hover.
pub fn isa(altitude: Length, delta_t_k: f64) -> Air {
    const R0: f64 = 6_356_766.0;
    let z = altitude.as_m();
    let h = (R0 * z / (R0 + z)).clamp(-500.0, 11_000.0);
    let t_std = ISA_T0_K - ISA_LAPSE_K_PER_M * h;
    let p = ISA_P0_PA * (t_std / ISA_T0_K).powf(G0 / (ISA_LAPSE_K_PER_M * ISA_R_J_PER_KG_K));
    let t = t_std + delta_t_k;
    let rho = p / (ISA_R_J_PER_KG_K * t);
    let mu = SUTHERLAND_MU_REF * (t / SUTHERLAND_T_REF_K).powf(1.5) * (SUTHERLAND_T_REF_K + SUTHERLAND_S_K)
        / (t + SUTHERLAND_S_K);
    Air {
        altitude,
        temperature: Temperature::k(t),
        pressure: Pressure::pa(p),
        density: Density::kg_m3(rho),
        viscosity_pa_s: mu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sea_level_standard() {
        let a = isa(Length::m(0.0), 0.0);
        assert!((a.density.as_kg_m3() - 1.225).abs() < 1e-3);
        assert!((a.viscosity_pa_s - 1.789e-5).abs() < 1e-7);
        let hot = isa(Length::m(0.0), 15.0);
        assert!(hot.density.as_kg_m3() < 1.17);
    }
}
