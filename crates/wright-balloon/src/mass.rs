//! The mass budget, built from the catalog, the enclosure geometry and the
//! slicer (never from a hand total).

use crate::catalog::Catalog;
use crate::model::Design;
use crate::rules;
use anyhow::Result;
use serde::Serialize;
use wright_core::units::Mass;

#[derive(Debug, Clone, Serialize)]
pub struct Masses {
    pub parts_g: f64,
    pub wiring_g: f64,
    pub foam_g: f64,
    /// Slicer grams when sliced, else the allowance (an upper bound).
    pub tray_g: f64,
    pub tray_sliced: bool,
    pub chute_g: f64,
    pub line_g: f64,
    /// Flight-train rigging from the catalog's extras (chute harness), g.
    pub rigging_g: f64,
    pub balloon_g: f64,
}

impl Masses {
    /// The payload package: the box and everything in it.
    pub fn package(&self) -> Mass {
        Mass::g(self.parts_g + self.wiring_g + self.foam_g + self.tray_g)
    }
    /// Everything under the balloon.
    pub fn flight_train(&self) -> Mass {
        self.package() + Mass::g(self.chute_g + self.line_g + self.rigging_g)
    }
    /// Mass under the chute after burst, with the latex remnant still
    /// attached (`rules::BALLOON_REMNANT`).
    pub fn descent(&self) -> Mass {
        self.flight_train() + Mass::g(self.balloon_g * rules::BALLOON_REMNANT)
    }
}

/// Outer size of the enclosure, mm.
pub fn outer_mm(d: &Design) -> [f64; 3] {
    let t = d.enclosure.wall_mm;
    let i = d.enclosure.inner_mm;
    [i[0] + 2.0 * t, i[1] + 2.0 * t, i[2] + 2.0 * t]
}

pub fn masses(d: &Design, cat: &Catalog) -> Result<Masses> {
    let mut parts_g = 0.0;
    for it in &d.items {
        parts_g += cat.part(&it.part)?.mass_g * it.qty as f64;
    }
    for b in &d.buses {
        parts_g += cat.part(&b.battery)?.mass_g * (b.series * b.parallel) as f64;
        for r in &b.rails {
            parts_g += cat.part(r)?.mass_g;
        }
    }
    let ins = cat.insulation(&d.enclosure.insulation)?;
    let o = outer_mm(d);
    let i = d.enclosure.inner_mm;
    let foam_mm3 = o[0] * o[1] * o[2] - i[0] * i[1] * i[2];
    let foam_g = foam_mm3 * 1e-9 * ins.density_kg_m3 * 1e3;
    let (tray_g, tray_sliced) = match (&d.printed, d.enclosure.printed_tray) {
        (_, false) => (0.0, true),
        (Some(p), true) => (p.grams, true),
        (None, true) => (d.printed_allowance_g, false),
    };
    Ok(Masses {
        parts_g,
        wiring_g: parts_g * rules::WIRING_FRACTION,
        foam_g,
        tray_g,
        tray_sliced,
        chute_g: cat.chute(&d.parachute)?.mass_g,
        line_g: cat.line(&d.line)?.g_per_m * d.line_m,
        rigging_g: cat.extras.iter().filter(|e| e.per == crate::catalog::ExtraPer::Flight).map(|e| e.mass_g).sum(),
        balloon_g: cat.balloon(&d.balloon)?.mass_g,
    })
}
