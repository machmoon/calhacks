//! `kicad-cli` as the verifier: ERC, DRC, schematic parity, STEP export.
//! Replaces the Python baseline's `engine/silkscreen/verify/kicad.py`, which runs it the way
//! atopile does (`src/faebryk/libs/kicad/drc.py::run_drc`: a JSON report,
//! all severities, parsed); counts as the baseline's `scripts/board_eval.py:127
//! kicad_counts` (`sheets[].violations`, `violations`, `unconnected_items`,
//! `schematic_parity`).
//!
//! * KiCad's own `severity` decides error vs warning; nothing is re-ranked.
//! * The types the writer causes by design (library footprints and symbols
//!   are embedded, not looked up in the user's lib tables) are counted as
//!   warnings, never errors: the baseline's `BENIGN_TYPES`.
//! * The tool missing, crashing, timing out or writing no JSON is
//!   `ran: false`: unverified, never a pass.
//! * The check runs on copies in a fresh directory, with the schematic and
//!   project renamed to the board's stem, so KiCad pairs exactly these files
//!   for parity and leaves no `.kicad_prl` beside the caller's board. The
//!   JSON reports are kept beside the board (`<stem>.erc.json`,
//!   `<stem>.drc.json`) as evidence.

use crate::{kicad_cli, CliVerdict};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Per kicad-cli call.
pub const TIMEOUT: Duration = Duration::from_secs(180);

/// Report types the writer produces on purpose (the baseline's `verify/kicad.py`
/// `BENIGN_TYPES`): counted as warnings whatever KiCad's severity.
pub const BENIGN_TYPES: &[&str] = &["lib_symbol_issues", "lib_symbol_mismatch", "footprint_link_issues", "lib_footprint_issues", "lib_footprint_mismatch"];

/// Output of one kicad-cli run, or why there is none.
fn run(args: &[&std::ffi::OsStr], cwd: &Path) -> Result<std::process::Output, String> {
    let cli = kicad_cli();
    if !cli.exists() {
        return Err(format!("kicad-cli not found at {} (install KiCad or set WRIGHT_KICAD_CLI)", cli.display()));
    }
    let mut child = Command::new(&cli).args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("kicad-cli did not start: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| format!("kicad-cli output: {e}")),
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("kicad-cli timed out after {} s", TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("kicad-cli wait: {e}")),
        }
    }
}

/// `kicad-cli version` (cached per process), e.g. `10.0.6`.
pub fn tool_version() -> Option<String> {
    static V: OnceLock<Option<String>> = OnceLock::new();
    V.get_or_init(|| {
        let out = run(&["version".as_ref()], &std::env::temp_dir()).ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !s.is_empty()).then_some(s)
    })
    .clone()
}

fn tail(out: &std::process::Output) -> String {
    let t = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

fn not_run(check: &str, why: String) -> CliVerdict {
    CliVerdict { check: check.into(), ran: false, tool_version: tool_version(), errors: 0, warnings: 0, unconnected: 0, details: vec![why], report: None }
}

/// Count one list of KiCad violations into a verdict.
fn verdict(check: &str, list: &[Value], unconnected: &[Value], report: Option<PathBuf>) -> CliVerdict {
    let (mut errors, mut warnings, mut details) = (0, 0, Vec::new());
    let mut errs = Vec::new();
    let mut warns = Vec::new();
    for v in list {
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("unknown");
        let sev = v.get("severity").and_then(Value::as_str).unwrap_or("error");
        let items: Vec<&str> = v.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("description").and_then(Value::as_str)).collect()).unwrap_or_default();
        let line = format!("{ty}: {}{}", v.get("description").and_then(Value::as_str).unwrap_or(""), if items.is_empty() { String::new() } else { format!(" [{}]", items.join("; ")) });
        if sev == "error" && !BENIGN_TYPES.contains(&ty) {
            errors += 1;
            errs.push(line);
        } else if sev != "exclusion" && sev != "ignore" {
            warnings += 1;
            warns.push(format!("(warning) {line}"));
        }
    }
    for u in unconnected {
        let items: Vec<&str> = u.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("description").and_then(Value::as_str)).collect()).unwrap_or_default();
        errs.push(format!("unconnected_items: {}", items.join(" <-> ")));
    }
    // Errors first; then warnings; capped so a bad board stays readable.
    details.extend(errs.into_iter().take(40));
    details.extend(warns.into_iter().take(20));
    CliVerdict { check: check.into(), ran: true, tool_version: tool_version(), errors, warnings, unconnected: unconnected.len(), details, report }
}

fn read_json(p: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(p).map_err(|e| format!("no report at {}: {e}", p.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("report {} is not JSON: {e}", p.display()))
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> std::io::Result<Scratch> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("wright-kicad-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d)?;
    Ok(Scratch(d))
}

/// Where the reports are kept: beside the board, `<stem>.<check>.json`.
fn keep(src: &Path, pcb: &Path, check: &str) -> Option<PathBuf> {
    let stem = pcb.file_stem()?.to_string_lossy().to_string();
    let dst = pcb.parent().unwrap_or(Path::new(".")).join(format!("{stem}.{check}.json"));
    std::fs::copy(src, &dst).ok().map(|_| dst)
}

/// ERC on the schematic; DRC (zones refilled) and parity on the board from
/// one `pcb drc --schematic-parity` run. Always `[erc, drc, parity]`.
pub fn verify(pcb: &Path, sch: &Path) -> Vec<CliVerdict> {
    let dir = match scratch() {
        Ok(d) => d,
        Err(e) => return ["erc", "drc", "parity"].iter().map(|c| not_run(c, format!("no scratch dir: {e}"))).collect(),
    };
    let stem = "wright_check";
    let (pcb_c, sch_c, pro_c) = (dir.0.join(format!("{stem}.kicad_pcb")), dir.0.join(format!("{stem}.kicad_sch")), dir.0.join(format!("{stem}.kicad_pro")));
    let pcb_ok = std::fs::copy(pcb, &pcb_c).map_err(|e| format!("cannot read board {}: {e}", pcb.display()));
    let sch_ok = std::fs::copy(sch, &sch_c).map_err(|e| format!("cannot read schematic {}: {e}", sch.display()));
    // The project beside the board carries the net classes and rules.
    if let Some(pro) = [pcb.with_extension("kicad_pro"), sch.with_extension("kicad_pro")].into_iter().find(|p| p.exists()) {
        let _ = std::fs::copy(pro, &pro_c);
    }

    let erc = match &sch_ok {
        Err(e) => not_run("erc", e.clone()),
        Ok(_) => {
            let rep = dir.0.join("erc.json");
            match run(&["sch".as_ref(), "erc".as_ref(), "--format".as_ref(), "json".as_ref(), "--severity-all".as_ref(), "-o".as_ref(), rep.as_os_str(), sch_c.as_os_str()], &dir.0) {
                Err(e) => not_run("erc", e),
                Ok(out) => match read_json(&rep) {
                    Err(e) => not_run("erc", format!("{e}; exit {:?}: {}", out.status.code(), tail(&out))),
                    Ok(j) => {
                        let list: Vec<Value> = arr(&j, "sheets").iter().flat_map(|s| arr(s, "violations").to_vec()).collect();
                        verdict("erc", &list, &[], keep(&rep, pcb, "erc"))
                    }
                },
            }
        }
    };

    let (drc, parity) = match &pcb_ok {
        Err(e) => (not_run("drc", e.clone()), not_run("parity", e.clone())),
        Ok(_) => {
            let rep = dir.0.join("drc.json");
            let mut args: Vec<&std::ffi::OsStr> = vec!["pcb".as_ref(), "drc".as_ref(), "--format".as_ref(), "json".as_ref(), "--severity-all".as_ref(), "--refill-zones".as_ref()];
            if sch_ok.is_ok() {
                args.push("--schematic-parity".as_ref());
            }
            args.extend(["-o".as_ref(), rep.as_os_str(), pcb_c.as_os_str()]);
            match run(&args, &dir.0) {
                Err(e) => (not_run("drc", e.clone()), not_run("parity", e)),
                Ok(out) => match read_json(&rep) {
                    Err(e) => {
                        let why = format!("{e}; exit {:?}: {}", out.status.code(), tail(&out));
                        (not_run("drc", why.clone()), not_run("parity", why))
                    }
                    Ok(j) => {
                        let kept = keep(&rep, pcb, "drc");
                        let drc = verdict("drc", arr(&j, "violations"), arr(&j, "unconnected_items"), kept.clone());
                        let parity = match &sch_ok {
                            Err(e) => not_run("parity", e.clone()),
                            Ok(_) => verdict("parity", arr(&j, "schematic_parity"), &[], kept),
                        };
                        (drc, parity)
                    }
                },
            }
        }
    };
    vec![erc, drc, parity]
}

/// DRC alone (no schematic): the verdict for a board on its own.
pub fn drc(pcb: &Path) -> CliVerdict {
    let missing = pcb.with_extension("wright-no-schematic");
    let mut v = verify(pcb, &missing);
    v.swap_remove(1)
}

/// `kicad-cli pcb drc --refill-zones --save-board` on a copy: the board with
/// its pours computed, for viewers and STEP (the baseline's `verify/kicad.py
/// fill_zones`). Writes `out`.
pub fn fill_zones(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    let dir = scratch()?;
    let b = dir.0.join("fill.kicad_pcb");
    std::fs::copy(pcb, &b)?;
    if let Some(pro) = Some(pcb.with_extension("kicad_pro")).filter(|p| p.exists()) {
        std::fs::copy(pro, dir.0.join("fill.kicad_pro"))?;
    }
    let rep = dir.0.join("fill.json");
    let o = run(&["pcb".as_ref(), "drc".as_ref(), "--refill-zones".as_ref(), "--save-board".as_ref(), "--format".as_ref(), "json".as_ref(), "-o".as_ref(), rep.as_os_str(), b.as_os_str()], &dir.0).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(o.status.success(), "kicad-cli fill exited {:?}: {}", o.status.code(), tail(&o));
    std::fs::copy(&b, out)?;
    Ok(out.to_path_buf())
}

fn export(kind: &str, pcb: &Path, out: &Path, ext: &str, extra: &[&str]) -> anyhow::Result<PathBuf> {
    let target = if out.is_dir() { out.join(format!("{}.{ext}", pcb.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "board".into()))) } else { out.to_path_buf() };
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&target);
    let cwd = pcb.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut args: Vec<&std::ffi::OsStr> = vec!["pcb".as_ref(), "export".as_ref(), kind.as_ref()];
    args.extend(extra.iter().map(std::ffi::OsStr::new));
    args.extend(["--force".as_ref(), "-o".as_ref(), target.as_os_str(), pcb.as_os_str()]);
    // KiCad drops a `.kicad_prl` (UI state) beside a board it opens; a
    // caller asking for an export gets no new file (the baseline's `_without_stray_prl`).
    let prl = pcb.with_extension("kicad_prl");
    let had_prl = prl.exists();
    let o = run(&args, &cwd).map_err(anyhow::Error::msg);
    if !had_prl {
        let _ = std::fs::remove_file(&prl);
    }
    let o = o?;
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    anyhow::ensure!(size > 0, "kicad-cli pcb export {kind} wrote no {} (exit {:?}): {}", target.display(), o.status.code(), tail(&o));
    Ok(target)
}

/// `kicad-cli pcb export step --subst-models` (board + component models).
/// `out` is the file to write, or a directory to write `<stem>.step` into.
pub fn export_step(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    export("step", pcb, out, "step", &["--subst-models"])
}

/// `kicad-cli pcb export glb` (binary glTF, for the desktop 3D view).
pub fn export_glb(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    export("glb", pcb, out, "glb", &["--subst-models", "--include-tracks", "--include-pads", "--include-zones"])
}

/// The file to write: `out` itself, or `<out>/<stem>.<ext>` when `out` is a
/// directory.
fn target_for(src: &Path, out: &Path, suffix: &str) -> PathBuf {
    if out.is_dir() {
        out.join(format!("{}{suffix}", src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "board".into())))
    } else {
        out.to_path_buf()
    }
}

/// Runs a kicad-cli export into a scratch directory and moves the one
/// `.svg` it wrote to `target` (kicad-cli names SVGs itself).
/// `single`: `-o` names the file (`pcb export svg --mode-single`), else a
/// directory.
fn svg_via_scratch(args_before_out: &[&str], input: &Path, target: &Path, single: bool) -> anyhow::Result<PathBuf> {
    let dir = scratch()?;
    let o_path = if single { dir.0.join("out.svg") } else { dir.0.clone() };
    let mut args: Vec<&std::ffi::OsStr> = args_before_out.iter().map(std::ffi::OsStr::new).collect();
    args.extend(["-o".as_ref(), o_path.as_os_str(), input.as_os_str()]);
    let cwd = input.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let prl = input.with_extension("kicad_prl");
    let had_prl = prl.exists();
    let o = run(&args, &cwd).map_err(anyhow::Error::msg);
    if !had_prl {
        let _ = std::fs::remove_file(&prl);
    }
    let o = o?;
    let mut svgs: Vec<PathBuf> = std::fs::read_dir(&dir.0)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "svg")).collect();
    svgs.sort();
    let src = svgs.first().ok_or_else(|| anyhow::anyhow!("kicad-cli {} wrote no SVG (exit {:?}): {}", args_before_out.join(" "), o.status.code(), tail(&o)))?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, target)?;
    Ok(target.to_path_buf())
}

/// `kicad-cli pcb export svg` of the given layers (e.g. `["F.Cu", "B.Cu",
/// "Edge.Cuts"]`) as one SVG cropped to the board, zones refilled, no
/// drawing sheet. `out` is the file, or a directory for `<stem>.svg`.
pub fn export_pcb_svg_layers(pcb: &Path, out: &Path, layers: &[&str]) -> anyhow::Result<PathBuf> {
    let target = target_for(pcb, out, ".svg");
    let list = layers.join(",");
    svg_via_scratch(&["pcb", "export", "svg", "--mode-single", "--layers", &list, "--page-size-mode", "2", "--exclude-drawing-sheet", "--check-zones", "--drill-shape-opt", "2"], pcb, &target, true)
}

/// The board's copper view: F.Cu, B.Cu and Edge.Cuts in one SVG.
pub fn export_pcb_svg(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    export_pcb_svg_layers(pcb, out, &["F.Cu", "B.Cu", "Edge.Cuts"])
}

/// `kicad-cli sch export svg` (root sheet). `out` is the file, or a
/// directory for `<stem>.sch.svg`.
pub fn export_sch_svg(sch: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    let target = target_for(sch, out, ".sch.svg");
    svg_via_scratch(&["sch", "export", "svg", "--exclude-drawing-sheet"], sch, &target, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_follow_kicad_severity_and_benign_types() {
        let j: Value = serde_json::from_str(r#"{"violations":[
            {"type":"clearance","severity":"error","description":"Clearance violation","items":[{"description":"Pad 1 of R1"}]},
            {"type":"lib_footprint_issues","severity":"error","description":"lib"},
            {"type":"silk_over_copper","severity":"warning","description":"silk"}],
            "unconnected_items":[{"items":[{"description":"Pad 1 of R1"},{"description":"Pad 2 of R2"}]}]}"#).unwrap();
        let v = verdict("drc", arr(&j, "violations"), arr(&j, "unconnected_items"), None);
        assert!(v.ran);
        assert_eq!((v.errors, v.warnings, v.unconnected), (1, 2, 1));
        assert!(v.details[0].starts_with("clearance: Clearance violation [Pad 1 of R1]"));
        assert!(v.details.iter().any(|d| d.starts_with("unconnected_items: Pad 1 of R1 <-> Pad 2 of R2")));
    }

    #[test]
    fn a_missing_tool_is_unverified_not_a_pass() {
        let v = not_run("erc", "kicad-cli not found".into());
        assert!(!v.ran);
        assert_eq!(v.errors, 0);
        assert_eq!(v.details, ["kicad-cli not found"]);
    }
}
