//! Lift, ascent and descent laws, in SI through the `units` newtypes.
//!
//! Two independent models of the ascent exist on purpose:
//! * [`cusf_fill_for_ascent`] is the sizing rule, a transcription of the
//!   CUSF burst calculator (cuspaceflight/cusf-burst-calc @ a9c4d17,
//!   `js/calc.js` lines 292-328): constant sea-level air density 1.205,
//!   exponential density model with scale height 7238.3 m, constant Cd.
//! * the check (`flight::simulate`) integrates the ascent through the US76
//!   atmosphere with the gas expanding with the air and the drag
//!   coefficient following Reynolds number as measured on Totex balloons
//!   by Gallice et al. (2011), [`gallice_cd`].
//!
//! The CUSF calculator does not model Cd against Reynolds number; it uses a
//! constant Cd per balloon (0.25 or 0.30, calc.js `find_cd`, from Kaymont
//! Totex data). The Reynolds dependence comes from Gallice et al.

use crate::atmosphere::Air;
use crate::catalog::LiftGas;
use crate::rules;
use std::f64::consts::PI;
use wright_core::units::{Force, Length, Mass, Speed, Volume, G0};

/// Molar mass of the lift gas with `purity` mole fraction, the rest air, kg/mol.
pub fn gas_molar_mass(gas: LiftGas, purity: f64) -> f64 {
    let m = match gas {
        LiftGas::Helium => rules::M_HE,
        LiftGas::Hydrogen => rules::M_H2,
    };
    purity * m + (1.0 - purity) * rules::M_AIR
}

/// Moles of gas filling `volume` at the given air state (gas at air
/// temperature and pressure: zero superheat and superpressure, as CUSF and
/// the habhub predictor assume).
pub fn moles(volume: Volume, air: &Air) -> f64 {
    air.pressure.as_pa() * volume.as_m3() / (rules::R_UNIVERSAL * air.temperature.as_k())
}

/// Gross lift of `n` moles of gas: displaced air mass minus gas mass. It does
/// not change with altitude while the envelope is slack (Archimedes with the
/// ideal gas law: both scale with n).
pub fn gross_lift(n_moles: f64, gas: LiftGas, purity: f64) -> Mass {
    Mass::kg(n_moles * (rules::M_AIR - gas_molar_mass(gas, purity)))
}

/// Envelope volume of `n` moles at an air state, m^3.
pub fn volume_at(n_moles: f64, air: &Air) -> Volume {
    Volume::m3(n_moles * rules::R_UNIVERSAL * air.temperature.as_k() / air.pressure.as_pa())
}

pub fn sphere_diameter(v: Volume) -> Length {
    Length::m((6.0 * v.as_m3() / PI).cbrt())
}

/// Free lift: gross lift minus everything it carries, as a force.
pub fn free_lift(gross: Mass, carried: Mass) -> Force {
    Force::n((gross - carried).as_kg() * G0)
}

/// Terminal ascent rate of a sphere of `volume` with `free` lift:
/// F = 1/2 rho v^2 Cd A  =>  v = sqrt(2F / (rho Cd A)), A = pi d^2 / 4.
/// Zero or negative free lift gives 0 m/s (the balloon does not rise).
pub fn ascent_rate(free: Force, volume: Volume, cd: f64, air: &Air) -> Speed {
    let d = sphere_diameter(volume).as_m();
    let area = PI * d * d / 4.0;
    let f = free.as_n();
    if f <= 0.0 {
        return Speed::mps(0.0);
    }
    Speed::mps((2.0 * f / (air.density.as_kg_m3() * cd * area)).sqrt())
}

/// Drag coefficient of a latex sounding balloon against Reynolds number
/// Re = rho R v / mu (R the radius: their Sect. 2.3.1 defines "Re = ρaRvz/µ"
/// with R the balloon effective radius; re-read 2026-10-24 after a review
/// asked whether it is the diameter), from flights: Gallice et al. 2011,
/// "Modeling the ascent of sounding balloons", Atmos. Meas. Tech. 4,
/// 2235-2253, eq. (6): cD = 4.808e-2 (ln Re)^2 - 1.406 ln Re + 10.490,
/// fitted on ten Totex TX1200 night flights for Re ~6e4..9e5 (+-0.04).
/// Re is clamped to that range.
pub fn gallice_cd(re: f64) -> f64 {
    let l = re.clamp(rules::GALLICE_RE_MIN, rules::GALLICE_RE_MAX).ln();
    4.808e-2 * l * l - 1.406 * l + 10.490
}

/// Terminal ascent rate with the Reynolds-dependent drag of [`gallice_cd`]
/// (fixed-point on v, since Re depends on v). Returns (rate, Cd).
pub fn ascent_rate_re(free: Force, volume: Volume, air: &Air) -> (Speed, f64) {
    let r = sphere_diameter(volume).as_m() / 2.0;
    let mut cd = 0.3;
    let mut v = ascent_rate(free, volume, cd, air);
    for _ in 0..50 {
        let re = air.density.as_kg_m3() * r * v.as_mps() / air.viscosity;
        let c = gallice_cd(re);
        let nv = ascent_rate(free, volume, c, air);
        let done = (nv.as_mps() - v.as_mps()).abs() < 1e-6;
        v = Speed::mps(0.5 * (v.as_mps() + nv.as_mps()));
        cd = c;
        if done {
            break;
        }
    }
    (v, cd)
}

/// Terminal descent rate under a canopy of nominal diameter `d`:
/// v = sqrt(2 m g / (rho Cd A)), A = pi d^2 / 4 (Rocketman / Spherachutes
/// quote Cd on the nominal area).
pub fn descent_rate(mass: Mass, chute_d: Length, cd: f64, air: &Air) -> Speed {
    let area = PI * chute_d.as_m().powi(2) / 4.0;
    Speed::mps((2.0 * mass.as_kg() * G0 / (air.density.as_kg_m3() * cd * area)).sqrt())
}

/// The CUSF fill for a target ascent rate: solves calc.js's cubic
/// a r^3 + b r^2 + d = 0 (lines 298-315) for the launch radius and returns
/// (launch volume, neck lift). Inputs: balloon and payload mass, Cd, gas
/// density at CUSF's reference (kg/m^3).
pub fn cusf_fill_for_ascent(balloon: Mass, payload: Mass, cd: f64, rho_gas: f64, target: Speed) -> (Volume, Mass) {
    let rho_a = rules::CUSF_RHO_AIR;
    let ga = G0;
    let tar = target.as_mps();
    let a = ga * (rho_a - rho_gas) * (4.0 / 3.0) * PI;
    let b = -0.5 * tar * tar * cd * rho_a * PI;
    let c = 0.0;
    let d = -(payload.as_kg() + balloon.as_kg()) * ga;
    let f = ((3.0 * c) / a) - (b * b / (a * a)) / 3.0;
    let g = ((2.0 * b.powi(3)) / a.powi(3) - (9.0 * b * c) / (a * a) + (27.0 * d) / a) / 27.0;
    let h = g * g / 4.0 + f.powi(3) / 27.0;
    let r_ = -0.5 * g + h.sqrt();
    let s = r_.cbrt();
    let t = -0.5 * g - h.sqrt();
    let u = t.cbrt();
    let radius = (s + u) - b / (3.0 * a);
    let v = (4.0 / 3.0) * PI * radius.powi(3);
    let gross = v * (rho_a - rho_gas);
    (Volume::m3(v), Mass::kg(gross) - balloon)
}

/// CUSF's burst-altitude estimate (calc.js line 327): -adm ln(V_launch / V_burst).
pub fn cusf_burst_altitude(launch: Volume, burst_diameter: Length) -> Length {
    let vb = PI / 6.0 * burst_diameter.as_m().powi(3);
    Length::m(-rules::CUSF_ADM_M * (launch.as_m3() / vb).ln())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atmosphere;

    /// The CUSF page's own example: a Totex 1000 g (burst 7.86 m, Cd 0.3)
    /// with a 1000 g payload at 5 m/s ascent on helium. Re-deriving the
    /// ascent rate from the solved radius must return 5 m/s.
    #[test]
    fn cusf_cubic_round_trips() {
        let (v, neck) = cusf_fill_for_ascent(Mass::g(1000.0), Mass::g(1000.0), 0.3, rules::CUSF_RHO_HE, Speed::mps(5.0));
        let r = (3.0 * v.as_m3() / (4.0 * PI)).cbrt();
        let gross = v.as_m3() * (rules::CUSF_RHO_AIR - rules::CUSF_RHO_HE);
        let free = (gross - 2.0) * G0;
        let ar = (free / (0.5 * 0.3 * PI * r * r * rules::CUSF_RHO_AIR)).sqrt();
        assert!((ar - 5.0).abs() < 1e-6, "{ar}");
        assert!(neck.as_g() > 1000.0);
        let burst = cusf_burst_altitude(v, Length::m(7.86));
        assert!(burst.as_km() > 25.0 && burst.as_km() < 40.0, "{}", burst.as_km());
    }

    #[test]
    fn helium_lifts_about_1_kg_per_m3() {
        let air = atmosphere::at(Length::ZERO);
        let n = moles(Volume::m3(1.0), &air);
        let l = gross_lift(n, LiftGas::Helium, 1.0).as_kg();
        // rho_air - rho_He at 15 C, 1 atm = 1.225 - 0.169 = 1.056 kg/m^3
        assert!((l - 1.056).abs() < 0.005, "{l}");
    }
}
