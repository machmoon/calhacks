//! Planar 3-DOF flight (x downrange, z up, pitch theta): thrust along the
//! body axis, Barrowman normal force at the CP, OpenRocket-style drag, the
//! Mandell/Barrowman pitch damping moment (aerodynamic + jet damping, as in
//! Apogee "Peak of Flight" #193-195 and OpenRocket's
//! `BarrowmanCalculator.getDampingMultiplier` lineage), a launch rail, and
//! wind with a 1/7 power-law profile (Peterson & Hennessey 1978, J. Appl.
//! Meteor. 17:390). Integrated with classical RK4.
//!
//! Rail handling follows RocketPy (`rocketpy/simulation/flight.py`,
//! `u_dot_rail1`): motion constrained to the rail axis until the vehicle
//! has travelled the rail length; we use CG travel (RocketPy uses the upper
//! rail button), stated.

use crate::atmosphere;
use crate::vehicle::Vehicle;
use serde::{Deserialize, Serialize};
use wright_core::units::G0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Launch {
    /// Site elevation MSL, m.
    pub elevation: f64,
    pub rail_length: f64,
    /// Rail angle from vertical, rad (tilted toward +x).
    pub rail_tilt: f64,
    /// Wind speed at 10 m AGL, m/s, blowing toward -x (into the tilt).
    pub wind_10m: f64,
    /// Thrust scale (1.0 nominal) for dispersion runs.
    pub thrust_scale: f64,
    /// Drag scale (1.0 nominal) for dispersion runs.
    pub drag_scale: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightResult {
    pub apogee_agl: f64,
    pub apogee_time: f64,
    pub rail_exit_speed: f64,
    pub rail_exit_time: f64,
    pub max_speed: f64,
    pub max_mach: f64,
    pub max_q: f64,
    pub max_q_alt_agl: f64,
    pub max_accel_g: f64,
    pub burnout_alt_agl: f64,
    pub burnout_speed: f64,
    /// Static margin (cal) sampled from rail exit to burnout: min, max.
    pub margin_min: f64,
    pub margin_max: f64,
    pub margin_at_rail_exit: f64,
    /// Peak angle of attack after rail exit, deg.
    pub max_aoa_deg: f64,
    pub liftoff_twr: f64,
    pub drift_at_apogee: f64,
    pub trace: Vec<[f64; 6]>,
}

#[derive(Clone, Copy)]
struct S {
    x: f64,
    z: f64,
    vx: f64,
    vz: f64,
    th: f64,
    w: f64,
}

fn wind(l: &Launch, z_agl: f64) -> f64 {
    if z_agl <= 0.0 {
        return -l.wind_10m * 0.5;
    }
    -l.wind_10m * (z_agl.max(1.0) / 10.0).powf(1.0 / 7.0)
}

pub fn fly(v: &Vehicle, l: &Launch) -> FlightResult {
    let e = &v.engine;
    let tb = v.burn_time;
    let aref = v.aero.ref_area();
    let d = v.diameter;
    let exit_frac = e.exit_area() / aref;
    let mut r = FlightResult { margin_min: f64::MAX, margin_max: f64::MIN, ..Default::default() };
    let th0 = std::f64::consts::FRAC_PI_2 - l.rail_tilt;
    let mut s = S { x: 0.0, z: 0.0, vx: 0.0, vz: 0.0, th: th0, w: 0.0 };
    let mut t = 0.0;
    let mut on_rail = true;
    let m0 = v.mass_at(0.0);
    r.liftoff_twr = v.thrust_at(0.0, atmosphere::at(l.elevation).p) * l.thrust_scale / (m0.mass * G0);

    // Derivatives; also returns (accel magnitude, q, mach, margin, aoa).
    let deriv = |t: f64, s: &S, on_rail: bool| -> (S, [f64; 5]) {
        let z_msl = l.elevation + s.z.max(0.0);
        let air = atmosphere::at(z_msl);
        let ms = v.mass_at(t);
        let thrust = v.thrust_at(t, air.p) * l.thrust_scale;
        let wx = wind(l, s.z);
        let (rvx, rvz) = (s.vx - wx, s.vz);
        let vr = (rvx * rvx + rvz * rvz).sqrt();
        let mach = vr / air.a;
        let q = 0.5 * air.rho * vr * vr;
        let (cna, xcp) = v.aero.cn_alpha_cp(mach.max(0.01));
        let margin = (xcp - ms.cg) / d;
        // Angle of attack: body axis vs relative wind.
        let (bx, bz) = (s.th.cos(), s.th.sin());
        let alpha = if vr > 1e-3 { (bx * rvz - bz * rvx).atan2(bx * rvx + bz * rvz) } else { 0.0 };
        let cd = v.aero.cd0(mach, vr.max(0.1), air.mu / air.rho, t < tb, exit_frac) * l.drag_scale;
        let drag = q * aref * cd;
        let normal = q * aref * cna * alpha.sin();
        // Drag along -relative velocity; normal force perpendicular to the
        // body axis, opposing alpha.
        let (dx, dz) = if vr > 1e-3 { (-rvx / vr * drag, -rvz / vr * drag) } else { (0.0, 0.0) };
        // Body normal unit vector (rotate body axis +90 deg): (-bz, bx).
        let (nx, nz) = (bz * normal, -bx * normal);
        let mut fx = thrust * bx + dx + nx;
        let mut fz = thrust * bz + dz + nz - ms.mass * G0;
        let mut ax = fx / ms.mass;
        let mut az = fz / ms.mass;
        let mut wdot = 0.0;
        if on_rail {
            // Project onto the rail axis; no rotation; no sliding back.
            let a_along = ax * bx + az * bz;
            let a_along = if s.vx * bx + s.vz * bz <= 0.0 { a_along.max(0.0) } else { a_along };
            ax = a_along * bx;
            az = a_along * bz;
            fx = ax * ms.mass;
            fz = az * ms.mass;
        } else {
            // Restoring moment from the normal force at the CP, plus damping
            // C2 = rho v /2 A sum(CNa_i (x_i - cg)^2) + mdot (x_nozzle - cg)^2.
            let moment = normal * (xcp - ms.cg);
            let mdot = v.mdot_at(t);
            let c2_aero = 0.5 * air.rho * vr * aref * (2.0 * (v.nose_length * 0.5 - ms.cg).powi(2) + (cna - 2.0) * (xcp - ms.cg).powi(2));
            let c2_jet = mdot * (v.length - ms.cg).powi(2);
            wdot = (moment - (c2_aero + c2_jet) * s.w) / ms.inertia;
        }
        let _ = (fx, fz);
        let accel = ((ax).powi(2) + (az + G0).powi(2)).sqrt() / G0;
        (S { x: s.vx, z: s.vz, vx: ax, vz: az, th: if on_rail { 0.0 } else { s.w }, w: wdot }, [accel, q, mach, margin, alpha.to_degrees()])
    };

    let add = |a: &S, b: &S, h: f64| S { x: a.x + b.x * h, z: a.z + b.z * h, vx: a.vx + b.vx * h, vz: a.vz + b.vz * h, th: a.th + b.th * h, w: a.w + b.w * h };
    let mut rail_done_at = None;
    for _ in 0..200_000 {
        let dt = if t < tb + 1.0 { 0.01 } else { 0.05 };
        let (k1, info) = deriv(t, &s, on_rail);
        let (k2, _) = deriv(t + dt / 2.0, &add(&s, &k1, dt / 2.0), on_rail);
        let (k3, _) = deriv(t + dt / 2.0, &add(&s, &k2, dt / 2.0), on_rail);
        let (k4, _) = deriv(t + dt, &add(&s, &k3, dt), on_rail);
        let comb = |f: fn(&S) -> f64| (f(&k1) + 2.0 * f(&k2) + 2.0 * f(&k3) + f(&k4)) / 6.0;
        let ns = S {
            x: s.x + dt * comb(|k| k.x),
            z: s.z + dt * comb(|k| k.z),
            vx: s.vx + dt * comb(|k| k.vx),
            vz: s.vz + dt * comb(|k| k.vz),
            th: s.th + dt * comb(|k| k.th),
            w: s.w + dt * comb(|k| k.w),
        };
        let speed = (s.vx * s.vx + s.vz * s.vz).sqrt();
        r.max_accel_g = r.max_accel_g.max(info[0]);
        if info[1] > r.max_q {
            r.max_q = info[1];
            r.max_q_alt_agl = s.z;
        }
        r.max_mach = r.max_mach.max(info[2]);
        r.max_speed = r.max_speed.max(speed);
        if !on_rail && t <= tb {
            r.margin_min = r.margin_min.min(info[3]);
            r.margin_max = r.margin_max.max(info[3]);
            if rail_done_at.is_some_and(|t0: f64| t > t0 + 0.3) {
                r.max_aoa_deg = r.max_aoa_deg.max(info[4].abs());
            }
        }
        if (t * 10.0).round() as i64 != ((t + dt) * 10.0).round() as i64 {
            r.trace.push([t, s.x, s.z, speed, info[2], info[3]]);
        }
        if t < tb && t + dt >= tb {
            r.burnout_alt_agl = ns.z;
            r.burnout_speed = (ns.vx * ns.vx + ns.vz * ns.vz).sqrt();
        }
        s = ns;
        t += dt;
        if on_rail && (s.x * s.x + s.z * s.z).sqrt() >= l.rail_length {
            on_rail = false;
            r.rail_exit_speed = (s.vx * s.vx + s.vz * s.vz).sqrt();
            r.rail_exit_time = t;
            r.margin_at_rail_exit = deriv(t, &s, false).1[3];
            r.margin_min = r.margin_min.min(r.margin_at_rail_exit);
            r.margin_max = r.margin_max.max(r.margin_at_rail_exit);
            rail_done_at = Some(t);
        }
        if on_rail && t > 5.0 {
            // Never left the rail: thrust below weight.
            break;
        }
        if !on_rail && t > tb && s.vz <= 0.0 {
            r.apogee_agl = s.z;
            r.apogee_time = t;
            r.drift_at_apogee = s.x;
            break;
        }
    }
    if r.margin_min == f64::MAX {
        r.margin_min = f64::NAN;
        r.margin_max = f64::NAN;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::vehicle::build;

    /// With drag off and a vertical rail, the integrator must reproduce the
    /// closed-form burnout velocity of a constant-thrust, constant-mdot
    /// rocket under uniform gravity: v_bo = Isp ln(m0/mf) - g t_b (Isp from
    /// the vehicle's own thrust/mdot at the mean ambient is approximated by
    /// its site value; the check uses 2 %).
    #[test]
    fn vacuum_burnout_matches_rocket_equation() {
        let cat = Catalog::load().unwrap();
        let v = build(&crate::reference::eureka2(), &cat).unwrap();
        let l = Launch { elevation: 0.0, rail_length: 5.0, rail_tilt: 0.0, wind_10m: 0.0, thrust_scale: 1.0, drag_scale: 0.0 };
        let r = fly(&v, &l);
        let m0 = v.mass_at(0.0).mass;
        let mf = v.mass_at(v.burn_time).mass;
        // Thrust rises with altitude; bracket with sea-level and vacuum Isp.
        let isp_sl = v.engine.isp(101_325.0);
        let isp_vac = v.engine.isp(0.0);
        let lo = isp_sl * (m0 / mf).ln() - G0 * v.burn_time;
        let hi = isp_vac * (m0 / mf).ln() - G0 * v.burn_time;
        assert!(r.burnout_speed > lo * 0.99 && r.burnout_speed < hi * 1.01, "{} not in [{lo}, {hi}]", r.burnout_speed);
        // Coast with no drag: apogee = h_bo + v^2 / 2g (g varies < 0.5 %).
        let h = r.burnout_alt_agl + r.burnout_speed.powi(2) / (2.0 * G0);
        assert!((r.apogee_agl - h).abs() / h < 0.01, "{} vs {h}", r.apogee_agl);
    }

    #[test]
    fn drag_coefficient_curve() {
        let cat = Catalog::load().unwrap();
        let v = build(&crate::reference::eureka2(), &cat).unwrap();
        for m in [0.3, 0.6, 0.9, 1.0, 1.2, 1.5] {
            let cd = v.aero.cd0(m, m * 340.0, 1.5e-5, false, 0.0);
            println!("M {m}: Cd {cd:.3} (power on {:.3})", v.aero.cd0(m, m * 340.0, 1.5e-5, true, v.engine.exit_area() / v.aero.ref_area()));
        }
    }
}
