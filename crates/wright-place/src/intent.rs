//! What the layout agent tells the placer: functional groups, fixed
//! (mechanical) poses, proximity claims and datasheet templates.
//!
//! The shapes are KiCadRoutingTools v0.23.7's (MIT,
//! github.com/drandyhaas/KiCadRoutingTools, read 2026-10-24):
//!
//! * [`GroupsDoc`] is `groups.json` schema 1 as `py_placer/placement/
//!   grouping.py` `validate_doc` reads it: top-level `schema`, `board`,
//!   `groups`, `note`; per group `name`, `role`, `refs`, `anchor`, `why`,
//!   `edge` (N/S/E/W, full names normalised), `side` (F/B), `near`,
//!   `apart`, `note`, `templates` (each `chip`, `kind` in `switching
//!   stage` / `rf front end` / `none`, `why`, `skip`). Unknown keys are
//!   refused, as upstream does. Coverage follows `grouping.py analyze`:
//!   a movable part in no group, or a ref in two groups, is an error.
//! * [`FixedPose`] is the intent's `fixed_poses` (`place_simple.py`:
//!   written first and held; never nudged).
//! * [`Claim`] is an intent `proximity[]` row (`ref`, `near`, `max_mm`).
//!   Crystal claims are added automatically, `crystals.py`: crystal to its
//!   IC within `LEG_MM` 8.1 mm, each load cap to the crystal within
//!   `CAP_MM` 2.2 mm.
//! * [`Template`] is a `templates.py` row: `anchor`, `members` (`ref`,
//!   `at` [dx, dy] mm in the chip's unturned frame, +y down, `rot` in
//!   degrees), `tolerance_mm` (0.3), `source`, `kind`, `why`.
//!
//! Example, as `PlaceOptions` JSON (lane E maps the agent's groups.json
//! into `groups`; every field but `groups[].name` and `groups[].refs` is
//! optional):
//!
//! ```json
//! {"groups": {"schema": 1, "groups": [
//!    {"name": "power", "role": "power entry", "refs": ["J1", "U1", "C1", "C2"],
//!     "anchor": "U1", "why": "USB VBUS into the LDO", "edge": "W", "near": ["core"]},
//!    {"name": "core", "role": "core", "refs": ["U3", "C4", "R1", "R4"], "anchor": "U3",
//!     "why": "module and its decoupling", "apart": ["power"],
//!     "templates": [{"chip": "U3", "kind": "none", "why": "a module, not a stage"}]}]},
//!  "intent": {"fixed_poses": [{"ref": "J1", "at": [0.0, 12.0], "rot": 90}],
//!             "proximity": [{"ref": "C4", "near": "U3", "max_mm": 2.0, "why": "bulk cap at VDD"}]}}
//! ```

use crate::anneal::EdgeSide;
use crate::decap::decoupling_caps;
use std::collections::{BTreeMap, BTreeSet};
use wright_eda::ir::NetRole;
use wright_eda::{Netlist, PartKind};

pub const LEG_MM: f64 = 8.1;
pub const CAP_MM: f64 = 2.2;
pub const TEMPLATE_TOLERANCE_MM: f64 = 0.3;

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupsDoc {
    #[serde(default = "one")]
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    pub groups: Vec<Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// `N`, `S`, `E`, `W` (or the full names); board top is N (min y).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// `F` (default) or `B`. The slice is single-sided: `B` is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apart: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<TemplateCandidate>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateCandidate {
    pub chip: String,
    /// `switching stage`, `rf front end` or `none`.
    pub kind: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Member {
    #[serde(rename = "ref")]
    pub refdes: String,
    /// [dx, dy] mm from the chip's origin, chip unturned, +y down.
    pub at: [f64; 2],
    /// Degrees, a multiple of 90, relative to the chip.
    #[serde(default)]
    pub rot: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Template {
    pub anchor: String,
    pub members: Vec<Member>,
    #[serde(default = "tol")]
    pub tolerance_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

fn tol() -> f64 {
    TEMPLATE_TOLERANCE_MM
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FixedPose {
    #[serde(rename = "ref")]
    pub refdes: String,
    /// Footprint origin, mm, board frame (Y down).
    pub at: [f64; 2],
    /// Degrees CCW, a multiple of 90.
    #[serde(default)]
    pub rot: i32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Claim {
    #[serde(rename = "ref")]
    pub refdes: String,
    pub near: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

fn yes() -> bool {
    true
}

/// Everything else the agent may say about a layout (the groups travel in
/// `PlaceOptions.groups`). `Intent::default()` asks for nothing; with no
/// groups they are derived (`decap:<IC>` blocks, upstream
/// `groups.py`'s `decap` source) and crystals and built-in templates are on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Intent {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_poses: Vec<FixedPose>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proximity: Vec<Claim>,
    /// Fitted template rows; candidates in `groups[].templates` and the
    /// built-in ones are fitted by the placer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    #[serde(default = "yes")]
    pub crystals: bool,
    /// Built-in switching-stage templates for chips Wright knows (TPS5450).
    #[serde(default = "yes")]
    pub builtin_templates: bool,
    /// Each decoupling cap's loop limit, mm (rail leg + ground leg, the
    /// netlist's cap -> IC pairing), priced 10 per mm past it (upstream's
    /// intent decap limits, `decap_limit_weight`). Defaults to the
    /// `decoupling_loop` check's 10 mm; `None` turns the term off.
    #[serde(default = "decap_max")]
    pub decap_max_mm: Option<f64>,
}

pub const DECAP_MAX_MM: f64 = 10.0;

fn decap_max() -> Option<f64> {
    Some(DECAP_MAX_MM)
}

impl Default for Intent {
    fn default() -> Self {
        Intent { fixed_poses: Vec::new(), proximity: Vec::new(), templates: Vec::new(), crystals: true, builtin_templates: true, decap_max_mm: Some(DECAP_MAX_MM) }
    }
}

/// A group with part indices.
#[derive(Debug, Clone, PartialEq)]
pub struct RGroup {
    pub name: String,
    pub members: Vec<usize>,
    pub anchor: Option<usize>,
    pub near: Vec<usize>,
    pub apart: Vec<usize>,
    pub edge: Option<EdgeSide>,
}

/// A claim with part indices and pad numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct RClaim {
    pub subject: usize,
    pub subject_pads: Vec<String>,
    pub partner: usize,
    pub partner_pads: Vec<String>,
    pub max_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolved {
    pub groups: Vec<RGroup>,
    /// (part, origin mm, quarter turns).
    pub fixed: Vec<(usize, (f64, f64), usize)>,
    pub claims: Vec<RClaim>,
    /// Chips to fit a template for (switching stages), with the source note.
    pub stage_chips: Vec<(usize, String)>,
    pub templates: Vec<Template>,
}

fn edge_of(s: &str) -> Option<EdgeSide> {
    Some(match s.to_ascii_uppercase().as_str() {
        "N" | "NORTH" => EdgeSide::Top,
        "S" | "SOUTH" => EdgeSide::Bottom,
        "E" | "EAST" => EdgeSide::Right,
        "W" | "WEST" => EdgeSide::Left,
        _ => return None,
    })
}

fn quarter(deg: i32) -> Option<usize> {
    (deg.rem_euclid(90) == 0).then(|| (deg.rem_euclid(360) / 90) as usize)
}

/// Is part `i` a crystal? Upstream finds crystals by pin function
/// (`emc_roles`); the netlist here has no crystal kind, so the name, value
/// or package says so.
fn is_crystal(n: &Netlist, i: usize) -> bool {
    let p = &n.parts[i];
    let t = format!("{} {} {}", p.name, p.value, p.package.as_deref().unwrap_or("")).to_ascii_lowercase();
    !p.kind.is_passive() && (t.contains("crystal") || t.contains("xtal") || t.starts_with("y_"))
}

/// Crystal claims, `crystals.py`: the crystal's two load nets reach an IC
/// (claim crystal -> IC, `LEG_MM`); a capacitor from a load net to ground
/// is a load cap (claim cap -> crystal, `CAP_MM`).
pub fn crystal_claims(n: &Netlist) -> Vec<RClaim> {
    let mut out = Vec::new();
    for x in 0..n.parts.len() {
        if !is_crystal(n, x) {
            continue;
        }
        let load: Vec<(String, usize)> = n.parts[x].pins.iter().filter_map(|p| p.net.filter(|&k| n.nets[k].role != NetRole::Ground).map(|k| (p.pad.clone(), k))).collect();
        if load.is_empty() {
            continue;
        }
        let nets: BTreeSet<usize> = load.iter().map(|l| l.1).collect();
        let mut ics: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for &k in &nets {
            for pr in &n.nets[k].pins {
                if pr.part != x && n.parts[pr.part].kind == PartKind::Ic {
                    ics.entry(pr.part).or_default().push(pr.pad.clone());
                }
            }
        }
        if let Some((&ic, pads)) = ics.iter().max_by_key(|(i, p)| (p.len(), std::cmp::Reverse(**i))) {
            out.push(RClaim { subject: x, subject_pads: load.iter().map(|l| l.0.clone()).collect(), partner: ic, partner_pads: pads.clone(), max_mm: Some(LEG_MM) });
        }
        for (c, part) in n.parts.iter().enumerate() {
            if part.kind != PartKind::Capacitor || part.pins.len() != 2 {
                continue;
            }
            let (a, b) = (&part.pins[0], &part.pins[1]);
            let role = |p: &wright_eda::ir::Pin| p.net.map(|k| n.nets[k].role);
            for (on, other) in [(a, b), (b, a)] {
                if let (Some(k), Some(NetRole::Ground)) = (on.net, role(other)) {
                    if let Some(l) = load.iter().find(|l| l.1 == k) {
                        out.push(RClaim { subject: c, subject_pads: vec![on.pad.clone()], partner: x, partner_pads: vec![l.0.clone()], max_mm: Some(CAP_MM) });
                    }
                }
            }
        }
    }
    out
}

/// Chips with a built-in switching-stage template.
fn builtin_stage(n: &Netlist, i: usize) -> Option<String> {
    let p = &n.parts[i];
    let v = format!("{} {}", p.name, p.value).to_ascii_uppercase();
    (p.kind == PartKind::Ic && v.contains("TPS5450")).then(|| "TI SLVS757 TPS5450 datasheet, 'Layout' (PCB layout guidelines and example)".to_string())
}

/// Resolve and check an intent against the netlist; every problem in one
/// error, like the netlist's.
pub fn resolve(n: &Netlist, groups: Option<&GroupsDoc>, intent: &Intent) -> anyhow::Result<Resolved> {
    let mut problems = Vec::new();
    let idx = |r: &str| n.parts.iter().position(|p| p.refdes == r);
    let mut out = Resolved::default();
    for f in &intent.fixed_poses {
        match (idx(&f.refdes), quarter(f.rot)) {
            (Some(i), Some(q)) => out.fixed.push((i, (f.at[0], f.at[1]), q)),
            (None, _) => problems.push(format!("fixed_poses: {} is not on this board", f.refdes)),
            (_, None) => problems.push(format!("fixed_poses: {} rot {} is not a multiple of 90", f.refdes, f.rot)),
        }
    }
    let fixed: BTreeSet<usize> = out.fixed.iter().map(|f| f.0).collect();
    let mut chips_named = BTreeSet::new();
    if let Some(doc) = groups {
        if doc.schema != 1 {
            problems.push(format!("schema {}, this reader knows 1", doc.schema));
        }
        if doc.groups.is_empty() {
            problems.push("groups is empty".into());
        }
        let names: Vec<&str> = doc.groups.iter().map(|g| g.name.as_str()).collect();
        let mut owner: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
        for (gi, g) in doc.groups.iter().enumerate() {
            if g.name.trim().is_empty() {
                problems.push(format!("group {gi} has no name"));
            }
            if names.iter().filter(|&&x| x == g.name).count() > 1 && names.iter().position(|&x| x == g.name) == Some(gi) {
                problems.push(format!("group name {:?} is used twice", g.name));
            }
            if g.refs.is_empty() {
                problems.push(format!("group {} has no refs", g.name));
            }
            let mut members = Vec::new();
            for r in &g.refs {
                match idx(r) {
                    Some(i) => {
                        owner.entry(i).or_default().push(&g.name);
                        if !members.contains(&i) {
                            members.push(i);
                        }
                    }
                    None => problems.push(format!("group {}: {r} is not on this board", g.name)),
                }
            }
            let anchor = match &g.anchor {
                Some(a) if g.refs.contains(a) => idx(a),
                Some(a) => {
                    problems.push(format!("group {}: anchor {a} is not one of its refs", g.name));
                    None
                }
                None => None,
            };
            let edge = match &g.edge {
                None => None,
                Some(e) => match edge_of(e) {
                    Some(s) => Some(s),
                    None => {
                        problems.push(format!("group {}: edge {e:?} is not N, S, E or W", g.name));
                        None
                    }
                },
            };
            match g.side.as_deref().map(str::to_ascii_uppercase).as_deref() {
                None | Some("F") => {}
                Some("B") => problems.push(format!("group {}: side B: this placer is single-sided (top) for now", g.name)),
                Some(s) => problems.push(format!("group {}: side {s:?} is not F or B", g.name)),
            }
            let look = |list: &[String], what: &str, problems: &mut Vec<String>| -> Vec<usize> {
                list.iter()
                    .filter_map(|o| {
                        let p = names.iter().position(|&x| x == o);
                        if p.is_none() {
                            problems.push(format!("group {}: {what} names {o:?}, which is not a group", g.name));
                        }
                        p
                    })
                    .filter(|&p| p != gi)
                    .collect()
            };
            let near = look(&g.near, "near", &mut problems);
            let apart = look(&g.apart, "apart", &mut problems);
            for t in &g.templates {
                if !g.refs.contains(&t.chip) {
                    problems.push(format!("group {}: template chip {} is not one of its refs", g.name, t.chip));
                    continue;
                }
                if !chips_named.insert(t.chip.clone()) {
                    problems.push(format!("template chip {} is named twice", t.chip));
                }
                if t.why.trim().is_empty() {
                    problems.push(format!("group {}: template for {} has no why", g.name, t.chip));
                }
                match t.kind.as_str() {
                    "switching stage" | "rf front end" => {
                        if t.skip.is_none() {
                            if let Some(i) = idx(&t.chip) {
                                out.stage_chips.push((i, t.why.clone()));
                            }
                        }
                    }
                    "none" => {}
                    k => problems.push(format!("group {}: template kind {k:?} is not 'switching stage', 'rf front end' or 'none'", g.name)),
                }
            }
            out.groups.push(RGroup { name: g.name.clone(), members, anchor, near, apart, edge });
        }
        for (i, gs) in &owner {
            if gs.len() > 1 {
                problems.push(format!("{} is in more than one group ({})", n.parts[*i].refdes, gs.join(", ")));
            }
        }
        let loose: Vec<&str> = (0..n.parts.len()).filter(|i| !fixed.contains(i) && !owner.contains_key(i)).map(|i| n.parts[i].refdes.as_str()).collect();
        if !loose.is_empty() {
            problems.push(format!("movable parts in no group: {}", loose.join(", ")));
        }
    }
    for c in &intent.proximity {
        match (idx(&c.refdes), idx(&c.near)) {
            (Some(s), Some(p)) => {
                // The legs: the subject's pads on nets the partner shares.
                let nets: BTreeSet<usize> = n.parts[p].pins.iter().filter_map(|x| x.net).collect();
                let sp: Vec<String> = n.parts[s].pins.iter().filter(|x| x.net.is_some_and(|k| nets.contains(&k))).map(|x| x.pad.clone()).collect();
                let snets: BTreeSet<usize> = n.parts[s].pins.iter().filter_map(|x| x.net).collect();
                let pp: Vec<String> = n.parts[p].pins.iter().filter(|x| x.net.is_some_and(|k| snets.contains(&k))).map(|x| x.pad.clone()).collect();
                if sp.is_empty() {
                    problems.push(format!("proximity: {} shares no net with {}", c.refdes, c.near));
                } else {
                    out.claims.push(RClaim { subject: s, subject_pads: sp, partner: p, partner_pads: pp, max_mm: c.max_mm });
                }
            }
            _ => problems.push(format!("proximity: {} or {} is not on this board", c.refdes, c.near)),
        }
    }
    if intent.crystals {
        for c in crystal_claims(n) {
            if !out.claims.iter().any(|o| o.subject == c.subject && o.partner == c.partner) {
                out.claims.push(c);
            }
        }
    }
    for t in &intent.templates {
        if idx(&t.anchor).is_none() {
            problems.push(format!("template anchor {} is not on this board", t.anchor));
        }
        for m in &t.members {
            if idx(&m.refdes).is_none() {
                problems.push(format!("template {}: member {} is not on this board", t.anchor, m.refdes));
            }
            if quarter(m.rot).is_none() {
                problems.push(format!("template {}: member {} rot {} is not a multiple of 90", t.anchor, m.refdes, m.rot));
            }
        }
        out.templates.push(t.clone());
    }
    if intent.builtin_templates {
        for i in 0..n.parts.len() {
            let named = chips_named.contains(&n.parts[i].refdes) || intent.templates.iter().any(|t| t.anchor == n.parts[i].refdes);
            if !named {
                if let Some(src) = builtin_stage(n, i) {
                    out.stage_chips.push((i, src));
                }
            }
        }
    }
    if !problems.is_empty() {
        anyhow::bail!("placement intent has {} problem(s): {}", problems.len(), problems.join("; "));
    }
    if groups.is_none() {
        // Upstream's `decap` block source: an IC and the caps dealt to it.
        let mut by_ic: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for d in decoupling_caps(n) {
            by_ic.entry(d.ic).or_default().push(d.cap);
        }
        for (ic, caps) in by_ic {
            let mut members = vec![ic];
            members.extend(caps.into_iter().filter(|c| !fixed.contains(c)));
            members.dedup();
            if members.len() >= 2 {
                out.groups.push(RGroup { name: format!("decap:{}", n.parts[ic].refdes), members, anchor: Some(ic), near: vec![], apart: vec![], edge: None });
            }
        }
    }
    Ok(out)
}
