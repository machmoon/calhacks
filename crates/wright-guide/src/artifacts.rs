//! A build guide from any domain's build output, with no domain code: the
//! STL files are the parts (or the assembly the pictures are drawn from),
//! the G-code files are the plates (their time and grams read back from the
//! slicer's own comments), and everything else is a file to hand over. The CLI calls this after every `Domain::build`, so a
//! new vehicle family gets a guide the day it can write an STL.

use crate::guide::{write_guide, Guide, Section};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wright_core::{ArtifactKind, Artifacts, Bom, Finished};
use wright_geom::slice::{stats, Sliced};
use wright_geom::{Mesh, Part, PartKind};

/// The file stem a part and its G-code share ("fin_alignment_jig").
fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every file of `artifacts`, directories expanded (sorted, recursively,
/// dotfiles skipped), each labelled "<dir label>: <file name>".
fn expand(artifacts: &Artifacts) -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for (label, path) in &artifacts.files {
        if path.is_dir() {
            let mut files = Vec::new();
            walk(path, &mut files);
            for f in files {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push((format!("{label}: {name}"), f));
            }
        } else {
            out.push((label.clone(), path.clone()));
        }
    }
    out
}

/// Write `out_dir/guide.html` from `artifacts`.
///
/// What each STL is comes from the build when it says
/// ([`Artifacts::add_kind`]); otherwise a G-code sharing its stem makes it
/// printed, a name with "jig"/"fixture" makes it a printed tool, and a name
/// with "assembly" makes it the assembly. Anything else is a guess, so it
/// is listed as "not sliced" with that said, never as "bought" (review 4,
/// finding 13). When there is an assembly, pictures are drawn from it and
/// the printed pieces (laid out for the bed, not in the vehicle's frame)
/// are listed under prints only. Directory artifacts (the aircraft's
/// `print/`) are expanded: the guide once read only top-level files and
/// came out with 0 parts and blank pictures (finding 6).
pub fn from_artifacts(
    title: &str,
    summary: &str,
    artifacts: &Artifacts,
    bom: &Bom,
    checks: &Finished,
    mut before_use: Option<Section>,
    out_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let mut stls: BTreeMap<String, (String, PathBuf)> = BTreeMap::new();
    let mut gcodes: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut wiring = None;
    for (label, path) in expand(artifacts) {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("stl") => {
                stls.insert(stem(&path), (label, path));
            }
            Some("gcode") => {
                gcodes.insert(stem(&path), path);
            }
            Some("svg") if wiring.is_none() && label.to_ascii_lowercase().contains("wir") => wiring = Some(path),
            // convention: `checklist.md` (a `# Title` line, then `- ` items)
            // becomes the guide's last section ("Before launch", ...)
            Some("md") if (stem(&path) == "checklist" || stem(&path).ends_with("_checklist")) && before_use.is_none() => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let title = text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or("Before first use").trim().to_string();
                let items: Vec<String> = text.lines().filter_map(|l| l.trim_start().strip_prefix("- ")).map(str::to_string).collect();
                if items.is_empty() {
                    files.push((label, path));
                } else {
                    before_use = Some(Section::checklist(title, items));
                }
            }
            _ if path.is_file() => files.push((label, path)),
            _ => {}
        }
    }
    let mut model = Vec::new();
    let mut pieces = Vec::new();
    let mut unknown = Vec::new();
    for (name, (label, path)) in &stls {
        let lower = format!("{name} {}", label.to_ascii_lowercase());
        let kind = artifacts.kind_of(path).unwrap_or_else(|| {
            if lower.contains("jig") || lower.contains("fixture") {
                ArtifactKind::PrintedTool
            } else if lower.contains("assembly") {
                ArtifactKind::Assembly
            } else if gcodes.contains_key(name) {
                ArtifactKind::PrintedPart
            } else {
                unknown.push(name.clone());
                ArtifactKind::PrintedPart
            }
        });
        // jigs and fixtures are printed tools, not part of the vehicle: they
        // are listed under prints (their G-code) but kept out of the
        // assembly pictures, where they would sit inside the vehicle
        if kind == ArtifactKind::PrintedTool {
            continue;
        }
        match Mesh::read_stl(path) {
            Ok(mesh) if !mesh.is_empty() => {
                let part = Part {
                    name: name.clone(),
                    kind: if kind == ArtifactKind::PrintedPart { PartKind::Printed } else { PartKind::Bought },
                    material: if kind == ArtifactKind::Assembly { "the whole vehicle, as designed".into() } else { String::new() },
                    mesh,
                    touches: vec![],
                };
                match kind {
                    ArtifactKind::Assembly => model.push(part),
                    _ => pieces.push(part),
                }
            }
            // an unreadable part is named in the guide, never silently dropped
            Ok(_) | Err(_) => files.push((format!("{name} (STL could not be read for pictures)"), path.clone())),
        }
    }
    // with an assembly, the pictures come from it; without one, the parts
    // are in the vehicle's frame and are the pictures
    let (parts, print_parts): (Vec<Part>, Vec<Part>) = if model.is_empty() {
        (pieces, Vec::new())
    } else {
        (model, pieces)
    };
    let mut plates = Vec::new();
    for (name, path) in &gcodes {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if let Ok((seconds, grams)) = stats(&text) {
            plates.push((name.clone(), Sliced { gcode: path.clone(), seconds, grams, printer: String::new(), generic_gcode: false }));
        }
    }
    let mut g = Guide::new(title, &parts, bom, checks);
    g.summary = summary.to_string();
    g.plates = plates;
    g.wiring = wiring;
    g.files = files;
    g.before_use = before_use;
    g.print_parts = print_parts;
    g.kind_unknown = unknown;
    write_guide(&g, out_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{rasterize, Options, View};
    use wright_geom::{shapes::cuboid, v3};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-guide-art-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const GCODE: &str = "; filament used [g] = 12.0\n; estimated printing time (normal mode) = 1h 0m 0s\n";

    fn fact<'a>(html: &'a str, key: &str) -> &'a str {
        let i = html.find(&format!("<dt>{key}</dt><dd>")).unwrap_or_else(|| panic!("no fact {key}")) + key.len() + 13;
        &html[i..i + html[i..].find("</dd>").unwrap()]
    }

    /// Review 4, finding 6: the aircraft's build writes `cad/assembly.stl`
    /// and a `print/` directory; a top-level listing gave a guide with 0
    /// parts and blank pictures. The build's own Artifacts are used now,
    /// directories expanded.
    #[test]
    fn an_assembly_and_a_print_directory_make_a_real_guide() {
        let d = tmp("air");
        std::fs::create_dir_all(d.join("cad")).unwrap();
        std::fs::create_dir_all(d.join("print")).unwrap();
        cuboid(v3(-500.0, -600.0, 0.0), v3(500.0, 600.0, 120.0)).write_stl(&d.join("cad/assembly.stl")).unwrap();
        for piece in ["pod_0", "wing_L_0"] {
            cuboid(v3(0.0, 0.0, 0.0), v3(150.0, 150.0, 100.0)).write_stl(&d.join(format!("print/{piece}.stl"))).unwrap();
            std::fs::write(d.join(format!("print/{piece}.gcode")), GCODE).unwrap();
        }
        std::fs::write(d.join("print/summary.json"), "{}").unwrap();
        let mut a = Artifacts::default();
        a.add("assembly (STL, mm)", d.join("cad/assembly.stl"));
        a.add("printed pieces: 2 STL + G-code", d.join("print"));
        let out = d.join("guide");
        let p = from_artifacts("aircraft build guide", "s", &a, &Bom::default(), &Finished { outcomes: vec![] }, None, &out).unwrap();
        let html = std::fs::read_to_string(p).unwrap();
        assert_eq!(fact(&html, "Parts in the model"), "1");
        assert!(fact(&html, "Print").starts_with("2 plates"), "{}", fact(&html, "Print"));
        assert!(html.contains("<b>pod_0</b>") && html.contains("<b>wing_L_0</b>"));
        // the pictures are of something
        let blank = rasterize(&[], &Options::new(View::Iso, 1200, 760)).png();
        assert_ne!(std::fs::read(out.join("img/cover-iso.png")).unwrap(), blank);
        let _ = std::fs::remove_dir_all(d);
    }

    /// Review 4, finding 13: a printed part nobody sliced was drawn and
    /// listed as "bought" because no G-code shared its name.
    #[test]
    fn a_guessed_kind_is_not_sliced_never_bought_and_a_declared_kind_wins() {
        let d = tmp("kind");
        let cube = |n: &str| {
            cuboid(v3(0.0, 0.0, 0.0), v3(100.0, 20.0, 5.0)).write_stl(&d.join(format!("{n}.stl"))).unwrap();
            d.join(format!("{n}.stl"))
        };
        let mut a = Artifacts::default();
        a.add("STL wing panel (print, vase)", cube("wing_panel"));
        a.add_kind("STL airframe tube", cube("airframe_tube"), ArtifactKind::BoughtPart);
        a.add_kind("STL nose cone", cube("nose_cone"), ArtifactKind::PrintedPart);
        a.add("STL drill jig", cube("drill_jig"));
        std::fs::write(d.join("drill_jig.gcode"), GCODE).unwrap();
        a.add("G-code drill jig", d.join("drill_jig.gcode"));
        let p = from_artifacts("t", "s", &a, &Bom::default(), &Finished { outcomes: vec![] }, None, &d.join("guide")).unwrap();
        let html = std::fs::read_to_string(p).unwrap();
        let row = |name: &str| {
            let i = html.find(&format!("<b>{name}</b>")).unwrap_or_else(|| panic!("{name} not listed"));
            &html[i..i + html[i..].find("</tr>").or_else(|| html[i..].find("</li>")).unwrap()]
        };
        // the unknown one: in the prints table as not sliced, with the reason
        assert!(html.contains("<tr class=\"unsliced\"><td><img class=\"thumb\" src=\"img/print-wing-panel.png\""), "wing_panel is not in the prints table");
        assert!(row("wing_panel").contains("did not say whether this part is printed or bought"));
        // the declared printed one: not sliced, without the guess note
        assert!(row("nose_cone").contains("not sliced") && !row("nose_cone").contains("did not say"));
        // the declared bought one is not a print
        assert!(!html.contains("img/print-airframe-tube.png"));
        assert!(html.contains("<b>airframe_tube</b> <span class=\"small\">(bought"));
        // the jig is a plate, not a part
        assert_eq!(fact(&html, "Parts in the model"), "3");
        let _ = std::fs::remove_dir_all(d);
    }
}
