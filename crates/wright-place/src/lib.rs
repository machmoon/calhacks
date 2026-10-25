//! Placement (lane C of docs/electronics-plan.md). Replaces the Python baseline's
//! `packing.py` (OR-Tools CP-SAT) and `simple/` with KiCadRoutingTools
//! v0.23.7's placer (MIT, github.com/drandyhaas/KiCadRoutingTools,
//! `py_placer/place_simple.py` + `py_placer/placement/{simple,
//! simple_anneal, legality, templates, crystals, grouping}.py`),
//! deterministic from a seed, no OR-Tools, no new dependencies.
//!
//! [`place_candidates`] follows `place_simple.py`:
//!
//! 1. Mechanical parts first: `fixed_poses` are applied and locked, and
//!    checked for legality, never nudged.
//! 2. Groups ([`intent::GroupsDoc`], groups.json schema 1) are resolved,
//!    or `decap:<IC>` groups are derived; crystal claims are added
//!    (`crystals.py`); switching-stage templates are fitted (`templates.py
//!    fit`; the TPS5450 buck is built in).
//! 3. Twelve seeds, six scattered (0-5) and six group-gathered (6-11)
//!    (`SCATTERED_SEEDS`, `CLUSTERED_SEEDS`), annealed in parallel on std
//!    threads ([`anneal`]). Each goes through the 10 um snap and the exact
//!    legality gate ([`legal`]); seeds left illegal are dropped, as
//!    upstream never judges them.
//! 4. The legal seeds ranked by cost, then seed; the six cheapest
//!    (`JUDGED_SEEDS`) are returned for lane E to judge by a quick route.
//!    [`place`] returns the first.
//! 5. Outline: courtyard bbox + `Rules.edge_clearance`, out to 0.5 mm; net
//!    classes from roles ([`classes::classes_for`]).
//!
//! If no seed comes out legal, the deterministic shelf packing ([`shelf`],
//! the baseline's `packing.py:213 _shelf_pack`) is returned, marked `strategy: shelf`.

pub mod anneal;
pub mod classes;
pub mod congest;
pub mod decap;
pub mod geom;
pub mod intent;
pub mod legal;
pub mod rng;
pub mod shelf;
pub mod template;

use anneal::{Annealer, CNet, CapLeg, Cluster, EdgeSide, Kind, Model, PClaim, PGroup, PTemplate, Pose, Problem, Site, Weights};
use geom::{PartGeom, NM, R};
use intent::{GroupsDoc, Intent, Resolved};
use std::collections::{BTreeMap, BTreeSet};
use wright_eda::ir::NetRole;
use wright_eda::{Board, Footprint, Netlist, Nm, PartKind, Placed, Pt, Rect, Rules, Side};

pub use classes::classes_for;
pub use decap::{decoupling_caps, decoupling_loop_mm, Decoupling};

/// Placement grid for the final poses, nm (10 um).
pub const GRID_NM: i64 = 10_000;
/// Outline grid, nm.
pub const OUTLINE_GRID_NM: i64 = 500_000;
pub const SCATTERED_SEEDS: u64 = 6;
pub const CLUSTERED_SEEDS: u64 = 6;
pub const JUDGED_SEEDS: usize = 6;
/// Upstream's run length per seed (`place_simple.py MOVES_PER_PART`).
pub const MOVES_PER_PART: u64 = 500;
/// Scatter board fill: the seeds scatter over a square this full.
pub const SCATTER_FILL: f64 = 0.55;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaceOptions {
    pub seed: u64,
    /// Annealing moves budget over all twelve seeds (work, not seconds:
    /// same input, same board). Each seed runs upstream's 500 moves per
    /// movable part, or its share of this budget if that is smaller.
    pub max_moves: u64,
    /// Courtyard-to-courtyard spacing added on every side.
    pub spacing_nm: i64,
    /// Fixed outline (w, h) in nm; `None` = shrink to fit.
    pub outline: Option<(i64, i64)>,
    /// Refs pinned to an edge: (refdes, "left"|"right"|"top"|"bottom").
    pub edge: Vec<(String, String)>,
    /// Functional groups (KRT groups.json schema 1); `None` derives
    /// `decap:<IC>` groups.
    #[serde(default)]
    pub groups: Option<GroupsDoc>,
    /// Fixed poses, proximity claims, templates, decap limit.
    #[serde(default)]
    pub intent: Intent,
    /// Mounting holes in the four corners (`MountingHoles::m3()` for the
    /// VTOL bay); `None` = no holes.
    #[serde(default)]
    pub mounting_holes: Option<MountingHoles>,
}

/// Four NPTH mounting holes, one per board corner, each with a keep-out
/// square no part courtyard may come within `spacing` of. Sizes follow
/// KiCad's `MountingHole.pretty/MountingHole_3.2mm_M3.kicad_mod`: 3.2 mm
/// drill, courtyard circle r = 3.45 mm (the screw head plus margin).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MountingHoles {
    pub drill_nm: i64,
    /// Keep-out radius (the hole footprint's courtyard), nm.
    pub keepout_nm: i64,
}

impl MountingHoles {
    pub fn m3() -> Self {
        MountingHoles { drill_nm: 3_200_000, keepout_nm: 3_450_000 }
    }
}

impl Default for PlaceOptions {
    fn default() -> Self {
        PlaceOptions { seed: 1, max_moves: 200_000, spacing_nm: 250_000, outline: None, edge: Vec::new(), groups: None, intent: Intent::default(), mounting_holes: None }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaceReport {
    pub cost: f64,
    /// Sum of per-net half-perimeter wirelength, nm.
    pub hpwl_nm: i64,
    /// Courtyard overlaps left (must be 0 for a legal placement).
    pub overlaps: usize,
    pub moves: u64,
    pub seconds: f64,
    /// Outline area, mm^2 (Edge.Cuts rectangle).
    #[serde(default)]
    pub area_mm2: f64,
    /// The baseline's `decoupling_loop_mm` on this board (sum over decoupling caps).
    #[serde(default)]
    pub decoupling_loop_mm: f64,
    /// `anneal`, or `shelf` when no seed produced anything legal.
    #[serde(default)]
    pub strategy: String,
    /// Which seed (0-11) and start (`scattered`, `clustered`, `shelf`).
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub start: String,
    /// Parts the exact pass had to move after the anneal.
    #[serde(default)]
    pub reseated: Vec<String>,
    /// Groups used (derived or given), templates fitted or refused.
    #[serde(default)]
    pub notes: Vec<String>,
    /// The final cost split by term (wire, decap_limit, templates, ...).
    #[serde(default)]
    pub terms: Vec<(String, f64)>,
}

fn side_of(s: &str) -> anyhow::Result<EdgeSide> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "left" | "w" | "west" => EdgeSide::Left,
        "right" | "e" | "east" => EdgeSide::Right,
        "top" | "n" | "north" => EdgeSide::Top,
        "bottom" | "s" | "south" => EdgeSide::Bottom,
        "any" => EdgeSide::Any,
        other => anyhow::bail!("edge side {other:?} is not left, right, top, bottom or any"),
    })
}

fn snap(v: f64) -> i64 {
    ((v * NM / GRID_NM as f64).round() as i64) * GRID_NM
}

/// Sum over nets of the half perimeter of the pads' box, nm.
pub fn hpwl_nm(netlist: &Netlist, parts: &[Placed]) -> i64 {
    let mut total = 0;
    for net in &netlist.nets {
        let pts: Vec<Pt> = net.pins.iter().filter_map(|pr| parts.iter().find(|p| p.part == pr.part).and_then(|p| p.footprint.pads.iter().find(|q| q.number == pr.pad).map(|q| p.pad_abs(q)))).collect();
        if pts.len() < 2 {
            continue;
        }
        let r = pts.iter().fold(Rect::from_corners(pts[0], pts[0]), |acc, p| acc.union(&Rect::from_corners(*p, *p)));
        total += r.width().0 + r.height().0;
    }
    total
}

/// Upstream's fine-pitch test (`simple.py`): at least `HALO_MIN_PADS`
/// connected pads at a median nearest-neighbour distance <= 1.0 mm.
fn is_fine(g: &PartGeom, connected: &[usize]) -> bool {
    if connected.len() < anneal::HALO_MIN_PADS {
        return false;
    }
    let mut nn: Vec<f64> = connected
        .iter()
        .map(|&a| {
            let (ax, ay) = g.pads[0][a];
            connected.iter().filter(|&&b| b != a).map(|&b| (g.pads[0][b].0 - ax).hypot(g.pads[0][b].1 - ay)).fold(f64::INFINITY, f64::min)
        })
        .collect();
    nn.sort_by(f64::total_cmp);
    nn[nn.len() / 2] <= anneal::HALO_PITCH_MM
}

struct Built {
    prob: Problem,
    notes: Vec<String>,
    fixed: Vec<(usize, Pose)>,
}

fn build(netlist: &Netlist, footprints: &[Footprint], rules: &Rules, opts: &PlaceOptions, weights: Weights, inner: Option<Rect>) -> anyhow::Result<Built> {
    let n = netlist.parts.len();
    let mut geoms: Vec<PartGeom> = footprints.iter().map(PartGeom::new).collect();
    let res: Resolved = intent::resolve(netlist, opts.groups.as_ref(), &opts.intent)?;
    let mut notes = Vec::new();
    let mut locked = vec![false; n];
    let mut fixed = Vec::new();
    for &(i, (x, y), q) in &res.fixed {
        locked[i] = true;
        fixed.push((i, Pose { x, y, q }));
    }
    // Pins to pads, batched.
    let mut missing = Vec::new();
    let mut nets = Vec::new();
    for net in &netlist.nets {
        let mut pins = Vec::new();
        for pr in &net.pins {
            match geoms[pr.part].pads_numbered(&pr.pad).first() {
                Some(&k) => pins.push((pr.part, k)),
                None => missing.push(format!("{}: no pad {:?} on {} (net {})", netlist.parts[pr.part].refdes, pr.pad, footprints[pr.part].lib_id, net.name)),
            }
        }
        let kind = match net.role {
            NetRole::Signal => Kind::Signal,
            NetRole::Power => Kind::Rail,
            NetRole::Ground => Kind::Ground,
        };
        if pins.len() >= 2 {
            nets.push(CNet { kind, pins, congest: kind != Kind::Ground });
        }
    }
    anyhow::ensure!(missing.is_empty(), "footprints are missing pads the netlist uses: {}", missing.join("; "));

    // Groups (given or derived), and templates.
    let mut groups: Vec<intent::RGroup> = res.groups.clone();
    let mut templates: Vec<PTemplate> = Vec::new();
    let mut templated: BTreeSet<usize> = BTreeSet::new();
    let idx = |r: &str| netlist.parts.iter().position(|p| p.refdes == r);
    let half = opts.spacing_nm as f64 / NM / 2.0 + GRID_NM as f64 / NM;
    let mut rows = res.templates.clone();
    for (chip, src) in &res.stage_chips {
        let mut exclude: BTreeSet<usize> = locked.iter().enumerate().filter(|x| *x.1).map(|x| x.0).collect();
        exclude.extend(rows.iter().flat_map(|t| t.members.iter().filter_map(|m| idx(&m.refdes))));
        let mut members = template::neighbours(netlist, *chip, &exclude);
        if opts.groups.is_some() {
            // A given grouping bounds the stage to the chip's own group.
            if let Some(g) = groups.iter().find(|g| g.members.contains(chip)) {
                members.retain(|m| g.members.contains(m));
            }
        }
        match template::fit(netlist, &geoms, *chip, &members, 2.0 * half + 0.001, src) {
            Ok(t) => {
                notes.push(format!("template {} fitted: {}", t.anchor, t.members.iter().map(|m| m.refdes.as_str()).collect::<Vec<_>>().join(", ")));
                rows.push(t);
            }
            Err(e) => notes.push(format!("template for {} not fitted: {e}", netlist.parts[*chip].refdes)),
        }
    }
    for t in &rows {
        let Some(a) = idx(&t.anchor) else { continue };
        let mut ms = Vec::new();
        for m in &t.members {
            if let Some(i) = idx(&m.refdes) {
                if i != a && !locked[i] && templated.insert(i) {
                    ms.push((i, m.at[0], m.at[1], (m.rot.rem_euclid(360) / 90) as usize));
                }
            }
        }
        if opts.groups.is_none() {
            // Derived groups: the stage is one group.
            let gi = groups.iter().position(|g| g.members.contains(&a)).unwrap_or_else(|| {
                groups.push(intent::RGroup { name: format!("template:{}", t.anchor), members: vec![a], anchor: Some(a), near: vec![], apart: vec![], edge: None });
                groups.len() - 1
            });
            for &(i, ..) in &ms {
                for g in groups.iter_mut() {
                    g.members.retain(|&x| x != i || x == a);
                }
                if !groups[gi].members.contains(&i) {
                    groups[gi].members.push(i);
                }
            }
        }
        templates.push(PTemplate { anchor: a, members: ms });
    }
    groups.retain(|g| !g.members.is_empty());
    for g in &groups {
        notes.push(format!("group {}: {}", g.name, g.members.iter().map(|&i| netlist.parts[i].refdes.as_str()).collect::<Vec<_>>().join(" ")));
    }
    let group_of = |i: usize| groups.iter().position(|g| g.members.contains(&i));

    // Decap clusters (upstream `_sites` / `cluster_match`), with group
    // preference: a cap's own group's ICs first, the rail's ICs otherwise.
    type Key = (usize, Vec<(usize, String)>);
    let mut clusters: BTreeMap<Key, Vec<CapLeg>> = BTreeMap::new();
    let mut cluster_ground: BTreeMap<Key, usize> = BTreeMap::new();
    for d in decoupling_caps(netlist) {
        let cap = &netlist.parts[d.cap];
        let rail = cap.pins.iter().find(|p| p.pad == d.cap_rail_pad).and_then(|p| p.net).unwrap_or(0);
        let ground = cap.pins.iter().find(|p| p.pad == d.cap_ground_pad).and_then(|p| p.net).unwrap_or(0);
        let all: Vec<(usize, String)> = netlist.nets[rail].pins.iter().filter(|p| netlist.parts[p.part].kind == PartKind::Ic).map(|p| (p.part, p.pad.clone())).collect();
        let g = group_of(d.cap);
        let mine: Vec<(usize, String)> = all.iter().filter(|(ic, _)| g.is_some() && group_of(*ic) == g).cloned().collect();
        let mut pins = if mine.is_empty() { all } else { mine };
        pins.sort();
        pins.dedup();
        let key = (rail, pins);
        cluster_ground.insert(key.clone(), ground);
        clusters.entry(key).or_default().push(CapLeg { part: d.cap, rail: geoms[d.cap].pads_numbered(&d.cap_rail_pad), ground: geoms[d.cap].pads_numbered(&d.cap_ground_pad) });
    }
    let mut pclusters = Vec::new();
    for ((rail, pins), caps) in clusters {
        let ground = cluster_ground.get(&(rail, pins.clone())).copied().unwrap_or(0);
        let mut sites: Vec<Site> = Vec::new();
        let ics: BTreeSet<usize> = pins.iter().map(|p| p.0).collect();
        for ic in ics {
            // Union-find over this IC's supply pins, joined within SITE_MM.
            let mine: Vec<Vec<usize>> = pins.iter().filter(|p| p.0 == ic).map(|p| geoms[ic].pads_numbered(&p.1)).collect();
            let mut parent: Vec<usize> = (0..mine.len()).collect();
            fn root(p: &mut [usize], i: usize) -> usize {
                let mut r = i;
                while p[r] != r {
                    r = p[r];
                }
                p[i] = r;
                r
            }
            for a in 0..mine.len() {
                for b in a + 1..mine.len() {
                    let close = mine[a].iter().any(|&x| mine[b].iter().any(|&y| {
                        let (p, q) = (geoms[ic].pads[0][x], geoms[ic].pads[0][y]);
                        (p.0 - q.0).hypot(p.1 - q.1) <= anneal::SITE_MM
                    }));
                    if close {
                        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                        parent[ra.max(rb)] = ra.min(rb);
                    }
                }
            }
            let gnd: Vec<usize> = netlist.nets[ground].pins.iter().filter(|p| p.part == ic).flat_map(|p| geoms[ic].pads_numbered(&p.pad)).collect();
            let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for (a, pads) in mine.iter().enumerate() {
                let r = root(&mut parent, a);
                by_root.entry(r).or_default().extend(pads.iter().copied());
            }
            for (_, pads) in by_root {
                sites.push(Site { ic, pads, ground: gnd.clone() });
            }
        }
        pclusters.push(Cluster { caps, sites });
    }

    let claims: Vec<PClaim> = res
        .claims
        .iter()
        .map(|c| PClaim {
            subject: c.subject,
            subject_pads: c.subject_pads.iter().flat_map(|p| geoms[c.subject].pads_numbered(p)).collect(),
            partner: c.partner,
            partner_pads: c.partner_pads.iter().flat_map(|p| geoms[c.partner].pads_numbered(p)).collect(),
            max_mm: c.max_mm,
        })
        .filter(|c| !c.subject_pads.is_empty() && !c.partner_pads.is_empty())
        .collect();

    // Fine-pitch fronts per quarter turn, and halo exemptions.
    let mut fine = vec![None; n];
    let mut halo_exempt = Vec::new();
    for (i, p) in netlist.parts.iter().enumerate() {
        let connected: Vec<(usize, usize)> = p
            .pins
            .iter()
            .filter_map(|pin| {
                let k = pin.net?;
                let pad = *geoms[i].pads_numbered(&pin.pad).first()?;
                Some((pad, if netlist.nets[k].role == NetRole::Ground { usize::MAX } else { k }))
            })
            .collect();
        if !is_fine(&geoms[i], &connected.iter().map(|c| c.0).collect::<Vec<_>>()) {
            continue;
        }
        fine[i] = Some([0, 1, 2, 3].map(|q| anneal::fronts(&geoms[i].pads[q], &connected, weights.halo_coef)));
        let my_nets: BTreeSet<usize> = p.pins.iter().filter_map(|x| x.net).collect();
        for (j, o) in netlist.parts.iter().enumerate() {
            if j != i && o.pins.len() <= anneal::HALO_SMALL_PADS && o.pins.iter().any(|x| x.net.is_some_and(|k| my_nets.contains(&k) && netlist.nets[k].role != NetRole::Ground)) {
                halo_exempt.push((i.min(j), i.max(j)));
            }
        }
    }

    // Seats: a part named on an edge, and every connector of a group with
    // an edge, is held to that edge and slides along it (upstream: declared
    // edge parts are seated). Named `any` stays a soft pull.
    let mut edges = Vec::new();
    let mut seat: Vec<Option<anneal::Seat>> = vec![None; n];
    for (r, s) in &opts.edge {
        let i = idx(r).ok_or_else(|| anyhow::anyhow!("edge hint names {r}, which is not a part"))?;
        match side_of(s)? {
            EdgeSide::Any => edges.push((i, EdgeSide::Any, 5.0 * weights.edge)),
            side if !locked[i] => seat[i] = Some(anneal::Seat::Edge(side)),
            _ => {}
        }
    }
    for g in &groups {
        if let Some(e) = g.edge {
            for &i in &g.members {
                if netlist.parts[i].kind == PartKind::Connector && !locked[i] && seat[i].is_none() {
                    seat[i] = Some(anneal::Seat::Edge(e));
                }
            }
        }
    }
    for (i, p) in netlist.parts.iter().enumerate() {
        if p.kind == PartKind::Connector && !locked[i] && seat[i].is_none() && !edges.iter().any(|e| e.0 == i) && group_of(i).is_none_or(|g| groups[g].edge.is_none()) {
            edges.push((i, EdgeSide::Any, weights.edge));
        }
    }
    for g in geoms.iter_mut() {
        g.can_turn = true;
    }
    // A seated part faces out: an asymmetric body (a horizontal connector's
    // mating end) turns so the courtyard reaches past its pads toward the
    // edge; a symmetric one lies with its long side along the edge.
    let mut turns: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let Some(anneal::Seat::Edge(side)) = seat[i] else { continue };
        let (nx, ny) = match side {
            EdgeSide::Left => (-1.0, 0.0),
            EdgeSide::Right => (1.0, 0.0),
            EdgeSide::Top => (0.0, -1.0),
            _ => (0.0, 1.0),
        };
        let g = &geoms[i];
        let out: Vec<f64> = (0..4)
            .map(|q| {
                let k = g.pads[q].len().max(1) as f64;
                let (px, py) = g.pads[q].iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / k, a.1 + p.1 / k));
                (g.crt[q].cx() - px) * nx + (g.crt[q].cy() - py) * ny
            })
            .collect();
        let (lo, hi) = out.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, &v| (a.0.min(v), a.1.max(v)));
        turns[i] = if hi - lo > 0.5 {
            vec![(0..4).fold(0, |b, q| if out[q] > out[b] + 1e-9 { q } else { b })]
        } else {
            let along = |q: usize| if nx != 0.0 { g.crt[q].h() >= g.crt[q].w() - 1e-9 } else { g.crt[q].w() >= g.crt[q].h() - 1e-9 };
            (0..4).filter(|&q| along(q)).collect()
        };
    }
    // Decap loop limits, measured as the verifier measures them
    // (wright-electronics `checks.rs decoupling`): for each IC and each
    // power rail on its pins, every cap on that rail and the IC's first
    // ground, nearest rail pad pair plus nearest ground pad pair. The term
    // is upstream's `decap_limit` (KRT `place_simple.py`, steep past it).
    let pads_on = |part: usize, net: usize| -> Vec<usize> {
        let p = &netlist.parts[part];
        geoms[part].numbers.iter().enumerate().filter(|(_, num)| p.pins.iter().any(|q| &q.pad == *num && q.net == Some(net))).map(|(k, _)| k).collect()
    };
    let mut decap_limits: Vec<anneal::DLimit> = Vec::new();
    if let Some(max_mm) = opts.intent.decap_max_mm {
        for (ui, u) in netlist.parts.iter().enumerate().filter(|(_, p)| p.kind == PartKind::Ic) {
            let Some(g) = u.pins.iter().filter_map(|q| q.net).find(|&k| netlist.nets[k].role == NetRole::Ground) else { continue };
            let mut rails: Vec<usize> = u.pins.iter().filter_map(|q| q.net).filter(|&k| netlist.nets[k].role == NetRole::Power).collect();
            rails.sort_unstable();
            rails.dedup();
            for rail in rails {
                for (ci, c) in netlist.parts.iter().enumerate() {
                    if c.kind == PartKind::Capacitor && c.pins.iter().any(|q| q.net == Some(rail)) && c.pins.iter().any(|q| q.net == Some(g)) {
                        decap_limits.push(anneal::DLimit { cap: ci, ic: ui, rail: (pads_on(ci, rail), pads_on(ui, rail)), ground: Some((pads_on(ci, g), pads_on(ui, g))), max_mm });
                    }
                }
            }
        }
    }
    // Mounting holes: four corner-seated pseudo-parts after the real ones.
    if let Some(h) = opts.mounting_holes {
        let r = h.keepout_nm as f64 / NM;
        for (a, b) in [(EdgeSide::Left, EdgeSide::Top), (EdgeSide::Right, EdgeSide::Top), (EdgeSide::Left, EdgeSide::Bottom), (EdgeSide::Right, EdgeSide::Bottom)] {
            let sq = R { x0: -r, y0: -r, x1: r, y1: r };
            geoms.push(PartGeom { crt: [sq; 4], pads: Default::default(), pad_rects: Default::default(), drilled: Vec::new(), numbers: Vec::new(), can_turn: false });
            locked.push(false);
            seat.push(Some(anneal::Seat::Corner(a, b)));
            turns.push(vec![0]);
            fine.push(None);
        }
    }

    // Board to scatter on, and the canvas.
    let s = opts.spacing_nm as f64 / NM;
    let area: f64 = geoms.iter().map(|g| (g.crt[0].w() + s) * (g.crt[0].h() + s)).sum();
    let widest = geoms.iter().map(|g| g.crt[0].w().max(g.crt[0].h()) + s).fold(0.0, f64::max);
    let (board, canvas) = match inner {
        Some(r) => {
            let r = R::from_rect(r);
            (r, r)
        }
        None => {
            let side = (area / SCATTER_FILL).sqrt().max(widest);
            let b = R { x0: 0.0, y0: 0.0, x1: side, y1: side };
            let mut c = b.grow(side / 2.0);
            for &(i, ps) in &fixed {
                c = c.union(&geoms[i].crt[ps.q].at(ps.x, ps.y).grow(side / 2.0));
            }
            (b, c)
        }
    };
    let pgroups = groups.iter().map(|g| PGroup { members: g.members.clone(), near: g.near.clone(), apart: g.apart.clone(), edge: g.edge }).collect();
    let prob = Problem {
        parts: geoms,
        locked,
        nets,
        clusters: pclusters,
        claims,
        groups: pgroups,
        templates,
        fine,
        halo_exempt,
        edges,
        seat,
        turns,
        decap_limits,
        half,
        canvas,
        board,
        fixed_outline: inner.is_some(),
        copper_layers: rules.copper_layers,
        half_clearance: rules.min_clearance.as_mm() / 2.0,
        weights,
    };
    Ok(Built { prob, notes, fixed })
}

/// A board with every part placed inside `outline`, no tracks yet: the
/// cheapest of [`place_candidates`].
pub fn place(netlist: &Netlist, footprints: &[Footprint], rules: &Rules, opts: &PlaceOptions) -> anyhow::Result<(Board, PlaceReport)> {
    place_candidates(netlist, footprints, rules, opts)?.into_iter().next().ok_or_else(|| anyhow::anyhow!("placement produced no board"))
}

/// The `JUDGED_SEEDS` cheapest legal placements, best first (one shelf
/// placement when no seed is legal).
pub fn place_candidates(netlist: &Netlist, footprints: &[Footprint], rules: &Rules, opts: &PlaceOptions) -> anyhow::Result<Vec<(Board, PlaceReport)>> {
    place_weighted(netlist, footprints, rules, opts, Weights::default())
}

/// [`place_candidates`] with explicit cost weights.
pub fn place_weighted(netlist: &Netlist, footprints: &[Footprint], rules: &Rules, opts: &PlaceOptions, weights: Weights) -> anyhow::Result<Vec<(Board, PlaceReport)>> {
    let started = std::time::Instant::now();
    anyhow::ensure!(footprints.len() == netlist.parts.len(), "{} footprints for {} parts (one per part, in part order)", footprints.len(), netlist.parts.len());
    anyhow::ensure!(opts.spacing_nm >= 0, "spacing_nm must be >= 0");
    let spacing = opts.spacing_nm;
    let ec = rules.edge_clearance.0.max(0);
    let inner = opts.outline.map(|(w, h)| Rect::from_corners(Pt::new(ec, ec), Pt::new(w - ec, h - ec)));
    if let Some(r) = &inner {
        anyhow::ensure!(r.width().0 > 0 && r.height().0 > 0, "outline {:?} leaves no room inside the edge clearance", opts.outline);
    }
    let mut weights = weights;
    if inner.is_some() {
        weights.area = 0.0;
    }
    let Built { prob, notes, fixed } = build(netlist, footprints, rules, opts, weights, inner)?;
    let n = footprints.len();
    let total = prob.parts.len();
    let freedom: Vec<legal::Freedom> = (0..n)
        .map(|i| legal::Freedom {
            turns: prob.turns[i].iter().map(|&q| q as i32).collect(),
            slide: match prob.seat[i] {
                Some(anneal::Seat::Edge(EdgeSide::Left | EdgeSide::Right)) => Some(false),
                Some(anneal::Seat::Edge(_)) => Some(true),
                _ => None,
            },
        })
        .collect();
    let can_turn: Vec<bool> = (0..n).map(|i| prob.turns[i].len() != 1).collect();
    let holes = opts.mounting_holes;
    let hole_rect = |c: Pt| {
        let r = holes.map_or(0, |h| h.keepout_nm);
        Rect::from_corners(Pt::new(c.x.0 - r, c.y.0 - r), Pt::new(c.x.0 + r, c.y.0 + r))
    };
    let pose_of = |p: &Placed| Pose { x: p.at.x.0 as f64 / NM, y: p.at.y.0 as f64 / NM, q: p.rot.rem_euclid(4) as usize };

    // Fixed poses: checked, never nudged.
    let mut base: Vec<Placed> = footprints.iter().enumerate().map(|(i, f)| Placed { part: i, footprint: f.clone(), at: Pt::default(), rot: 0, side: Side::Top, locked: false }).collect();
    for &(i, ps) in &fixed {
        base[i].at = Pt::new(snap(ps.x), snap(ps.y));
        base[i].rot = ps.q as i32;
        base[i].locked = true;
    }
    {
        let locked: Vec<Placed> = base.iter().filter(|p| p.locked).cloned().collect();
        let bad = legal::conflicts(&locked, spacing);
        anyhow::ensure!(bad.is_empty(), "fixed_poses collide: {}", bad.iter().map(|&(a, b)| format!("{} / {}", netlist.parts[locked[a].part].refdes, netlist.parts[locked[b].part].refdes)).collect::<Vec<_>>().join(", "));
    }
    let start: Vec<Pose> = (0..total).map(|i| fixed.iter().find(|f| f.0 == i).map_or(Pose { x: 0.0, y: 0.0, q: 0 }, |f| f.1)).collect();

    // Twelve seeds in parallel; results indexed by seed, so thread timing
    // never changes the answer.
    let seeds = SCATTERED_SEEDS + CLUSTERED_SEEDS;
    // Upstream's 500 moves per movable part per seed, capped by the budget.
    let movable = (0..n).filter(|&i| !prob.locked[i]).count() as u64;
    let per = (opts.max_moves / seeds).min(MOVES_PER_PART * movable).max(1);
    let outcomes: Vec<(u64, anneal::Outcome)> = if n == 0 || opts.max_moves == 0 {
        Vec::new()
    } else {
        std::thread::scope(|sc| {
            let handles: Vec<_> = (0..seeds)
                .map(|k| {
                    let (prob, start) = (&prob, start.clone());
                    sc.spawn(move || {
                        let mut model = Model::new(prob, start);
                        let seed = opts.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(k);
                        let mut a = Annealer::new(&mut model, seed);
                        a.scatter(k >= SCATTERED_SEEDS);
                        (k, a.run(per))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("anneal thread panicked")).collect()
        })
    };
    let moves: u64 = outcomes.iter().map(|o| o.1.moves).sum();

    // Snap, the exact gate (hole keep-outs as walls), and the cost each
    // legal seed ends at.
    type Cand = (f64, u64, Vec<Placed>, Vec<Pt>, Vec<String>, Vec<(String, f64)>);
    let mut legal_seeds: Vec<Cand> = Vec::new();
    for (k, out) in outcomes {
        let Some(poses) = out.best else { continue };
        let mut cand = base.clone();
        for (i, p) in poses.iter().take(n).enumerate() {
            if !cand[i].locked {
                cand[i].at = Pt::new(snap(p.x), snap(p.y));
                cand[i].rot = p.q as i32;
            }
        }
        let centres: Vec<Pt> = poses[n..].iter().map(|p| Pt::new(snap(p.x), snap(p.y))).collect();
        let walls: Vec<Rect> = centres.iter().map(|&c| hole_rect(c)).collect();
        let mut bad: Vec<usize> = legal::conflicts(&cand, spacing).iter().flat_map(|&(a, b)| [a, b]).filter(|&i| !cand[i].locked).collect();
        if !legal::legalize(&mut cand, spacing, inner.as_ref(), &walls, &freedom, 250_000, 6_000_000) {
            continue;
        }
        bad.sort_unstable();
        bad.dedup();
        let mut ps: Vec<Pose> = cand.iter().map(pose_of).collect();
        ps.extend(centres.iter().map(|c| Pose { x: c.x.0 as f64 / NM, y: c.y.0 as f64 / NM, q: 0 }));
        let model = Model::new(&prob, ps);
        legal_seeds.push((model.total(), k, cand, centres, bad.iter().map(|&i| netlist.parts[i].refdes.clone()).collect(), model.breakdown()));
    }
    legal_seeds.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    legal_seeds.truncate(JUDGED_SEEDS);
    let mut strategy = "anneal";
    if legal_seeds.is_empty() {
        strategy = "shelf";
        let movable: Vec<usize> = (0..n).filter(|&i| !base[i].locked).collect();
        let fps: Vec<Footprint> = movable.iter().map(|&i| footprints[i].clone()).collect();
        let turn: Vec<bool> = movable.iter().map(|&i| can_turn[i]).collect();
        let hole_w = holes.map_or(0, |h| 2 * h.keepout_nm + spacing);
        let sh = shelf::shelf_pack(&fps, &turn, spacing, inner.map(|r| r.width().0 - 2 * hole_w));
        let mut cand = base.clone();
        // Beside the fixed parts (or inside the outline's inset), with room
        // for the holes outside the shelf's corners.
        let off = match &inner {
            Some(r) => r.min + Pt::new(hole_w, hole_w),
            None => {
                let fx = cand.iter().filter(|p| p.locked).map(|p| p.courtyard_abs().max.x.0 + spacing).max().unwrap_or(0);
                Pt::new(fx + hole_w, hole_w)
            }
        };
        for (k, &i) in movable.iter().enumerate() {
            cand[i].at = sh.poses[k].0 + off;
            cand[i].rot = sh.poses[k].1;
        }
        let mut centres = Vec::new();
        if let Some(h) = holes {
            let bb = cand.iter().map(Placed::courtyard_abs).reduce(|a, b| a.union(&b)).unwrap_or_default();
            let d = h.keepout_nm + spacing;
            for (x, y) in [(bb.min.x.0 - d, bb.min.y.0 - d), (bb.max.x.0 + d, bb.min.y.0 - d), (bb.min.x.0 - d, bb.max.y.0 + d), (bb.max.x.0 + d, bb.max.y.0 + d)] {
                centres.push(Pt::new(x, y));
            }
        }
        let walls: Vec<Rect> = centres.iter().map(|&c| hole_rect(c)).collect();
        anyhow::ensure!(legal::conflicts(&cand, spacing).is_empty() && cand.iter().all(|p| walls.iter().all(|w| !legal::too_close(&p.courtyard_abs(), w, spacing))), "shelf fallback is not legal (fixed parts in the way)");
        let mut ps: Vec<Pose> = cand.iter().map(pose_of).collect();
        ps.extend(centres.iter().map(|c| Pose { x: c.x.0 as f64 / NM, y: c.y.0 as f64 / NM, q: 0 }));
        let model = Model::new(&prob, ps);
        legal_seeds.push((model.total(), 0, cand, centres, Vec::new(), model.breakdown()));
    }

    let mut out = Vec::new();
    for (cost, k, mut parts, mut centres, reseated, terms) in legal_seeds {
        let rects: Vec<Rect> = parts.iter().map(Placed::courtyard_abs).chain(centres.iter().map(|&c| hole_rect(c))).collect();
        let outline = match opts.outline {
            Some((w, h)) => {
                let o = Rect::from_corners(Pt::new(0, 0), Pt::new(w, h));
                let inr = inner.unwrap_or(o);
                for (j, c) in rects.iter().enumerate() {
                    let what = if j < n { netlist.parts[parts[j].part].refdes.clone() } else { format!("mounting hole {}", j - n + 1) };
                    anyhow::ensure!(c.min.x >= inr.min.x && c.min.y >= inr.min.y && c.max.x <= inr.max.x && c.max.y <= inr.max.y, "{what} does not fit inside the {}x{} mm outline", Nm(w).kicad(), Nm(h).kicad());
                }
                o
            }
            None => {
                let bb = rects.iter().copied().reduce(|a, b| a.union(&b)).unwrap_or_default();
                let up = |v: i64| (v + OUTLINE_GRID_NM - 1).div_euclid(OUTLINE_GRID_NM) * OUTLINE_GRID_NM;
                let (w, h) = (up(bb.width().0 + 2 * ec), up(bb.height().0 + 2 * ec));
                if fixed.is_empty() {
                    // Centre the parts in the grid-rounded outline at (0, 0).
                    let ox = ((w - bb.width().0) / 2 / GRID_NM) * GRID_NM - bb.min.x.0;
                    let oy = ((h - bb.height().0) / 2 / GRID_NM) * GRID_NM - bb.min.y.0;
                    for p in parts.iter_mut() {
                        p.at = p.at + Pt::new(ox, oy);
                    }
                    for c in centres.iter_mut() {
                        *c = *c + Pt::new(ox, oy);
                    }
                    Rect::from_corners(Pt::new(0, 0), Pt::new(w, h))
                } else {
                    // Fixed parts keep their board coordinates.
                    let x0 = (bb.min.x.0 - (w - bb.width().0) / 2).div_euclid(GRID_NM) * GRID_NM;
                    let y0 = (bb.min.y.0 - (h - bb.height().0) / 2).div_euclid(GRID_NM) * GRID_NM;
                    Rect::from_corners(Pt::new(x0, y0), Pt::new(x0 + w, y0 + h))
                }
            }
        };
        let classes = classes_for(netlist, &BTreeMap::new(), rules);
        let drill = Nm(holes.map_or(0, |h| h.drill_nm));
        let board = Board { outline, parts, tracks: Vec::new(), vias: Vec::new(), zones: Vec::new(), classes, rules: rules.clone(), holes: centres.iter().map(|&c| (c, drill)).collect() };
        let report = PlaceReport {
            cost,
            hpwl_nm: hpwl_nm(netlist, &board.parts),
            overlaps: legal::courtyard_overlaps(&board.parts),
            moves,
            seconds: started.elapsed().as_secs_f64(),
            area_mm2: board.outline.width().as_mm() * board.outline.height().as_mm(),
            decoupling_loop_mm: decoupling_loop_mm(netlist, &board),
            strategy: strategy.into(),
            seed: k,
            start: if strategy == "shelf" { "shelf".into() } else if k >= SCATTERED_SEEDS { "clustered".into() } else { "scattered".into() },
            reseated,
            notes: notes.clone(),
            terms,
        };
        out.push((board, report));
    }
    Ok(out)
}
