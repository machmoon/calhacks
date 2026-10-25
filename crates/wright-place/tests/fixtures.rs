//! Lane C on the Python baseline's own eval boards. `tests/fixtures/<name>.baseline.json` holds
//! the footprints the baseline placed (pads, courtyards, pad nets) and the baseline's poses,
//! extracted from its fresh 10/24 eval output by `examples/baseline_fixture.rs`,
//! so Wright places exactly what the baseline placed and the numbers compare like for
//! like. The circuits are `evals/electronics/fixtures/*.json`.

use std::path::{Path, PathBuf};
use wright_eda::{Board, Circuit, Footprint, Netlist, Placed, Pt, Rules, Side};
use wright_place::{decoupling_loop_mm, legal, place, PlaceOptions};

/// The baseline's `board_eval.py` numbers, 10/24 22:05 (docs/electronics-plan.md 2):
/// (name, area mm^2 of its placement box, decoupling loop mm, seconds).
const BASELINE: [(&str, f64, f64, f64); 6] = [
    ("ldo", 140.6, 19.9, 5.3),
    ("blinker", 172.3, 12.4, 21.5),
    ("blinker10", 312.8, 13.1, 24.4),
    ("ref25", 116.5, 13.3, 11.8),
    ("two_chip_usb", 305.6, 38.6, 22.9),
    ("esp32_devboard", 1525.7, 60.9, 100.5),
];
const ALL: [&str; 7] = ["ldo", "blinker", "blinker10", "ref25", "two_chip_usb", "esp32_devboard", "servo_power_6v5a"];

#[derive(serde::Deserialize)]
struct BaselinePart {
    refdes: String,
    value: String,
    at: Pt,
    rot_deg: f64,
    footprint: Footprint,
    pad_nets: Vec<(String, String)>,
}

#[derive(serde::Deserialize)]
struct BaselineBoard {
    outline: wright_eda::Rect,
    parts: Vec<BaselinePart>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The netlist, the footprints in part order, and the baseline's board of them.
fn load(name: &str) -> (Netlist, Vec<Footprint>, Board) {
    let c = std::fs::read_to_string(root().join(format!("../../evals/electronics/fixtures/{name}.json"))).unwrap();
    let netlist = Circuit::from_json(&c).unwrap().validate().unwrap();
    let a: BaselineBoard = serde_json::from_str(&std::fs::read_to_string(root().join(format!("tests/fixtures/{name}.baseline.json"))).unwrap()).unwrap();
    let sig = |pairs: &mut Vec<(String, String)>| {
        pairs.sort();
        pairs.dedup();
        pairs.clone()
    };
    let mut used = vec![false; a.parts.len()];
    let mut fps = Vec::new();
    let mut baseline_parts = Vec::new();
    for (i, p) in netlist.parts.iter().enumerate() {
        let mut mine: Vec<(String, String)> = p.pins.iter().filter_map(|pin| pin.net.map(|n| (pin.pad.clone(), netlist.nets[n].name.clone()))).collect();
        let mine = sig(&mut mine);
        let theirs: Vec<Vec<(String, String)>> = a.parts.iter().map(|q| sig(&mut q.pad_nets.clone())).collect();
        // The baseline numbered refs in its spec's order (scripts/board_eval.py),
        // which the key-sorted fixture JSON lost; two ESP32 caps are
        // otherwise indistinguishable (same value, same nets).
        let pinned = match (name, p.name.as_str()) {
            ("esp32_devboard", "C_ESP") => Some("C3"),
            ("esp32_devboard", "C_CH") => Some("C4"),
            _ => None,
        };
        let j = (0..a.parts.len())
            .find(|&j| !used[j] && pinned.is_some_and(|r| a.parts[j].refdes == r))
            .or_else(|| (0..a.parts.len()).find(|&j| !used[j] && theirs[j] == mine && a.parts[j].value == p.value))
            .or_else(|| (0..a.parts.len()).find(|&j| !used[j] && theirs[j] == mine))
            .or_else(|| (0..a.parts.len()).find(|&j| !used[j] && mine.iter().all(|x| theirs[j].contains(x))))
            // Same nets on swapped pads (a later pin map put an LED's
            // cathode on pad 1, as KiCad's LED footprints number it).
            .or_else(|| {
                let nets = |v: &[(String, String)]| {
                    let mut n: Vec<String> = v.iter().map(|x| x.1.clone()).collect();
                    n.sort();
                    n
                };
                (0..a.parts.len()).find(|&j| !used[j] && a.parts[j].value == p.value && nets(&theirs[j]) == nets(&mine))
            })
            .unwrap_or_else(|| panic!("{name}: no baseline footprint for {} {mine:?}", p.refdes));
        used[j] = true;
        let q = &a.parts[j];
        fps.push(q.footprint.clone());
        baseline_parts.push(Placed { part: i, footprint: q.footprint.clone(), at: q.at, rot: (q.rot_deg / 90.0).round() as i32, side: Side::Top, locked: false });
    }
    let baseline = Board { outline: a.outline, parts: baseline_parts, tracks: vec![], vias: vec![], zones: vec![], classes: vec![], rules: Rules::default(), holes: vec![] };
    (netlist, fps, baseline)
}

#[test]
fn the_loop_metric_reproduces_the_baseline_eval_on_its_boards() {
    for (name, _, loop_mm, _) in BASELINE {
        let (n, _, baseline) = load(name);
        let got = decoupling_loop_mm(&n, &baseline);
        let dc = wright_place::decoupling_caps(&n);
        for (d, l) in dc.iter().zip(wright_place::decap::loops_nm(&baseline, &dc)) {
            eprintln!("{name}: {} -> {}.{} / {:?}: {:.2} mm", n.parts[d.cap].refdes, n.parts[d.ic].refdes, d.ic_supply_pad, d.ic_ground_pad, l / 1e6);
        }
        assert!((got - loop_mm).abs() < 0.11, "{name}: the baseline reported {loop_mm} mm, this metric reads {got:.2} mm on the baseline's board");
    }
}

fn check_legal(name: &str, b: &Board, spacing: i64) {
    assert_eq!(legal::courtyard_overlaps(&b.parts), 0, "{name}: courtyard overlap");
    assert!(legal::conflicts(&b.parts, spacing).is_empty(), "{name}: spacing violated");
    let ec = b.rules.edge_clearance.0;
    for p in &b.parts {
        let c = p.courtyard_abs();
        assert!(c.min.x.0 >= b.outline.min.x.0 + ec && c.min.y.0 >= b.outline.min.y.0 + ec, "{name}: part {} past the edge clearance", p.part);
        assert!(c.max.x.0 <= b.outline.max.x.0 - ec && c.max.y.0 <= b.outline.max.y.0 - ec, "{name}: part {} past the edge clearance", p.part);
    }
    assert_eq!(b.classes[0].name, "Default");
}

#[test]
fn no_overlaps_on_any_fixture_over_seeds() {
    for name in ALL {
        let (n, fps, _) = load(name);
        for seed in 1..=5 {
            let opts = PlaceOptions { seed, max_moves: 40_000, ..PlaceOptions::default() };
            let (b, r) = place(&n, &fps, &Rules::default(), &opts).unwrap();
            assert_eq!(r.overlaps, 0);
            check_legal(name, &b, opts.spacing_nm);
        }
    }
}

#[test]
fn shelf_alone_is_legal_on_every_fixture() {
    for name in ALL {
        let (n, fps, _) = load(name);
        let opts = PlaceOptions { max_moves: 0, ..PlaceOptions::default() };
        let (b, r) = place(&n, &fps, &Rules::default(), &opts).unwrap();
        assert_eq!(r.strategy, "shelf");
        check_legal(name, &b, opts.spacing_nm);
    }
}

#[test]
fn same_seed_same_board() {
    for name in ["esp32_devboard", "servo_power_6v5a"] {
        let (n, fps, _) = load(name);
        let opts = PlaceOptions { seed: 7, ..PlaceOptions::default() };
        let (a, ra) = place(&n, &fps, &Rules::default(), &opts).unwrap();
        let (b, rb) = place(&n, &fps, &Rules::default(), &opts).unwrap();
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap(), "{name}");
        assert_eq!(ra.cost.to_bits(), rb.cost.to_bits());
        let (c, _) = place(&n, &fps, &Rules::default(), &PlaceOptions { seed: 8, ..opts }).unwrap();
        assert_ne!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&c).unwrap(), "{name}: the seed is ignored");
    }
}

#[test]
fn fixed_outline_is_respected() {
    let (n, fps, _) = load("blinker");
    let opts = PlaceOptions { outline: Some((20_000_000, 16_000_000)), ..PlaceOptions::default() };
    let (b, _) = place(&n, &fps, &Rules::default(), &opts).unwrap();
    assert_eq!((b.outline.width().0, b.outline.height().0), (20_000_000, 16_000_000));
    check_legal("blinker fixed", &b, opts.spacing_nm);
    let tiny = PlaceOptions { outline: Some((5_000_000, 5_000_000)), ..PlaceOptions::default() };
    assert!(place(&n, &fps, &Rules::default(), &tiny).is_err(), "eight parts do not fit in 5x5 mm");
}

/// The baseline vs Wright table. Area: Wright's whole Edge.Cuts rectangle (edge
/// clearance included) against the baseline's placement box, which excludes the baseline's 2 mm
/// margin, so the comparison is graded in the baseline's favour.
#[test]
fn beats_baseline_on_area_and_decoupling() {
    eprintln!("| fixture | baseline area mm2 (box / Edge.Cuts) | Wright area mm2 | baseline loop mm | Wright loop mm | baseline s | Wright s | moves |");
    eprintln!("|---|---|---|---|---|---|---|---|");
    for name in ALL {
        let (n, fps, baseline) = load(name);
        let (b, r) = place(&n, &fps, &Rules::default(), &PlaceOptions::default()).unwrap();
        check_legal(name, &b, PlaceOptions::default().spacing_nm);
        let baseline_cut = baseline.outline.width().as_mm() * baseline.outline.height().as_mm();
        let baseline_loop = decoupling_loop_mm(&n, &baseline);
        let row = BASELINE.iter().find(|a| a.0 == name);
        let (a_area, a_s) = row.map_or((f64::NAN, f64::NAN), |a| (a.1, a.3));
        eprintln!("| {name} | {a_area:.1} / {baseline_cut:.1} | {:.1} | {baseline_loop:.1} | {:.1} | {a_s} | {:.2} | {} |", r.area_mm2, r.decoupling_loop_mm, r.seconds, r.moves);
        if name == "esp32_devboard" {
            assert!(r.area_mm2 < 1525.7, "ESP32 area {:.1} mm2 does not beat the baseline's 1525.7", r.area_mm2);
            assert!(r.decoupling_loop_mm < 60.9, "ESP32 loop {:.1} mm does not beat the baseline's 60.9", r.decoupling_loop_mm);
        }
        if !cfg!(debug_assertions) {
            assert!(r.seconds < 2.0, "{name}: {:.2} s", r.seconds);
        }
    }
}

#[test]
fn candidates_are_ranked_legal_and_at_most_six() {
    let (n, fps, _) = load("two_chip_usb");
    let c = wright_place::place_candidates(&n, &fps, &Rules::default(), &PlaceOptions { max_moves: 60_000, ..PlaceOptions::default() }).unwrap();
    assert!(!c.is_empty() && c.len() <= wright_place::JUDGED_SEEDS);
    for w in c.windows(2) {
        assert!(w[0].1.cost <= w[1].1.cost);
    }
    for (b, r) in &c {
        assert_eq!(r.strategy, "anneal");
        check_legal("two_chip_usb candidate", b, PlaceOptions::default().spacing_nm);
    }
    let seeds: std::collections::BTreeSet<u64> = c.iter().map(|x| x.1.seed).collect();
    assert_eq!(seeds.len(), c.len(), "one candidate per seed");
}

#[test]
fn servo_buck_template_is_fitted_and_kept_rigid() {
    let (n, fps, _) = load("servo_power_6v5a");
    let (b, r) = place(&n, &fps, &Rules::default(), &PlaceOptions::default()).unwrap();
    let note = r.notes.iter().find(|x| x.starts_with("template U1")).unwrap_or_else(|| panic!("no template note: {:?}", r.notes));
    assert!(note.contains("fitted") && note.contains("C3"), "{note}");
    // Every buck support part ends within a few mm of the chip.
    let u1 = b.parts.iter().find(|p| n.parts[p.part].refdes == "U1").unwrap().courtyard_abs();
    for refdes in ["C1", "C2", "C3", "R1", "R2"] {
        let p = b.parts.iter().find(|p| n.parts[p.part].refdes == refdes).unwrap().courtyard_abs();
        let gap = (p.min.x.0 - u1.max.x.0).max(u1.min.x.0 - p.max.x.0).max(0) + (p.min.y.0 - u1.max.y.0).max(u1.min.y.0 - p.max.y.0).max(0);
        assert!(gap < 6_000_000, "{refdes} is {} mm from U1", gap as f64 / 1e6);
    }
}

#[test]
fn groups_doc_is_validated_like_upstream() {
    use wright_place::intent::{Group, GroupsDoc};
    let (n, fps, _) = load("ldo");
    let g = |name: &str, refs: &[&str]| Group { name: name.into(), refs: refs.iter().map(|s| s.to_string()).collect(), ..Group::default() };
    // C2 in no group, U9 unknown, C1 twice.
    let bad = GroupsDoc { schema: 1, board: None, note: None, groups: vec![g("a", &["U1", "C1"]), g("b", &["C1", "U9"])] };
    let e = place(&n, &fps, &Rules::default(), &PlaceOptions { groups: Some(bad), ..PlaceOptions::default() }).unwrap_err().to_string();
    assert!(e.contains("no group: C2") && e.contains("U9") && e.contains("more than one group"), "{e}");
    let mut core = g("core", &["U1", "C1", "C2"]);
    core.edge = Some("W".into());
    let good = GroupsDoc { schema: 1, board: None, note: None, groups: vec![core] };
    let (b, r) = place(&n, &fps, &Rules::default(), &PlaceOptions { groups: Some(good), ..PlaceOptions::default() }).unwrap();
    assert!(r.notes.iter().any(|x| x.starts_with("group core")));
    check_legal("ldo grouped", &b, PlaceOptions::default().spacing_nm);
    // Unknown keys are refused, as grouping.py validate_doc does.
    assert!(serde_json::from_str::<GroupsDoc>(r#"{"groups": [{"name": "a", "refs": ["U1"], "weights": 1}]}"#).is_err());
}

#[test]
fn fixed_poses_are_held_exactly() {
    use wright_place::intent::FixedPose;
    let (n, fps, _) = load("esp32_devboard");
    let mut opts = PlaceOptions { max_moves: 60_000, ..PlaceOptions::default() };
    opts.intent.fixed_poses = vec![FixedPose { refdes: "J1".into(), at: [5.0, 20.0], rot: 270 }];
    let (b, _) = place(&n, &fps, &Rules::default(), &opts).unwrap();
    let j1 = b.parts.iter().find(|p| n.parts[p.part].refdes == "J1").unwrap();
    assert_eq!((j1.at, j1.rot, j1.locked), (Pt::mm(5.0, 20.0), 3, true));
    check_legal("esp32 fixed J1", &b, opts.spacing_nm);
}
