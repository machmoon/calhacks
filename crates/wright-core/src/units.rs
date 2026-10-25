//! SI quantities as newtypes, so a length can never be passed where a mass
//! is expected and millimetres never leak in as metres.
//!
//! The Python baseline's aircraft package carried bare `f64`s in mixed m/mm and degrees vs
//! radians; one review found a 57x error from a per-degree slope divided by
//! a per-radian one (`layout.py` neutral point). Every value here is stored
//! in its SI base unit and made only through a named constructor
//! (`Length::mm(250.0)`), and read back the same way (`l.as_mm()`).
//! Products across dimensions go through `.si()` at the formula, where the
//! formula's comment states the units.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

macro_rules! quantity {
    ($name:ident, $unit:literal, { $($ctor:ident / $read:ident = $scale:expr),* $(,)? }) => {
        #[doc = concat!("A quantity in ", $unit, " (its SI unit).")]
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(f64);

        impl $name {
            pub const ZERO: Self = Self(0.0);
            /// From the SI value.
            pub const fn si(v: f64) -> Self { Self(v) }
            /// The SI value.
            pub const fn value(self) -> f64 { self.0 }
            $(
                pub fn $ctor(v: f64) -> Self { Self(v * $scale) }
                pub fn $read(self) -> f64 { self.0 / $scale }
            )*
            pub fn abs(self) -> Self { Self(self.0.abs()) }
            pub fn min(self, o: Self) -> Self { Self(self.0.min(o.0)) }
            pub fn max(self, o: Self) -> Self { Self(self.0.max(o.0)) }
            pub fn is_finite(self) -> bool { self.0.is_finite() }
        }
        impl Add for $name { type Output = Self; fn add(self, o: Self) -> Self { Self(self.0 + o.0) } }
        impl AddAssign for $name { fn add_assign(&mut self, o: Self) { self.0 += o.0 } }
        impl Sub for $name { type Output = Self; fn sub(self, o: Self) -> Self { Self(self.0 - o.0) } }
        impl Neg for $name { type Output = Self; fn neg(self) -> Self { Self(-self.0) } }
        impl Mul<f64> for $name { type Output = Self; fn mul(self, k: f64) -> Self { Self(self.0 * k) } }
        impl Mul<$name> for f64 { type Output = $name; fn mul(self, q: $name) -> $name { $name(self * q.0) } }
        impl Div<f64> for $name { type Output = Self; fn div(self, k: f64) -> Self { Self(self.0 / k) } }
        /// A ratio of like quantities is a plain number.
        impl Div for $name { type Output = f64; fn div(self, o: Self) -> f64 { self.0 / o.0 } }
        impl std::iter::Sum for $name {
            fn sum<I: Iterator<Item = Self>>(it: I) -> Self { Self(it.map(|q| q.0).sum()) }
        }
    };
}

quantity!(Length, "m", { m / as_m = 1.0, mm / as_mm = 1e-3, km / as_km = 1e3, inch / as_inch = 0.0254, ft / as_ft = 0.3048 });
quantity!(Area, "m^2", { m2 / as_m2 = 1.0, mm2 / as_mm2 = 1e-6, cm2 / as_cm2 = 1e-4 });
quantity!(Volume, "m^3", { m3 / as_m3 = 1.0, mm3 / as_mm3 = 1e-9, cm3 / as_cm3 = 1e-6, liters / as_liters = 1e-3 });
quantity!(Mass, "kg", { kg / as_kg = 1.0, g / as_g = 1e-3, lb / as_lb = 0.45359237 });
quantity!(Time, "s", { s / as_s = 1.0, minutes / as_minutes = 60.0, h / as_h = 3600.0 });
quantity!(Speed, "m/s", { mps / as_mps = 1.0, kmh / as_kmh = 1.0 / 3.6 });
quantity!(Force, "N", { n / as_n = 1.0, kn / as_kn = 1e3, lbf / as_lbf = 4.4482216152605 });
quantity!(Pressure, "Pa", { pa / as_pa = 1.0, kpa / as_kpa = 1e3, mpa / as_mpa = 1e6, bar / as_bar = 1e5, psi / as_psi = 6894.757293168 });
quantity!(Power, "W", { w / as_w = 1.0, kw / as_kw = 1e3 });
quantity!(Energy, "J", { j / as_j = 1.0, wh / as_wh = 3600.0, kj / as_kj = 1e3 });
quantity!(Current, "A", { a / as_a = 1.0, ma / as_ma = 1e-3 });
quantity!(Voltage, "V", { v / as_v = 1.0 });
quantity!(Charge, "C", { c / as_c = 1.0, mah / as_mah = 3.6, ah / as_ah = 3600.0 });
quantity!(Temperature, "K", { k / as_k = 1.0 });
quantity!(Angle, "rad", { rad / as_rad = 1.0, deg / as_deg = std::f64::consts::PI / 180.0 });
quantity!(Density, "kg/m^3", { kg_m3 / as_kg_m3 = 1.0, g_cm3 / as_g_cm3 = 1e3 });
quantity!(Money, "USD", { usd / as_usd = 1.0 });

impl Temperature {
    pub fn celsius(c: f64) -> Self { Self(c + 273.15) }
    pub fn as_celsius(self) -> f64 { self.0 - 273.15 }
}

/// Standard gravity, m/s^2 (CGPM 1901; ISO 80000-3).
pub const G0: f64 = 9.80665;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_round_trip_through_si() {
        assert!((Length::mm(250.0).as_m() - 0.25).abs() < 1e-12);
        assert!((Angle::deg(180.0).as_rad() - std::f64::consts::PI).abs() < 1e-12);
        assert!((Charge::mah(1000.0).as_ah() - 1.0).abs() < 1e-12);
        assert!((Pressure::psi(14.6959).as_kpa() - 101.325).abs() < 1e-3);
        assert!((Temperature::celsius(15.0).as_k() - 288.15).abs() < 1e-12);
    }

    #[test]
    fn like_quantities_divide_to_a_number() {
        assert_eq!(Length::m(3.0) / Length::m(1.5), 2.0);
    }
}
