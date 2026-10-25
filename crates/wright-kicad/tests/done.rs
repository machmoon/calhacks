//! Lane A done-test (docs/electronics-plan.md §6): every fixture resolves to
//! KiCad library footprints; a hand-placed R_0603 board written by
//! `write_pcb` passes `kicad-cli pcb drc` with 0 errors and 0 unconnected;
//! written output re-parses; a STEP file is exported. Gated: skipped (with a
//! message) when KiCad is not installed.

use std::path::{Path, PathBuf};
use wright_eda::sexpr::Sexpr;
use wright_eda::{Board, Circuit, Layer, NetClass, Netlist, Nm, Placed, Pt, Rect, Rules, Side, Track};

fn have_kicad() -> bool {
    let ok = wright_kicad::kicad_cli().exists() && wright_kicad::kicad_share().join("footprints").is_dir();
    if !ok {
        eprintln!("KiCad not installed: skipping");
    }
    ok
}

fn fixtures() -> Vec<(String, Netlist)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/electronics/fixtures");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().to_string(), Circuit::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap().validate().unwrap()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn out_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn default_class() -> NetClass {
    let r = Rules::default();
    NetClass { name: "Default".into(), track_width: Nm::mm(0.25), clearance: r.min_clearance, via_diameter: r.min_via_diameter, via_drill: r.min_via_drill, nets: vec![] }
}

#[test]
fn all_seven_fixtures_resolve_to_library_footprints() {
    if !have_kicad() {
        return;
    }
    let fx = fixtures();
    assert!(fx.len() >= 7, "expected 7 fixtures, found {}", fx.len());
    for (name, n) in &fx {
        let fps = wright_kicad::resolve_footprints(n, &wright_kicad::kicad_share()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fps.len(), n.parts.len());
        for (p, f) in n.parts.iter().zip(&fps) {
            assert!(!f.source.is_empty(), "{name} {}: no library source", p.refdes);
            for pin in &p.pins {
                assert!(f.pads.iter().any(|x| x.number == pin.pad), "{name} {}: pad {}", p.refdes, pin.pad);
            }
        }
        eprintln!("{name}: {}", n.parts.iter().zip(&fps).map(|(p, f)| format!("{}={}", p.refdes, f.lib_id)).collect::<Vec<_>>().join(" "));
    }
}

#[test]
fn unknown_parts_are_batched_into_one_error() {
    if !have_kicad() {
        return;
    }
    let c = Circuit::from_json(
        r#"{"devices": {"NOPE9000": {"pins": {"A": "1", "B": "2"}}, "J1": {"kind": "connector", "package": "PinHeader_1x02_P2.54mm", "pins": {"1": "1", "2": "2", "3": "3"}, "no_connect": ["3"]}},
            "nets": {"X": ["NOPE9000.A", "J1.1"], "Y": ["NOPE9000.B", "J1.2"]}}"#,
    )
    .unwrap();
    let n = c.validate().unwrap();
    let e = wright_kicad::resolve_footprints(&n, &wright_kicad::kicad_share()).unwrap_err().to_string();
    assert!(e.contains("NOPE9000") && e.contains("no KiCad symbol"), "{e}");
    assert!(e.contains("J1") && e.contains("no pad for pins 3=3"), "{e}");
}

/// Two R_0603s in KiCad's own footprint, rotated 90 degrees, joined by two
/// tracks; optionally a third on the bottom side wired on B.Cu.
fn r_pair(bottom: bool) -> (Netlist, Board) {
    let n = Circuit::from_json(
        r#"{"passives": {"r1": {"type": "resistor", "value": "10k"}, "r2": {"type": "resistor", "value": "4k7"}, "r3": {"type": "resistor", "value": "1k"}},
            "nets": {"A": ["r1.1", "r2.1", "r3.1"], "B": ["r1.2", "r2.2", "r3.2"]}}"#,
    )
    .unwrap()
    .validate()
    .unwrap();
    let fps = wright_kicad::resolve_footprints(&n, &wright_kicad::kicad_share()).unwrap();
    assert_eq!(fps[0].lib_id, "Resistor_SMD:R_0603_1608Metric");
    let place = |part: usize, at: Pt, rot: i32, side: Side| Placed { part, footprint: fps[part].clone(), at, rot, side, locked: false };
    let mut parts = vec![place(0, Pt::mm(5.0, 5.0), 1, Side::Top), place(1, Pt::mm(8.0, 5.0), 1, Side::Top)];
    let a = n.net_index("A").unwrap();
    let b = n.net_index("B").unwrap();
    let pad = |p: &Placed, num: &str| p.pad_abs(p.footprint.pads.iter().find(|x| x.number == num).unwrap());
    let w = Nm::mm(0.25);
    let mut tracks = vec![
        Track { a: pad(&parts[0], "1"), b: pad(&parts[1], "1"), width: w, layer: Layer::FCu, net: a },
        Track { a: pad(&parts[0], "2"), b: pad(&parts[1], "2"), width: w, layer: Layer::FCu, net: b },
    ];
    // rot 1 puts pad 1 at +y: the two tracks are horizontal at y = 5 +- 0.825.
    assert_eq!(tracks[0].a, Pt::mm(5.0, 5.825));
    let mut vias = vec![];
    if bottom {
        // r3 on the bottom, unrotated: mirrored, pad 1 at +x.
        let p3 = place(2, Pt::mm(11.0, 5.0), 0, Side::Bottom);
        assert_eq!(pad(&p3, "1"), Pt::mm(11.825, 5.0));
        let via_a = Pt::mm(11.825, 7.5);
        let via_b = Pt::mm(10.175, 2.5);
        vias.push(wright_eda::Via { at: via_a, diameter: Nm::mm(0.6), drill: Nm::mm(0.3), net: a });
        vias.push(wright_eda::Via { at: via_b, diameter: Nm::mm(0.6), drill: Nm::mm(0.3), net: b });
        tracks.push(Track { a: pad(&p3, "1"), b: via_a, width: w, layer: Layer::BCu, net: a });
        tracks.push(Track { a: pad(&p3, "2"), b: via_b, width: w, layer: Layer::BCu, net: b });
        // F.Cu from the top resistors' pads to the vias.
        tracks.push(Track { a: pad(&parts[1], "1"), b: Pt::mm(9.5, 5.825), width: w, layer: Layer::FCu, net: a });
        tracks.push(Track { a: Pt::mm(9.5, 5.825), b: via_a, width: w, layer: Layer::FCu, net: a });
        tracks.push(Track { a: pad(&parts[1], "2"), b: Pt::mm(8.0, 2.5), width: w, layer: Layer::FCu, net: b });
        tracks.push(Track { a: Pt::mm(8.0, 2.5), b: via_b, width: w, layer: Layer::FCu, net: b });
        parts.push(p3);
    } else {
        // r3 top side, rotated 180, between nothing: wired with its own pair.
        let p3 = place(2, Pt::mm(11.0, 5.0), 2, Side::Top);
        assert_eq!(pad(&p3, "1"), Pt::mm(11.825, 5.0));
        tracks.push(Track { a: pad(&parts[1], "1"), b: Pt::mm(11.825, 5.825), width: w, layer: Layer::FCu, net: a });
        tracks.push(Track { a: Pt::mm(11.825, 5.825), b: pad(&p3, "1"), width: w, layer: Layer::FCu, net: a });
        tracks.push(Track { a: pad(&parts[1], "2"), b: Pt::mm(10.175, 4.175), width: w, layer: Layer::FCu, net: b });
        tracks.push(Track { a: Pt::mm(10.175, 4.175), b: pad(&p3, "2"), width: w, layer: Layer::FCu, net: b });
        parts.push(p3);
    }
    let board = Board { outline: Rect::from_corners(Pt::mm(0.0, 0.0), Pt::mm(15.0, 10.0)), parts, tracks, vias, zones: vec![], classes: vec![default_class()], rules: Rules::default(), holes: vec![(Pt::mm(1.5, 8.5), Nm::mm(1.0))] };
    (n, board)
}

fn write(dir: &Path, n: &Netlist, b: &Board) -> PathBuf {
    let pcb = dir.join("board.kicad_pcb");
    let text = wright_kicad::write_pcb(n, b, "wright lane A test").unwrap();
    // Re-parses with our own reader, and twice gives the same bytes.
    let e = Sexpr::parse(&text).unwrap();
    assert_eq!(e.head(), Some("kicad_pcb"));
    assert_eq!(e.children("footprint").count(), b.parts.len() + b.holes.len());
    assert_eq!(text, wright_kicad::write_pcb(n, b, "wright lane A test").unwrap());
    std::fs::write(&pcb, text).unwrap();
    std::fs::write(dir.join("board.kicad_pro"), wright_kicad::write_project(n, b, "board").unwrap()).unwrap();
    pcb
}

fn assert_clean(v: &wright_kicad::CliVerdict) {
    eprintln!("{} ran={} version={:?} errors={} warnings={} unconnected={}\n  {}", v.check, v.ran, v.tool_version, v.errors, v.warnings, v.unconnected, v.details.join("\n  "));
    assert!(v.ran, "{v:?}");
    assert_eq!(v.errors, 0, "{v:?}");
    assert_eq!(v.unconnected, 0, "{v:?}");
}

#[test]
fn r0603_pair_board_passes_kicad_drc_and_exports_step() {
    if !have_kicad() {
        return;
    }
    let dir = out_dir("r_pair");
    let (n, b) = r_pair(false);
    let pcb = write(&dir, &n, &b);
    let v = wright_kicad::drc(&pcb);
    assert_clean(&v);
    assert!(v.report.as_ref().is_some_and(|r| r.exists()));
    let step = wright_kicad::export_step(&pcb, &dir).unwrap();
    let size = std::fs::metadata(&step).unwrap().len();
    eprintln!("STEP {} ({size} bytes)", step.display());
    assert!(size > 10_000, "STEP too small: {size}");
    let head = std::fs::read_to_string(&step).unwrap();
    assert!(head.starts_with("ISO-10303-21"));
    let svg = wright_kicad::export_pcb_svg(&pcb, &dir).unwrap();
    let text = std::fs::read_to_string(&svg).unwrap();
    eprintln!("SVG {} ({} bytes)", svg.display(), text.len());
    assert!(text.contains("<svg"), "{}", &text[..text.len().min(200)]);
    assert!(!pcb.with_extension("kicad_prl").exists(), "export left a .kicad_prl");
}

#[test]
fn bottom_side_parts_land_where_to_board_says() {
    if !have_kicad() {
        return;
    }
    let dir = out_dir("r_pair_bottom");
    let (n, b) = r_pair(true);
    let pcb = write(&dir, &n, &b);
    assert_clean(&wright_kicad::drc(&pcb));
}

#[test]
fn verify_without_a_schematic_is_unverified_for_erc_and_parity() {
    if !have_kicad() {
        return;
    }
    let dir = out_dir("no_sch");
    let (n, b) = r_pair(false);
    let pcb = write(&dir, &n, &b);
    let v = wright_kicad::verify(&pcb, &dir.join("missing.kicad_sch"));
    assert_eq!(v.iter().map(|x| x.check.as_str()).collect::<Vec<_>>(), ["erc", "drc", "parity"]);
    assert!(!v[0].ran && !v[2].ran);
    assert!(v[1].ran);
}

/// Every fixture's footprints, shelf-packed (no copper), load in KiCad and
/// raise no DRC error; nets are unconnected, which is the router's job.
#[test]
fn every_fixture_board_loads_in_kicad_without_drc_errors() {
    if !have_kicad() {
        return;
    }
    for (name, n) in fixtures() {
        let fps = wright_kicad::resolve_footprints(&n, &wright_kicad::kicad_share()).unwrap();
        let gap = Nm::mm(1.0);
        let (mut x, mut y, mut row_h) = (Nm::mm(2.0), Nm::mm(2.0), Nm(0));
        let mut parts = Vec::new();
        for (i, f) in fps.iter().enumerate() {
            // Alternate rotations to exercise the transform on real parts.
            let rot = (i % 2) as i32;
            let p0 = Placed { part: i, footprint: f.clone(), at: Pt::new(0, 0), rot, side: Side::Top, locked: false };
            let c = p0.courtyard_abs();
            if x + c.width() > Nm::mm(80.0) {
                x = Nm::mm(2.0);
                y = y + row_h + gap;
                row_h = Nm(0);
            }
            let at = Pt { x: x - c.min.x, y: y - c.min.y };
            parts.push(Placed { at, ..p0 });
            x = x + c.width() + gap;
            row_h = row_h.max(c.height());
        }
        let outline = Rect::from_corners(Pt::new(0, 0), Pt { x: Nm::mm(84.0), y: y + row_h + Nm::mm(2.0) });
        let b = Board { outline, parts, tracks: vec![], vias: vec![], zones: vec![], classes: vec![default_class()], rules: Rules::default(), holes: vec![] };
        let dir = out_dir(&format!("fixture_{name}"));
        let pcb = write(&dir, &n, &b);
        let v = wright_kicad::drc(&pcb);
        eprintln!("{name}: ran={} errors={} warnings={} unconnected={}", v.ran, v.errors, v.warnings, v.unconnected);
        assert!(v.ran, "{name}: {v:?}");
        assert_eq!(v.errors, 0, "{name}: {:#?}", v.details);
    }
}

#[test]
fn schematic_svg_export_writes_one_file() {
    if !have_kicad() {
        return;
    }
    let dir = out_dir("sch_svg");
    let sch = dir.join("board.kicad_sch");
    std::fs::write(&sch, "(kicad_sch (version 20250114) (generator \"wright\") (uuid \"6c7a0d1e-0000-4000-8000-000000000001\") (paper \"A4\") (lib_symbols) (text \"wright\" (at 50 50 0) (effects (font (size 1.27 1.27)))) (sheet_instances (path \"/\" (page \"1\"))))\n").unwrap();
    let svg = wright_kicad::export_sch_svg(&sch, &dir).unwrap();
    assert_eq!(svg, dir.join("board.sch.svg"));
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
}
