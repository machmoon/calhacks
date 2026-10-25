//! validate -> footprints -> net classes -> place -> route, then build
//! (write + kicad-cli + STEP + BOM). The stages are the lane crates; this
//! file only wires them (baseline `agents/pipeline.py` `generate_pcb` order).

use crate::{templates, Design, Spec};
use std::path::{Path, PathBuf};
use wright_core::{Artifacts, Manifest};
use wright_eda::ir::PartKind;
use wright_eda::Netlist;

/// The circuit a spec asks for.
pub fn circuit(spec: &Spec) -> anyhow::Result<wright_eda::Circuit> {
    match (&spec.template, &spec.circuit) {
        (Some(t), _) => templates::expand(t),
        (None, Some(c)) => Ok(c.clone()),
        (None, None) => anyhow::bail!("spec names neither a template nor a circuit"),
    }
}

/// Connectors on the board edge: the battery plug on the left, a signal
/// header on top, servo/output headers along the bottom (baseline `board.py`
/// edge hints did the same for connectors).
pub fn edge_hints(n: &Netlist) -> Vec<(String, String)> {
    n.parts
        .iter()
        .filter(|p| p.kind == PartKind::Connector)
        .map(|p| {
            let side = if p.name.contains("BAT") || p.name.ends_with("_IN") {
                "left"
            } else if p.name.contains("SIG") {
                "top"
            } else if p.name.ends_with("_OUT") {
                "right"
            } else {
                "bottom"
            };
            (p.refdes.clone(), side.to_string())
        })
        .collect()
}

/// What the seed judge did (KiCadRoutingTools `py_placer/judge_seeds.py`:
/// quick-route each candidate, rank by `quick_score.judge_key`, route the
/// winner in full; the quick board is never shipped).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JudgeReport {
    /// Candidate placements judged (lane C returns the 6 cheapest of 12 seeds).
    pub candidates: usize,
    /// Index into the candidates of the one kept.
    pub kept: usize,
    /// Its placement seed and start (`scattered` / `clustered`).
    pub kept_seed: u64,
    pub ranking: Vec<wright_route::quick::Judged>,
    pub seconds: f64,
}

pub fn design(spec: &Spec) -> anyhow::Result<Design> {
    let circuit = circuit(spec)?;
    let netlist = circuit.validate()?;
    let footprints = wright_kicad::resolve_footprints(&netlist, &wright_kicad::kicad_share())?;
    let rules = wright_eda::Rules::default();
    let net_current_a = templates::net_currents(spec.template.as_ref(), &netlist);
    // edge hints only when the agent gave no groups (groups carry their own edges)
    let edge = if spec.groups.is_some() { Vec::new() } else { edge_hints(&netlist) };
    let opts = wright_place::PlaceOptions { edge, groups: spec.groups.clone(), intent: spec.intent.clone().unwrap_or_default(),
        // four M3 corner holes when the board mounts in a bay (Lane C)
        mounting_holes: spec.max_board_mm.map(|_| wright_place::MountingHoles::m3()),
        // NOT the bay as a fixed outline: tried 10/25, lane C's placer then
        // refuses ("J2 does not fit inside the 60x40 mm outline")
        ..Default::default() };
    let classes = |b: &mut wright_eda::Board| b.classes = crate::copper::classes_for(&netlist, &net_current_a, &b.rules);
    let full = wright_route::RouteOptions::default();
    if spec.single_seed {
        let (mut board, place) = wright_place::place(&netlist, &footprints, &rules, &opts)?;
        // class widths from the per-net currents (IPC-2221 inverse, 20 C rise)
        classes(&mut board);
        let route = wright_route::route(&netlist, &mut board, &full);
        return Ok(Design { circuit, netlist, board, route, place, net_current_a, judge: None, sourced: None });
    }
    let mut cands = wright_place::place_candidates(&netlist, &footprints, &rules, &opts)?;
    anyhow::ensure!(!cands.is_empty(), "placement produced no candidate");
    for (b, _) in cands.iter_mut() {
        classes(b);
    }
    let t = std::time::Instant::now();
    let boards: Vec<wright_eda::Board> = cands.iter().map(|c| c.0.clone()).collect();
    let (kept, board, route, ranking) = wright_route::judge_and_route(&netlist, &boards, &full).ok_or_else(|| anyhow::anyhow!("seed judge returned nothing"))?;
    let place = cands.swap_remove(kept).1;
    let judge = JudgeReport { candidates: boards.len(), kept, kept_seed: place.seed, ranking, seconds: t.elapsed().as_secs_f64() };
    Ok(Design { circuit, netlist, board, route, place, net_current_a, judge: Some(judge), sourced: None })
}

pub const STEM: &str = "board";

/// `(pcb, sch, pro)` text for a design (deterministic: same design, same bytes).
pub fn kicad_texts(d: &Design) -> anyhow::Result<(String, String, String)> {
    let fps: Vec<_> = d.board.parts.iter().map(|p| p.footprint.clone()).collect();
    // the schematic wants footprints in netlist part order
    let mut by_part = vec![None; d.netlist.parts.len()];
    for (b, f) in d.board.parts.iter().zip(fps) {
        if let Some(slot) = by_part.get_mut(b.part) {
            *slot = Some(f);
        }
    }
    let ordered: Vec<_> = by_part.into_iter().enumerate().map(|(i, f)| f.ok_or_else(|| anyhow::anyhow!("part {i} is not on the board"))).collect::<anyhow::Result<_>>()?;
    Ok((
        wright_kicad::write_pcb(&d.netlist, &d.board, STEM)?,
        wright_sch::write_schematic(&d.netlist, &ordered, STEM)?,
        wright_kicad::write_project(&d.netlist, &d.board, STEM)?,
    ))
}

/// Write the three files into `dir`; returns `(pcb, sch)` paths.
pub fn write_texts(dir: &Path, t: &(String, String, String)) -> anyhow::Result<(PathBuf, PathBuf)> {
    let pcb = dir.join(format!("{STEM}.kicad_pcb"));
    let sch = dir.join(format!("{STEM}.kicad_sch"));
    std::fs::write(&pcb, &t.0)?;
    std::fs::write(&sch, &t.1)?;
    std::fs::write(dir.join(format!("{STEM}.kicad_pro")), &t.2)?;
    Ok((pcb, sch))
}

/// Parts whose footprint names no 3D model, or names one the KiCad library
/// does not ship (as `.step` or `.wrl`): the STEP export leaves them out.
pub fn missing_models(netlist: &Netlist, board: &wright_eda::Board) -> Vec<String> {
    let dir = wright_kicad::kicad_share().join("3dmodels");
    board
        .parts
        .iter()
        .filter_map(|b| {
            let refdes = netlist.parts.get(b.part).map(|p| p.refdes.as_str()).unwrap_or("?");
            let Some(m) = &b.footprint.model3d else { return Some(format!("{refdes} ({}: none named)", b.footprint.lib_id)) };
            // `${KICAD10_3DMODEL_DIR}/Lib.3dshapes/Name.wrl` -> share/3dmodels/...
            let rel = m.split_once('}').map(|(_, r)| r.trim_start_matches('/')).unwrap_or(m);
            let p = dir.join(rel);
            let found = [p.with_extension("step"), p.with_extension("wrl"), p.clone()].iter().any(|q| q.exists());
            (!found).then(|| format!("{refdes} ({rel})"))
        })
        .collect()
}

pub fn build(design: &Design, out: &Path) -> anyhow::Result<Artifacts> {
    std::fs::create_dir_all(out)?;
    let texts = kicad_texts(design)?;
    let (pcb, sch) = write_texts(out, &texts)?;
    let mut a = Artifacts::default();
    a.add("KiCad board", &pcb);
    a.add("KiCad schematic", &sch);
    a.add("KiCad project", out.join(format!("{STEM}.kicad_pro")));
    // the oracle's input (docs/electronics-plan.md section 5, "Board JSON")
    let bj = out.join("board.json");
    std::fs::write(&bj, serde_json::to_string(&(&design.netlist, &design.board, &design.route))?)?;
    a.add("board JSON (netlist, board, route) for the Haskell oracle", &bj);
    // kicad-cli on the files the builder gets
    // the same texts `check` judged, so its cached verdicts (one ERC/DRC
    // pass per design, keyed by the file texts) serve here
    let verdicts = crate::checks::kicad_verdicts(design).unwrap_or_else(|_| wright_kicad::verify(&pcb, &sch));
    let vj = out.join("kicad-verdicts.json");
    std::fs::write(&vj, serde_json::to_string_pretty(&verdicts)?)?;
    a.add("kicad-cli ERC / DRC / parity verdicts", &vj);
    // BOM
    let bom = crate::catalog::bom(&design.netlist, &design.board);
    let bp = out.join("bom.csv");
    std::fs::write(&bp, bom.csv())?;
    a.add("BOM", &bp);
    // board STEP for the CAD side (Astra; docs/cad-handoff.md)
    let cad = out.join("cad");
    std::fs::create_dir_all(&cad)?;
    let step = cad.join("board.step");
    let (w, h) = (design.board.outline.width().as_mm(), design.board.outline.height().as_mm());
    let manifest = match wright_kicad::export_step(&pcb, &step) {
        Ok(p) => {
            let mut m = Manifest::new("cad", "PCB assembly STEP (kicad-cli pcb export step)");
            m.add(&cad, &p, "board.step", &format!("{w:.1} x {h:.1} mm 2-layer board with KiCad library 3D models"), &[("width_mm", w), ("height_mm", h), ("thickness_mm", 1.6)]);
            m.check("step_written", p.exists(), &p.display().to_string());
            let missing = missing_models(&design.netlist, &design.board);
            let detail = if missing.is_empty() { "every footprint's 3D model is in the KiCad library".to_string() } else { format!("no KiCad 3D model, so the STEP has no body for: {}", missing.join("; ")) };
            m.check("component_models", missing.is_empty(), &detail);
            a.add("board STEP (for CAD)", &p);
            m
        }
        Err(e) => Manifest::failed("cad", format!("kicad-cli pcb export step: {e:#}")),
    };
    manifest.write(&cad)?;
    Ok(a)
}
