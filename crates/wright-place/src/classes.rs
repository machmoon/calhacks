//! Net classes from net roles and per-net current: `Board.classes`, which
//! lane D routes at and lane A writes to `.kicad_pro` `net_settings`.
//!
//! Width is the IPC-2221 external-layer curve inverted (IPC-2221A 6.2,
//! I = k dT^0.44 A^0.725, A in mil^2, k = 0.048), at a 20 C rise, the same
//! constants the Python baseline uses (`aircraft/electronics.py:386 trace_capacity_a`),
//! clamped below at `Rules.min_track`. Class shape follows KiCad's
//! `.kicad_pro` `net_settings.classes` (name, track_width, clearance,
//! via_diameter, via_drill), with `Default` first as KiCad requires.
//!
//! Without a stated current: signals take `Rules.min_track`; power and
//! ground take the width for 1 A, floored at twice `min_track` (0.4 mm on
//! JLCPCB rules), so a rail is never drawn at signal width by omission,
//! which is the baseline's one-width bug (`routing.py:79`).

use std::collections::BTreeMap;
use wright_eda::ir::NetRole;
use wright_eda::{NetClass, Netlist, Nm, Rules};

pub const IPC_K_EXTERNAL: f64 = 0.048;
pub const COPPER_1OZ_MIL: f64 = 1.378;
pub const RISE_C: f64 = 20.0;
/// Current assumed for a rail with no stated current, A.
pub const DEFAULT_RAIL_A: f64 = 1.0;

/// Current an external track of `width` carries at `rise_c` (the baseline's forward
/// formula, kept for the round-trip test and for lane E's ampacity check).
pub fn capacity_a(width: Nm, rise_c: f64, oz: f64) -> f64 {
    let area_mil2 = width.as_mm() / 0.0254 * COPPER_1OZ_MIL * oz;
    IPC_K_EXTERNAL * rise_c.powf(0.44) * area_mil2.powf(0.725)
}

/// Narrowest external track (rounded up to 0.01 mm) that carries `amps`.
pub fn width_for(amps: f64, rise_c: f64, oz: f64) -> Nm {
    if amps <= 0.0 {
        return Nm(0);
    }
    let area_mil2 = (amps / (IPC_K_EXTERNAL * rise_c.powf(0.44))).powf(1.0 / 0.725);
    let mm = area_mil2 / (COPPER_1OZ_MIL * oz) * 0.0254;
    Nm(((mm * 100.0).ceil() as i64) * 10_000)
}

/// Classes for every net. `currents` maps a net name to its continuous
/// current in amps; unknown names are ignored. Nets that need the same
/// width share a class; the class name says the role and the width.
pub fn classes_for(netlist: &Netlist, currents: &BTreeMap<String, f64>, rules: &Rules) -> Vec<NetClass> {
    let oz = if rules.copper_oz > 0.0 { rules.copper_oz } else { 1.0 };
    let via = |w: Nm| {
        // A via at least as wide as the track it carries, on the fab's
        // minimum drill/ring; the drill keeps the fab's ring ratio.
        let d = rules.min_via_diameter.max(w);
        let drill = Nm(rules.min_via_drill.0.max(d.0 - (rules.min_via_diameter.0 - rules.min_via_drill.0)));
        (d, drill)
    };
    let (vd, vh) = via(rules.min_track);
    let mut classes = vec![NetClass { name: "Default".into(), track_width: rules.min_track, clearance: rules.min_clearance, via_diameter: vd, via_drill: vh, nets: Vec::new() }];
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for (k, net) in netlist.nets.iter().enumerate() {
        let stated = currents.get(&net.name).copied();
        let need = match (net.role, stated) {
            (_, Some(a)) => width_for(a, RISE_C, oz),
            (NetRole::Signal, None) => Nm(0),
            (_, None) => width_for(DEFAULT_RAIL_A, RISE_C, oz).max(Nm(2 * rules.min_track.0)),
        };
        let w = need.max(rules.min_track);
        if w == rules.min_track && net.role == NetRole::Signal {
            classes[0].nets.push(k);
            continue;
        }
        let role = match net.role {
            NetRole::Ground => "Ground",
            NetRole::Power => "Power",
            NetRole::Signal => "Signal",
        };
        let name = format!("{role}_{}mm", w.kicad());
        let idx = *by_name.entry(name.clone()).or_insert_with(|| {
            let (d, h) = via(w.min(Nm::mm(1.0)));
            classes.push(NetClass { name, track_width: w, clearance: rules.min_clearance, via_diameter: d, via_drill: h, nets: Vec::new() });
            classes.len() - 1
        });
        classes[idx].nets.push(k);
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_inverse_round_trips() {
        for a in [0.5, 1.0, 3.2, 5.0, 10.0] {
            let w = width_for(a, 20.0, 1.0);
            assert!(capacity_a(w, 20.0, 1.0) >= a, "{a} A at {} mm", w.kicad());
            assert!(capacity_a(Nm(w.0 - 10_000), 20.0, 1.0) < a, "{a} A not minimal");
        }
        // The baseline's electronics.py: 1.0 mm carries about 3.2 A; 5 A needs more.
        assert!((capacity_a(Nm::mm(1.0), 20.0, 1.0) - 3.2).abs() < 0.1);
        assert!(width_for(5.0, 20.0, 1.0) > Nm::mm(1.5));
    }
}
