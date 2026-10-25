//! The vehicle as placed parts (wright-geom meshes, mm, z up from the
//! nozzle exit plane), the all-pairs fit check, and `build()` artifacts.
//! Physics (`vehicle`) and CAD read the same `Vehicle`, so the two cannot
//! drift: every dimension here comes from `Vehicle` or the catalog row.

use crate::aero::von_karman_radius;
use crate::catalog::Catalog;
use crate::domain::RocketDesign;
use crate::vehicle::Vehicle;
use wright_core::Artifacts;
use wright_geom::{interference, shapes, v3, Mesh, Part, PartKind};

const N: usize = 40;

fn z(v: &Vehicle, x: f64) -> f64 {
    (v.length - x) * 1000.0
}

fn part(name: &str, kind: PartKind, material: &str, mesh: Mesh, touches: &[&str]) -> Part {
    Part { name: name.into(), kind, material: material.into(), mesh, touches: touches.iter().map(|s| s.to_string()).collect() }
}

/// Every part of the vehicle, placed.
pub fn parts(v: &Vehicle, cat: &Catalog) -> anyhow::Result<Vec<Part>> {
    let p = &v.params;
    let r_out = v.diameter / 2.0 * 1000.0;
    let af = cat.get(&p.airframe_tube).ok_or_else(|| anyhow::anyhow!("airframe tube missing"))?;
    let scale = v.diameter / (af.f("od_mm").unwrap() * 1e-3);
    let r_in = af.f("id_mm").unwrap() / 2.0 * scale;
    let tube = cat.get(&p.tank_tube).ok_or_else(|| anyhow::anyhow!("tank tube missing"))?;
    let t_wall = tube.f("wall_mm").unwrap();
    // The aluminium tank tube at its own OD (6.0 in = the G12 bore: the tank
    // ends slip into the fibreglass bays), not the airframe OD.
    let r_tank = tube.f("od_mm").unwrap() / 2.0 * scale;
    let mut out = Vec::new();
    // Nose: von Karman solid of revolution, base at the recovery bay.
    let ln = v.nose_length;
    let prof: Vec<(f64, f64)> = (0..=24).map(|i| {
        let x = ln * i as f64 / 24.0;
        (von_karman_radius(x, ln, v.diameter / 2.0) * 1000.0, z(v, x))
    }).collect();
    out.push(part("nose", PartKind::Bought, &p.nose, shapes::revolve(&prof, N), &["recovery bay"]));
    let sec = |n: &str| v.sections.iter().find(|s| s.0 == n).map(|s| (s.1, s.2)).unwrap();
    // The oxidiser tank is named for what it holds (the section key stays).
    let (ox_tank, ox_bh) = if v.profile.is_some() { ("N2O tank", "N2O bulkheads") } else { ("LOX tank", "LOX bulkheads") };
    let bays = [("recovery", "recovery bay"), ("pressurant", "pressurant bay"), ("engine_bay", "engine bay")];
    for (key, name) in bays {
        let (a, b) = sec(key);
        out.push(part(name, PartKind::Bought, &p.airframe_tube, shapes::tube(v3(0.0, 0.0, z(v, b)), v3(0.0, 0.0, z(v, a)), r_out, r_in, N), &["nose", "recovery bay", "pressurant bay", ox_tank, "fuel tank", "engine bay", "intertank"]));
    }
    for (key, name) in [("lox_tank", ox_tank), ("intertank", "intertank"), ("fuel_tank", "fuel tank")] {
        let (a, b) = sec(key);
        out.push(part(name, PartKind::Bought, &p.tank_tube, shapes::tube(v3(0.0, 0.0, z(v, b)), v3(0.0, 0.0, z(v, a)), r_tank, r_tank - t_wall * scale, N), &["pressurant bay", "intertank", ox_tank, "fuel tank", "engine bay", ox_bh, "fuel bulkheads"]));
    }
    for (t, name) in [(&v.ox_tank, ox_bh), (&v.fuel_tank, "fuel bulkheads")] {
        let ri = t.inner_d / 2.0 * 1000.0;
        let x0 = t.x_fwd;
        let x1 = t.x_fwd + 2.0 * t.bulkhead_t + t.length;
        let fwd = shapes::cylinder(v3(0.0, 0.0, z(v, x0 + t.bulkhead_t)), v3(0.0, 0.0, z(v, x0)), ri - 0.05, N);
        let aft = shapes::cylinder(v3(0.0, 0.0, z(v, x1)), v3(0.0, 0.0, z(v, x1 - t.bulkhead_t)), ri - 0.05, N);
        out.push(part(name, PartKind::Printed, "6061-T6 plate", fwd.union(&aft), &[if name == ox_bh { ox_tank } else { "fuel tank" }]));
    }
    // GN2 bottles stacked in the pressurant bay.
    let bottle = cat.get(&p.bottle).ok_or_else(|| anyhow::anyhow!("bottle missing"))?;
    let (bd, bl) = match (bottle.f("diameter_mm"), bottle.f("length_mm")) {
        (Some(d), Some(l)) => (d, l),
        _ => anyhow::bail!("bottle {} has no published diameter/length", bottle.id()),
    };
    let (a, _) = sec("pressurant");
    for i in 0..p.bottles {
        let x0 = a + 0.05 + i as f64 * (bl * 1e-3 + 0.02);
        out.push(part(&format!("GN2 bottle {}", i + 1), PartKind::Bought, &p.bottle, shapes::cylinder(v3(0.0, 0.0, z(v, x0 + bl * 1e-3)), v3(0.0, 0.0, z(v, x0)), bd / 2.0, N), &[]));
    }
    // Engine: outer envelope (case over chamber, cone to the throat, nozzle).
    let e = &v.engine;
    let x_exit = v.length - v.params.extra_length;
    let x_head = x_exit - e.length();
    let ro = e.outer_d() / 2.0 * 1000.0;
    let rt = (e.throat_d / 2.0 + e.throat_wall.iter().map(|l| l.thickness).sum::<f64>()) * 1000.0;
    let re = (e.exit_d / 2.0 + 0.006) * 1000.0;
    let xc = x_head + 0.04 + e.chamber_cyl_len;
    let xt = xc + e.converging_len;
    let eprof = vec![(0.0, z(v, x_head)), (ro, z(v, x_head)), (ro, z(v, xc)), (rt.max(1.0), z(v, xt)), (re, z(v, x_exit)), (0.0, z(v, x_exit))];
    out.push(part("engine", PartKind::Printed, "6061-T6 + phenolic", shapes::revolve(&eprof, N), &[]));
    // Fins on the engine bay, flush with the aft end.
    let fin = cat.get(&p.fin_material).ok_or_else(|| anyhow::anyhow!("fin material missing"))?;
    let th = fin.f("thickness_mm").unwrap();
    for k in 0..p.fin_count {
        let le = v.length - p.fin_root;
        let (zr0, zr1) = (z(v, le), z(v, v.length));
        let (zt0, zt1) = (z(v, le + p.fin_sweep), z(v, le + p.fin_sweep + p.fin_tip));
        let s = p.fin_span * 1000.0;
        let ring = |y: f64| vec![v3(r_out - 0.5, y, zr1), v3(r_out - 0.5, y, zr0), v3(r_out + s, y, zt0), v3(r_out + s, y, zt1)];
        let m = shapes::loft(&[ring(-th / 2.0), ring(th / 2.0)]);
        let m = m.rotate(v3(0.0, 0.0, 1.0), 2.0 * std::f64::consts::PI * k as f64 / p.fin_count as f64);
        out.push(part(&format!("fin {}", k + 1), PartKind::Bought, &p.fin_material, m, &["engine bay"]));
    }
    Ok(out)
}

/// All-pairs interference (> 1 cm^3 counts) of the placed parts.
pub fn fit_report(v: &Vehicle, cat: &Catalog) -> anyhow::Result<Vec<(String, String, f64)>> {
    let ps = parts(v, cat)?;
    Ok(interference(&ps, 1000.0))
}

/// Write STLs (every part), a printable fin-alignment jig sliced by
/// PrusaSlicer when installed, the engine drawing table, BOM CSV and the
/// design JSON.
pub fn build(d: &RocketDesign, out: &std::path::Path) -> anyhow::Result<Artifacts> {
    use std::fmt::Write as _;
    std::fs::create_dir_all(out)?;
    let cat = Catalog::load()?;
    // Built and flown from the parameters, not the stored derived fields.
    let r = crate::domain::reflown(&d.launch_spec(), d)?;
    let v = &r.vehicle;
    let (lo, hi) = r.dispersion.iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(x.1), a.1.max(x.1)));
    let mut art = Artifacts::default();
    let mut ps = parts(v, &cat)?;
    // FreeCAD builds the solids (wright_geom::kernel): B-rep volumes, OCCT
    // interference, AP242 STEP + GLB; csg.rs meshes only without FreeCAD
    let k = wright_geom::kernel::realize_parts(&mut ps, out, &wright_geom::kernel::Options { stem: "rocket".into(), ..Default::default() });
    art.add(format!("CAD kernel report: {}", k.summary()), out.join("rocket-kernel-report.json"));
    for (what, path) in k.files() {
        art.add(format!("rocket {what}"), path);
    }
    for p in &ps {
        let f = out.join(format!("{}.stl", p.name.replace(' ', "_")));
        p.mesh.write_stl(&f)?;
        art.add(format!("STL {}", p.name), f);
    }
    // Fin alignment jig: a 25 mm ring over the airframe with fin slots.
    let r_out = v.diameter / 2.0 * 1000.0;
    let th = cat.get(&d.params.fin_material).and_then(|f| f.f("thickness_mm")).unwrap_or(4.76);
    let mut jig = shapes::tube(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 25.0), r_out + 30.0, r_out + 0.3, 64);
    for k in 0..d.params.fin_count {
        let slot = shapes::cuboid(v3(r_out - 1.0, -(th + 0.4) / 2.0, -1.0), v3(r_out + 35.0, (th + 0.4) / 2.0, 26.0))
            .rotate(v3(0.0, 0.0, 1.0), 2.0 * std::f64::consts::PI * k as f64 / d.params.fin_count as f64);
        jig = jig.subtract(&slot);
    }
    let jig_dir = out.join("jig");
    let mut jig_part = [part("fin alignment jig", PartKind::Printed, "petg", jig, &[])];
    let jk = wright_geom::kernel::realize_parts(&mut jig_part, &jig_dir, &wright_geom::kernel::Options { interference: false, stem: "fin_alignment_jig".into(), ..Default::default() });
    art.add(format!("CAD kernel report (jig): {}", jk.summary()), jig_dir.join("fin_alignment_jig-kernel-report.json"));
    let [jig_part] = jig_part;
    let jig = jig_part.mesh;
    let jig_path = out.join("fin_alignment_jig.stl");
    jig.on_bed().write_stl(&jig_path)?;
    art.add("STL fin alignment jig (print)", &jig_path);
    if wright_geom::slice::prusaslicer().is_some() {
        // the smallest common printer whose bed holds the jig (a 4" airframe's
        // jig is ~217 mm across: too deep for a Prusa MK4's 210 mm bed)
        let jig_on_bed = jig.on_bed();
        if let (Some(pr), Ok(m)) = (wright_geom::slice::printer_that_fits(&jig_on_bed), wright_geom::slice::material("petg")) {
            match wright_geom::slice::slice(&jig_path, &out.join("fin_alignment_jig.gcode"), pr, m, wright_geom::slice::Recipe::Structural { perimeters: 3, infill_pct: 30 }) {
                Ok(s) => art.add(format!("G-code fin jig ({:.0} min, {:.0} g)", s.seconds / 60.0, s.grams), s.gcode),
                Err(e) => art.add(format!("fin jig not sliced: {e}"), &jig_path),
            }
        }
    }
    // Engine drawing table (metal parts: dimensions, not G-code).
    let e = &v.engine;
    let mut t = String::from("| feature | value |\n|---|---|\n");
    let rows = [
        ("throat diameter", format!("{:.2} mm", e.throat_d * 1e3)),
        ("exit diameter", format!("{:.2} mm (Ae/At {:.2}, 15 deg conical)", e.exit_d * 1e3, e.area_ratio)),
        ("chamber bore", format!("{:.2} mm (Ac/At {:.2})", e.chamber_d * 1e3, e.contraction)),
        ("chamber cylinder length", format!("{:.1} mm (L* {:.2} m)", e.chamber_cyl_len * 1e3, e.lstar)),
        ("converging length", format!("{:.1} mm (30 deg)", e.converging_len * 1e3)),
        ("diverging length", format!("{:.1} mm", e.diverging_len * 1e3)),
        ("phenolic liner, chamber / throat", format!("{:.1} / {:.1} mm", e.chamber_wall[0].thickness * 1e3, e.throat_wall[0].thickness * 1e3)),
        ("6061-T6 case wall", format!("{:.1} mm", e.chamber_wall[1].thickness * 1e3)),
        ("Pc / O/F / mdot", format!("{:.1} bar / {:.2} / {:.3} kg/s", e.spec.pc / 1e5, e.spec.of, e.mdot)),
    ];
    let inj: Vec<(&str, String)> = match &e.injector {
        crate::engine::Injector::Pintle(p) => vec![
            ("pintle tip OD", format!("{:.2} mm", p.pintle_d * 1e3)),
            ("LOX radial holes", format!("{} x {:.2} mm drill", p.ox_holes, p.ox_hole_d * 1e3)),
            ("fuel annulus gap", format!("{:.3} mm", p.fuel_gap * 1e3)),
        ],
        crate::engine::Injector::Triplet(tri) => vec![
            ("O-F-O triplet elements", format!("{}", tri.elements)),
            ("N2O holes", format!("{} x {:.2} mm drill, {:.0} deg each side of the element axis", 2 * tri.elements, tri.ox_hole_d * 1e3, tri.ox_angle_deg)),
            ("fuel holes", format!("{} x {:.2} mm drill, axial", tri.elements, tri.fuel_hole_d * 1e3)),
            ("fuel regulator setpoint", format!("{:.1} bar", tri.fuel_tank_p / 1e5)),
        ],
    };
    let rows = rows.into_iter().chain(inj);
    for (k, val) in rows {
        let _ = writeln!(t, "| {k} | {val} |");
    }
    let dpath = out.join("engine_drawing.md");
    std::fs::write(&dpath, t)?;
    art.add("engine drawing table", dpath);
    // the guide's last section (wright_guide::from_artifacts reads
    // checklist.md): every number from this design
    let st = &d.start;
    let p = &d.params;
    let mut c = String::from("# Before launch\n\n");
    let ox = if v.profile.is_some() { "N2O" } else { "LOX" };
    let mut items = vec![
        format!("Proof-test each tank alone with water to {:.1} bar ({ox}) and {:.1} bar (fuel), 1.5 x MEOP, before any propellant: ports plugged, relief valve and main valve removed (they are rated for MEOP, {:.0} / {:.0} psi, not for proof); {} + {} radial closure screws per bulkhead", 1.5 * v.ox_tank.meop / 1e5, 1.5 * v.fuel_tank.meop / 1e5, v.ox_tank.meop / 6894.757, v.fuel_tank.meop / 6894.757, v.ox_tank.closure_screws, v.fuel_tank.closure_screws),
        format!("Cold-flow the feed system: command the {ox} main valve open {:.2} s after igniter light and the fuel main {:.2} s after ({ox} lead; propellant reaches the injector later by the {:.2} s valve stroke plus line fill, which the start_sequence check reports)", st.lox_open_s, st.fuel_open_s, st.valve_stroke_s),
        format!("Static-fire before flight: confirm Pc {:.1} bar and O/F {:.2} against the engine table; the igniter must burn {:.1} s", p.engine.pc / 1e5, p.engine.of, st.igniter_burn_s),
        "Weigh the vehicle and enter the measured dry mass and CG in the spec (measured_dry); re-run the checks before flying".to_string(),
        format!("Ground-test both altimeters ({}) and the GPS tracker ({}); dual-deploy charges sized and ground-tested for the drogue ({}) and main ({})", p.altimeters.join(", "), p.tracker, p.drogue, p.main),
        format!("Rail: the design leaves the rail at {:.0} ft/s; use the rail length it was checked for and confirm the rail guides slide freely", r.nominal.rail_exit_speed / 0.3048),
        format!("Predicted apogee {:.0} ft (dispersion band {:.0}-{:.0} ft): file it as the contract altitude", r.nominal.apogee_agl / 0.3048, lo / 0.3048, hi / 0.3048),
    ];
    if v.profile.is_some() {
        items.insert(1, format!(
            "N2O: load {:.1} kg into the {:.1} L run tank and hold it between {:.0} and {:.0} C with a heater blanket (vapour pressure {:.1}-{:.1} bar; the checks fly both ends). Set the relief at {:.0} psi and the fuel regulator to {:.1} bar. Confirm the nitrous grade: speed-shop fills can carry a sulfur dioxide additive",
            v.ox_tank.propellant_kg,
            v.ox_tank.volume() * 1e3,
            crate::rules::N2O_FILL_COLD_K - 273.15,
            crate::rules::N2O_FILL_HOT_K - 273.15,
            crate::nitrous::p_sat(crate::rules::N2O_FILL_COLD_K).unwrap_or(f64::NAN) / 1e5,
            crate::nitrous::p_sat(crate::rules::N2O_FILL_HOT_K).unwrap_or(f64::NAN) / 1e5,
            v.ox_tank.meop / 6894.757,
            v.fuel_tank.p_op / 1e5
        ));
    }
    for it in items {
        c.push_str(&format!("- {it}\n"));
    }
    let cpath = out.join("checklist.md");
    std::fs::write(&cpath, c)?;
    art.add("pre-launch checklist", cpath);
    let bom = crate::domain::bom_for(d, &cat);
    let bpath = out.join("bom.csv");
    std::fs::write(&bpath, bom.csv())?;
    art.add("BOM CSV", bpath);
    let jpath = out.join("design.json");
    std::fs::write(&jpath, serde_json::to_string_pretty(d)?)?;
    art.add("design JSON", jpath);
    Ok(art)
}
