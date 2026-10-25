//! The fab stage: from a build directory, a process plan for every part
//! (printed / milled / turned / routed / cut / bought, with stock, machine,
//! tool and time), the CAM files, and a film of every process, the
//! assembly and a turntable; `<out>/fab/manifest.json` lists them.

use crate::assembly;
use crate::cut::{self, CutPlan};
use crate::film;
use crate::print;
use crate::render::Renderer;
use crate::rocket::{self, split_csv};
use crate::toolpath::{Machine, Program};
use crate::video::Painter;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wright_core::{Manifest, Stage, StageCtx};

#[derive(Debug, Clone, Serialize)]
pub struct PlanRow {
    pub part: String,
    pub process: String,
    pub qty: u32,
    pub stock: String,
    pub machine: String,
    pub tools: String,
    /// per piece, setups and tool changes included
    pub minutes: f64,
    /// finished part (printed: filament)
    pub grams: f64,
    pub files: Vec<String>,
    pub notes: String,
}

pub struct Options {
    /// Make films (false: plans and CAM files only).
    pub films: bool,
    /// At most this many print timelapses (longest prints first).
    pub max_prints: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { films: true, max_prints: 8 }
    }
}

/// The directory holding the build's files (bom.csv, STLs, print/).
pub fn build_dir(out_dir: &Path, artifacts: &wright_core::Artifacts) -> PathBuf {
    if out_dir.join("bom.csv").exists() || out_dir.join("design.json").exists() {
        return out_dir.to_path_buf();
    }
    for (_, p) in &artifacts.files {
        if p.file_name().is_some_and(|n| n == "bom.csv" || n == "design.json") {
            if let Some(d) = p.parent() {
                return d.to_path_buf();
            }
        }
    }
    out_dir.to_path_buf()
}

pub fn guess_domain(dir: &Path, design: &Value) -> String {
    if design.pointer("/vehicle/engine").is_some() {
        "rocket".into()
    } else if dir.join("vtol.param").exists() || dir.join("cad").join("assembly.stl").exists() {
        "aircraft".into()
    } else {
        "balloon".into()
    }
}

fn rel(dir: &Path, p: &Path) -> String {
    p.strip_prefix(dir).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn gcode_files(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in [dir.to_path_buf(), dir.join("print")] {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "gcode") {
                    v.push(p);
                }
            }
        }
    }
    v.sort();
    v
}

/// Header stats of a PrusaSlicer file without parsing moves.
fn gcode_stats(text: &str) -> (f64, f64, String, String, String) {
    let mut mins = f64::NAN;
    let mut g = f64::NAN;
    let (mut printer, mut fil, mut nozzle) = (String::new(), String::new(), String::new());
    for l in text.lines().rev().take(600) {
        if let Some(v) = l.strip_prefix("; estimated printing time (normal mode) = ") {
            let mut t = 0.0;
            for tok in v.split_whitespace() {
                let (n, u) = tok.split_at(tok.len() - 1);
                let n: f64 = n.parse().unwrap_or(0.0);
                t += n * match u { "d" => 1440.0, "h" => 60.0, "m" => 1.0, _ => 1.0 / 60.0 };
            }
            mins = t;
        } else if let Some(v) = l.strip_prefix("; filament used [g] = ") {
            g = v.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).sum();
        } else if let Some(v) = l.strip_prefix("; printer_settings_id = ") {
            printer = v.trim().into();
        } else if let Some(v) = l.strip_prefix("; filament_settings_id = ") {
            fil = v.trim().trim_matches('"').into();
        } else if let Some(v) = l.strip_prefix("; nozzle_diameter = ") {
            nozzle = format!("{} mm nozzle", v.trim());
        }
    }
    (mins, g, printer, fil, nozzle)
}

pub fn run(domain: &str, build: &Path, design: &Value, fab: &Path, opt: &Options) -> anyhow::Result<Manifest> {
    std::fs::create_dir_all(fab)?;
    let bom = std::fs::read_to_string(build.join("bom.csv")).unwrap_or_default();
    let title = design.pointer("/vehicle/name").or_else(|| design.get("name")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{domain} build"));
    let mut man = Manifest::new("fab", format!("Fabrication: {title}"));
    let mut rows: Vec<PlanRow> = Vec::new();
    let gdir = fab.join("gcode");
    let vdir = fab.join("video");
    let idir = fab.join("img");
    for d in [&gdir, &vdir, &idir] {
        std::fs::create_dir_all(d)?;
    }
    let renderer = if opt.films && crate::video::ffmpeg().is_some() { Renderer::new().ok() } else { None };
    if opt.films && renderer.is_none() {
        man.check("films rendered", false, "no GPU adapter or no ffmpeg: plans and CAM files only");
    }
    let painter = Painter::default();
    let mut stock_items: Vec<String> = Vec::new();

    // ---------------- machined parts ----------------
    let mut programs: Vec<Program> = Vec::new();
    if domain == "rocket" {
        let rp = rocket::programs(design, &bom)?;
        let bad: Vec<String> = rp.fits.iter().filter(|f| f.1 > f.2 - 0.5).map(|f| format!("{} needs {:.1} mm, stock {:.1} mm", f.0, f.1, f.2)).collect();
        man.check(
            "every machined part fits its stock",
            bad.is_empty(),
            &if bad.is_empty() { format!("{} parts fit (0.5 mm to clean up)", rp.fits.len()) } else { bad.join("; ") },
        );
        man.check(
            "machining stock is in the BOM",
            rp.missing_stock.is_empty(),
            &if rp.missing_stock.is_empty() { "liner rod, case/injector bar, bulkhead blanks, G10 sheet all listed".to_string() } else { rp.missing_stock.iter().map(|m| format!("{}: {}", m.0, m.1)).collect::<Vec<_>>().join("; ") },
        );
        programs = rp.programs;
        stock_items.extend(["phenolic", "round bar", "rod", "g10"].iter().map(|s| s.to_string()));
    }
    let mut filmed_bodies: BTreeMap<String, String> = BTreeMap::new();
    for p in &programs {
        let ngc = gdir.join(format!("{}.ngc", p.name));
        let g = p.gcode();
        std::fs::write(&ngc, &g)?;
        let process = match p.machine {
            Machine::Lathe => "turned",
            Machine::Mill => "milled",
            Machine::Router => "routed",
        };
        let tools: Vec<String> = p.tools().iter().map(|t| format!("T{} {}", t.number, t.name)).collect();
        let mut row = PlanRow {
            part: p.part.clone(),
            process: process.into(),
            qty: p.qty,
            stock: format!("{} ({})", crate::toolpath::stock_text(&p.stock), p.material),
            machine: p.machine.name().into(),
            tools: tools.join("; "),
            minutes: p.minutes(),
            grams: f64::NAN,
            files: vec![rel(fab, &ngc)],
            notes: p.notes.join(" | "),
        };
        // identical programs (fuel vs ox bulkhead of the same size) share a film
        let body: String = g.lines().filter(|l| !l.starts_with('(')).collect::<Vec<_>>().join("\n");
        let simulated = !p.setups.iter().all(|s| s.ops.iter().all(|o| o.moves.iter().any(|m| matches!(m, crate::toolpath::Mv::Index(_)))));
        if let (Some(r), true) = (&renderer, simulated) {
            if let Some(prev) = filmed_bodies.get(&body) {
                row.files.push(prev.clone());
            } else {
                let mp4 = vdir.join(format!("{}.mp4", p.name));
                let png = idir.join(format!("{}.png", p.name));
                let fr = film::film(r, &painter, p, &mp4, &png)?;
                row.grams = fr.part_g;
                row.files.push(rel(fab, &mp4));
                filmed_bodies.insert(body, rel(fab, &mp4));
                man.add(
                    fab,
                    &mp4,
                    &format!("{} ({process})", p.part),
                    &format!("{} on the {}: {} setups, {} ops, simulated material removal", crate::toolpath::stock_text(&p.stock), p.machine.name().split(" (").next().unwrap_or(""), p.setups.len(), p.setups.iter().map(|s| s.ops.len()).sum::<usize>()),
                    &[("minutes", p.minutes()), ("grams", fr.part_g), ("chips_g", fr.removed_g), ("stock_g", p.stock_grams()), ("qty", p.qty as f64)],
                );
                man.add(fab, &png, &format!("{} finished ({process})", p.part), "last frame of the simulation", &[("grams", fr.part_g)]);
            }
        }
        rows.push(row);
    }

    // ---------------- prints ----------------
    let mut prints: Vec<(PathBuf, f64, f64, String, String, String)> = gcode_files(build)
        .into_iter()
        .filter_map(|p| {
            let t = std::fs::read_to_string(&p).ok()?;
            let (m, g, pr, fi, nz) = gcode_stats(&t);
            Some((p, m, g, pr, fi, nz))
        })
        .collect();
    let printed_g: f64 = prints.iter().map(|p| p.2).filter(|g| g.is_finite()).sum();
    let printed_min: f64 = prints.iter().map(|p| p.1).filter(|g| g.is_finite()).sum();
    let mut seen: BTreeMap<u64, String> = BTreeMap::new();
    prints.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut filmed = 0;
    for (path, mins, grams, printer, fil, nozzle) in &prints {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(path)?;
        let h = {
            use std::hash::{Hash, Hasher};
            let mut s = std::collections::hash_map::DefaultHasher::new();
            bytes.len().hash(&mut s);
            bytes[bytes.len().saturating_sub(4096)..].hash(&mut s);
            bytes[..bytes.len().min(1 << 20)].hash(&mut s);
            s.finish()
        };
        let mut row = PlanRow {
            part: stem.replace('_', " "),
            process: "printed".into(),
            qty: 1,
            stock: if fil.is_empty() { "filament".into() } else { fil.clone() },
            machine: if printer.is_empty() { "FDM printer (PrusaSlicer G-code)".into() } else { printer.clone() },
            tools: nozzle.clone(),
            minutes: *mins,
            grams: *grams,
            files: vec![rel(build, path)],
            notes: String::new(),
        };
        if let Some(prev) = seen.get(&h) {
            row.files.push(prev.clone());
            row.notes = "same G-code as another plate".into();
        } else if let (Some(r), true) = (&renderer, filmed < opt.max_prints) {
            let mp4 = vdir.join(format!("print_{stem}.mp4"));
            let png = idir.join(format!("print_{stem}.png"));
            let pf = print::film(r, &painter, path, &stem.replace('_', " "), &mp4, &png)?;
            filmed += 1;
            row.files.push(rel(fab, &mp4));
            seen.insert(h, rel(fab, &mp4));
            man.add(fab, &mp4, &format!("{} (print)", stem.replace('_', " ")), &format!("print timelapse from the sliced G-code, {} layers", pf.layers), &[("minutes", pf.minutes), ("grams", pf.grams), ("layers", pf.layers as f64)]);
        } else {
            seen.insert(h, rel(build, path));
        }
        rows.push(row);
    }
    if !prints.is_empty() {
        let unfilmed = prints.len() - filmed;
        let _ = unfilmed;
    }

    // ---------------- cuts ----------------
    let mut groups = cut::from_bom(&bom);
    if domain == "rocket" {
        groups.extend(cut::rocket_cuts(design, &bom));
    }
    let cut_plan: Option<CutPlan> = (!groups.is_empty()).then(|| cut::plan(groups));
    if let Some(cp) = &cut_plan {
        let csv = fab.join("cutlist.csv");
        std::fs::write(&csv, cp.csv())?;
        let short: Vec<String> = cp.shortfalls().into_iter().filter(|s| s.1 > s.2).map(|s| format!("{}: {} bars needed, {} bought", s.0, s.1, s.2)).collect();
        let unplaced: Vec<String> = cp.unplaced.iter().map(|(si, c)| format!("{} {:.0} mm > {} {:.0} mm", c.label, c.length_mm, cp.stocks[*si].id, cp.stocks[*si].length_mm)).collect();
        let ok = short.is_empty() && unplaced.is_empty();
        man.check("cut lists fit the bought tube/rod", ok, &if ok { format!("{} cuts from {} bars", cp.bars.iter().map(|b| b.cuts.len()).sum::<usize>(), cp.bars.len()) } else { [short, unplaced].concat().join("; ") });
        man.add(fab, &csv, "cut list (first-fit decreasing, kerf per cut)", "every tube/rod cut with its bar, offset and offcut", &[("minutes", cp.minutes), ("cuts", cp.bars.iter().map(|b| b.cuts.len()).sum::<usize>() as f64), ("bars", cp.bars.len() as f64)]);
        for b in &cp.bars {
            let sk = &cp.stocks[b.stock];
            for (c, off) in &b.cuts {
                rows.push(PlanRow {
                    part: c.label.clone(),
                    process: "cut".into(),
                    qty: 1,
                    stock: format!("{} (bar {}, from {:.0} mm)", sk.name, b.index, off),
                    machine: if sk.material.contains("6061") { "horizontal bandsaw".into() } else { "abrasive cut-off wheel / fine-tooth saw, mitre box".into() },
                    tools: format!("kerf {:.1} mm", sk.kerf_mm),
                    minutes: crate::rules::CUT_MIN,
                    grams: f64::NAN,
                    files: vec!["cutlist.csv".into()],
                    notes: format!("{:.1} mm", c.length_mm),
                });
            }
        }
        if let Some(r) = &renderer {
            let mp4 = vdir.join("cut_tubes.mp4");
            let png = idir.join("cut_tubes.png");
            cut::film(r, &painter, cp, &title, &mp4, &png)?;
            man.add(fab, &mp4, "tube and rod cutting", &format!("{} cuts from {} bars", cp.bars.iter().map(|b| b.cuts.len()).sum::<usize>(), cp.bars.len()), &[("minutes", cp.minutes)]);
        }
    }

    // ---------------- bought ----------------
    for line in bom.lines().skip(1) {
        let c = split_csv(line);
        if c.len() < 4 {
            continue;
        }
        let l = c[2].to_ascii_lowercase();
        let is_stock = l.contains("filament") || c[0] == "filaments" || l.contains(" rod") || l.contains("round bar") || l.contains("sheet") || l.contains("tube") || l.contains("pla ") || l.contains("petg");
        rows.push(PlanRow {
            part: c[2].clone(),
            process: if is_stock { "stock".into() } else { "bought".into() },
            qty: c[3].parse::<f64>().unwrap_or(1.0).ceil() as u32,
            stock: String::new(),
            machine: String::new(),
            tools: String::new(),
            minutes: 0.0,
            grams: f64::NAN,
            files: vec![],
            notes: c.get(7).cloned().unwrap_or_default(),
        });
    }

    // ---------------- assembly + turntable ----------------
    match assembly::load(build) {
        Ok(l) => {
            if let Some(r) = &renderer {
                let mp4 = vdir.join("assembly.mp4");
                let png = idir.join("assembly.png");
                let n = assembly::film(r, &painter, &l, &title, &mp4, &png)?;
                man.add(fab, &mp4, "assembly", &format!("{} parts fly in in the build guide's order ({})", n, l.source), &[("parts", n as f64), ("steps", l.steps.len() as f64)]);
                let mp4 = vdir.join("turntable.mp4");
                let png = idir.join("render_hero.png");
                assembly::turntable(r, &painter, &l, &title, &mp4, &png)?;
                man.add(fab, &mp4, "turntable", "the finished vehicle", &[("parts", l.parts.len() as f64)]);
                man.add(fab, &png, "render", "the finished vehicle", &[]);
                let wide = idir.join("render_wide.png");
                if wide.exists() {
                    man.add(fab, &wide, "render (wide)", "the whole vehicle", &[]);
                }
            }
        }
        Err(e) => man.check("assembly geometry", false, &format!("{e:#}")),
    }

    // ---------------- plan ----------------
    let made: Vec<&PlanRow> = rows.iter().filter(|r| matches!(r.process.as_str(), "turned" | "milled" | "routed" | "printed" | "cut")).collect();
    let total_min: f64 = made.iter().map(|r| r.minutes * r.qty as f64).filter(|m| m.is_finite()).sum();
    let mach_min: f64 = made.iter().filter(|r| matches!(r.process.as_str(), "turned" | "milled" | "routed")).map(|r| r.minutes * r.qty as f64).sum();
    let plan_json = fab.join("plan.json");
    std::fs::write(&plan_json, serde_json::to_vec_pretty(&serde_json::json!({
        "domain": domain,
        "rows": rows,
        "totals": { "machine_minutes": mach_min, "print_minutes": printed_min, "print_grams": printed_g, "all_minutes": total_min },
        "programs": programs.iter().map(|p| serde_json::json!({"name": p.name, "part": p.part, "machine": p.machine, "minutes": p.minutes(), "stock": crate::toolpath::stock_text(&p.stock), "qty": p.qty, "notes": p.notes})).collect::<Vec<_>>(),
    }))?)?;
    let plan_csv = fab.join("plan.csv");
    let mut csv = String::from("part,process,qty,stock,machine,tools,minutes_each,grams_each,files,notes\n");
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for r in &rows {
        csv.push_str(&format!("{},{},{},{},{},{},{},{},{},{}\n", q(&r.part), r.process, r.qty, q(&r.stock), q(&r.machine), q(&r.tools), if r.minutes.is_finite() { format!("{:.1}", r.minutes) } else { String::new() }, if r.grams.is_finite() { format!("{:.1}", r.grams) } else { String::new() }, q(&r.files.join(" ")), q(&r.notes)));
    }
    std::fs::write(&plan_csv, csv)?;
    let n_by = |p: &str| rows.iter().filter(|r| r.process == p).count() as f64;
    man.items.insert(0, wright_core::Item {
        label: "process plan".into(),
        path: rel(fab, &plan_json),
        media: "json".into(),
        summary: format!("{} made parts ({} machined programs, {} prints, {} cuts), {} bought lines; G-code in gcode/", made.len(), programs.len(), prints.len(), n_by("cut"), n_by("bought")),
        metrics: [("minutes", total_min), ("machine_minutes", mach_min), ("print_minutes", printed_min), ("print_grams", printed_g)].into_iter().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k.to_string(), v)).collect(),
    });
    man.items.insert(1, wright_core::Item { label: "process plan (table)".into(), path: rel(fab, &plan_csv), media: "csv".into(), summary: "one row per part and process".into(), metrics: Default::default() });
    let printed_parts_ok = !prints.is_empty() || domain == "rocket";
    if domain != "rocket" {
        man.check("printed parts are sliced", printed_parts_ok, &format!("{} G-code plates, {:.0} min, {:.0} g", prints.len(), printed_min, printed_g));
    }
    Ok(man)
}

/// The pipeline stage (`wright_core::Stage`): `<out_dir>/fab`.
pub struct FabStage;

impl Stage for FabStage {
    fn name(&self) -> &'static str {
        "fab"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
        let build = build_dir(&ctx.out_dir, &ctx.artifacts);
        let design = if ctx.design.is_null() {
            std::fs::read(build.join("design.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
        } else {
            ctx.design.clone()
        };
        // the stage's design is the domain's design; the rocket's
        // design.json (vehicle + params) is what the CAM reads
        let design = if ctx.domain == "rocket" && design.get("vehicle").is_none() {
            std::fs::read(build.join("design.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(design)
        } else {
            design
        };
        run(&ctx.domain, &build, &design, &ctx.dir, &Options::default())
    }
}
