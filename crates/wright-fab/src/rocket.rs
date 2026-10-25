//! The rocket's made parts as CAM programs, from the build's design.json
//! (the same `Vehicle` the physics and the STLs come from) and bom.csv
//! (the stock that was bought).
//!
//! Made, not bought (crates/wright-rocket/src/domain.rs, `bom` and
//! `EngineStock`; geometry.rs, `parts`):
//! * chamber/nozzle liner: turned from the phenolic rod ("liner, converging
//!   section and nozzle machined from solid rod");
//! * chamber case: turned from the 6061 round bar ("chamber case and
//!   injector/pintle stock");
//! * injector plate (and the pintle, for a pintle injector): 6061 bar;
//! * tank bulkheads, 2 per tank: 6061, OD to the tank tube's bore with an
//!   O-ring gland, ports drilled, radial closure screw holes on an indexer;
//! * fins: routed from the G10 sheet.
//!
//! What the design does not say and this file assumes is written into
//! each program's `notes` (and so into the G-code header).

use crate::lathe::{self, Profile};
use crate::mill::{self, Outline};
use crate::rules;
use crate::toolpath::{Machine, Mv, Op, Program, Setup, Stock};
use serde_json::Value;
use wright_geom::v3;

fn f(v: &Value, path: &[&str]) -> Option<f64> {
    let mut x = v;
    for p in path {
        x = x.get(*p)?;
    }
    x.as_f64()
}

/// Bar/rod diameters (mm) in the BOM: (id, name, diameter_mm).
pub fn bom_stock(bom_csv: &str) -> Vec<(String, String, f64)> {
    let mut out = Vec::new();
    for line in bom_csv.lines().skip(1) {
        let cols = split_csv(line);
        if cols.len() < 3 {
            continue;
        }
        let (id, name) = (cols[1].clone(), cols[2].clone());
        let lower = name.to_ascii_lowercase();
        if !(lower.contains("rod") || lower.contains("round bar")) {
            continue;
        }
        // 3.500 in dia / 4" dia / 5" diameter
        let bytes: Vec<char> = name.chars().collect();
        let mut d = None;
        for i in 0..bytes.len() {
            if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_digit() && bytes[i - 1] != '.') {
                let mut j = i;
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == '.') {
                    j += 1;
                }
                let num: String = bytes[i..j].iter().collect();
                let rest: String = bytes[j..].iter().take(6).collect();
                if rest.starts_with('"') || rest.trim_start().starts_with("in") {
                    if let Ok(x) = num.parse::<f64>() {
                        d = Some(x * 25.4);
                        break;
                    }
                }
            }
        }
        if let Some(d) = d {
            out.push((id, name, d));
        }
    }
    out
}

pub fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if q && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

pub struct RocketParts {
    pub programs: Vec<Program>,
    /// (part, why it is not machined here / what is assumed)
    pub notes: Vec<String>,
    /// Stock that the BOM lacks: (part, stock wanted).
    pub missing_stock: Vec<(String, String)>,
    /// Every machined part's fit in its stock: (part, needed mm, stock mm).
    pub fits: Vec<(String, f64, f64)>,
}

pub fn programs(design: &Value, bom_csv: &str) -> anyhow::Result<RocketParts> {
    let v = design.get("vehicle").ok_or_else(|| anyhow::anyhow!("design.json has no vehicle"))?;
    let e = v.get("engine").ok_or_else(|| anyhow::anyhow!("no engine"))?;
    let mm = |x: f64| x * 1000.0;
    let rc = mm(f(e, &["chamber_d"]).unwrap_or(0.07)) / 2.0;
    let rt = mm(f(e, &["throat_d"]).unwrap_or(0.03)) / 2.0;
    let re = mm(f(e, &["exit_d"]).unwrap_or(0.07)) / 2.0;
    let lc = mm(f(e, &["chamber_cyl_len"]).unwrap_or(0.2));
    let lv = mm(f(e, &["converging_len"]).unwrap_or(0.03));
    let ld = mm(f(e, &["diverging_len"]).unwrap_or(0.07));
    let layer = |k: &str, i: usize| e.get(k).and_then(|w| w.get(i)).and_then(|l| l.get("thickness")).and_then(Value::as_f64).map(mm);
    let t_lc = layer("chamber_wall", 0).unwrap_or(3.0);
    let t_case = layer("chamber_wall", 1).unwrap_or(3.0);
    let t_lt = layer("throat_wall", 0).unwrap_or(5.0);
    let stock = bom_stock(bom_csv);
    let rod = stock.iter().find(|s| s.1.to_ascii_lowercase().contains("phenolic")).cloned();
    let bar = stock.iter().filter(|s| s.1.contains("6061")).max_by(|a, b| a.2.total_cmp(&b.2)).cloned();
    let mut out = RocketParts { programs: vec![], notes: vec![], missing_stock: vec![], fits: vec![] };

    // ---------------- liner (phenolic) ----------------
    let r_case_id = rc + t_lc;
    let r_noz = (rt + t_lt).max(re + 6.0).max(r_case_id);
    let len = lc + lv + ld;
    let od: Profile = if r_noz > r_case_id + 0.01 {
        vec![(0.0, r_case_id), (lc + lv, r_case_id), (lc + lv, r_noz), (len, r_noz)]
    } else {
        vec![(0.0, r_case_id), (len, r_case_id)]
    };
    let bore1: Profile = vec![(0.0, rc), (lc, rc), (lc + lv, rt)];
    let bore2: Profile = vec![(0.0, re), (ld, rt)];
    let (rod_d, rod_name) = match &rod {
        Some(r) => (r.2, r.1.clone()),
        None => {
            out.missing_stock.push(("liner".into(), format!("phenolic rod >= {:.1} mm dia", 2.0 * r_noz)));
            ((2.0 * r_noz + 6.0).ceil(), "phenolic rod (not in BOM)".into())
        }
    };
    out.fits.push(("chamber/nozzle liner".into(), 2.0 * r_noz, rod_d));
    {
        let ro = rod_d / 2.0;
        let grip = (0.25 * len).clamp(8.0, 25.0);
        let vc = rules::PHENOLIC_TURN_VC;
        let doc = rules::PHENOLIC_TURN_DOC;
        let pilot = (2.0 * rt * 0.8).min(25.4).floor();
        let mut s1 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s1.extend(lathe::turn_od(lathe::turning_tool(1), vc, doc, ro, &od, 0.0, len - grip - 2.0));
        s1.push(lathe::drill(lathe::lathe_drill(3, pilot), 0.0, len + 2.0));
        let mut b1 = lathe::bore(lathe::boring_bar(4, pilot), vc, doc, pilot / 2.0, &bore1, 0.0, lc + lv);
        b1[0].name = "rough bore chamber + converging (G71-style, internal)".into();
        s1.extend(b1);
        // setup 2: exit end out, soft jaws on the finished chamber-end OD
        let flip_od: Profile = od.iter().rev().map(|&(u, r)| (len - u, r)).collect();
        let mut s2 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s2.extend(lathe::turn_od(lathe::turning_tool(1), vc, doc, ro, &flip_od, 0.0, grip + 3.0));
        let mut b2 = lathe::bore(lathe::boring_bar(4, pilot), vc, doc, pilot / 2.0, &bore2, 0.0, ld);
        b2[0].name = "rough bore diverging cone to the throat".into();
        s2.extend(b2);
        out.programs.push(Program {
            name: "liner_lathe".into(),
            part: "chamber/nozzle liner".into(),
            machine: Machine::Lathe,
            material: "linen/canvas phenolic".into(),
            density: rules::PHENOLIC_DENSITY,
            stock: Stock::Round { d: rod_d, len: len + 3.0 },
            setups: vec![
                Setup { name: "setup 1: chamber end out".into(), flipped: false, ops: s1 },
                Setup { name: "setup 2: nozzle exit end out (soft jaws)".into(), flipped: true, ops: s2 },
            ],
            notes: vec![
                format!("stock: {rod_name}"),
                format!("bore: chamber {:.2} dia x {:.1}, 30-deg-class cone to throat {:.2} dia, diverging to exit {:.2} dia over {:.1}", 2.0 * rc, lc, 2.0 * rt, 2.0 * re, ld),
                "roughing written out as G0/G1 passes after LinuxCNC G71 semantics (interp_g7x.cc), not as a canned call".into(),
                "phenolic dust: carbide tools, no coolant, extraction on".into(),
            ],
            qty: 1,
        });
    }

    // ---------------- case (6061) ----------------
    let r_case = r_case_id + t_case;
    let case_len = 40.0 + lc + lv;
    let (bar_d, bar_name) = match &bar {
        Some(b) => (b.2, b.1.clone()),
        None => {
            out.missing_stock.push(("chamber case".into(), format!("6061 bar >= {:.1} mm", 2.0 * r_case)));
            ((2.0 * r_case + 6.0).ceil(), "6061 bar (not in BOM)".into())
        }
    };
    out.fits.push(("chamber case".into(), 2.0 * r_case, bar_d));
    {
        let ro = bar_d / 2.0;
        let grip = 20.0;
        let vc = rules::AL_TURN_VC;
        let doc = rules::AL_TURN_DOC;
        let drill_d = 25.0f64.min(2.0 * r_case_id - 6.0).max(6.0);
        let prof: Profile = vec![(0.0, r_case), (case_len, r_case)];
        let boreprof: Profile = vec![(0.0, r_case_id), (case_len, r_case_id)];
        let mut s1 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s1.extend(lathe::turn_od(lathe::turning_tool(1), vc, doc, ro, &prof, 0.0, case_len - grip - 2.0));
        s1.push(lathe::drill(lathe::lathe_drill(3, drill_d), 0.0, case_len + 2.0));
        s1.extend(lathe::bore(lathe::boring_bar(4, drill_d), vc, doc, drill_d / 2.0, &boreprof, 0.0, case_len + 1.0));
        let mut s2 = vec![lathe::face(lathe::turning_tool(1), vc, ro, r_case_id, 1.5, 0.0)];
        s2.extend(lathe::turn_od(lathe::turning_tool(1), vc, doc, ro, &prof, 0.0, grip + 3.0));
        out.programs.push(Program {
            name: "case_lathe".into(),
            part: "chamber case".into(),
            machine: Machine::Lathe,
            material: "6061-T6511 aluminium".into(),
            density: rules::AL6061_DENSITY,
            stock: Stock::Round { d: bar_d, len: case_len + 3.0 },
            setups: vec![
                Setup { name: "setup 1: turn OD, drill, bore".into(), flipped: false, ops: s1 },
                Setup { name: "setup 2: flipped, face to length, finish OD".into(), flipped: true, ops: s2 },
            ],
            notes: vec![
                format!("stock: {bar_name}"),
                format!("case {:.2} OD x {:.2} ID (liner slip fit) x {:.1} long: 40 mm injector head + chamber + converging", 2.0 * r_case, 2.0 * r_case_id, case_len),
                "ASSUMED: the injector-to-case joint is not in the rocket model; the forward 40 mm is plain bore".into(),
            ],
            qty: 1,
        });
    }

    // ---------------- injector plate (+ pintle) ----------------
    let inj = e.get("injector").cloned().unwrap_or(Value::Null);
    let triplet = inj.get("Triplet").cloned();
    let pintle = inj.get("Pintle").cloned().or_else(|| inj.get("pintle_d").map(|_| inj.clone()));
    let plate_r = (r_case + 12.0).min(bar_d / 2.0 - 1.0);
    let plate_t = 12.0;
    out.fits.push(("injector plate".into(), 2.0 * plate_r, bar_d));
    {
        // lathe: face + OD from a bar slice
        let ro = bar_d / 2.0;
        let vc = rules::AL_TURN_VC;
        let prof: Profile = vec![(0.0, plate_r), (plate_t, plate_r)];
        let mut s1 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s1.extend(lathe::turn_od(lathe::turning_tool(1), vc, rules::AL_TURN_DOC, ro, &prof, 0.0, plate_t - 3.0));
        let s2 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        out.programs.push(Program {
            name: "injector_lathe".into(),
            part: "injector plate".into(),
            machine: Machine::Lathe,
            material: "6061-T6511 aluminium".into(),
            density: rules::AL6061_DENSITY,
            stock: Stock::Round { d: bar_d, len: plate_t + 3.0 },
            setups: vec![
                Setup { name: "setup 1: face, turn OD".into(), flipped: false, ops: s1 },
                Setup { name: "setup 2: flipped, face to thickness".into(), flipped: true, ops: s2 },
            ],
            notes: vec![format!("stock: slice of {bar_name}"), format!("plate {:.1} dia x {plate_t:.1}", 2.0 * plate_r)],
            qty: 1,
        });
        // mill: manifold groove, element holes, bolt circle
        let mut ops: Vec<Op> = Vec::new();
        let (n_face, f_face) = mill::al_speeds(12.0);
        ops.push(mill::face("face back side", mill::end_mill(1, 12.0, "12 mm 3-flute carbide end mill"), n_face, f_face, -plate_r, plate_r, -plate_r, plate_r, -0.3));
        let mut holes_note = String::new();
        let r_bc = 0.5 * (r_case + plate_r).max(r_case_id + 5.0);
        let (n6, f6) = mill::al_speeds(6.0);
        if let Some(t) = &triplet {
            let n_el = t.get("elements").and_then(Value::as_u64).unwrap_or(24) as usize;
            let d_ox = mm(t.get("ox_hole_d").and_then(Value::as_f64).unwrap_or(0.002));
            let d_fu = mm(t.get("fuel_hole_d").and_then(Value::as_f64).unwrap_or(0.001));
            let ang = t.get("ox_angle_deg").and_then(Value::as_f64).unwrap_or(30.0);
            // rings of elements inside 0.8 of the chamber radius
            let pts = ring_layout(n_el, 0.8 * rc, 3.0 * d_ox.max(2.0));
            let fuel: Vec<(f64, f64)> = pts.iter().flat_map(|&(x, y)| {
                let a = y.atan2(x);
                let o = 1.6 * d_ox.max(1.5);
                [(x + o * a.cos(), y + o * a.sin()), (x - o * a.cos(), y - o * a.sin())]
            }).collect();
            // manifold: annulus behind the elements
            let r_in = 4.0;
            let r_out = 0.85 * rc;
            ops.push(mill::pocket("ox manifold pocket (annulus, back side)", mill::end_mill(2, 6.0, "6 mm 3-flute carbide end mill"), n6, f6, &mill::circle(0.0, 0.0, r_out, 96), &[mill::circle(0.0, 0.0, r_in, 48)], -0.3, -0.3 - 0.35 * plate_t, rules::AL_DOC_MM));
            ops.push(mill::drill(&format!("{n_el} ox orifices {d_ox:.2} mm"), mill::drill_tool(3, d_ox, &format!("{d_ox:.2} mm carbide micro drill")), &pts, -0.3, -plate_t - 0.5));
            ops.push(mill::drill(&format!("{} fuel orifices {d_fu:.2} mm", fuel.len()), mill::drill_tool(4, d_fu, &format!("{d_fu:.2} mm carbide micro drill")), &fuel, -0.3, -plate_t - 0.5));
            holes_note = format!("triplet: {n_el} elements, ox {d_ox:.2} mm on axis, 2 fuel {d_fu:.2} mm each; the {ang:.0} deg impingement angle needs a tilted fixture: drilled here perpendicular as spot locations, finish on a {ang:.0} deg sine plate");
        } else if let Some(pn) = &pintle {
            let pd = mm(pn.get("pintle_d").and_then(Value::as_f64).unwrap_or(0.018));
            let gap = mm(pn.get("fuel_gap").and_then(Value::as_f64).unwrap_or(0.0005));
            let r_hole = pd / 2.0 + gap;
            ops.push(mill::pocket("fuel manifold pocket (annulus, back side)", mill::end_mill(2, 6.0, "6 mm 3-flute carbide end mill"), n6, f6, &mill::circle(0.0, 0.0, 0.8 * rc, 96), &[mill::circle(0.0, 0.0, r_hole + 4.0, 48)], -0.3, -0.3 - 0.4 * plate_t, rules::AL_DOC_MM));
            let (n4, f4) = mill::al_speeds(4.0);
            // pintle bore: helical-style circular pocket (rings) through
            ops.push(mill::pocket(&format!("pintle bore {:.3} dia (fuel annulus OD), through", 2.0 * r_hole), mill::end_mill(5, 4.0, "4 mm 2-flute carbide end mill"), n4, f4, &mill::circle(0.0, 0.0, r_hole, 64), &[], -0.3, -plate_t - 0.3, 1.0));
            holes_note = format!("pintle: post {pd:.2} mm, fuel annulus gap {gap:.3} mm => bore {:.3} mm (ream to size)", 2.0 * r_hole);
        }
        let bolts: Vec<(f64, f64)> = (0..8).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / 8.0;
            (r_bc * a.cos(), r_bc * a.sin())
        }).collect();
        ops.push(mill::drill("8 bolt holes 5.5 mm (M5 clearance)", mill::drill_tool(6, 5.5, "5.5 mm HSS drill"), &bolts, -0.3, -plate_t - 0.5));
        let _ = (n_face, n6);
        out.programs.push(Program {
            name: "injector_mill".into(),
            part: "injector plate".into(),
            machine: Machine::Mill,
            material: "6061-T6511 aluminium".into(),
            density: rules::AL6061_DENSITY,
            stock: Stock::Round { d: 2.0 * plate_r, len: plate_t },
            setups: vec![Setup { name: "mill: plate in soft jaws, back side up".into(), flipped: false, ops }],
            notes: vec![holes_note, "ASSUMED: 8 x M5 bolt circle to the case flange (the rocket model has no injector joint)".into()],
            qty: 1,
        });
        if let Some(pn) = &pintle {
            let pd = mm(pn.get("pintle_d").and_then(Value::as_f64).unwrap_or(0.018));
            let n_holes = pn.get("ox_holes").and_then(Value::as_u64).unwrap_or(12) as usize;
            let hd = mm(pn.get("ox_hole_d").and_then(Value::as_f64).unwrap_or(0.0019));
            let pin_len = 30.0;
            let flange_r = (pd / 2.0 + 10.0).min(bar_d / 2.0 - 1.0);
            let ro = (flange_r + 3.0).min(bar_d / 2.0);
            let prof: Profile = vec![(0.0, pd / 2.0), (pin_len, pd / 2.0), (pin_len, flange_r), (pin_len + 8.0, flange_r)];
            let vc = rules::AL_TURN_VC;
            let mut s1 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
            s1.extend(lathe::turn_od(lathe::turning_tool(1), vc, rules::AL_TURN_DOC, ro, &prof, 0.0, pin_len + 6.0));
            let bore_d = (0.6 * pd).floor().max(3.0);
            s1.push(lathe::drill(lathe::lathe_drill(3, bore_d), 0.0, pin_len - 2.0));
            // radial LOX holes on the mill's indexer: written as an A-axis op
            let mut radial = vec![Mv::Rapid(v3(0.0, 0.0, 30.0))];
            for k in 0..n_holes {
                radial.push(Mv::Index(360.0 * k as f64 / n_holes as f64));
                radial.push(Mv::Drill { x: 0.0, y: 0.0, bottom: pd / 2.0 - bore_d / 2.0 - 0.5, r_plane: pd / 2.0 + 2.0, peck: hd * 1.5, feed: 60.0 });
            }
            radial.push(Mv::Index(0.0));
            let rad_op = Op { name: format!("{n_holes} radial LOX holes {hd:.2} mm (4th axis, Y = 0 on the post's top, X at {:.1} mm from the tip)", pin_len - 4.0), tool: mill::drill_tool(7, hd, &format!("{hd:.2} mm carbide micro drill")), rpm: rules::MILL_MAX_RPM, css: None, moves: radial };
            out.programs.push(Program {
                name: "pintle_lathe".into(),
                part: "pintle".into(),
                machine: Machine::Lathe,
                material: "6061-T6511 aluminium".into(),
                density: rules::AL6061_DENSITY,
                stock: Stock::Round { d: 2.0 * ro, len: pin_len + 8.0 + 3.0 },
                setups: vec![Setup { name: "setup 1: turn post and flange, drill LOX passage".into(), flipped: false, ops: s1 }],
                notes: vec![format!("post {pd:.2} dia x {pin_len:.0}, flange {:.1} dia; then the radial holes on the mill (program pintle_radial)", 2.0 * flange_r)],
                qty: 1,
            });
            out.programs.push(Program {
                name: "pintle_radial".into(),
                part: "pintle".into(),
                machine: Machine::Mill,
                material: "6061-T6511 aluminium".into(),
                density: rules::AL6061_DENSITY,
                stock: Stock::Round { d: pd, len: pin_len },
                setups: vec![Setup { name: "mill: pintle in a 5C collet on the 4th axis".into(), flipped: false, ops: vec![rad_op] }],
                notes: vec!["A-axis indexing; not simulated (height-field sim is 3-axis)".into()],
                qty: 1,
            });
        }
    }

    // ---------------- bulkheads ----------------
    for key in ["ox_tank", "fuel_tank"] {
        let Some(t) = v.get(key) else { continue };
        let name = t.get("name").and_then(Value::as_str).unwrap_or(key).replace(" run", "");
        let r_in = mm(f(t, &["inner_d"]).unwrap_or(0.146)) / 2.0;
        let th = mm(f(t, &["bulkhead_t"]).unwrap_or(0.014));
        let screws = t.get("closure_screws").and_then(Value::as_u64).unwrap_or(12) as usize;
        let r_od = r_in - 0.05; // slip fit in the tube bore
        // AS568-2xx (0.139 in = 3.53 mm cross-section) static radial gland:
        // depth ~2.85 mm, width ~4.8 mm (Parker O-Ring Handbook ORD 5700,
        // gland tables; values recalled, verify before cutting)
        let (g_w, g_d) = (4.8, 2.85);
        let grooves = if th >= 22.0 { 2 } else { 1 };
        let blank_d = match stock.iter().filter(|s| s.1.contains("6061")).find(|s| s.2 >= 2.0 * r_od + 2.0) {
            Some(s) => s.2,
            None => {
                let want = (2.0 * r_od + 4.0) / 25.4;
                let d_in = (want * 2.0).ceil() / 2.0;
                out.missing_stock.push((format!("{name} bulkheads"), format!("6061 round bar {d_in:.1} in dia x {:.0} mm (2 blanks)", 2.0 * (th + 3.0) + 2.0 * rules::SAW_KERF_MM)));
                d_in * 25.4
            }
        };
        out.fits.push((format!("{name} bulkhead"), 2.0 * r_od, blank_d));
        let vc = rules::AL_TURN_VC;
        let ro = blank_d / 2.0;
        let prof: Profile = vec![(0.0, r_od), (th, r_od)];
        let mut s1 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s1.extend(lathe::turn_od(lathe::turning_tool(1), vc, rules::AL_TURN_DOC, ro, &prof, 0.0, th - 2.0));
        for g in 0..grooves {
            let uc = th * (g as f64 + 1.0) / (grooves as f64 + 1.0);
            s1.push(lathe::groove(lathe::groove_tool(5, 3.0), vc, 0.0, uc, g_w, r_od, r_od - g_d));
        }
        let mut s2 = vec![lathe::face(lathe::turning_tool(1), vc, ro, 0.0, 1.5, 0.0)];
        s2.extend(lathe::turn_od(lathe::turning_tool(1), vc, rules::AL_TURN_DOC, ro, &prof.iter().map(|&(u, r)| (th - u, r)).rev().collect::<Vec<_>>(), 0.0, 3.0));
        out.programs.push(Program {
            name: format!("{}_bulkhead_lathe", key.trim_end_matches("_tank")),
            part: format!("{name} bulkhead"),
            machine: Machine::Lathe,
            material: "6061-T6 aluminium".into(),
            density: rules::AL6061_DENSITY,
            stock: Stock::Round { d: blank_d, len: th + 3.0 },
            setups: vec![
                Setup { name: "setup 1: face, OD, O-ring gland".into(), flipped: false, ops: s1 },
                Setup { name: "setup 2: flipped, face to thickness".into(), flipped: true, ops: s2 },
            ],
            notes: vec![
                format!("OD {:.2} = tank bore {:.2} - 0.1 slip fit; {grooves} AS568-2xx gland(s) {g_w} wide x {g_d} deep (Parker ORD 5700 static radial, verify)", 2.0 * r_od, 2.0 * r_in),
                format!("thickness {th:.1} mm as sized by the rocket's plate stress check: no lightening pockets (they would void that check)"),
            ],
            qty: 2,
        });
        // mill: ports (NPT tap drills) + radial closure holes on the indexer
        let mut ops = Vec::new();
        // 1/2 NPT tap drill 23/32 in (18.26 mm); 1/4 NPT 7/16 in (11.11 mm):
        // ASME B1.20.1 tap-drill tables
        let (n12, f12) = mill::al_speeds(12.0);
        ops.push(mill::pocket("1/2 NPT feed port, tap drill 23/32 in (18.26), helical-style rings", mill::end_mill(1, 12.0, "12 mm 3-flute carbide end mill"), n12, f12, &mill::circle(0.0, 0.0, 18.26 / 2.0, 64), &[], 0.0, -th - 0.3, rules::AL_DOC_MM * 1.5));
        let r_port = 0.55 * r_od;
        ops.push(mill::drill("1/4 NPT vent / pressurant ports, tap drill 7/16 in (11.11)", mill::drill_tool(2, 11.11, "7/16 in HSS drill"), &[(r_port, 0.0), (-r_port, 0.0)], 0.0, -th - 1.0));
        // #7 tap drill (5.11 mm) for 1/4-20 radial screws, into the rim
        let mut radial = vec![Mv::Rapid(v3(0.0, 0.0, 30.0))];
        for k in 0..screws {
            radial.push(Mv::Index(360.0 * k as f64 / screws as f64));
            radial.push(Mv::Drill { x: -th / 2.0, y: 0.0, bottom: -12.0, r_plane: 2.0, peck: 4.0, feed: 150.0 });
        }
        radial.push(Mv::Index(0.0));
        let radial_op = Op { name: format!("{screws} radial 1/4-20 holes, #7 tap drill (4th axis)"), tool: mill::drill_tool(3, 5.11, "#7 (5.11 mm) HSS drill"), rpm: rules::rpm(rules::DRILL_VC, 5.11, rules::MILL_MAX_RPM), css: None, moves: radial };
        out.programs.push(Program {
            name: format!("{}_bulkhead_mill", key.trim_end_matches("_tank")),
            part: format!("{name} bulkhead"),
            machine: Machine::Mill,
            material: "6061-T6 aluminium".into(),
            density: rules::AL6061_DENSITY,
            stock: Stock::Round { d: 2.0 * r_od, len: th },
            setups: vec![
                Setup { name: "setup 1: bulkhead flat in soft jaws, ports".into(), flipped: false, ops },
                Setup { name: "setup 2: 4th axis (A about X), disc axis along X, Z0 on the rim".into(), flipped: false, ops: vec![radial_op] },
            ],
            notes: vec![
                "ports: 1/2 NPT on axis (feed / downcomer), 2 x 1/4 NPT (vent, pressurant); tap after drilling (ASME B1.20.1 tap drills)".into(),
                format!("{screws} radial 1/4-20 closure screws into the rim (rocket rules.rs: radial closure through the tube wall); radial holes are A-axis moves, not simulated"),
            ],
            qty: 2,
        });
    }

    // ---------------- fins (G10, router) ----------------
    let p = design.get("params").unwrap_or(&Value::Null);
    let root = mm(f(p, &["fin_root"]).unwrap_or(0.3));
    let tip = mm(f(p, &["fin_tip"]).unwrap_or(0.12));
    let span = mm(f(p, &["fin_span"]).unwrap_or(0.12));
    let sweep = mm(f(p, &["fin_sweep"]).unwrap_or(0.1));
    let count = p.get("fin_count").and_then(Value::as_u64).unwrap_or(4) as usize;
    let th = f(v, &["aero", "fin_thickness"]).map(mm).unwrap_or(4.76);
    let fin: Outline = vec![(0.0, 0.0), (root, 0.0), (sweep + tip, span), (sweep, span)];
    let sheet = 304.8; // 1 sq ft sheets (BOM: wildman_g10 "x 1 sq ft")
    let gap = 12.0;
    let w = root.max(sweep + tip);
    let per_row = ((sheet - gap) / (w + gap)).floor().max(1.0) as usize;
    let rows = ((sheet - gap) / (span + gap)).floor().max(1.0) as usize;
    let per_sheet = per_row * rows;
    let sheets = count.div_ceil(per_sheet.max(1));
    out.fits.push(("fin".into(), w.max(span), sheet));
    for sh in 0..sheets {
        let n = (count - sh * per_sheet).min(per_sheet);
        let tool = mill::end_mill(1, 3.175, "1/8 in diamond-cut carbide router bit");
        let mut moves = Vec::new();
        for i in 0..n {
            let (cx, cy) = (gap + (i % per_row) as f64 * (w + gap), gap + (i / per_row) as f64 * (span + gap));
            let o: Outline = fin.iter().map(|&(x, y)| (x + cx, y + cy)).collect();
            let op = mill::profile(&format!("fin {} profile, 3 tabs", sh * per_sheet + i + 1), tool.clone(), rules::G10_ROUTER_RPM, rules::G10_FEED_MM_MIN, &o, th, -0.3, rules::G10_DOC_MM, 3, 6.0, 1.2);
            moves.push(op);
        }
        out.programs.push(Program {
            name: format!("fins_router_{}", sh + 1),
            part: "fins (G10)".into(),
            machine: Machine::Router,
            material: "G10 fibreglass sheet".into(),
            density: rules::G10_DENSITY,
            stock: Stock::Block { x: sheet, y: sheet, z: th },
            setups: vec![Setup { name: format!("router: G10 sheet {} on the spoilboard", sh + 1), flipped: false, ops: moves }],
            notes: vec![
                format!("{n} fins: root {root:.1}, tip {tip:.1}, span {span:.1}, sweep {sweep:.1}, {th:.2} thick"),
                "G10 dust is hazardous: vacuum at the cutter, respirator; diamond-cut bit (fibreglass dulls carbide fast)".into(),
                "tabs 6 mm wide, 1.2 mm thick: snap and sand flush".into(),
            ],
            qty: 1,
        });
    }
    Ok(out)
}

/// Element positions on concentric rings out to `r_max`, spacing `pitch`.
fn ring_layout(n: usize, r_max: f64, pitch: f64) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    let mut r = r_max;
    while pts.len() < n && r > pitch {
        let cap = ((std::f64::consts::TAU * r) / pitch).floor() as usize;
        let k = cap.min(n - pts.len());
        for i in 0..k {
            let a = std::f64::consts::TAU * i as f64 / k as f64 + r * 0.01;
            pts.push((r * a.cos(), r * a.sin()));
        }
        r -= pitch;
    }
    if pts.len() < n {
        pts.push((0.0, 0.0));
    }
    pts
}
