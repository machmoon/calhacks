//! Longitudinal static stability: the neutral point.
//!
//! Wing plus horizontal tail (Etkin and Reid, Dynamics of Flight, eq.
//! 2.3,27), downwash gradient 2 a_w / (pi AR) (lifting line, elliptic), tail
//! efficiency `rules::TAIL_ETA`, minus the pod's destabilising shift (Raymer,
//! Aircraft Design, 6th ed., eq. 16.25):
//!
//! ```text
//! Cm_alpha_fus = K_fus W_f^2 L_f / (c S_w)      PER DEGREE (K_fus from fig. 16.14)
//! dx_np / c    = Cm_alpha_fus / CL_alpha         both in the SAME angle unit
//! ```
//!
//! The prototype divided the per-degree moment slope by a per-radian lift
//! slope and got the pod's shift 57x too small. Here a slope is a
//! [`LiftSlope`], made and read only through named units, so the formula
//! states which one it means.

use crate::layout::Layout;
use crate::rules::{K_FUS_PER_DEG, TAIL_ETA};
use wright_core::units::Length;

/// A derivative with respect to angle of attack, stored per radian.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct LiftSlope(f64);

impl LiftSlope {
    pub fn per_rad(v: f64) -> Self {
        Self(v)
    }
    pub fn per_deg(v: f64) -> Self {
        Self(v * 180.0 / std::f64::consts::PI)
    }
    pub fn as_per_rad(self) -> f64 {
        self.0
    }
    pub fn as_per_deg(self) -> f64 {
        self.0 * std::f64::consts::PI / 180.0
    }
}

/// Finite-wing lift slope from the section's: DATCOM / Raymer eq. 12.6 at
/// low Mach, unswept (AeroSandbox `CL_over_Cl`, the same formula).
pub fn wing_slope(aspect_ratio: f64, section: LiftSlope) -> LiftSlope {
    let ar = aspect_ratio;
    LiftSlope::per_rad(ar / (2.0 + (4.0 + ar * ar).sqrt()) * section.as_per_rad())
}

/// The pod's neutral-point shift (m, positive forward): Raymer eq. 16.25.
pub fn pod_shift(width: Length, length: Length, area_m2: f64, mac: Length, cl_alpha: LiftSlope) -> Length {
    // Cm_alpha_fus per degree; dx/c = Cm_alpha / CL_alpha (per degree too)
    let cm_alpha_per_deg = K_FUS_PER_DEG * width.as_m().powi(2) * length.as_m() / (mac.as_m() * area_m2);
    mac * (cm_alpha_per_deg / cl_alpha.as_per_deg())
}

pub fn neutral_point(lay: &Layout) -> Length {
    let wing = lay.parts.wing_airfoil;
    let tail = lay.parts.tail_airfoil;
    // section slopes at a representative cruise Re (20 m/s, sea level)
    let re_w = 20.0 * lay.mac.as_m() / 1.46e-5;
    let re_t = 20.0 * lay.h_chord.as_m().max(0.03) / 1.46e-5;
    let a_w = wing_slope(lay.aspect_ratio, LiftSlope::per_rad(wing.lift_slope_per_rad(re_w)));
    let h_area = lay.h_area.as_m2();
    let ar_h = if h_area > 0.0 { lay.h_span.as_m().powi(2) / h_area } else { 4.0 };
    let a_h = wing_slope(ar_h, LiftSlope::per_rad(tail.lift_slope_per_rad(re_t)));
    let deps = 2.0 * a_w.as_per_rad() / (std::f64::consts::PI * lay.aspect_ratio);
    let k = TAIL_ETA * (h_area / lay.area.as_m2()) * (a_h.as_per_rad() / a_w.as_per_rad()) * (1.0 - deps);
    let x_np = (lay.x_ac_wing.as_m() + k * lay.x_ac_tail.as_m()) / (1.0 + k);
    let a_total = LiftSlope::per_rad(a_w.as_per_rad() * (1.0 + k));
    let len = lay.pod_x1 - lay.pod_x0;
    let wid = lay.pod_width.max(lay.pod_height);
    Length::m(x_np) - pod_shift(wid, len, lay.area.as_m2(), lay.mac, a_total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slope_units_round_trip() {
        let s = LiftSlope::per_rad(5.0);
        assert!((s.as_per_deg() - 5.0 * std::f64::consts::PI / 180.0).abs() < 1e-12);
        assert!((LiftSlope::per_deg(0.1).as_per_rad() - 5.729_577_951).abs() < 1e-6);
    }
    #[test]
    fn pod_shift_is_not_57x_small() {
        // a 0.08 m wide, 0.45 m long pod on a 0.228 m^2, 0.19 m MAC wing,
        // CL_alpha 4.5 /rad: K W^2 L / (c S) = 0.01*0.0064*0.45/(0.19*0.228)
        // = 6.65e-4 /deg; CL_alpha = 0.0785 /deg; dx/c = 0.0085
        let dx = pod_shift(Length::m(0.08), Length::m(0.45), 0.228, Length::m(0.19), LiftSlope::per_rad(4.5));
        assert!((dx.as_m() / 0.19 - 0.00847).abs() < 2e-4, "{}", dx.as_m() / 0.19);
    }
}
