//! Dev tool: turn a baseline-placed `.kicad_pcb` into a placement fixture
//! (`tests/fixtures/<name>.baseline.json`): every footprint's pads and courtyard
//! in its own frame, the pad -> net names, and the Python baseline's pose and Edge.Cuts
//! outline. The placer tests then place the *same* footprints the baseline placed,
//! so the area and decoupling comparison is like for like.
//!
//! `cargo run -p wright-place --example baseline_fixture -- <board.kicad_pcb> <name> <out_dir>`
//!
//! Courtyard: the bbox of `F.CrtYd` graphics, as the baseline's `kicad.py
//! _courtyard_points` reads it; pad bbox + 0.25 mm when there is none.

use serde::Serialize;
use wright_eda::sexpr::Sexpr;
use wright_eda::{Footprint, Nm, Pad, PadShape, Pt, Rect};

#[derive(Serialize)]
struct Part {
    refdes: String,
    value: String,
    at: Pt,
    rot_deg: f64,
    footprint: Footprint,
    pad_nets: Vec<(String, String)>,
}

#[derive(Serialize)]
struct Fixture {
    name: String,
    source: String,
    /// The baseline's Edge.Cuts bbox.
    outline: Rect,
    parts: Vec<Part>,
}

fn pt(s: &Sexpr) -> Option<Pt> {
    Some(Pt::mm(s.num(1)?, s.num(2)?))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "usage: baseline_fixture <board.kicad_pcb> <name> <out_dir>");
    let text = std::fs::read_to_string(&args[1])?;
    let root = Sexpr::parse(&text)?;
    let mut edge: Option<Rect> = None;
    for g in root.children("gr_line").chain(root.children("gr_rect")) {
        if g.child("layer").and_then(|l| l.text(1)) != Some("Edge.Cuts") {
            continue;
        }
        let (Some(a), Some(b)) = (g.child("start").and_then(pt), g.child("end").and_then(pt)) else { continue };
        let r = Rect::from_corners(a, b);
        edge = Some(edge.map_or(r, |e| e.union(&r)));
    }
    let mut parts = Vec::new();
    for f in root.children("footprint") {
        let lib_id = f.text(1).unwrap_or_default().to_string();
        let at = f.child("at").ok_or_else(|| anyhow::anyhow!("footprint without at"))?;
        let rot_deg = at.num(3).unwrap_or(0.0);
        let prop = |name: &str| {
            f.children("property").find(|p| p.text(1) == Some(name)).and_then(|p| p.text(2)).unwrap_or_default().to_string()
        };
        let mut pads = Vec::new();
        let mut pad_nets = Vec::new();
        for p in f.children("pad") {
            let number = p.text(1).unwrap_or_default().to_string();
            let kind = p.text(2).unwrap_or_default();
            let shape = match p.text(3).unwrap_or_default() {
                "rect" => PadShape::Rect,
                "roundrect" => PadShape::RoundRect(p.child("roundrect_rratio").and_then(|r| r.num(1)).unwrap_or(0.25)),
                "circle" => PadShape::Circle,
                "oval" => PadShape::Oval,
                _ => PadShape::Other,
            };
            let pa = p.child("at").ok_or_else(|| anyhow::anyhow!("pad without at"))?;
            let size = p.child("size").ok_or_else(|| anyhow::anyhow!("pad without size"))?;
            // A board file stores the pad angle with the footprint's own
            // rotation added; take it back out.
            let prot = (pa.num(3).unwrap_or(0.0) - rot_deg).rem_euclid(360.0);
            let drill = p.child("drill").and_then(|d| d.num(1)).map(Nm::mm);
            pads.push(Pad {
                number: number.clone(),
                at: Pt::mm(pa.num(1).unwrap_or(0.0), pa.num(2).unwrap_or(0.0)),
                rot_deg: prot,
                size: (Nm::mm(size.num(1).unwrap_or(0.0)), Nm::mm(size.num(2).unwrap_or(0.0))),
                shape,
                drill,
                plated: kind == "thru_hole",
                layers: Vec::new(),
            });
            if let Some(n) = p.child("net").and_then(|n| n.text(2)) {
                if !n.is_empty() && !n.starts_with("unconnected-") {
                    pad_nets.push((number, n.to_string()));
                }
            }
        }
        let mut crt: Option<Rect> = None;
        for g in f.items() {
            if !matches!(g.head(), Some("fp_line" | "fp_rect" | "fp_poly" | "fp_circle" | "fp_arc")) {
                continue;
            }
            if g.child("layer").and_then(|l| l.text(1)) != Some("F.CrtYd") {
                continue;
            }
            let mut pts: Vec<Pt> = ["start", "end", "mid", "center"].iter().filter_map(|k| g.child(k).and_then(pt)).collect();
            if let Some(ps) = g.child("pts") {
                pts.extend(ps.children("xy").filter_map(pt));
            }
            if g.head() == Some("fp_circle") {
                if let (Some(c), Some(e)) = (g.child("center").and_then(pt), g.child("end").and_then(pt)) {
                    let r = Nm(c.dist(e).ceil() as i64);
                    pts.push(Pt { x: c.x - r, y: c.y - r });
                    pts.push(Pt { x: c.x + r, y: c.y + r });
                }
            }
            for q in pts {
                let r = Rect::from_corners(q, q);
                crt = Some(crt.map_or(r, |c| c.union(&r)));
            }
        }
        let courtyard = crt.unwrap_or_else(|| {
            let b = pads.iter().map(Pad::local_bbox).reduce(|a, b| a.union(&b)).unwrap_or_default();
            b.inflate(Nm::mm(0.25))
        });
        let model3d = f.child("model").and_then(|m| m.text(1)).map(str::to_string);
        parts.push(Part {
            refdes: prop("Reference"),
            value: prop("Value"),
            at: Pt::mm(at.num(1).unwrap_or(0.0), at.num(2).unwrap_or(0.0)),
            rot_deg,
            footprint: Footprint { lib_id, pads, courtyard, model3d, source: String::new() },
            pad_nets,
        });
    }
    let fx = Fixture { name: args[2].clone(), source: args[1].clone(), outline: edge.unwrap_or_default(), parts };
    let out = std::path::Path::new(&args[3]).join(format!("{}.baseline.json", args[2]));
    std::fs::write(&out, serde_json::to_string_pretty(&fx)?)?;
    eprintln!("wrote {} ({} parts)", out.display(), fx.parts.len());
    Ok(())
}
