//! Track ampacity, IPC-2221 (§6.2, the curve fit of the Fig. 6-4 charts):
//!
//! ```text
//! I = k · ΔT^0.44 · A^0.725
//! ```
//!
//! with `I` in amperes, `ΔT` the temperature rise in °C, `A` the copper
//! cross-section in square mils, and `k = 0.048` for external layers
//! (0.024 internal). Copper is 1.378 mil thick per ounce. The inverse used
//! for widths is `A = (I / (k · ΔT^0.44))^(1/0.725)`, `w = A / t`.
//!
//! Same constants as the Python baseline's `aircraft/electronics.py:386` (k = 0.048, 20 °C),
//! so the numbers compare: 1.0 mm of 1 oz copper carries 3.25 A, and the
//! 5 A of five stalled servos needs 1.82 mm.

use wright_eda::ir::NetRole;
use wright_eda::{NetClass, Netlist, Nm, Rules};

pub const K_EXTERNAL: f64 = 0.048;
pub const MIL_PER_OZ: f64 = 1.378;
pub const NM_PER_MIL: f64 = 25_400.0;
/// Design temperature rise (the baseline and the plan both use 20 °C).
pub const RISE_C: f64 = 20.0;

/// Current a track of `width` carries on an external layer.
pub fn current_a(width: Nm, oz: f64, rise_c: f64) -> f64 {
    let area_mil2 = width.0 as f64 / NM_PER_MIL * MIL_PER_OZ * oz;
    K_EXTERNAL * rise_c.powf(0.44) * area_mil2.powf(0.725)
}

/// Narrowest external track carrying `amps`, rounded up to 0.05 mm.
pub fn width_for(amps: f64, oz: f64, rise_c: f64) -> Nm {
    if amps.is_nan() || amps <= 0.0 {
        return Nm(0);
    }
    let area = (amps / (K_EXTERNAL * rise_c.powf(0.44))).powf(1.0 / 0.725);
    let w_nm = area / (MIL_PER_OZ * oz) * NM_PER_MIL;
    let step = 50_000.0;
    Nm(((w_nm / step).ceil() * step) as i64)
}

/// Net classes from per-net design current (amperes, indexed like
/// `netlist.nets`; missing entries count as 0). One class per distinct
/// width, plus `Default` first at `rules.min_track`. Width is the IPC-2221
/// inverse at a 20 °C rise clamped at `rules.min_track`; ground nets get at
/// least 0.4 mm so their fan-outs are not hairlines. Lane C owns the real
/// `classes_for`; this is the same rule, here so the router can be driven
/// before that lands.
pub fn classes_for_currents(netlist: &Netlist, rules: &Rules, current_a: &[f64]) -> Vec<NetClass> {
    let via = |w: Nm| {
        // A larger via for wide nets: drill ≥ 0.4 mm once the track passes 1 mm.
        if w >= Nm::mm(1.0) {
            (Nm::mm(0.8).max(rules.min_via_diameter), Nm::mm(0.4).max(rules.min_via_drill))
        } else {
            (rules.min_via_diameter, rules.min_via_drill)
        }
    };
    let mut classes = vec![NetClass {
        name: "Default".into(),
        track_width: rules.min_track,
        clearance: rules.min_clearance,
        via_diameter: rules.min_via_diameter,
        via_drill: rules.min_via_drill,
        nets: Vec::new(),
    }];
    for (i, n) in netlist.nets.iter().enumerate() {
        let amps = current_a.get(i).copied().unwrap_or(0.0);
        let mut w = width_for(amps, rules.copper_oz, RISE_C).max(rules.min_track);
        if n.role == NetRole::Ground {
            w = w.max(Nm::mm(0.4));
        }
        if w == rules.min_track {
            classes[0].nets.push(i);
            continue;
        }
        let name = format!("{}_{}mm", if n.role == NetRole::Ground { "Ground" } else { "Power" }, w.kicad());
        if let Some(c) = classes.iter_mut().find(|c| c.name == name) {
            c.nets.push(i);
        } else {
            let (vd, vh) = via(w);
            classes.push(NetClass { name, track_width: w, clearance: rules.min_clearance, via_diameter: vd, via_drill: vh, nets: vec![i] });
        }
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc2221_matches_baseline_numbers() {
        // The baseline's electronics.py:390: 1.0 mm at 20 C carries about 3.2 A.
        let i = current_a(Nm::mm(1.0), 1.0, 20.0);
        assert!((i - 3.25).abs() < 0.02, "{i}");
        // 5 A needs 1.82 mm, rounded up to 1.85 mm.
        let w = width_for(5.0, 1.0, 20.0);
        assert_eq!(w, Nm::mm(1.85));
        assert!(current_a(w, 1.0, 20.0) >= 5.0);
        assert!(current_a(Nm::mm(1.80), 1.0, 20.0) < 5.0);
        assert_eq!(width_for(0.0, 1.0, 20.0), Nm(0));
    }
}
