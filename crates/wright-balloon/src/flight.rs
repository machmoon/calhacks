//! The flight, time-stepped: ascent through US76 with the gas expanding
//! until the envelope reaches its burst diameter, descent under the chute,
//! drift on the wind profile, then the recovery window on the ground.
//! This is the check's model; the fill was sized by the CUSF closed form
//! (`physics::cusf_fill_for_ascent`), so the two can disagree and the
//! check sees it.

use crate::atmosphere;
use crate::catalog::Catalog;
use crate::mass::Masses;
use crate::model::{Design, Spec, WindPoint};
use crate::physics;
use anyhow::Result;
use serde::Serialize;
use wright_core::units::{Length, Mass, Speed};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Phase {
    Ascent,
    Descent,
    Landed,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sample {
    pub t_s: f64,
    pub z_m: f64,
    /// East and north of the launch site, m.
    pub x_m: f64,
    pub y_m: f64,
    pub phase: Phase,
}

#[derive(Debug, Clone, Serialize)]
pub struct Flight {
    pub samples: Vec<Sample>,
    pub launch_volume_m3: f64,
    pub launch_diameter_m: f64,
    pub free_lift_kg: f64,
    pub launch_ascent_mps: f64,
    pub mean_ascent_mps: f64,
    /// None: the envelope never reached its burst diameter (a floater, or no lift).
    pub burst_altitude_m: Option<f64>,
    pub burst_t_s: f64,
    pub landing_speed_mps: f64,
    pub landing_t_s: f64,
    pub landing_xy_m: (f64, f64),
    /// The CUSF estimate of the same fill, for comparison.
    pub cusf_burst_altitude_m: f64,
}

/// ASSUMPTION (no forecast wired in): a typical mid-latitude profile,
/// westerly, with a 30 m/s jet at the tropopause and light stratospheric
/// winds. Replace with GFS winds for a real launch (predict.sondehub.org).
pub fn default_wind() -> Vec<WindPoint> {
    [(0.0, 3.0), (2_000.0, 8.0), (6_000.0, 18.0), (11_000.0, 30.0), (16_000.0, 12.0), (20_000.0, 4.0), (30_000.0, 8.0), (45_000.0, 12.0)]
        .into_iter()
        .map(|(alt_m, speed_mps)| WindPoint { alt_m, speed_mps, toward_deg: 90.0 })
        .collect()
}

fn wind_at(profile: &[WindPoint], z: f64) -> (f64, f64) {
    let (s, dir) = if z <= profile[0].alt_m {
        (profile[0].speed_mps, profile[0].toward_deg)
    } else if z >= profile[profile.len() - 1].alt_m {
        let p = profile[profile.len() - 1];
        (p.speed_mps, p.toward_deg)
    } else {
        let i = profile.windows(2).position(|w| z >= w[0].alt_m && z <= w[1].alt_m).unwrap_or(0);
        let (a, b) = (profile[i], profile[i + 1]);
        let f = (z - a.alt_m) / (b.alt_m - a.alt_m);
        (a.speed_mps + f * (b.speed_mps - a.speed_mps), a.toward_deg + f * (b.toward_deg - a.toward_deg))
    };
    let r = dir.to_radians();
    (s * r.sin(), s * r.cos())
}

const DT_S: f64 = 5.0;
/// Give up on an ascent after 6 h (a floater).
const MAX_ASCENT_S: f64 = 6.0 * 3600.0;

pub fn simulate(spec: &Spec, d: &Design, cat: &Catalog, m: &Masses) -> Result<Flight> {
    let balloon = cat.balloon(&d.balloon)?;
    let gas = cat.gas(&d.gas)?;
    let chute = cat.chute(&d.parachute)?;
    let wind = if spec.wind.is_empty() { default_wind() } else { spec.wind.clone() };
    let z0 = spec.launch.elevation_m;
    let air0 = atmosphere::at(Length::m(z0));

    // Fill: the neck lift the team measures fixes the moles of gas.
    let gross = Mass::g(d.neck_lift_g + balloon.mass_g);
    let per_mol = crate::rules::M_AIR - physics::gas_molar_mass(gas.gas, gas.purity);
    let n = gross.as_kg() / per_mol;
    let v0 = physics::volume_at(n, &air0);
    let carried = Mass::g(balloon.mass_g) + m.flight_train();
    let free = physics::free_lift(gross, carried);
    let burst_d = balloon.burst_diameter_m;

    let mut s = Vec::new();
    let (mut t, mut z, mut x, mut y) = (0.0, z0, 0.0, 0.0);
    let launch_rate = physics::ascent_rate_re(free, v0, &air0).0.as_mps();
    let mut burst = None;
    s.push(Sample { t_s: 0.0, z_m: z, x_m: 0.0, y_m: 0.0, phase: Phase::Ascent });
    while t < MAX_ASCENT_S {
        let air = atmosphere::at(Length::m(z));
        let v = physics::volume_at(n, &air);
        if physics::sphere_diameter(v).as_m() >= burst_d {
            burst = Some(z);
            break;
        }
        let rate = physics::ascent_rate_re(free, v, &air).0.as_mps();
        if rate < 0.05 {
            break;
        }
        let (wx, wy) = wind_at(&wind, z);
        z += rate * DT_S;
        x += wx * DT_S;
        y += wy * DT_S;
        t += DT_S;
        s.push(Sample { t_s: t, z_m: z, x_m: x, y_m: y, phase: Phase::Ascent });
    }
    let burst_t = t;
    let mean_ascent = if t > 0.0 { (z - z0) / t } else { 0.0 };

    // Descent under the chute (from wherever the ascent ended).
    let desc_mass = m.descent();
    let mut landing_speed = 0.0;
    if burst.is_some() {
        while z > z0 {
            let air = atmosphere::at(Length::m(z));
            let rate = physics::descent_rate(desc_mass, Length::m(chute.diameter_m), chute.cd, &air).as_mps();
            let (wx, wy) = wind_at(&wind, z);
            let step = (rate * DT_S).min(z - z0);
            let dt = step / rate;
            z -= step;
            x += wx * dt;
            y += wy * dt;
            t += dt;
            s.push(Sample { t_s: t, z_m: z, x_m: x, y_m: y, phase: Phase::Descent });
        }
        landing_speed = physics::descent_rate(desc_mass, Length::m(chute.diameter_m), chute.cd, &air0).as_mps();
    }
    let land_t = t;
    let end = t + spec.recovery_window_h * 3600.0;
    while t < end {
        t = (t + 60.0).min(end);
        s.push(Sample { t_s: t, z_m: z, x_m: x, y_m: y, phase: Phase::Landed });
    }

    let rho_g = match gas.gas {
        crate::catalog::LiftGas::Helium => crate::rules::CUSF_RHO_HE,
        crate::catalog::LiftGas::Hydrogen => crate::rules::CUSF_RHO_H2,
    };
    let v_cusf = gross.as_kg() / (crate::rules::CUSF_RHO_AIR - rho_g);
    let cusf_burst = physics::cusf_burst_altitude(wright_core::units::Volume::m3(v_cusf), Length::m(burst_d)).as_m() + z0;

    Ok(Flight {
        samples: s,
        launch_volume_m3: v0.as_m3(),
        launch_diameter_m: physics::sphere_diameter(v0).as_m(),
        free_lift_kg: free.as_n() / wright_core::units::G0,
        launch_ascent_mps: launch_rate,
        mean_ascent_mps: mean_ascent,
        burst_altitude_m: burst,
        burst_t_s: burst_t,
        landing_speed_mps: landing_speed,
        landing_t_s: land_t,
        landing_xy_m: (x, y),
        cusf_burst_altitude_m: cusf_burst,
    })
}

/// The fill (neck lift, g) the CUSF rule gives for the target ascent rate.
pub fn size_fill(d: &Design, cat: &Catalog, m: &Masses, target: Speed) -> Result<f64> {
    // CUSF sizes with its constant per-balloon Cd; the check uses Gallice Cd(Re).
    let balloon = cat.balloon(&d.balloon)?;
    let gas = cat.gas(&d.gas)?;
    let rho_g = match gas.gas {
        crate::catalog::LiftGas::Helium => crate::rules::CUSF_RHO_HE,
        crate::catalog::LiftGas::Hydrogen => crate::rules::CUSF_RHO_H2,
    };
    let (_, neck) = physics::cusf_fill_for_ascent(Mass::g(balloon.mass_g), m.flight_train(), balloon.cd, rho_g, target);
    Ok(neck.as_g())
}
