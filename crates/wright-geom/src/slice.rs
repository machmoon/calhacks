//! Slicing printed parts with PrusaSlicer's command line, so a design ends
//! in G-code with a real print time and real grams, not a mass estimate.
//!
//! Flags from PrusaSlicer 2.9.6's own option table
//! (`src/libslic3r/PrintConfig.cpp`, tag version_2.9.6): `--export-gcode`,
//! `-o`, and every config key as `--key value` (`Config.cpp`); we pass a full
//! machine/material description on the command line instead of vendor
//! profile names, so nothing depends on a configured data directory. The
//! statistics are the G-code's own comments, written by
//! `GCodeProcessor.cpp` (`; estimated printing time (normal mode) = ...`)
//! and `Print.cpp` (`; filament used [g] = ...`).
//!
//! Bambu printers take a sliced `.gcode.3mf` from Bambu Studio or
//! OrcaSlicer, which are not installed here; for them this writes generic
//! G-code with their bed and nozzle, good for time and grams, and says so.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Printer {
    pub id: &'static str,
    pub name: &'static str,
    /// Build volume, mm (manufacturer specifications).
    pub bed: [f64; 3],
    pub nozzle_mm: f64,
    /// True when the slicer output here is generic G-code, not the
    /// printer's native job format.
    pub generic_gcode: bool,
}

pub const PRINTERS: &[Printer] = &[
    Printer { id: "bambu_a1", name: "Bambu Lab A1", bed: [256.0, 256.0, 256.0], nozzle_mm: 0.4, generic_gcode: true },
    Printer { id: "bambu_a1_mini", name: "Bambu Lab A1 mini", bed: [180.0, 180.0, 180.0], nozzle_mm: 0.4, generic_gcode: true },
    Printer { id: "bambu_p1s", name: "Bambu Lab P1S", bed: [256.0, 256.0, 256.0], nozzle_mm: 0.4, generic_gcode: true },
    Printer { id: "bambu_x1c", name: "Bambu Lab X1 Carbon", bed: [256.0, 256.0, 256.0], nozzle_mm: 0.4, generic_gcode: true },
    Printer { id: "prusa_mk4", name: "Prusa MK4S", bed: [250.0, 210.0, 220.0], nozzle_mm: 0.4, generic_gcode: false },
    Printer { id: "prusa_core_one", name: "Prusa CORE One", bed: [250.0, 220.0, 270.0], nozzle_mm: 0.4, generic_gcode: false },
    Printer { id: "creality_k1", name: "Creality K1", bed: [220.0, 220.0, 250.0], nozzle_mm: 0.4, generic_gcode: false },
    Printer { id: "voron_350", name: "Voron 2.4 350", bed: [350.0, 350.0, 340.0], nozzle_mm: 0.4, generic_gcode: false },
];

/// The smallest-bed printer in [`PRINTERS`] that holds `mesh` as it sits
/// (x, y footprint and height), or `None` when no listed printer does (split
/// the part with `crate::split_to_fit`). Smallest first, so a part is not sent
/// to a Voron that a Prusa could print.
pub fn printer_that_fits(mesh: &crate::Mesh) -> Option<Printer> {
    let s = mesh.bounds().size();
    let mut fits: Vec<Printer> = PRINTERS
        .iter()
        .copied()
        .filter(|p| {
            let (a, b) = (s.x.max(s.y), s.x.min(s.y));
            let (bx, by) = (p.bed[0].max(p.bed[1]), p.bed[0].min(p.bed[1]));
            a <= bx - 2.0 && b <= by - 2.0 && s.z <= p.bed[2] - 1.0
        })
        .collect();
    fits.sort_by(|a, b| (a.bed[0] * a.bed[1]).total_cmp(&(b.bed[0] * b.bed[1])));
    fits.first().copied()
}

pub fn printer(id: &str) -> anyhow::Result<Printer> {
    PRINTERS.iter().copied().find(|p| p.id == id).with_context(|| {
        format!("unknown printer {id:?}; known: {}", PRINTERS.iter().map(|p| p.id).collect::<Vec<_>>().join(", "))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub id: &'static str,
    /// Solid filament density, g/cm^3 (maker TDS).
    pub density: f64,
    /// Extrusion multiplier: below 1 for foaming filaments, which expand in
    /// the nozzle (Bambu's PLA-Aero profile sets flow 0.6; OrcaSlicer
    /// `resources/profiles/BBL/filament/Bambu PLA Aero @BBL X1C.json`, v2.4.2).
    pub flow: f64,
    pub nozzle_c: f64,
    pub bed_c: f64,
    /// Max volumetric speed, mm^3/s.
    pub max_flow_mm3s: f64,
}

pub const MATERIALS: &[Material] = &[
    Material { id: "pla", density: 1.24, flow: 1.0, nozzle_c: 215.0, bed_c: 60.0, max_flow_mm3s: 15.0 },
    // Bambu PLA-Aero: density 1.21 g/cm^3 (`Bambu PLA Aero @base.json`), flow
    // 0.6 and 6 mm^3/s (`@BBL X1C.json`), 220-260 C nozzle, 35-45 C bed (store page)
    Material { id: "pla_aero", density: 1.21, flow: 0.6, nozzle_c: 240.0, bed_c: 40.0, max_flow_mm3s: 6.0 },
    Material { id: "petg", density: 1.27, flow: 1.0, nozzle_c: 245.0, bed_c: 75.0, max_flow_mm3s: 12.0 },
];

pub fn material(id: &str) -> anyhow::Result<Material> {
    MATERIALS.iter().copied().find(|m| m.id == id).with_context(|| format!("unknown material {id:?}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Recipe {
    /// Single-wall spiral (vase mode): wing and fuselage skins.
    Vase,
    /// Walls and sparse infill: fittings, mounts, brackets.
    Structural { perimeters: u32, infill_pct: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sliced {
    pub gcode: PathBuf,
    pub seconds: f64,
    pub grams: f64,
    pub printer: String,
    pub generic_gcode: bool,
}

/// The PrusaSlicer executable: `WRIGHT_PRUSASLICER`, else the usual
/// install locations.
pub fn prusaslicer() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WRIGHT_PRUSASLICER") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    [
        format!("{home}/Applications/PrusaSlicer.app/Contents/MacOS/PrusaSlicer"),
        "/Applications/PrusaSlicer.app/Contents/MacOS/PrusaSlicer".to_string(),
        "/usr/bin/prusa-slicer".to_string(),
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

/// The memo key of one slice: the STL's bytes, every setting that reaches
/// the command line, and the slicer binary's identity (path, size, mtime:
/// a PrusaSlicer upgrade is a different key). ccache's direct mode keys a
/// compile the same way (input bytes + compiler identity + arguments;
/// `wright_core::cache`).
fn slice_key(exe: &Path, stl: &Path, p: &Printer, m: &Material, recipe: &Recipe) -> Option<String> {
    let bytes = std::fs::read(stl).ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let mut h = wright_core::cache::Hasher::new();
    h.update(&bytes).str(&format!("{p:?}|{m:?}|{recipe:?}|{}|{}|{mtime}|layer0.2|marlin2|v1", exe.display(), meta.len()));
    Some(h.hex())
}

#[derive(Serialize, Deserialize)]
struct Memo {
    seconds: f64,
    grams: f64,
}

/// Slice one STL (already oriented on the bed) to `out`. Memoized by
/// content (`slice_key`, `~/.wright/cache/slice`): an identical part under
/// the same settings and slicer is copied from the cache instead of
/// re-sliced (a balloon rack's four identical posts, the same rack in the
/// search and again in the build). `WRIGHT_CACHE=0` turns it off.
pub fn slice(stl: &Path, out: &Path, p: Printer, m: Material, recipe: Recipe) -> anyhow::Result<Sliced> {
    let Some(exe) = prusaslicer() else {
        bail!("PrusaSlicer not found: install it or set WRIGHT_PRUSASLICER");
    };
    let cache = wright_core::cache::Cache::named("slice");
    let key = slice_key(&exe, stl, &p, &m, &recipe);
    if let Some(k) = &key {
        let gcode = cache.dir.join(format!("{k}.gcode"));
        if let Some(memo) = cache.get::<Memo>(k, None) {
            if std::fs::copy(&gcode, out).is_ok() {
                return Ok(Sliced { gcode: out.to_path_buf(), seconds: memo.seconds, grams: memo.grams, printer: p.name.to_string(), generic_gcode: p.generic_gcode });
            }
        }
    }
    let s = slice_uncached(&exe, stl, out, p, m, recipe)?;
    if let (Some(k), true) = (&key, wright_core::cache::enabled()) {
        if std::fs::create_dir_all(&cache.dir).is_ok() && std::fs::copy(out, cache.dir.join(format!("{k}.gcode"))).is_ok() {
            cache.put(k, &Memo { seconds: s.seconds, grams: s.grams });
        }
    }
    Ok(s)
}

fn slice_uncached(exe: &Path, stl: &Path, out: &Path, p: Printer, m: Material, recipe: Recipe) -> anyhow::Result<Sliced> {
    let bed = format!("0x0,{0}x0,{0}x{1},0x{1}", p.bed[0], p.bed[1]);
    let mut cmd = Command::new(exe);
    cmd.arg("--export-gcode")
        .args(["--bed-shape", &bed])
        .args(["--max-print-height", &p.bed[2].to_string()])
        .args(["--nozzle-diameter", &p.nozzle_mm.to_string()])
        .args(["--filament-diameter", "1.75"])
        .args(["--filament-density", &m.density.to_string()])
        .args(["--extrusion-multiplier", &m.flow.to_string()])
        .args(["--temperature", &m.nozzle_c.to_string()])
        .args(["--first-layer-temperature", &m.nozzle_c.to_string()])
        .args(["--bed-temperature", &m.bed_c.to_string()])
        .args(["--first-layer-bed-temperature", &m.bed_c.to_string()])
        .args(["--filament-max-volumetric-speed", &m.max_flow_mm3s.to_string()])
        .args(["--layer-height", "0.2"])
        .args(["--gcode-flavor", "marlin2"])
        .arg("--center")
        .arg(format!("{},{}", p.bed[0] / 2.0, p.bed[1] / 2.0));
    match recipe {
        Recipe::Vase => {
            // booleans are bare flags on PrusaSlicer's command line
            cmd.args(["--spiral-vase", "--perimeters", "1", "--top-solid-layers", "0", "--fill-density", "0%"]);
        }
        Recipe::Structural { perimeters, infill_pct } => {
            cmd.args(["--perimeters", &perimeters.to_string(), "--fill-density", &format!("{infill_pct}%")]);
        }
    }
    cmd.arg("-o").arg(out).arg(stl);
    let res = run_bounded(&mut cmd, slice_timeout(), &out.with_extension("slicer.log"))
        .with_context(|| format!("PrusaSlicer on {}", stl.display()))?;
    if !res.success || !out.exists() {
        // the slicer's last non-empty line says why
        let last = res.output.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output").to_string();
        bail!("PrusaSlicer failed on {}: {last}", stl.display());
    }
    let text = std::fs::read_to_string(out)?;
    let (seconds, grams) = stats(&text)?;
    Ok(Sliced { gcode: out.to_path_buf(), seconds, grams, printer: p.name.to_string(), generic_gcode: p.generic_gcode })
}

/// How long one slice may take: `WRIGHT_SLICE_TIMEOUT_S`, else 10 minutes.
/// PrusaSlicer has been seen to hang on a mesh (review 5, F10: a probe
/// deadlocked with no limit); a part that does not slice in time is
/// reported as not sliced, and its print checks as missing evidence.
pub fn slice_timeout() -> std::time::Duration {
    let s = std::env::var("WRIGHT_SLICE_TIMEOUT_S").ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(600.0);
    std::time::Duration::from_secs_f64(s)
}

/// What a bounded child process did.
#[derive(Debug)]
pub struct Bounded {
    pub success: bool,
    /// Its stdout then stderr, as text.
    pub output: String,
}

/// Run `cmd` with its output going to `log` (a file, so a chatty child can
/// never block on a full pipe), polling for exit; past `limit` the child is
/// killed and reaped, and the result is an error that says so. The shape
/// of the `wait-timeout` crate's `ChildExt::wait_timeout` (spawn, then poll
/// `try_wait`), with the kill done here.
pub fn run_bounded(cmd: &mut Command, limit: std::time::Duration, log: &Path) -> anyhow::Result<Bounded> {
    let f = std::fs::File::create(log).with_context(|| format!("creating {}", log.display()))?;
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f)
        .spawn()
        .context("starting the process")?;
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(log);
            bail!("timed out after {:.0} s and was killed (WRIGHT_SLICE_TIMEOUT_S)", limit.as_secs_f64());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let output = std::fs::read_to_string(log).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    Ok(Bounded { success: status.success(), output })
}

/// Print time (s) and filament (g) from PrusaSlicer's G-code comments.
pub fn stats(gcode: &str) -> anyhow::Result<(f64, f64)> {
    let mut secs = None;
    let mut grams = None;
    for line in gcode.lines().rev().take(5000) {
        if let Some(v) = line.strip_prefix("; estimated printing time (normal mode) = ") {
            secs = Some(parse_duration(v));
        } else if let Some(v) = line.strip_prefix("; filament used [g] = ") {
            grams = v.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(0.0)).sum::<f64>().into();
        }
    }
    match (secs, grams) {
        (Some(s), Some(g)) => Ok((s, g)),
        _ => bail!("the G-code has no time/filament statistics"),
    }
}

/// "1d 2h 3m 4s" -> seconds.
fn parse_duration(s: &str) -> f64 {
    s.split_whitespace()
        .map(|tok| {
            let (num, unit) = tok.split_at(tok.len() - 1);
            let n: f64 = num.parse().unwrap_or(0.0);
            n * match unit {
                "d" => 86400.0,
                "h" => 3600.0,
                "m" => 60.0,
                _ => 1.0,
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_hung_child_is_killed_at_the_limit_and_named() {
        let log = std::env::temp_dir().join(format!("wright-bounded-{}.log", std::process::id()));
        let t = std::time::Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let e = run_bounded(&mut cmd, std::time::Duration::from_millis(300), &log).unwrap_err();
        assert!(e.to_string().contains("timed out"), "{e}");
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
        let mut ok = Command::new("sh");
        ok.args(["-c", "echo out; echo why >&2; exit 3"]);
        let r = run_bounded(&mut ok, std::time::Duration::from_secs(10), &log).unwrap();
        assert!(!r.success && r.output.contains("out") && r.output.contains("why"));
    }

    use super::*;
    use crate::{shapes, v3};

    #[test]
    fn the_smallest_bed_that_holds_a_part_is_chosen() {
        let ring = shapes::cuboid(v3(0.0, 0.0, 0.0), v3(216.5, 216.5, 25.0));
        let p = printer_that_fits(&ring).unwrap();
        assert!(p.bed[0] >= 218.0 && p.bed[1] >= 218.0, "{}", p.name);
        assert!(printer_that_fits(&shapes::cuboid(v3(0.0, 0.0, 0.0), v3(400.0, 10.0, 10.0))).is_none());
    }

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration("1h 2m 3s"), 3723.0);
        assert_eq!(parse_duration("1d 0h 0m 10s"), 86410.0);
    }

    /// Slices a real part when PrusaSlicer is installed (skips otherwise,
    /// like the prototype's KiCad and ngspice gates).
    #[test]
    fn a_cube_slices_to_plausible_grams() {
        if prusaslicer().is_none() {
            eprintln!("skipped: no PrusaSlicer");
            return;
        }
        let dir = std::env::temp_dir().join(format!("wright-slice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("cube.stl");
        shapes::cuboid(v3(0.0, 0.0, 0.0), v3(20.0, 20.0, 20.0)).write_stl(&stl).unwrap();
        let s = slice(
            &stl,
            &dir.join("cube.gcode"),
            printer("prusa_mk4").unwrap(),
            material("pla").unwrap(),
            Recipe::Structural { perimeters: 2, infill_pct: 15 },
        )
        .unwrap();
        // a 20 mm cube at 2 walls / 15 % infill is a few grams, minutes long
        assert!(s.grams > 1.0 && s.grams < 9.92, "{} g", s.grams);
        assert!(s.seconds > 60.0 && s.seconds < 3.0 * 3600.0, "{} s", s.seconds);
    }
}
