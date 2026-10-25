//! The agent side of AI placement, after KiCadRoutingTools v0.23.7's
//! `plan-pcb-placement` skill (MIT, github.com/drandyhaas/KiCadRoutingTools,
//! read 2026-10-24: `.claude/skills/plan-pcb-placement/SKILL.md`,
//! `references/part-roles.md`, `references/placement-lessons.md`,
//! `py_tools/board_context.py`, `py_placer/judge_seeds.py`). The split is
//! theirs: the model supplies judgement (functional groups with near /
//! apart / edge hints and a reason each), deterministic code arranges
//! (lane C's annealer over 12 seeds), and the router picks the winner (lane
//! D's quick-route judge), then a verifier re-measures what was claimed.
//!
//! * [`board_context`]: `board_context.py` `build_context` cut to what a
//!   two-layer board needs: per part ref, value, footprint, body size,
//!   role, pads with pin name and net, partners by shared signal nets; per
//!   net role, design current and pins; the decoupling pairs (`serves`).
//! * [`LESSONS`]: the subset of `placement-lessons.md` that applies to a
//!   2-layer power board, as **advisory** text for the prompt. They never
//!   override a check (CLAUDE.md principle 7; the upstream file itself
//!   calls them "targets and checks, not hard limits").
//! * [`verify_claims`]: SKILL.md's single verifier pass, deterministic
//!   here: every group's near/apart/edge claim and every proximity claim
//!   re-measured on the routed board, next to kicad-cli's DRC verdict.

use crate::Design;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wright_eda::ir::{NetRole, PartKind};
use wright_eda::{Footprint, Netlist, Pt};
use wright_place::intent::GroupsDoc;

/// `placement-lessons.md` items that bear on a 2-layer board with a buck
/// stage, connectors and passives (numbers are the upstream file's).
pub const LESSONS: &str = "\
Placement lessons (KiCadRoutingTools references/placement-lessons.md; ADVISORY priors, not rules: \
the checks and kicad-cli DRC decide):
- Place by function: 5-8 groups on a small board, each compact and on one face (62, 63, 65).
- Put power conditioners at the power entry, not at the board edge; no edge hint on the power group (69, 48).
- Cluster a switching regulator's power stage: inductor first, then input cap, output caps, then feedback divider and boot cap (36).
- Inductor against the SW pin, switch-node pad facing it, pad gap <= 2.5 mm on 2 layers (37).
- Input cap about 0.8 mm from VIN with its ground pad toward chip ground; loop target 5 mm on 2 layers (38); input cap and inductor about 90 degrees apart around the chip (39).
- Output caps about 0.8 mm from the inductor's output pad (40); feedback divider at the FB pin, about 0.9 mm (42); boot cap about 1 mm from BOOT, within 2.5 mm of SW (43).
- Asynchronous buck: catch diode within about 2 mm of SW, beside the inductor's switch-node pad (44).
- Do not leave extra room around power parts for wide copper; shorten power nets by clustering instead (46).
- LDO caps about 0.9 mm from their own pins (47). Bulk caps (10 uF+) at the regulator or power entry (33).
- Identical edge connectors as one row at a fixed pitch with about 1 mm gaps (78); about 1 mm mating clearance around headers (79).
- Order power entry from the connector inward: protection, bulk cap, regulator (49).
- Decap distance budget about 4 mm centre to pin (21); one cap per supply pin, place only the caps the schematic has (26).";

/// What the model is told about the board (KRT `board_context.py`).
pub fn board_context(netlist: &Netlist, footprints: &[Footprint], currents: &[f64]) -> Value {
    let decaps = wright_place::decoupling_caps(netlist);
    let parts: Vec<Value> = netlist
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let fp = footprints.get(i);
            let c = fp.map(|f| f.courtyard);
            let pads: Vec<Value> = p.pins.iter().map(|q| json!({"pad": q.pad, "pin": q.name, "net": q.net.map(|n| netlist.nets[n].name.clone()), "no_connect": q.no_connect})).collect();
            let mut partners: Vec<String> = p
                .pins
                .iter()
                .filter_map(|q| q.net)
                .filter(|&n| netlist.nets[n].role == NetRole::Signal)
                .flat_map(|n| netlist.nets[n].pins.iter().map(|r| netlist.parts[r.part].refdes.clone()))
                .filter(|r| r != &p.refdes)
                .collect();
            partners.sort();
            partners.dedup();
            let serves: Vec<String> = decaps.iter().filter(|d| d.cap == i).map(|d| netlist.parts[d.ic].refdes.clone()).collect();
            json!({
                "ref": p.refdes, "name": p.name, "kind": crate::catalog::kind_name(p.kind), "value": p.value,
                "footprint": fp.map(|f| f.lib_id.clone()),
                "body_mm": c.map(|c| [round2(c.width().as_mm()), round2(c.height().as_mm())]),
                "pads": pads, "partners_by_signal_net": partners, "decouples": serves,
            })
        })
        .collect();
    let nets: Vec<Value> = netlist
        .nets
        .iter()
        .enumerate()
        .map(|(i, n)| {
            json!({"name": n.name, "role": format!("{:?}", n.role).to_lowercase(), "design_current_a": round2(currents.get(i).copied().unwrap_or(0.0)),
                   "pins": n.pins.iter().map(|r| format!("{}.{}", netlist.parts[r.part].refdes, r.pad)).collect::<Vec<_>>()})
        })
        .collect();
    json!({"parts": parts, "nets": nets, "copper_layers": 2, "note": "ground and the main supply rail touch almost every part: judge grouping by signal nets (part-roles.md)"})
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The layout agent's system prompt.
pub fn groups_system() -> String {
    format!(
        "You are the layout engineer for a 2-layer PCB. You write the functional groups a deterministic placer \
then arranges (KiCadRoutingTools groups.json schema 1). Every movable part goes in exactly one group; refs must \
be the board's reference designators. Per group: name, role, refs, anchor (a ref in refs, the part the group is \
built around), why (one sentence of engineering reason), optional edge (N/S/E/W, board top is N) for connector \
groups only, optional near / apart (names of other groups). Do not use side B. Judge by nets and pin functions, \
never by designator. Answer only by calling write_groups.\n\n{LESSONS}"
    )
}

pub fn groups_tool() -> Value {
    json!({
        "name": "write_groups",
        "description": "Write the board's functional groups (KiCadRoutingTools groups.json schema 1).",
        "input_schema": {
            "type": "object",
            "required": ["schema", "groups"],
            "properties": {
                "schema": {"type": "integer", "enum": [1]},
                "note": {"type": "string"},
                "groups": {"type": "array", "items": {
                    "type": "object", "required": ["name", "refs", "why"],
                    "properties": {
                        "name": {"type": "string"}, "role": {"type": "string"},
                        "refs": {"type": "array", "items": {"type": "string"}},
                        "anchor": {"type": "string"}, "why": {"type": "string"},
                        "edge": {"type": "string", "enum": ["N", "S", "E", "W"]},
                        "near": {"type": "array", "items": {"type": "string"}},
                        "apart": {"type": "array", "items": {"type": "string"}}
                    }, "additionalProperties": false}}
            },
            "additionalProperties": false
        }
    })
}

/// Parse and validate a model's groups against the netlist with lane C's
/// own rules (`wright_place::intent::resolve`: coverage, unknown refs,
/// anchors, near/apart names). Errors are batched in one message for one
/// repair turn.
pub fn parse_groups(netlist: &Netlist, v: &Value) -> Result<GroupsDoc, String> {
    let g: GroupsDoc = serde_json::from_value(v.clone()).map_err(|e| format!("groups JSON does not match schema 1: {e}"))?;
    wright_place::intent::resolve(netlist, Some(&g), &wright_place::intent::Intent::default()).map_err(|e| format!("{e:#}"))?;
    Ok(g)
}

/// One re-measured claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub claim: String,
    pub measured: String,
    pub holds: bool,
}

fn centroid(d: &Design, refs: &[String]) -> Option<Pt> {
    let pts: Vec<Pt> = refs
        .iter()
        .filter_map(|r| d.netlist.parts.iter().position(|p| &p.refdes == r))
        .filter_map(|i| d.board.parts.iter().find(|b| b.part == i))
        .map(|b| {
            let c = b.courtyard_abs();
            Pt::new((c.min.x.0 + c.max.x.0) / 2, (c.min.y.0 + c.max.y.0) / 2)
        })
        .collect();
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as i64;
    Some(Pt::new(pts.iter().map(|p| p.x.0).sum::<i64>() / n, pts.iter().map(|p| p.y.0).sum::<i64>() / n))
}

/// SKILL.md's verifier: re-measure the agent's claims on the routed board.
/// A `near` pair holds when the two groups' centroids are closer than the
/// median distance between all group pairs, `apart` when farther; an edge
/// claim when the group's courtyards touch that edge band (within 3 mm of
/// it); every decoupling cap against [`crate::checks::DECAP_LOOP_MAX_MM`].
/// kicad-cli's DRC is the last row: the claims are about intent, DRC is
/// about the copper.
pub fn verify_claims(groups: Option<&GroupsDoc>, d: &Design, drc: Option<&wright_kicad::CliVerdict>) -> Vec<Claim> {
    let mut out = Vec::new();
    if let Some(g) = groups {
        let cents: Vec<(String, Option<Pt>)> = g.groups.iter().map(|x| (x.name.clone(), centroid(d, &x.refs))).collect();
        let at = |n: &str| cents.iter().find(|c| c.0 == n).and_then(|c| c.1);
        let mut all: Vec<f64> = Vec::new();
        for i in 0..cents.len() {
            for j in i + 1..cents.len() {
                if let (Some(a), Some(b)) = (cents[i].1, cents[j].1) {
                    all.push(a.dist(b) / 1e6);
                }
            }
        }
        all.sort_by(f64::total_cmp);
        let median = all.get(all.len() / 2).copied().unwrap_or(f64::NAN);
        let o = d.board.outline;
        for x in &g.groups {
            for (rel, others) in [("near", &x.near), ("apart", &x.apart)] {
                for y in others {
                    let dist = at(&x.name).zip(at(y)).map(|(a, b)| a.dist(b) / 1e6).unwrap_or(f64::NAN);
                    let holds = if rel == "near" { dist <= median } else { dist >= median };
                    out.push(Claim { claim: format!("group {} {rel} {y}", x.name), measured: format!("centroids {dist:.1} mm apart (median over group pairs {median:.1} mm)"), holds: holds && dist.is_finite() });
                }
            }
            if let Some(e) = &x.edge {
                let band = 3e6 as i64;
                let parts: Vec<_> = x.refs.iter().filter_map(|r| d.netlist.parts.iter().position(|p| &p.refdes == r)).filter_map(|i| d.board.parts.iter().find(|b| b.part == i)).collect();
                let on = |c: wright_eda::Rect| match e.chars().next().unwrap_or(' ').to_ascii_uppercase() {
                    'N' | 'T' => c.min.y.0 - o.min.y.0 <= band,
                    'S' | 'B' => o.max.y.0 - c.max.y.0 <= band,
                    'W' | 'L' => c.min.x.0 - o.min.x.0 <= band,
                    'E' | 'R' => o.max.x.0 - c.max.x.0 <= band,
                    _ => false,
                };
                let conn: Vec<_> = parts.iter().filter(|b| d.netlist.parts[b.part].kind == PartKind::Connector).collect();
                let judged = if conn.is_empty() { parts.iter().collect::<Vec<_>>() } else { conn };
                let ok = judged.iter().filter(|b| on(b.courtyard_abs())).count();
                out.push(Claim { claim: format!("group {} on edge {e}", x.name), measured: format!("{ok}/{} of its {} within 3 mm of that edge", judged.len(), if judged.len() == parts.len() { "parts" } else { "connectors" }), holds: ok == judged.len() && !judged.is_empty() });
            }
        }
    }
    let loop_mm = wright_place::decap::loops_nm(&d.board, &wright_place::decoupling_caps(&d.netlist));
    for (dc, l) in wright_place::decoupling_caps(&d.netlist).iter().zip(loop_mm) {
        let mm = l / 1e6;
        out.push(Claim {
            claim: format!("{} decouples {} within {} mm", d.netlist.parts[dc.cap].refdes, d.netlist.parts[dc.ic].refdes, crate::checks::DECAP_LOOP_MAX_MM),
            measured: format!("loop {mm:.1} mm"),
            holds: mm <= crate::checks::DECAP_LOOP_MAX_MM,
        });
    }
    match drc {
        Some(v) if v.ran => out.push(Claim { claim: "kicad-cli DRC clean".into(), measured: format!("{} errors, {} unconnected", v.errors, v.unconnected), holds: v.errors + v.unconnected == 0 }),
        _ => out.push(Claim { claim: "kicad-cli DRC clean".into(), measured: "DRC did not run".into(), holds: false }),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_and_prompt_name_the_upstream_files() {
        assert!(groups_system().contains("placement-lessons.md"));
        assert_eq!(groups_tool()["name"], "write_groups");
    }

    #[test]
    fn groups_are_validated_with_lane_c_rules() {
        let n = crate::templates::expand(&crate::templates::Template::Ldo { vin_v: 5.0, vout_v: 3.3, iout_a: 0.5 }).unwrap().validate().unwrap();
        let refs: Vec<String> = n.parts.iter().map(|p| p.refdes.clone()).collect();
        let ok = json!({"schema": 1, "groups": [{"name": "all", "refs": refs, "why": "one block"}]});
        assert!(parse_groups(&n, &ok).is_ok(), "{:?}", parse_groups(&n, &ok));
        let missing = json!({"schema": 1, "groups": [{"name": "some", "refs": [refs[0]], "why": "x"}]});
        assert!(parse_groups(&n, &missing).is_err());
        let unknown_key = json!({"schema": 1, "groups": [{"name": "all", "refs": refs, "colour": "red"}]});
        assert!(parse_groups(&n, &unknown_key).is_err());
    }
}
