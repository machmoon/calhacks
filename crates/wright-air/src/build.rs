//! Everything a builder needs: CAD (STL per printed piece and an assembly),
//! sliced G-code, the BOM, the ArduPilot parameter file, the harness, the
//! check report, and a first-flight checklist.
//!
//! The checklist answers the crashes builders report most (ArduPilot forum
//! threads 65883, 31535, 75675 via docs/product/brief.md): the first
//! hover-to-forward transition with a reversed surface or a wrong Q_
//! parameter, and hover current nobody predicted. Every number in it comes
//! from the same evaluation the checks judged.

use crate::ardupilot::MOTOR_ORDER;
use crate::checks::{self, Evaluation};
use crate::geometry;
use crate::powertrain::Factors;
use crate::print;
use crate::rules::WING_INTERNAL_FACTOR;
use crate::spec::Design;
use std::fmt::Write as _;
use std::path::Path;
use wright_core::Artifacts;

pub fn checklist(e: &Evaluation) -> String {
    let lay = &e.layout;
    let p = &e.params;
    let h = &e.perf.hover;
    let mut s = String::new();
    let _ = writeln!(s, "# First-flight checklist: {}\n", lay.design.name);
    let _ = writeln!(s, "Work top to bottom; a NO-GO line stops the session.\n");
    let _ = writeln!(s, "## 0. Before printing and ordering\n");
    let _ = writeln!(
        s,
        "- [ ] NO-GO until done: the wing STLs are vase-mode outer shells with NO internal ribs. The mass model counts ribs (x{WING_INTERNAL_FACTOR} on the skin) but the files do not contain them. Add internal ribs before printing: cut narrow slits (0.15-0.2 mm, wider than 2 x PrusaSlicer's 0.049 mm slice_closing_radius so they are not closed) from the lower skin to the spar every 40-60 mm of span, so the single vase wall runs in and out of each slit as a fused double-wall rib; or glue in 3 mm foam ribs at the same pitch. A skin-only panel has no crush or torsion strength beyond the spar."
    );
    {
        let c = crate::catalog::catalog();
        let unconfirmed: Vec<String> = e
            .bom
            .lines
            .iter()
            .filter_map(|l| {
                let id = l.id.strip_suffix("_hub").filter(|_| l.category == "props").unwrap_or(&l.id);
                let row = c.row(&l.category, id)?;
                (row.stock != crate::catalog::Stock::InStock).then(|| format!("{} ({})", row.id, row.stock.label()))
            })
            .collect();
        if unconfirmed.is_empty() {
            let _ = writeln!(s, "- [ ] Every bought part was in stock on its cited page when the catalog was read; re-check before ordering.\n");
        } else {
            let _ = writeln!(s, "- [ ] Confirm stock before ordering (the catalog row does not say in stock): {}.\n", unconfirmed.join(", "));
        }
    }
    if let crate::spec::BatteryChoice::Pack { count, .. } = &lay.design.battery {
        if *count > 1 {
            let _ = writeln!(s, "- [ ] NO-GO until done: before joining the {count} packs in parallel, charge them to the same voltage and measure each: within 0.05 V per cell of each other (0.3 V on 6S), or charge/discharge to match first. Parallel packs at different voltages dump current into each other through the harness.\n");
        }
    }
    if let Some(l) = lay.design.propulsion.lift_current_limit_a {
        let _ = writeln!(s, "- [ ] NO-GO until done: in the AM32 configurator set each of the four lift ESCs' Current Limit to {l:.0} A (eeprom limits.current = {:.0}; AM32 stores it in 2 A steps) and read it back. Hover T/W and every current check assume it; without it the motors can pull more than the pack, connector and motors are rated for.\n", l / 2.0);
    }
    let _ = writeln!(s, "## 1. Before power: CG and balance\n");
    let _ = writeln!(
        s,
        "- [ ] CG at {:.0} mm behind the wing root's leading edge (static margin {:.0} % MAC, neutral point {:.0} mm). Slide the battery to get it; do not fly tail-heavy.",
        lay.x_cg.as_mm(),
        100.0 * lay.static_margin,
        lay.x_np.as_mm()
    );
    let _ = writeln!(s, "- [ ] Take-off mass {:.0} g (+/- 5 %); heavier means more hover current than predicted below.\n", lay.mass().as_g());
    let _ = writeln!(s, "## 2. Control surfaces (props OFF), in FBWA\n");
    let _ = writeln!(s, "| output | function | stick input | expected deflection |\n|---|---|---|---|");
    let _ = writeln!(s, "| SERVO6 | left aileron | roll right | left aileron DOWN |");
    let _ = writeln!(s, "| SERVO7 | right aileron | roll right | right aileron UP (set SERVO7_REVERSED if not) |");
    let _ = writeln!(s, "| SERVO8 | elevator | pull back | elevator UP |");
    let _ = writeln!(s, "| SERVO9/10 | rudders | yaw right | both rudders' trailing edges RIGHT |");
    let _ = writeln!(s, "| (stabilisation) | | tilt the nose DOWN by hand | elevator moves UP to correct |");
    let _ = writeln!(s, "| (stabilisation) | | roll the right wing DOWN by hand | right aileron moves DOWN |\n");
    let _ = writeln!(s, "## 3. Motor order and spin (props OFF), ArduPilot Quad X\n");
    let _ = writeln!(s, "Mission Planner > Motor Test, one at a time:\n\n| output | position | spin (from above) |\n|---|---|---|");
    for (out, tag, spin) in MOTOR_ORDER {
        let pos = match tag {
            "FR" => "front right",
            "RL" => "rear left",
            "FL" => "front left",
            _ => "rear right",
        };
        let _ = writeln!(s, "| SERVO{out} (Motor{out}) | {pos} | {spin} |");
    }
    let _ = writeln!(s, "| SERVO5 | pusher | pushes air AFT (thrust forward) |\n");
    let _ = writeln!(s, "## 4. Parameters that matter for the first transition\n");
    for (k, why) in [
        ("Q_ENABLE", "QuadPlane on"),
        ("Q_FRAME_CLASS", "quad"),
        ("Q_FRAME_TYPE", "X, matching the motor table above"),
        ("Q_M_THST_HOVER", "predicted hover throttle; Q_M_HOVER_LEARN refines it"),
        ("Q_ASSIST_SPEED", "quad motors assist below this airspeed"),
        ("AIRSPEED_MIN", "transition completes above this; 1.25 x predicted stall"),
        ("AIRSPEED_CRUISE", "best-range speed"),
        ("Q_M_BAT_CURR_MAX", "lift current cap: pack and connector burst"),
        ("TRIM_THROTTLE", "pusher throttle at cruise"),
    ] {
        if let Some(v) = p.get(k) {
            let _ = writeln!(s, "- [ ] {k} = {v} ({why})");
        }
    }
    let _ = writeln!(s, "\nPredicted stall {:.1} m/s EAS (CLmax corrected for the airfoil's measured over-prediction; {:.1} m/s TAS at the field); transition airspeed (AIRSPEED_MIN) {:.0} m/s.\n", e.perf.stall_eas.as_mps(), e.perf.stall.as_mps(), p.get("AIRSPEED_MIN").unwrap_or(f64::NAN));
    let _ = writeln!(s, "## 5. Hover current: GO / NO-GO (first QHOVER, 1-2 m, calm)\n");
    match (&h.std_full, &h.hot_low) {
        (Ok(full), Ok(low)) => {
            let _ = writeln!(s, "| quantity | predicted | NO-GO if measured above |\n|---|---|---|");
            let _ = writeln!(s, "| per-motor current, full pack | {:.1} A | {:.1} A (+15 %) |", full.motor_a.as_a(), 1.15 * full.motor_a.as_a());
            let _ = writeln!(s, "| pack current, full pack | {:.1} A | {:.1} A (+15 %) |", h.power.as_w() / e.layout.battery.pack.voltage(wright_core::units::Current::a(0.0), false).value(), 1.15 * h.power.as_w() / e.layout.battery.pack.voltage(wright_core::units::Current::a(0.0), false).value());
            let _ = writeln!(s, "| hover throttle (Q_M_THST_HOVER learnt) | {:.2} | {:.2} |", full.throttle, crate::rules::HOVER_THROTTLE_MAX);
            let _ = writeln!(s, "| per-motor current, hot day, low pack (landing) | {:.1} A | rated {:.0} A |", low.motor_a.as_a(), lay.parts.lift_motor.max_current.as_a());
        }
        _ => {
            let _ = writeln!(s, "NO-GO: the model found no hover operating point.");
        }
    }
    let _ = writeln!(s, "\nMeasure on the bench with props on, the airframe held down, and a clamp meter on one motor's phase or the pack lead.\n");
    let _ = writeln!(s, "## 6. First transition\n");
    let _ = writeln!(s, "- [ ] QLOITER to 20 m, into wind; switch to FBWA; expect the pusher to spool and the quad motors to hand over above {:.0} m/s.", p.get("Q_ASSIST_SPEED").unwrap_or(f64::NAN));
    let _ = writeln!(s, "- [ ] If it pitches up as the pusher spools, abort to QHOVER: the thrust line is {:+.0} mm from the CG's height by design.", (lay.pusher_z - lay.z_cg).as_mm());
    s
}

pub fn build(d: &Design, out: &Path) -> anyhow::Result<Artifacts> {
    std::fs::create_dir_all(out)?;
    let mut a = Artifacts::default();
    let e = checks::evaluate(d, Factors::NOMINAL).map_err(|s| anyhow::anyhow!(s))?;
    let lay = &e.layout;
    // CAD
    let geo = geometry::parts(lay);
    let mut meshes = geometry::meshes(&geo);
    let cad = out.join("cad");
    std::fs::create_dir_all(&cad)?;
    // FreeCAD builds every solid (wright_geom::kernel): watertight STL,
    // AP242 STEP, GLB, OCCT volumes and interference; csg.rs only without it
    let k = wright_geom::kernel::realize_parts(&mut meshes, &cad, &wright_geom::kernel::Options { stem: "aircraft".into(), ..Default::default() });
    a.add(format!("CAD kernel report: {}", k.summary()), cad.join("aircraft-kernel-report.json"));
    for (what, path) in k.files() {
        a.add(format!("aircraft {what}"), path);
    }
    let mut all = vec![];
    for m in &meshes {
        all.extend(m.mesh.triangles());
    }
    wright_geom::Mesh::from_triangles(all).write_stl(&cad.join("assembly.stl"))?;
    a.add("assembly (STL, mm)", cad.join("assembly.stl"));
    // printed pieces, sliced
    let prints = out.join("print");
    match print::slice_all(lay, &prints) {
        Ok(pieces) => {
            let hours: f64 = pieces.iter().map(|p| p.seconds).sum::<f64>() / 3600.0;
            let grams: f64 = pieces.iter().map(|p| p.grams).sum();
            std::fs::write(prints.join("summary.json"), serde_json::to_string_pretty(&serde_json::json!({ "pieces": pieces, "hours": hours, "grams": grams }))?)?;
            a.add(format!("printed pieces: {} STL + G-code, {hours:.1} h, {grams:.0} g", pieces.len()), &prints);
        }
        Err(why) => {
            std::fs::write(prints.join("NOT_SLICED.txt"), why.to_string())?;
            a.add("printed pieces NOT sliced (see NOT_SLICED.txt)", prints.join("NOT_SLICED.txt"));
        }
    }
    std::fs::write(out.join("bom.csv"), e.bom.csv())?;
    a.add(format!("BOM (${:.0})", e.bom.total().as_usd()), out.join("bom.csv"));
    std::fs::write(out.join("component-cad.json"), serde_json::to_string_pretty(&crate::cad_catalog::report(&e.bom)?)?)?;
    a.add("supplier CAD availability by exact BOM part", out.join("component-cad.json"));
    let header = format!("ArduPilot QuadPlane parameters for {}, designed by Wright\nflight controller: {}\ncheck the board's UART map before the SERIALn lines", d.name, d.avionics.fc);
    std::fs::write(out.join("vtol.param"), e.params.text(&header))?;
    a.add("ArduPilot parameters", out.join("vtol.param"));
    let mut h = String::from("run,conductors,awg,length_m,continuous_a,peak_a\n");
    for r in &e.elec.runs {
        let _ = writeln!(h, "{},{},{},{:.2},{:.1},{:.1}", r.name, r.conductors, r.awg, r.length_m, r.continuous_a, r.peak_a);
    }
    std::fs::write(out.join("harness.csv"), h)?;
    a.add("harness", out.join("harness.csv"));
    std::fs::write(out.join("first_flight_checklist.md"), checklist(&e))?;
    a.add("first-flight checklist", out.join("first_flight_checklist.md"));
    std::fs::write(out.join("design.json"), serde_json::to_string_pretty(d)?)?;
    a.add("design (re-runnable)", out.join("design.json"));
    let report = checks::check(d);
    std::fs::write(out.join("checks.txt"), format!("{report}\n"))?;
    std::fs::write(out.join("checks.json"), serde_json::to_string_pretty(&report)?)?;
    a.add(format!("checks: {} of {} pass", report.outcomes.len() - report.failures().count(), report.outcomes.len()), out.join("checks.txt"));
    Ok(a)
}
