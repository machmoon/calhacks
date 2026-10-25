//! `.kicad_pro` writer: net classes and board design rules, so KiCad's DRC
//! checks the widths and clearances Wright routed to.
//!
//! The JSON shape follows KiCad's own shipped template
//! `SharedSupport/template/RaspberryPi-HAT/RaspberryPi-HAT.kicad_pro`
//! (project `meta.version` 3, `net_settings.meta.version` 4, the full
//! netclass field set, `board.design_settings.rules`), and the Python baseline's
//! `schematic.py emit_kicad_pro` for the minimal top-level keys. Nets go to
//! classes through `netclass_patterns` with the exact net name, as the baseline did
//! (the baseline's 2026-09-14 lesson, kept: a class may be stricter than a library
//! footprint's own pad gaps, which DRC then flags; the USB-C footprint's
//! 0.20 mm gaps fail a 0.25 mm clearance). Rules the `Rules` struct does not
//! carry keep the template's values (`min_hole_clearance` 0, as KiCad's
//! default: the USB4105 footprint's own pads sit 0.19 mm from its pegs).

use serde_json::{json, Value};
use wright_eda::{Board, NetClass, Netlist, Nm};

fn mm(v: Nm) -> f64 {
    v.as_mm()
}

fn class_json(c: &NetClass, priority: i64) -> Value {
    json!({
        "bus_width": 12,
        "clearance": mm(c.clearance),
        "diff_pair_gap": 0.25,
        "diff_pair_via_gap": 0.25,
        "diff_pair_width": 0.2,
        "line_style": 0,
        "microvia_diameter": 0.3,
        "microvia_drill": 0.1,
        "name": c.name,
        "pcb_color": "rgba(0, 0, 0, 0.000)",
        "priority": priority,
        "schematic_color": "rgba(0, 0, 0, 0.000)",
        "track_width": mm(c.track_width),
        "via_diameter": mm(c.via_diameter),
        "via_drill": mm(c.via_drill),
        "wire_width": 6
    })
}

/// The project as JSON.
pub fn project_json(netlist: &Netlist, board: &Board, name: &str) -> anyhow::Result<Value> {
    let r = &board.rules;
    let default = board.classes.first().cloned().unwrap_or(NetClass {
        name: "Default".into(),
        track_width: r.min_track,
        clearance: r.min_clearance,
        via_diameter: r.min_via_diameter,
        via_drill: r.min_via_drill,
        nets: vec![],
    });
    // KiCad requires the Default class to be called "Default" and gives it
    // the lowest priority (i32::MAX), as the template does.
    let mut classes = vec![class_json(&NetClass { name: "Default".into(), ..default }, i32::MAX as i64)];
    let mut patterns = Vec::new();
    for (k, c) in board.classes.iter().enumerate().skip(1) {
        anyhow::ensure!(c.name != "Default", "net class {k} is also named Default");
        classes.push(class_json(c, k as i64 - 1));
        for &n in &c.nets {
            let net = netlist.nets.get(n).ok_or_else(|| anyhow::anyhow!("class {} names net {n}, outside the netlist", c.name))?;
            patterns.push(json!({"netclass": c.name, "pattern": net.name}));
        }
    }
    let annular = (r.min_via_diameter.0 - r.min_via_drill.0) / 2;
    // A library footprint's own holes set the floor: KiCad's ESP32-WROOM-32E
    // and TI PowerPAD "ThermalVias" footprints carry 0.2 mm thermal vias,
    // which a 0.3 mm board minimum reports as `drill_out_of_range`. The
    // footprint is KiCad's, so the board rule follows it (JLCPCB drills
    // 0.2 mm; Wright's own vias still use `Rules.min_via_drill`).
    let min_hole = board.parts.iter().flat_map(|p| p.footprint.pads.iter().filter_map(|x| x.drill)).fold(r.min_via_drill, Nm::min);
    Ok(json!({
        "board": {
            "design_settings": {
                "defaults": {},
                "meta": {"version": 2},
                "rules": {
                    "allow_blind_buried_vias": false,
                    "allow_microvias": false,
                    "max_error": 0.005,
                    "min_clearance": mm(r.min_clearance),
                    "min_connection": 0.0,
                    "min_copper_edge_clearance": mm(r.edge_clearance),
                    "min_hole_clearance": 0.0,
                    "min_hole_to_hole": 0.25,
                    "min_microvia_diameter": 0.2,
                    "min_microvia_drill": 0.1,
                    "min_resolved_spokes": 2,
                    "min_silk_clearance": 0.0,
                    "min_text_height": 0.8,
                    "min_text_thickness": 0.08,
                    "min_through_hole_diameter": mm(min_hole),
                    "min_track_width": mm(r.min_track),
                    "min_via_annular_width": mm(Nm(annular.min(50_000))),
                    "min_via_diameter": mm(r.min_via_diameter),
                    "solder_mask_to_copper_clearance": 0.0,
                    "use_height_for_length_calcs": true
                }
            }
        },
        "boards": [],
        "libraries": {"pinned_footprint_libs": [], "pinned_symbol_libs": []},
        "meta": {"filename": format!("{name}.kicad_pro"), "version": 3},
        "net_settings": {
            "classes": classes,
            "meta": {"version": 4},
            "net_colors": null,
            "netclass_assignments": null,
            "netclass_patterns": patterns
        },
        "pcbnew": {"page_layout_descr_file": ""},
        "schematic": {"legacy_lib_dir": "", "legacy_lib_list": []},
        "sheets": [],
        "text_variables": {}
    }))
}

/// `.kicad_pro` text.
pub fn write_project(netlist: &Netlist, board: &Board, name: &str) -> anyhow::Result<String> {
    let mut s = serde_json::to_string_pretty(&project_json(netlist, board, name)?)?;
    s.push('\n');
    Ok(s)
}
