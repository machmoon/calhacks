//! Re-place a board from a `wright electronics design` run and print what
//! the anneal paid for: `cargo run --release -p wright-place --example
//! explain -- RUN/board.json [groups.json]`.
use wright_eda::{Board, Footprint, Netlist, PartKind, Rules};
use wright_place::{decap, place_candidates, MountingHoles, PlaceOptions};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let v: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&args[1])?)?;
    let n: Netlist = serde_json::from_value(v[0].clone())?;
    let b: Board = serde_json::from_value(v[1].clone())?;
    let mut fps: Vec<(usize, Footprint)> = b.parts.iter().map(|p| (p.part, p.footprint.clone())).collect();
    fps.sort_by_key(|f| f.0);
    let fps: Vec<Footprint> = fps.into_iter().map(|f| f.1).collect();
    let mut opts = PlaceOptions { mounting_holes: Some(MountingHoles::m3()), ..PlaceOptions::default() };
    if let Some(g) = args.get(2) {
        opts.groups = Some(serde_json::from_str(&std::fs::read_to_string(g)?)?);
    } else {
        // wright-electronics pipeline.rs edge_hints, copied for the probe.
        opts.edge = n
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Connector)
            .map(|p| {
                let s = if p.name.contains("BAT") || p.name.ends_with("_IN") { "left" } else if p.name.contains("SIG") { "top" } else if p.name.ends_with("_OUT") { "right" } else { "bottom" };
                (p.refdes.clone(), s.to_string())
            })
            .collect();
    }
    let out = place_candidates(&n, &fps, &Rules::default(), &opts)?;
    let (board, r) = &out[0];
    println!("seed {} {} cost {:.1} area {:.0} ({} x {} mm) loop {:.1} mm, {:.2} s", r.seed, r.start, r.cost, r.area_mm2, board.outline.width().as_mm(), board.outline.height().as_mm(), r.decoupling_loop_mm, r.seconds);
    for (k, c) in &r.terms {
        println!("  {k:12} {c:9.2}");
    }
    let dc = decap::decoupling_caps(&n);
    for (d, l) in dc.iter().zip(decap::loops_nm(board, &dc)) {
        println!("  loop {} -> {}.{}: {:.2} mm", n.parts[d.cap].refdes, n.parts[d.ic].refdes, d.ic_supply_pad, l / 1e6);
    }
    for p in &board.parts {
        let c = p.courtyard_abs();
        println!("  {:4} rot {} [{:6.2} {:6.2}] - [{:6.2} {:6.2}]", n.parts[p.part].refdes, p.rot, c.min.x.as_mm(), c.min.y.as_mm(), c.max.x.as_mm(), c.max.y.as_mm());
    }
    for h in &board.holes {
        println!("  hole ({:.2}, {:.2})", h.0.x.as_mm(), h.0.y.as_mm());
    }
    for n2 in &r.notes {
        println!("  note: {n2}");
    }
    Ok(())
}
