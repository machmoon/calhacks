//! Lengths in integer nanometres and the board-frame point and rectangle.
//! Follows the Python baseline's `engine/silkscreen/units.py` (integer nm everywhere) and
//! KiCad's own `int` internal units (`libs/kimath/include/geometry/eda_angle.h`
//! and `base_units.h`: 1 IU = 1 nm in pcbnew).

use serde::{Deserialize, Serialize};
use std::ops::{Add, Neg, Sub};

pub const NM_PER_MM: i64 = 1_000_000;
pub const NM_PER_MIL: i64 = 25_400;

/// A length in nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Nm(pub i64);

impl Nm {
    /// From millimetres, rounded to the nearest nanometre.
    pub fn mm(v: f64) -> Self {
        Nm((v * NM_PER_MM as f64).round() as i64)
    }
    pub fn mil(v: f64) -> Self {
        Nm((v * NM_PER_MIL as f64).round() as i64)
    }
    pub fn as_mm(self) -> f64 {
        self.0 as f64 / NM_PER_MM as f64
    }
    /// KiCad file text: millimetres, shortest exact decimal (at most 6 places).
    pub fn kicad(self) -> String {
        let neg = self.0 < 0;
        let a = self.0.unsigned_abs();
        let (int, frac) = (a / 1_000_000, a % 1_000_000);
        let mut s = if frac == 0 { int.to_string() } else { format!("{int}.{frac:06}").trim_end_matches('0').to_string() };
        if neg && a != 0 {
            s.insert(0, '-');
        }
        s
    }
}

impl Add for Nm {
    type Output = Nm;
    fn add(self, o: Nm) -> Nm {
        Nm(self.0 + o.0)
    }
}
impl Sub for Nm {
    type Output = Nm;
    fn sub(self, o: Nm) -> Nm {
        Nm(self.0 - o.0)
    }
}
impl Neg for Nm {
    type Output = Nm;
    fn neg(self) -> Nm {
        Nm(-self.0)
    }
}

/// A point in KiCad's board frame (Y down), nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Pt {
    pub x: Nm,
    pub y: Nm,
}

impl Pt {
    pub const fn new(x: i64, y: i64) -> Self {
        Pt { x: Nm(x), y: Nm(y) }
    }
    pub fn mm(x: f64, y: f64) -> Self {
        Pt { x: Nm::mm(x), y: Nm::mm(y) }
    }
    /// Rotate about the origin by a multiple of 90 degrees, KiCad's
    /// convention (positive = counter-clockwise on screen, Y down), exact.
    /// KiCad `RotatePoint` (libs/kimath/src/trigo.cpp) for the 90-degree
    /// cases: (x, y) -> (y, -x) per +90.
    pub fn rot90(self, quarter_turns: i32) -> Self {
        let (x, y) = (self.x.0, self.y.0);
        match quarter_turns.rem_euclid(4) {
            0 => Pt::new(x, y),
            1 => Pt::new(y, -x),
            2 => Pt::new(-x, -y),
            _ => Pt::new(-y, x),
        }
    }
    pub fn dist(self, o: Pt) -> f64 {
        let dx = (self.x.0 - o.x.0) as f64;
        let dy = (self.y.0 - o.y.0) as f64;
        dx.hypot(dy)
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt { x: self.x + o.x, y: self.y + o.y }
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt { x: self.x - o.x, y: self.y - o.y }
    }
}

/// Axis-aligned rectangle, `min` inclusive corner to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub min: Pt,
    pub max: Pt,
}

impl Rect {
    pub fn from_corners(a: Pt, b: Pt) -> Self {
        Rect {
            min: Pt { x: a.x.min(b.x), y: a.y.min(b.y) },
            max: Pt { x: a.x.max(b.x), y: a.y.max(b.y) },
        }
    }
    pub fn width(&self) -> Nm {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> Nm {
        self.max.y - self.min.y
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect::from_corners(
            Pt { x: self.min.x.min(o.min.x), y: self.min.y.min(o.min.y) },
            Pt { x: self.max.x.max(o.max.x), y: self.max.y.max(o.max.y) },
        )
    }
    /// Interiors overlap (touching edges do not count).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }
    /// The rectangle this one becomes when rotated about the origin.
    pub fn rot90(&self, q: i32) -> Rect {
        Rect::from_corners(self.min.rot90(q), self.max.rot90(q))
    }
    pub fn translate(&self, d: Pt) -> Rect {
        Rect { min: self.min + d, max: self.max + d }
    }
    /// Grown by `m` on every side.
    pub fn inflate(&self, m: Nm) -> Rect {
        Rect { min: Pt { x: self.min.x - m, y: self.min.y - m }, max: Pt { x: self.max.x + m, y: self.max.y + m } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kicad_text_is_exact_and_short() {
        assert_eq!(Nm::mm(1.27).kicad(), "1.27");
        assert_eq!(Nm(-500_000).kicad(), "-0.5");
        assert_eq!(Nm(0).kicad(), "0");
        assert_eq!(Nm(1).kicad(), "0.000001");
        assert_eq!(Nm::mm(-12.0).kicad(), "-12");
    }

    #[test]
    fn rotation_is_kicad_ccw_in_y_down() {
        // KiCad: a pad at +x rotated by 90 deg ends up at -y (up on screen).
        assert_eq!(Pt::new(1, 0).rot90(1), Pt::new(0, -1));
        assert_eq!(Pt::new(3, 4).rot90(4), Pt::new(3, 4));
        assert_eq!(Pt::new(3, 4).rot90(-1), Pt::new(3, 4).rot90(3));
    }
}
