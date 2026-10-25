//! Offline structure checks on every baseline fixture, plus the authority:
//! `kicad-cli sch erc --severity-error` on each one (skipped when KiCad is
//! not installed). The connectivity check re-derives pin positions from the
//! emitted file itself (library pin + instance `at`, Y flipped), not from
//! the emitter's own structs, so it cannot share the emitter's frame bug.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use wright_eda::{Circuit, PartKind, Rect};

fn fixtures() -> Vec<(String, Netlist)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/electronics/fixtures");
    let mut v: Vec<(String, Netlist)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "json").then(|| {
                let n = Circuit::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap().validate().unwrap();
                (p.file_stem().unwrap().to_string_lossy().into_owned(), n)
            })
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(v.len() >= 7, "expected the 7 baseline fixtures");
    v
}

/// Footprint ids as lane A will name them; only `lib_id` matters here.
fn footprints(n: &Netlist) -> Vec<Footprint> {
    n.parts
        .iter()
        .map(|p| {
            let lib_id = match (p.kind, p.package.as_deref()) {
                (_, Some(pk)) if pk.contains(':') => pk.to_string(),
                (PartKind::Resistor, _) => "Resistor_SMD:R_0603_1608Metric".into(),
                (PartKind::Capacitor, _) => "Capacitor_SMD:C_0603_1608Metric".into(),
                (PartKind::Inductor, _) => "Inductor_SMD:L_1210_3225Metric".into(),
                (PartKind::Diode, _) => "Diode_SMD:D_SOD-123".into(),
                (PartKind::Led, _) => "LED_SMD:LED_0603_1608Metric".into(),
                (_, Some(pk)) => format!("wright:{pk}"),
                (_, None) => format!("wright:{}", p.name),
            };
            Footprint { lib_id, pads: vec![], courtyard: Rect::default(), model3d: None, source: String::new() }
        })
        .collect()
}

fn nm(s: &Sexpr, i: usize) -> i64 {
    (s.num(i).unwrap() * 1e6).round() as i64
}

#[test]
fn every_fixture_reparses_and_is_deterministic() {
    for (name, n) in fixtures() {
        let fps = footprints(&n);
        let a = write_schematic(&n, &fps, &name).unwrap();
        assert_eq!(a, write_schematic(&n, &fps, &name).unwrap(), "{name}: not byte-stable");
        let s = Sexpr::parse(&a).unwrap();
        assert_eq!(s.head(), Some("kicad_sch"));
        // One instance per part with the footprint lib_id, plus one per flag.
        let insts: Vec<&Sexpr> = s.children("symbol").collect();
        let parts: Vec<&&Sexpr> = insts.iter().filter(|i| i.child("lib_id").unwrap().text(1) != Some(symbol::FLAG_LIB_ID)).collect();
        assert_eq!(parts.len(), n.parts.len(), "{name}");
        for (p, fp) in n.parts.iter().zip(&fps) {
            let inst = parts.iter().find(|i| i.children("property").any(|q| q.text(1) == Some("Reference") && q.text(2) == Some(p.refdes.as_str()))).unwrap_or_else(|| panic!("{name}: no symbol {}", p.refdes));
            let fpp = inst.children("property").find(|q| q.text(1) == Some("Footprint")).unwrap();
            assert_eq!(fpp.text(2), Some(fp.lib_id.as_str()));
            let r = inst.child("instances").unwrap().child("project").unwrap().child("path").unwrap().child("reference").unwrap();
            assert_eq!(r.text(1), Some(p.refdes.as_str()));
        }
        let nc_pins: usize = n.parts.iter().flat_map(|p| &p.pins).filter(|p| p.no_connect).count();
        assert_eq!(s.children("no_connect").count(), nc_pins, "{name}");
    }
}

/// Every connected pin, located from the file's own lib pin and instance,
/// meets a wire whose far end carries a global label with the pin's net
/// name; NC pins carry a no_connect flag; no two pins share a point.
#[test]
fn every_pin_reaches_its_net_label_on_the_sheet() {
    for (name, n) in fixtures() {
        let s = Sexpr::parse(&write_schematic(&n, &footprints(&n), &name).unwrap()).unwrap();
        let mut lib_pins: BTreeMap<String, Vec<(String, i64, i64)>> = BTreeMap::new();
        for sym in s.child("lib_symbols").unwrap().children("symbol") {
            let id = sym.text(1).unwrap().to_string();
            for unit in sym.children("symbol") {
                for pin in unit.children("pin") {
                    let a = pin.child("at").unwrap();
                    lib_pins.entry(id.clone()).or_default().push((pin.child("number").unwrap().text(1).unwrap().to_string(), nm(a, 1), nm(a, 2)));
                }
            }
        }
        let wires: Vec<((i64, i64), (i64, i64))> = s
            .children("wire")
            .map(|w| {
                let p: Vec<&Sexpr> = w.child("pts").unwrap().children("xy").collect();
                ((nm(p[0], 1), nm(p[0], 2)), (nm(p[1], 1), nm(p[1], 2)))
            })
            .collect();
        let labels: BTreeMap<(i64, i64), String> = s
            .children("global_label")
            .map(|l| {
                let a = l.child("at").unwrap();
                ((nm(a, 1), nm(a, 2)), l.text(1).unwrap().to_string())
            })
            .collect();
        let ncs: BTreeSet<(i64, i64)> = s.children("no_connect").map(|c| (nm(c.child("at").unwrap(), 1), nm(c.child("at").unwrap(), 2))).collect();
        let mut seen = BTreeSet::new();
        for inst in s.children("symbol") {
            let id = inst.child("lib_id").unwrap().text(1).unwrap();
            if id == symbol::FLAG_LIB_ID {
                continue;
            }
            let a = inst.child("at").unwrap();
            let (ax, ay) = (nm(a, 1), nm(a, 2));
            let refdes = inst.children("property").find(|q| q.text(1) == Some("Reference")).unwrap().text(2).unwrap();
            let part = n.part(refdes).unwrap();
            assert_eq!(lib_pins[id].len(), part.pins.len(), "{name} {refdes}: pin count");
            for (num, px, py) in &lib_pins[id] {
                let p = (ax + px, ay - py); // independent Y flip
                assert!(p.0 % GRID == 0 && p.1 % GRID == 0, "{name} {refdes}.{num} off grid");
                assert!(seen.insert(p), "{name} {refdes}.{num}: two pins at one point");
                let pin = part.pins.iter().find(|q| &q.pad == num).unwrap();
                match pin.net {
                    Some(k) => {
                        let w = wires.iter().find(|w| w.0 == p || w.1 == p).unwrap_or_else(|| panic!("{name} {refdes}.{num}: no wire"));
                        let far = if w.0 == p { w.1 } else { w.0 };
                        assert_eq!(labels.get(&far).map(String::as_str), Some(n.nets[k].name.as_str()), "{name} {refdes}.{num}");
                    }
                    None => assert!(ncs.contains(&p), "{name} {refdes}.{num}: NC pin without flag"),
                }
            }
        }
        // Every label name is a board net name exactly (no `/` prefix).
        let names: BTreeSet<&str> = n.nets.iter().map(|x| x.name.as_str()).collect();
        assert!(labels.values().all(|l| names.contains(l.as_str())), "{name}");
    }
}

#[test]
fn power_flags_go_on_undriven_supply_nets_only() {
    let (_, ldo) = fixtures().into_iter().find(|f| f.0 == "ldo").unwrap();
    let s = Sexpr::parse(&write_schematic(&ldo, &footprints(&ldo), "ldo").unwrap()).unwrap();
    let flagged: BTreeSet<String> = s
        .children("global_label")
        .filter(|l| l.child("uuid").is_some())
        .filter_map(|l| {
            let net = l.text(1)?.to_string();
            let seed = uuid_for(&format!("ldo/flag/{net}/label"));
            (l.child("uuid")?.text(1)? == seed).then_some(net)
        })
        .collect();
    // AMS1117 VOUT is a power_out, so +3V3 is driven; VIN and GND are not.
    assert_eq!(flagged, ["GND", "VIN"].iter().map(|x| x.to_string()).collect());
}

#[test]
fn bad_input_is_refused_with_every_problem() {
    let (_, ldo) = fixtures().into_iter().find(|f| f.0 == "ldo").unwrap();
    let mut bad = ldo.clone();
    bad.parts[0].pins[0].net = Some(99);
    let e = write_schematic(&bad, &[], "x").unwrap_err().to_string();
    assert!(e.contains("footprints for") && e.contains("past the"), "{e}");
}

fn kicad_cli() -> Option<PathBuf> {
    let p = std::env::var_os("WRIGHT_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"));
    p.exists().then_some(p)
}

/// `kicad-cli sch erc` on `sch`; returns (errors, all violation types).
pub(crate) fn erc(cli: &Path, sch: &Path, severity: &str) -> (usize, Vec<String>) {
    let json = sch.with_extension("erc.json");
    let out = std::process::Command::new(cli).args(["sch", "erc", "--format", "json", severity, "-o"]).arg(&json).arg(sch).output().unwrap();
    assert!(json.exists(), "kicad-cli wrote no report: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    let mut types = Vec::new();
    let mut errors = 0;
    for sheet in v["sheets"].as_array().into_iter().flatten() {
        for viol in sheet["violations"].as_array().into_iter().flatten() {
            if viol["severity"] == "error" {
                errors += 1;
            }
            types.push(format!("{}:{}", viol["severity"].as_str().unwrap_or("?"), viol["type"].as_str().unwrap_or("?")));
        }
    }
    (errors, types)
}

#[test]
fn every_fixture_passes_kicad_erc() {
    let Some(cli) = kicad_cli() else {
        eprintln!("kicad-cli not found: ERC test skipped");
        return;
    };
    // WRIGHT_SCH_KEEP=<dir> keeps the written sheets for inspection.
    let keep = std::env::var_os("WRIGHT_SCH_KEEP").map(PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("wright-sch-erc-{}", std::process::id())));
    let mut failures = Vec::new();
    for (name, n) in fixtures() {
        let d = dir.join(&name);
        std::fs::create_dir_all(&d).unwrap();
        let sch = d.join(format!("{name}.kicad_sch"));
        std::fs::write(&sch, write_schematic(&n, &footprints(&n), &name).unwrap()).unwrap();
        let (errors, types) = erc(&cli, &sch, "--severity-error");
        eprintln!("{name}: ERC errors {errors} {types:?}");
        if errors > 0 {
            failures.push(format!("{name}: {types:?}"));
        }
    }
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(failures.is_empty(), "ERC errors: {failures:#?}");
}

/// A floating supply pin is caught by KiCad, not hidden by the writer: drop
/// the GND net's flag-worthy pins' net and ERC must complain. Guards against
/// a writer that silences ERC by flagging everything.
#[test]
fn erc_still_catches_an_undeclared_open_pin() {
    let Some(cli) = kicad_cli() else { return };
    let (_, mut ldo) = fixtures().into_iter().find(|f| f.0 == "ldo").unwrap();
    // AMS1117 GND pin off its net and not declared NC: written as dangling.
    let gnd = ldo.parts[0].pins.iter().position(|p| p.name == "GND").unwrap();
    ldo.parts[0].pins[gnd].net = None;
    ldo.parts[0].pins[gnd].no_connect = true; // allowed through the writer...
    let text = write_schematic(&ldo, &footprints(&ldo), "ldo").unwrap();
    // ...then strip the no_connect flag so the pin is genuinely dangling.
    let s = Sexpr::parse(&text).unwrap();
    let kept: Vec<Sexpr> = s.items().iter().filter(|c| c.head() != Some("no_connect")).cloned().collect();
    let dir = std::env::temp_dir().join(format!("wright-sch-erc-neg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sch = dir.join("ldo.kicad_sch");
    std::fs::write(&sch, Sexpr::List(kept).to_kicad()).unwrap();
    let (errors, types) = erc(&cli, &sch, "--severity-error");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(errors > 0, "ERC missed a dangling power pin: {types:?}");
}

/// The schematic half of parity: KiCad's own netlist export of each sheet
/// has exactly the input's nets, by bare name, with exactly its pins. Any
/// other net KiCad reports must be its `unconnected-(...)` net for a
/// declared no-connect pin.
#[test]
fn kicad_netlist_export_matches_the_netlist() {
    let Some(cli) = kicad_cli() else { return };
    let dir = std::env::temp_dir().join(format!("wright-sch-net-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, n) in fixtures() {
        let sch = dir.join(format!("{name}.kicad_sch"));
        std::fs::write(&sch, write_schematic(&n, &footprints(&n), &name).unwrap()).unwrap();
        let net = dir.join(format!("{name}.net"));
        let out = std::process::Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadsexpr", "-o"]).arg(&net).arg(&sch).output().unwrap();
        assert!(net.exists(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        let s = Sexpr::parse(&std::fs::read_to_string(&net).unwrap()).unwrap();
        let mut got: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        for e in s.child("nets").unwrap().children("net") {
            let nm = e.child("name").unwrap().text(1).unwrap().to_string();
            let nodes = e.children("node").map(|d| (d.child("ref").unwrap().text(1).unwrap().to_string(), d.child("pin").unwrap().text(1).unwrap().to_string())).collect();
            got.insert(nm, nodes);
        }
        for (i, net) in n.nets.iter().enumerate() {
            let want: BTreeSet<(String, String)> = net.pins.iter().map(|p| (n.parts[p.part].refdes.clone(), p.pad.clone())).collect();
            assert_eq!(got.remove(&net.name), Some(want), "{name}: net {i} {}", net.name);
        }
        let nc: usize = n.parts.iter().flat_map(|p| &p.pins).filter(|p| p.no_connect).count();
        assert!(got.keys().all(|k| k.starts_with("unconnected-")), "{name}: extra nets {:?}", got.keys().collect::<Vec<_>>());
        assert_eq!(got.len(), nc, "{name}: one unconnected net per declared NC pin");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
