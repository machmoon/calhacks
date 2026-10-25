//! Forward-flight aerodynamics: a component drag buildup, trimmed.
//!
//! AeroSandbox's AeroBuildup (MIT, `aerosandbox/aerodynamics/aero_3D/
//! aero_buildup.py`), adapted to this one geometry, as the prototype did:
//!
//! * wing in spanwise strips, each reading its section polar at its own
//!   Reynolds number and angle, with AeroBuildup's finite-wing corrections
//!   (`CL_over_Cl` = AR / (2 + sqrt(4 + AR^2)); effective angle
//!   alpha - (1 - f^0.8) sin(2 alpha) / 2; section lift x f^0.2); induced drag
//!   CL^2 / (pi AR e) with the *inviscid* span efficiency from Nita and
//!   Scholz (2012), e_theo x k_e,F. AeroBuildup (`inviscid.oswalds_efficiency`)
//!   multiplies in k_e,D0 = 0.836 too, but Nita and Scholz define that factor
//!   as the viscous lift-dependent (profile) drag folded into a total Oswald
//!   factor (their sect. 4, from Hoerner's K), and the strips here already read
//!   that drag from the section polar at their lifted CL. Keeping both counted
//!   it twice: on the eval design at 18 m/s, 2.48 N of induced drag against
//!   AVL's 1.87 N (wright-cfd, Trefftz plane); without k_e,D0 it is 2.07 N.
//!   This is a stated deviation from the AeroBuildup source;
//! * pod: Goetten et al.'s form factor with its turbulent friction
//!   (3.46 log10 Re - 5.6)^-2 (`fuselage_form_factor`);
//! * booms: flat-plate friction (`Cf_flat_plate`, hybrid-sharpe-convex) x a
//!   slender-body form factor (Hoerner ch. 6);
//! * what a quadplane adds and AeroBuildup has no component for: motor bells
//!   in crossflow, stopped (or folded) lift props, faired legs, excrescence
//!   (`rules`).
//!
//! Trim: the tail carries what balances the wing's lift and moment about the
//! CG (thrust line through the CG); its profile and induced drag counted at
//! that lift.

use crate::atmosphere::Air;
use crate::layout::Layout;
use crate::powertrain::Factors;
use crate::rules::*;
use wright_core::units::{Force, Length, Speed};

const STRIPS: usize = 10;

fn softmax(a: f64, b: f64, h: f64) -> f64 {
    let m = a.max(b);
    m + ((h * (a - m)).exp() + (h * (b - m)).exp()).ln() / h
}

/// Mean flat-plate skin friction (AeroSandbox `Cf_flat_plate`, default).
pub fn cf_flat_plate(re: f64) -> f64 {
    let re = re.abs().max(1e3);
    softmax(1.328 / re.sqrt(), 0.02666 * re.powf(-0.139), 1e3)
}

pub fn cl_over_cl(ar: f64) -> f64 {
    ar / (2.0 + (4.0 + ar * ar).sqrt())
}

/// Nita and Scholz (2012) sect. 5, unswept: the inviscid span efficiency
/// e_theo x k_e,F (planform and fuselage). This is what multiplies CL^2 /
/// (pi AR) when the profile drag is read from polars at the lifted CL.
pub fn oswald_inviscid(taper: f64, ar: f64, d_over_b: f64) -> f64 {
    let lam = taper - (-0.357 + 0.45);
    let f = 0.0524 * lam.powi(4) - 0.15 * lam.powi(3) + 0.1659 * lam * lam - 0.0706 * lam + 0.0119;
    let e_theo = 1.0 / (1.0 + f * ar);
    let k_ef = 1.0 - 2.0 * d_over_b * d_over_b;
    e_theo * k_ef
}

/// Nita and Scholz's total Oswald factor, k_e,D0 included (the AeroSandbox
/// `oswalds_efficiency` value). For a drag polar CD0 + CL^2 / (pi AR e) with
/// no lift-dependent profile term; not used by `trim`, which has one.
pub fn oswald(taper: f64, ar: f64, d_over_b: f64) -> f64 {
    let k_ed0 = (0.873 + 0.864 + 0.804 + 0.804) / 4.0;
    oswald_inviscid(taper, ar, d_over_b) * k_ed0
}

/// Goetten et al. (2021) via AeroSandbox; corner radius ~1/3 of the width.
pub fn fuselage_form_factor(fineness: f64) -> f64 {
    let r: f64 = 2.0 * 0.35;
    let cs1 = -0.825885 * r.powf(0.411795) + 4.0001;
    let cs2 = -0.340977 * r.powf(7.54327) - 2.27920;
    let cs3 = -0.013846 * r.powf(1.34253) + 1.11029;
    cs1 * fineness.powf(cs2) + cs3
}

/// Drag areas D/q, m^2, by component.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct DragAreas {
    pub wing_profile: f64,
    pub wing_induced: f64,
    pub tail: f64,
    pub pod: f64,
    pub booms: f64,
    pub motors: f64,
    pub legs: f64,
    pub stopped_props: f64,
    pub excrescence: f64,
}

impl DragAreas {
    pub fn total(&self) -> f64 {
        self.wing_profile + self.wing_induced + self.tail + self.pod + self.booms + self.motors + self.legs + self.stopped_props + self.excrescence
    }
}

/// (CL, CDp, Cm_ac) of the wing at body angle `alpha_deg`, strip by strip.
pub struct WingModel<'a> {
    lay: &'a Layout,
    strips: Vec<(f64, f64, f64, f64)>, // (eta, chord, area both halves, Re)
    f: f64,
}

impl<'a> WingModel<'a> {
    pub fn new(lay: &'a Layout, v: Speed, air: &Air) -> Self {
        let half = lay.semi_span().as_m();
        let dy = half / STRIPS as f64;
        let strips = (0..STRIPS)
            .map(|k| {
                let y = (k as f64 + 0.5) * dy;
                let c = lay.chord_at(Length::m(y));
                (y / half, c.as_m(), 2.0 * c.as_m() * dy, air.reynolds(v, c))
            })
            .collect();
        WingModel { lay, strips, f: cl_over_cl(lay.aspect_ratio) }
    }
    pub fn coefficients(&self, alpha_deg: f64) -> (f64, f64, f64) {
        let w = &self.lay.design.wing;
        let af = self.lay.parts.wing_airfoil;
        let (mut cl, mut cd, mut cm) = (0.0, 0.0, 0.0);
        let f2 = self.f.powf(0.2);
        let f8 = 1.0 - self.f.powf(0.8);
        for &(eta, c, a, re) in &self.strips {
            let geo = alpha_deg + w.incidence_deg - w.washout_deg * eta;
            let eff = geo - f8 * (2.0 * geo).to_radians().sin() / 2.0 * 180.0 / std::f64::consts::PI;
            cl += af.cl(eff, re) * f2 * a;
            cd += af.cd(eff, re) * a;
            cm += af.cm(eff, re) * a * c;
        }
        let s = self.lay.area.as_m2();
        (cl / s, cd / s, cm / (s * self.lay.mac.as_m()))
    }
    /// The wing's CLmax over body angles -4..18 deg (washout delays the tips).
    pub fn cl_max(&self) -> f64 {
        (0..=44).map(|k| self.coefficients(-4.0 + 0.5 * k as f64).0).fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Everything but the lifting surfaces' lift-dependent drag, m^2.
pub fn parasite(lay: &Layout, v: Speed, air: &Air) -> DragAreas {
    let p = &lay.parts;
    let mut d = DragAreas::default();
    let len = (lay.pod_x1 - lay.pod_x0).max(Length::m(0.1));
    let diam = (lay.pod_width.as_m() * lay.pod_height.as_m()).sqrt();
    let re = air.reynolds(v, len);
    let cf = (3.46 * re.log10() - 5.6).powi(-2);
    let wet = std::f64::consts::PI * (lay.pod_width + lay.pod_height).as_m() / 2.0 * len.as_m() * 0.85;
    d.pod = cf * fuselage_form_factor(len.as_m() / diam) * wet;
    let bl = lay.boom_x1 - lay.boom_x0;
    let od = p.boom.od.as_m();
    let dl = od / bl.as_m();
    let ff = 1.0 + 1.5 * dl.powf(1.5) + 7.0 * dl.powi(3);
    d.booms = 2.0 * cf_flat_plate(air.reynolds(v, bl)) * ff * std::f64::consts::PI * od * bl.as_m();
    let frontal = p.lift_motor.body_diameter.as_m() * p.lift_motor.bell_length().as_m();
    d.motors = 4.0 * MOTOR_BELL_CD * (frontal + od * 0.015);
    d.legs = 4.0 * FAIRED_STRUT_CD * lay.leg_thickness.as_m() * lay.leg_height.as_m();
    let cd_blade = if p.lift_prop.folding { FOLDED_BLADE_CD } else { STOPPED_BLADE_CD };
    let solidity = PROP_SOLIDITY_2BLADE * p.lift_prop.blades as f64 / 2.0;
    d.stopped_props = 4.0 * cd_blade * solidity * p.lift_prop.data.disc_area_m2();
    let ta = p.tail_airfoil;
    let re_h = air.reynolds(v, lay.h_chord.max(Length::m(0.02)));
    let re_v = air.reynolds(v, lay.v_chord.max(Length::m(0.02)));
    d.tail = ta.cd(0.0, re_h) * lay.h_area.as_m2() + ta.cd(0.0, re_v) * 2.0 * lay.v_area_each.as_m2();
    d
}

#[derive(Debug, Clone)]
pub struct Trim {
    pub speed: Speed,
    pub alpha_deg: f64,
    pub cl_wing: f64,
    pub cl_tail: f64,
    pub drag: Force,
    pub areas: DragAreas,
    pub stalled: bool,
    pub weight: Force,
}

impl Trim {
    pub fn lift_to_drag(&self) -> f64 {
        self.weight / self.drag
    }
}

/// Level flight at `v` and load factor `n`.
pub fn trim(lay: &Layout, v: Speed, air: &Air, n: f64, f: Factors) -> Trim {
    let weight = lay.weight_n() * n;
    let q = air.q(v).value();
    let wm = WingModel::new(lay, v, air);
    let (x_cg, x_w, x_h) = (lay.x_cg.as_m(), lay.x_ac_wing.as_m(), lay.x_ac_tail.as_m());
    let s = lay.area.as_m2();
    let mac = lay.mac.as_m();
    let total = |alpha: f64| {
        let (cl, cd, cm) = wm.coefficients(alpha);
        let lw = q * s * cl;
        let m0 = q * s * mac * cm;
        let lh = (lw * (x_cg - x_w) + m0) / (x_h - x_cg);
        (cl, cd, lw + lh, lh)
    };
    let (mut lo, mut hi) = (-6.0, 16.0);
    let mut stalled = false;
    let alpha = if total(hi).2 < weight {
        stalled = true;
        hi
    } else {
        for _ in 0..28 {
            let mid = 0.5 * (lo + hi);
            if total(mid).2 < weight {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let (cl, cdp, _, lh) = total(alpha);
    let e = oswald_inviscid(lay.taper, lay.aspect_ratio, lay.pod_width / lay.span);
    let cdi = cl * cl / (std::f64::consts::PI * lay.aspect_ratio * e);
    let h_area = lay.h_area.as_m2();
    let cl_h = if h_area > 0.0 { lh / (q * h_area) } else { 0.0 };
    let ar_h = if h_area > 0.0 { lay.h_span.as_m().powi(2) / h_area } else { 4.0 };
    let ta = lay.parts.tail_airfoil;
    let re_h = air.reynolds(v, lay.h_chord.max(Length::m(0.02)));
    let a_h = ta.alpha_for_cl(cl_h.abs(), re_h).unwrap_or(ta.cl_max(re_h).1);
    let cd_h_extra = (ta.cd(a_h, re_h) - ta.cd(0.0, re_h)) * h_area;
    let cdi_h = cl_h * cl_h / (std::f64::consts::PI * ar_h * TAIL_SPAN_EFFICIENCY) * h_area;
    let mut d = parasite(lay, v, air);
    // the pessimistic factor scales every zero-lift component
    let k = f.parasite;
    d.pod *= k;
    d.booms *= k;
    d.motors *= k;
    d.legs *= k;
    d.stopped_props *= k;
    d.tail = d.tail * k + cd_h_extra + cdi_h;
    d.wing_profile = cdp * s * k;
    d.wing_induced = cdi * s;
    d.excrescence = (EXCRESCENCE_FACTOR - 1.0) * (d.pod + d.booms + d.motors + d.legs + d.stopped_props + d.tail + d.wing_profile);
    Trim {
        speed: v,
        alpha_deg: alpha,
        cl_wing: cl,
        cl_tail: cl_h,
        drag: Force::n(q * d.total()),
        areas: d,
        stalled,
        weight: Force::n(weight),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn friction_and_oswald_match_aerosandbox_ranges() {
        // Blasius at Re 1e4: 1.328/100 = 0.01328
        assert!((cf_flat_plate(1e4) - 0.01328).abs() < 2e-4);
        let e = oswald(0.7, 6.3, 0.07);
        assert!(e > 0.75 && e < 0.9, "{e}");
        // the inviscid part alone is the total over k_e,D0 = 0.836
        let ei = oswald_inviscid(0.7, 6.3, 0.07);
        assert!((ei * 0.83625 - e).abs() < 1e-9 && ei < 1.0, "{ei}");
        assert!(fuselage_form_factor(5.0) > 1.1 && fuselage_form_factor(5.0) < 1.6);
    }
}
