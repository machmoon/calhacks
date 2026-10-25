//! kicad-cli DRC on routed boards, the authority on copper. Gated: skipped
//! when kicad-cli or the library is missing. The servo fixture is always
//! checked; `WRIGHT_ROUTE_DRC_BOARDS` may name extra `board.json` files
//! (`(netlist, board, route)`, lane E's output) whose router copper is
//! stripped and re-routed here.

mod common;
use std::path::{Path, PathBuf};
use wright_eda::{Board, Netlist, RouteReport};
use wright_route::{route, RouteOptions};

/// Non-silkscreen DRC findings (errors and warnings) and unconnected count.
fn drc(netlist: &Netlist, board: &Board, dir: &Path) -> Option<(Vec<String>, usize)> {
    let cli = wright_kicad::kicad_cli();
    if !cli.exists() {
        return None;
    }
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("board.kicad_pcb"), wright_kicad::write_pcb(netlist, board, "board").unwrap()).unwrap();
    std::fs::write(dir.join("board.kicad_pro"), wright_kicad::write_project(netlist, board, "board").unwrap()).unwrap();
    let out = dir.join("drc.json");
    let st = std::process::Command::new(&cli)
        .args(["pcb", "drc", "--format", "json", "--refill-zones", "--severity-all", "-o"])
        .arg(&out)
        .arg(dir.join("board.kicad_pcb"))
        .output()
        .unwrap();
    assert!(out.exists(), "kicad-cli drc wrote nothing: {}", String::from_utf8_lossy(&st.stderr));
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let mut found = Vec::new();
    for v in j["violations"].as_array().unwrap() {
        let ty = v["type"].as_str().unwrap_or("");
        if ty.starts_with("silk") || ty == "lib_footprint_issues" || ty == "lib_footprint_mismatch" {
            continue;
        }
        let items: Vec<String> = v["items"].as_array().unwrap().iter().map(|i| i["description"].as_str().unwrap_or("").to_string()).collect();
        found.push(format!("{} {}: {}", v["severity"].as_str().unwrap_or(""), ty, items.join(" / ")));
    }
    Some((found, j["unconnected_items"].as_array().map(|a| a.len()).unwrap_or(0)))
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("wright-route-drc-{}-{name}", std::process::id()))
}

#[test]
fn servo_and_ldo_boards_pass_kicad_drc_with_no_copper_findings() {
    let share = wright_kicad::kicad_share();
    if !share.join("footprints").is_dir() || !wright_kicad::kicad_cli().exists() {
        eprintln!("skipped: kicad-cli or the KiCad library not found");
        return;
    }
    // servo: wide nets on SOIC PowerPAD; ldo: SOT-223 with two pads "2"
    // (pin and tab) and roundrect corners KiCad does not count as copper.
    for name in ["servo_power_6v5a", "ldo"] {
        let nl = common::fixture(name);
        let fps = wright_kicad::resolve_footprints(&nl, &share).unwrap();
        let mut b = common::shelf(&nl, fps, wright_eda::Nm::mm(4.3));
        b.classes = common::classes(&nl, &b.rules);
        let rep = route(&nl, &mut b, &RouteOptions::default());
        assert_eq!(rep.completion(), 1.0, "{name}: {:?}", rep.unrouted);
        let (found, unconnected) = drc(&nl, &b, &tmp(name)).unwrap();
        for f in &found {
            println!("  {name}: {f}");
        }
        assert!(found.is_empty(), "{name}: {} DRC findings", found.len());
        assert_eq!(unconnected, 0, "{name}: KiCad unconnected while the router says routed");
    }
}

#[test]
fn demo_boards_reroute_clean() {
    let Ok(list) = std::env::var("WRIGHT_ROUTE_DRC_BOARDS") else {
        return;
    };
    for (k, path) in list.split(':').filter(|s| !s.is_empty()).enumerate() {
        let (nl, mut b, _): (Netlist, Board, RouteReport) = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        b.tracks.clear();
        b.vias.clear();
        b.zones.clear();
        let rep = route(&nl, &mut b, &RouteOptions::default());
        let Some((found, unconnected)) = drc(&nl, &b, &tmp(&format!("demo{k}"))) else {
            return;
        };
        println!("{path}: {:.0}% routed, {} vias, {unconnected} unconnected, {} findings", rep.completion() * 100.0, b.vias.len(), found.len());
        for f in &found {
            println!("  {f}");
        }
        assert!(found.is_empty() && unconnected == 0 && rep.completion() == 1.0, "{path}");
    }
}
