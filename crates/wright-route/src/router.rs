//! Net ordering, per-net tree growth, rip-up-and-retry, ground pour and
//! stitching. Structure follows the Python baseline's `routing.py` `route` /
//! `_route_net` / the rip-up loop (lines ~1360-1600) and the baseline's `board.py:2263`
//! `stitch_filled_nets`; ordering follows Topola's
//! `src/autorouter/planar_preconfigurer.rs` (sort by ratline intersection
//! count and length), after per-net ratlines from a minimum spanning tree
//! (Topola `src/autorouter/ratsnest.rs` builds them from a Delaunay
//! triangulation; with at most a few dozen pads per net, Prim's algorithm on
//! the complete graph gives the same tree).

use crate::astar::{Fail, Grid, Search, Spec, ORTHO};
use crate::copper::{self, Obst, PadItem, Store, B, F};
use crate::geom::{Shape, P};
use crate::RouteOptions;
use std::collections::{BTreeMap, VecDeque};
use wright_eda::ir::NetRole;
use wright_eda::{Board, Layer, Netlist, Nm, Pt, Track, Via, Zone};

/// Safety margin on every clearance, nm (KiCad rounds to 1 nm).
const MARGIN: f64 = 1_000.0;
/// Where a track ends inside a pad: this far in from the pad edge.
const INSET: f64 = 10_000.0;
/// Mechanical hole-to-copper clearance (KiCad's default `hole_clearance`).
const HOLE_CLR: f64 = 250_000.0;
/// Rip-up probe cost per node standing on another net's copper (the baseline's
/// `_RIP_COST_STEPS = 25` orthogonal steps).
/// A pad with fewer legal entries than this makes its net route early.
const SCARCE_ENTRIES: usize = 40;
const RIP_COST: u32 = 25 * ORTHO;
/// The baseline's `_MAX_RIPS_PER_NET`.
const MAX_RIPS_PER_NET: u32 = 2;
/// Proximity term (see [`Spec::prox_cost`]): up to one orthogonal step's
/// cost per node within 0.5 mm of fixed copper beyond clearance.
const PROX_NM: f64 = 500_000.0;
const PROX_COST: u32 = ORTHO;
/// The baseline's `_VIA_COST_STEPS` (12 orthogonal steps).
const VIA_COST: u32 = 12 * ORTHO;

/// Per-net routing rule resolved from the board's net classes.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub width: Nm,
    pub clr: Nm,
    pub via_d: Nm,
    pub via_drill: Nm,
}

pub fn rule_for(board: &Board, net: usize) -> Rule {
    let r = &board.rules;
    match board.class_of(net) {
        Some(c) => Rule {
            width: c.track_width.max(r.min_track),
            clr: c.clearance.max(r.min_clearance),
            via_d: c.via_diameter.max(r.min_via_diameter),
            via_drill: c.via_drill.max(r.min_via_drill),
        },
        None => Rule { width: r.min_track, clr: r.min_clearance, via_d: r.min_via_diameter, via_drill: r.min_via_drill },
    }
}

/// Per-net outcome detail beyond the frozen [`wright_eda::RouteReport`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct NetStat {
    pub net: usize,
    pub name: String,
    pub width_mm: f64,
    pub tracks: usize,
    pub vias: usize,
    pub length_mm: f64,
    /// Narrowest track laid for the net, mm, neck-down stubs included.
    pub min_width_mm: f64,
    /// Narrowest track away from the pads (every lattice track), mm: the
    /// class width.
    pub body_width_mm: f64,
    /// Neck-down stubs (narrower than the class, pad to first lattice node)
    /// and the longest one, mm.
    pub necks: usize,
    pub longest_neck_mm: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RouteStats {
    pub nets: Vec<NetStat>,
    pub ripups: u32,
    pub stitching_vias: usize,
    pub grid_mm: f64,
}

struct NetCopper {
    tracks: Vec<Track>,
    vias: Vec<Via>,
}

pub struct Router<'a> {
    netlist: &'a Netlist,
    opts: &'a RouteOptions,
    grid: Grid,
    pads: Vec<PadItem>,
    base: Vec<Obst>,
    rules: Vec<Rule>,
    /// Terminal groups per net: pads that already touch count as one.
    groups: Vec<Vec<Vec<usize>>>,
    committed: Vec<Option<NetCopper>>,
    store: Store,
    search: Search,
    pub expansions: u64,
    pub ripups: u32,
    pour_net: Option<usize>,
    min_track: Nm,
    /// Drills not laid by this run: vias already on the board, NPTH holes.
    fixed_drills: Vec<(P, f64, Option<usize>)>,
}

fn pt(p: P) -> Pt {
    Pt::new(p.x.round() as i64, p.y.round() as i64)
}

impl<'a> Router<'a> {
    pub fn new(netlist: &'a Netlist, board: &Board, opts: &'a RouteOptions) -> Router<'a> {
        let (pads, holes) = copper::pads(netlist, board);
        let rules: Vec<Rule> = (0..netlist.nets.len()).map(|n| rule_for(board, n)).collect();
        let o = board.outline;
        let g = opts.grid_nm.max(10_000);
        let grid = Grid {
            ox: o.min.x.0,
            oy: o.min.y.0,
            g,
            nx: ((o.width().0 / g) as usize) + 1,
            ny: ((o.height().0 / g) as usize) + 1,
            lo: P::new(o.min.x.0 as f64, o.min.y.0 as f64),
            hi: P::new(o.max.x.0 as f64, o.max.y.0 as f64),
            edge_clr: board.rules.edge_clearance.0 as f64 + MARGIN,
        };
        let min_clr = board.rules.min_clearance.0 as f64;
        let clr_of = |net: Option<usize>| net.map(|n| rules[n].clr.0 as f64).unwrap_or(min_clr);
        let mut base = Vec::new();
        for p in &pads {
            base.push(Obst { shape: p.shape, layers: p.layers, net: p.net, clr: clr_of(p.net), soft: false });
        }
        for &(c, r) in &holes {
            base.push(Obst { shape: Shape::circle(c, r), layers: F | B, net: None, clr: HOLE_CLR.max(min_clr), soft: false });
        }
        for (c, d) in &board.holes {
            base.push(Obst { shape: Shape::circle(P::new(c.x.0 as f64, c.y.0 as f64), d.0 as f64 / 2.0), layers: F | B, net: None, clr: HOLE_CLR.max(min_clr), soft: false });
        }
        // Copper already on the board (a template's locked tracks) is fixed.
        for t in &board.tracks {
            base.push(Obst { shape: copper::track_shape(t), layers: copper::layer_bit(t.layer), net: Some(t.net), clr: clr_of(Some(t.net)), soft: false });
        }
        for v in &board.vias {
            base.push(Obst { shape: copper::via_shape(v), layers: F | B, net: Some(v.net), clr: clr_of(Some(v.net)), soft: false });
        }
        let groups = (0..netlist.nets.len()).map(|n| terminal_groups(&pads, n)).collect();
        let pour_net = if opts.pour_ground { netlist.nets.iter().position(|n| n.role == NetRole::Ground && n.pins.len() >= 2) } else { None };
        let mut r = Router {
            netlist,
            opts,
            search: Search::new(&grid),
            grid,
            pads,
            base,
            rules,
            groups,
            committed: (0..netlist.nets.len()).map(|_| None).collect(),
            store: Store::new(P::new(0.0, 0.0), P::new(1.0, 1.0), 1.0),
            expansions: 0,
            ripups: 0,
            pour_net,
            min_track: board.rules.min_track,
            fixed_drills: board
                .vias
                .iter()
                .map(|v| (P::new(v.at.x.0 as f64, v.at.y.0 as f64), v.drill.0 as f64 / 2.0, Some(v.net)))
                .chain(holes.iter().map(|&(c, r)| (c, r, None)))
                .chain(board.holes.iter().map(|(c, d)| (P::new(c.x.0 as f64, c.y.0 as f64), d.0 as f64 / 2.0, None)))
                .collect(),
        };
        r.rebuild();
        r
    }

    /// Obstacles from the base state plus every committed net, from scratch
    /// (the baseline: lifted nets are replayed from the base, never un-marked,
    /// because clearance halos overlap).
    fn rebuild(&mut self) {
        let lo = P::new(self.grid.lo.x - 5e6, self.grid.lo.y - 5e6);
        let hi = P::new(self.grid.hi.x + 5e6, self.grid.hi.y + 5e6);
        let mut s = Store::new(lo, hi, 1_000_000.0);
        for o in &self.base {
            s.push(o.clone());
        }
        self.store = s;
        for n in 0..self.committed.len() {
            if let Some(c) = self.committed[n].take() {
                self.add_copper(n, &c);
                self.committed[n] = Some(c);
            }
        }
        self.search.reset_cache();
    }

    fn add_copper(&mut self, net: usize, c: &NetCopper) {
        let clr = self.rules[net].clr.0 as f64;
        for t in &c.tracks {
            self.store.push(Obst { shape: copper::track_shape(t), layers: copper::layer_bit(t.layer), net: Some(net), clr, soft: true });
        }
        for v in &c.vias {
            self.store.push(Obst { shape: copper::via_shape(v), layers: F | B, net: Some(net), clr, soft: true });
        }
    }

    fn spec(&self, net: usize, probe: bool) -> Spec {
        let r = self.rules[net];
        let wide = r.width >= Nm::mm(1.0);
        let ground = Some(net) == self.pour_net;
        // Keep B.Cu for the ground pour: other nets pay 1.5x there, ground
        // pays 1.5x on F.Cu and its vias are cheap (each one is a stitch).
        let layer_mult = if self.pour_net.is_none() {
            [2, 2]
        } else if ground {
            [3, 2]
        } else {
            [2, 3]
        };
        let own_smd = self.pads.iter().filter(|p| p.net == Some(net) && !p.drilled).map(|p| p.shape).collect();
        Spec {
            net,
            hw: r.width.0 as f64 / 2.0 + MARGIN,
            clr: r.clr.0 as f64,
            via_r: r.via_d.0 as f64 / 2.0 + MARGIN,
            own_smd,
            layer_mult,
            // A wide power net through one small via is the weak link
            // (a 0.4 mm via is roughly a 1-2 A part), so wide nets avoid them.
            via_cost: if ground { VIA_COST / 3 } else if wide { VIA_COST * 8 } else { VIA_COST },
            allow_vias: self.opts.grid_nm > 0,
            probe_cost: probe.then_some(RIP_COST),
            prox_nm: PROX_NM,
            prox_cost: PROX_COST,
            h_weight: self.opts.heuristic_weight.max(1.0),
            via_drill_r: r.via_drill.0 as f64 / 2.0,
            own_drills: self.drills(Some(net)),
        }
    }

    /// Legal places to leave or enter the pad group: lattice nodes clear at
    /// the net's width whose straight stub to a point inside the pad is
    /// also clear (the baseline's pad escape, `_MAX_ESCAPE_STEPS`, generalised: the
    /// stub is exact, so no pad is sealed by lattice phase). Each entry
    /// carries the stub's width: the class width when it fits, else a
    /// neck-down stub, never narrower than the pad's short side (or
    /// `min_track`), used only between the pad and its first lattice node.
    /// Two wide nets on adjacent 1.27 mm-pitch pins (TPS5450 VIN and PH)
    /// cannot both end at full width inside their pads: the full-width end
    /// of one spills over the gap between the pads.
    fn entries(&mut self, s: &Spec, group: &[usize]) -> Vec<(u32, P, Nm)> {
        let mut out = Vec::new();
        let reach = s.hw + 2.0 * self.grid.g as f64;
        let width = Nm(((s.hw - MARGIN) * 2.0).round() as i64);
        for &pi in group {
            let pad = self.pads[pi].clone();
            let (lo, hi) = pad.shape.bbox();
            let short = Nm((hi.x - lo.x).min(hi.y - lo.y) as i64);
            let floor = width.min(short.max(self.min_track));
            let mut widths = vec![width];
            let mut w = width;
            while w.0 - 50_000 >= floor.0 {
                w = Nm(w.0 - 50_000);
                widths.push(w);
            }
            if *widths.last().unwrap() != floor {
                widths.push(floor);
            }
            let Some((i0, i1, j0, j1)) = self.grid.span(P::new(lo.x - reach, lo.y - reach), P::new(hi.x + reach, hi.y + reach)) else {
                continue;
            };
            for (l, bit) in [(0usize, F), (1, B)] {
                if pad.layers & bit == 0 {
                    continue;
                }
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let n = self.grid.idx(i, j, l);
                        let p = self.grid.pt(n);
                        if pad.shape.dist_seg(p, p) > reach {
                            continue;
                        }
                        if !self.search.node_ok(&self.grid, &self.store, s, n) {
                            continue;
                        }
                        let anchors = pad.anchors(p, INSET);
                        'w: for &w in &widths {
                            let sw = Spec { hw: w.0 as f64 / 2.0 + MARGIN, ..s.clone() };
                            for &q in &anchors {
                                if p.dist(q) <= 0.5 || Search::seg_ok(&self.grid, &self.store, &sw, p, q, bit) {
                                    out.push((n, q, w));
                                    break 'w;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Prefer full-width entries: sort by node, widest first, keep one.
        out.sort_by_key(|e| (e.0, std::cmp::Reverse(e.2)));
        out.dedup_by_key(|e| e.0);
        // Neck only where the pad gives no full-width way in at all.
        if out.iter().any(|e| e.2 == width) {
            out.retain(|e| e.2 == width);
        }
        out
    }

    fn group_bbox(&self, group: &[usize]) -> (P, P) {
        let mut lo = P::new(f64::MAX, f64::MAX);
        let mut hi = P::new(f64::MIN, f64::MIN);
        for &pi in group {
            let (a, b) = self.pads[pi].shape.bbox();
            lo = P::new(lo.x.min(a.x), lo.y.min(a.y));
            hi = P::new(hi.x.max(b.x), hi.y.max(b.y));
        }
        (lo, hi)
    }

    fn group_name(&self, group: &[usize]) -> String {
        let p = &self.pads[group[0]];
        format!("{}.{}", self.netlist.parts[p.part].refdes, p.number)
    }

    /// Grow the net's tree from its first terminal, joining the nearest
    /// unjoined terminal to everything laid so far (the baseline's `_route_net`).
    /// Returns the copper and, in probe mode, the nets it would cross.
    fn route_net(&mut self, net: usize, probe: bool) -> Result<(NetCopper, Vec<usize>), String> {
        // A wide power net first tries one layer: a 0.8/0.4 mm via carries
        // roughly 1.5 A (25 um plating, IPC-2221 internal curve), so a 5 A
        // rail through one via would be its weak link.
        if self.rules[net].width >= Nm::mm(1.0) && Some(net) != self.pour_net {
            let mut s = self.spec(net, probe);
            s.allow_vias = false;
            if let Ok(r) = self.route_net_with(net, probe, s) {
                return Ok(r);
            }
        }
        let s = self.spec(net, probe);
        self.route_net_with(net, probe, s)
    }

    fn route_net_with(&mut self, net: usize, probe: bool, mut s: Spec) -> Result<(NetCopper, Vec<usize>), String> {
        self.search.reset_cache();
        let groups = self.groups[net].clone();
        let width = self.rules[net].width;
        let mut copper = NetCopper { tracks: Vec::new(), vias: Vec::new() };
        let mut victims = Vec::new();
        if groups.len() < 2 {
            return Ok((copper, victims));
        }
        let order = prim_order(&groups, &self.pads);
        let mut sources: BTreeMap<u32, Option<(P, Nm)>> = BTreeMap::new();
        for (n, q, w) in self.entries(&s, &groups[order[0]]) {
            sources.entry(n).or_insert(Some((q, w)));
        }
        let mut budget = self.opts.max_expansions_per_net;
        for &gi in &order[1..] {
            let goals = self.entries(&s, &groups[gi]);
            if goals.is_empty() {
                return Err(format!("no legal {} mm entry to pad {}", width.as_mm(), self.group_name(&groups[gi])));
            }
            if sources.is_empty() {
                return Err(format!("no legal {} mm exit from pad {}", width.as_mm(), self.group_name(&groups[order[0]])));
            }
            let remaining = self.opts.max_expansions.saturating_sub(self.expansions);
            let goal_ids: Vec<u32> = goals.iter().map(|g| g.0).collect();
            self.search.mark_goals(&goal_ids);
            let src: Vec<u32> = sources.keys().copied().collect();
            let before = self.search.expansions;
            let target = self.group_bbox(&groups[gi]);
            let res = self.search.run(&self.grid, &self.store, &s, &src, target, budget.min(remaining));
            let used = self.search.expansions - before;
            self.expansions += used;
            budget = budget.saturating_sub(used);
            let path = match res {
                Ok(p) => p,
                Err(Fail::Blocked) => return Err(format!("blocked: no {} mm channel to pad {}", width.as_mm(), self.group_name(&groups[gi]))),
                Err(Fail::Budget) => return Err(format!("expansion budget spent reaching pad {}", self.group_name(&groups[gi]))),
            };
            let start_anchor = sources.get(&path[0]).copied().flatten();
            let end_anchor = goals.iter().find(|g| g.0 == *path.last().unwrap()).map(|g| (g.1, g.2));
            let (tracks, vias) = self.emit(net, width, &path, start_anchor, end_anchor);
            if probe {
                let hw = s.hw;
                for t in &tracks {
                    let sh = copper::track_shape(t);
                    if let Shape::Seg { a, b, .. } = sh {
                        self.store.soft_conflicts(a, b, copper::layer_bit(t.layer), net, s.clr, hw, &mut victims);
                    }
                }
                for v in &vias {
                    let c = P::new(v.at.x.0 as f64, v.at.y.0 as f64);
                    self.store.soft_conflicts(c, c, F | B, net, s.clr, s.via_r, &mut victims);
                }
            }
            for &n in &path {
                sources.insert(n, None);
            }
            for v in &vias {
                // A via joins both layers at its node.
                let (x, y) = (v.at.x.0, v.at.y.0);
                let i = ((x - self.grid.ox) / self.grid.g) as usize;
                let j = ((y - self.grid.oy) / self.grid.g) as usize;
                sources.insert(self.grid.idx(i, j, 0), None);
                sources.insert(self.grid.idx(i, j, 1), None);
            }
            copper.tracks.extend(tracks);
            if !vias.is_empty() {
                s.own_drills.extend(vias.iter().map(|v| (P::new(v.at.x.0 as f64, v.at.y.0 as f64), v.drill.0 as f64 / 2.0)));
                self.search.reset_via_cache();
            }
            copper.vias.extend(vias);
            for (n, q, w) in self.entries(&s, &groups[gi]) {
                sources.entry(n).or_insert(Some((q, w)));
            }
        }
        victims.sort_unstable();
        Ok((copper, victims))
    }

    /// Lattice path to tracks (collinear steps merged) and vias.
    fn emit(&self, net: usize, width: Nm, path: &[u32], start: Option<(P, Nm)>, end: Option<(P, Nm)>) -> (Vec<Track>, Vec<Via>) {
        let r = self.rules[net];
        let mut tracks = Vec::new();
        let mut vias = Vec::new();
        let layer = |n: u32| if self.grid.ijl(n).2 == 0 { Layer::FCu } else { Layer::BCu };
        let xy = |n: u32| {
            let (x, y) = self.grid.xy(n);
            Pt::new(x, y)
        };
        let mut push = |a: Pt, b: Pt, l: Layer, width: Nm| {
            if a != b {
                tracks.push(Track { a, b, width, layer: l, net });
            }
        };
        if let Some((q, w)) = start {
            push(pt(q), xy(path[0]), layer(path[0]), w);
        }
        let mut run_start = path[0];
        let mut run_dir: Option<(i64, i64)> = None;
        for w in path.windows(2) {
            let (u, v) = (w[0], w[1]);
            if layer(u) != layer(v) {
                push(xy(run_start), xy(u), layer(u), width);
                vias.push(Via { at: xy(u), diameter: r.via_d, drill: r.via_drill, net });
                run_start = v;
                run_dir = None;
                continue;
            }
            let (pu, pv) = (xy(u), xy(v));
            let d = ((pv.x.0 - pu.x.0).signum(), (pv.y.0 - pu.y.0).signum());
            if run_dir.is_some() && run_dir != Some(d) {
                push(xy(run_start), pu, layer(u), width);
                run_start = u;
            }
            run_dir = Some(d);
        }
        let last = *path.last().unwrap();
        push(xy(run_start), xy(last), layer(last), width);
        if let Some((q, w)) = end {
            push(xy(last), pt(q), layer(last), w);
        }
        (tracks, vias)
    }

    fn commit(&mut self, net: usize, c: NetCopper) {
        self.add_copper(net, &c);
        self.committed[net] = Some(c);
    }

    /// Order: Topola presort (ratline crossings, then length), with the
    /// plan's tie-breaks (fewest pins), wide power nets first so they get
    /// the room the baseline never gave them, and the poured ground last.
    fn order(&mut self, nets: &[usize]) -> Vec<usize> {
        // Fewest legal ways into its tightest pad on the empty board: a net
        // with a handful (a 1.5 mm rail into a 1.27 mm-pitch pin) goes before
        // a neighbour that has hundreds and would take them. Wide nets only:
        // fine-pitch signal pins are all scarce, and reordering them helps none.
        let scarce: Vec<bool> = nets
            .iter()
            .map(|&n| {
                let s = self.spec(n, false);
                self.search.reset_cache();
                let groups = self.groups[n].clone();
                groups.iter().map(|g| self.entries(&s, g).len()).min().unwrap_or(0) < SCARCE_ENTRIES
            })
            .collect();
        let lines: Vec<Vec<(P, P)>> = (0..self.netlist.nets.len())
            .map(|n| {
                let g = &self.groups[n];
                if g.len() < 2 {
                    return Vec::new();
                }
                let ord = prim_edges(g, &self.pads);
                ord.into_iter().map(|(a, b)| (centre(&self.pads, &g[a]), centre(&self.pads, &g[b]))).collect()
            })
            .collect();
        let mut keyed: Vec<(u8, u8, u8, usize, usize, i64, usize)> = nets
            .iter()
            .enumerate()
            .map(|(k, &n)| {
                let crossings = nets
                    .iter()
                    .filter(|&&m| m != n)
                    .map(|&m| lines[n].iter().map(|a| lines[m].iter().filter(|b| crate::geom::seg_seg(a.0, a.1, b.0, b.1) == 0.0 && a.0 != b.0 && a.0 != b.1 && a.1 != b.0 && a.1 != b.1).count()).sum::<usize>())
                    .sum();
                let len: f64 = lines[n].iter().map(|l| l.0.dist(l.1)).sum();
                let ground = (Some(n) == self.pour_net) as u8;
                let narrow = (self.rules[n].width < Nm::mm(1.0)) as u8;
                (ground, narrow, !(scarce[k] && narrow == 0) as u8, crossings, self.groups[n].len(), len as i64, n)
            })
            .collect();
        keyed.sort();
        keyed.into_iter().map(|k| k.6).collect()
    }

    /// Route every net in `nets`; returns failure reasons by net.
    pub fn run(&mut self, nets: &[usize]) -> BTreeMap<usize, String> {
        let order = self.order(nets);
        let mut failed: VecDeque<(usize, String)> = VecDeque::new();
        for &n in &order {
            match self.route_net(n, false) {
                Ok((c, _)) => self.commit(n, c),
                Err(e) => failed.push_back((n, e)),
            }
        }
        // Rip-up and retry (the baseline's routing.py "rip-up and retry"): the blocked
        // net probes through foreign router copper at a cost, the nets it
        // crosses are lifted, it routes, and they re-route behind it.
        let mut final_fail = BTreeMap::new();
        let mut rips = vec![0u32; self.netlist.nets.len()];
        while let Some((n, why)) = failed.pop_front() {
            if self.committed[n].is_some() {
                continue;
            }
            if self.ripups >= self.opts.max_ripups || rips[n] >= MAX_RIPS_PER_NET || self.expansions >= self.opts.max_expansions || why.starts_with("expansion") {
                final_fail.insert(n, why);
                continue;
            }
            self.ripups += 1;
            rips[n] += 1;
            let victims = match self.route_net(n, true) {
                Ok((_, v)) if !v.is_empty() => v,
                Ok(_) => {
                    final_fail.insert(n, why);
                    continue;
                }
                Err(e) => {
                    final_fail.insert(n, format!("{why}; rip-up probe: {e}"));
                    continue;
                }
            };
            for &v in &victims {
                self.committed[v] = None;
            }
            self.rebuild();
            match self.route_net(n, false) {
                Ok((c, _)) => self.commit(n, c),
                Err(e) => failed.push_back((n, format!("ripped up {} for it and still: {e}", self.names(&victims)))),
            }
            for &v in &victims {
                match self.route_net(v, false) {
                    Ok((c, _)) => self.commit(v, c),
                    Err(e) => failed.push_back((v, format!("lifted for {}, then: {e}", self.netlist.nets[n].name))),
                }
            }
        }
        final_fail
    }

    fn names(&self, nets: &[usize]) -> String {
        nets.iter().map(|&n| self.netlist.nets[n].name.as_str()).collect::<Vec<_>>().join(", ")
    }

    /// Ground pour on B.Cu over the outline, plus a stitching via beside
    /// every F.Cu SMD pad of the poured net that has none within 2 mm
    /// (the baseline's `board.py:2263 stitch_filled_nets`, its fan-out half; the
    /// grid half joins two pours and Wright pours one layer).
    pub fn pour(&mut self, board: &Board) -> Option<(Zone, usize)> {
        let net = self.pour_net?;
        let r = self.rules[net];
        let s = self.spec(net, false);
        self.search.reset_cache();
        let e = board.rules.edge_clearance;
        let o = board.outline;
        let outline = vec![
            Pt { x: o.min.x + e, y: o.min.y + e },
            Pt { x: o.max.x - e, y: o.min.y + e },
            Pt { x: o.max.x - e, y: o.max.y - e },
            Pt { x: o.min.x + e, y: o.max.y - e },
        ];
        let zone = Zone { net, layer: Layer::BCu, outline, clearance: r.clr.max(Nm::mm(0.3)), min_thickness: Nm::mm(0.25), thermal_spoke: r.width.max(Nm::mm(0.5)) };
        let mut added = 0;
        let smd: Vec<usize> = (0..self.pads.len()).filter(|&i| self.pads[i].net == Some(net) && !self.pads[i].drilled && self.pads[i].layers == F).collect();
        for pi in smd {
            let pad = self.pads[pi].clone();
            let near = self.committed[net].as_ref().map(|c| c.vias.iter().any(|v| P::new(v.at.x.0 as f64, v.at.y.0 as f64).dist(pad.centre) < 2e6)).unwrap_or(false);
            if near {
                continue;
            }
            if let Some((t, v)) = self.stitch_spot(&s, &pad, r) {
                let c = NetCopper { tracks: t.into_iter().collect(), vias: vec![v] };
                self.add_copper(net, &c);
                let entry = self.committed[net].get_or_insert(NetCopper { tracks: Vec::new(), vias: Vec::new() });
                entry.tracks.extend(c.tracks);
                entry.vias.extend(c.vias);
                added += 1;
            }
        }
        Some((zone, added))
    }

    fn stitch_spot(&self, s: &Spec, pad: &PadItem, r: Rule) -> Option<(Option<Track>, Via)> {
        // Every drill on the board, any net, the stitches placed so far
        // included (they are committed as they are placed): two stitching
        // vias at one spot was kicad-cli's `hole_to_hole` 0.000 mm.
        let drills = self.drills(None);
        let drill_r = r.via_drill.0 as f64 / 2.0;
        let (lo, hi) = pad.shape.bbox();
        let half = P::new((hi.x - lo.x) / 2.0, (hi.y - lo.y) / 2.0);
        let c = pad.centre;
        let dirs = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
        let mut gap = s.via_r + s.clr;
        while gap < 3e6 {
            for (dx, dy) in dirs {
                let v = P::new((c.x + dx * (half.x + gap)).round(), (c.y + dy * (half.y + gap)).round());
                let sl = self.store.slack(v, v, F | B, s.net, s.clr, s.via_r + 1.0);
                let edge = (v.x - self.grid.lo.x).min(self.grid.hi.x - v.x).min(v.y - self.grid.lo.y).min(self.grid.hi.y - v.y) - self.grid.edge_clr;
                if sl.hard < s.via_r || sl.soft < s.via_r || edge < s.via_r {
                    continue;
                }
                if s.own_smd.iter().any(|sh| sh.dist_seg(v, v) - s.clr < s.via_r) {
                    continue;
                }
                if !copper::drill_clear(v, drill_r, &drills) {
                    continue;
                }
                let q = pad.anchor_towards(v, INSET);
                let tl = self.store.slack(q, v, F, s.net, s.clr, s.hw + 1.0);
                // The outline is a rectangle, so a straight track's closest
                // approach to Edge.Cuts is at an end: check both.
                if tl.hard < s.hw || tl.soft < s.hw || self.grid.edge_slack(q).min(self.grid.edge_slack(v)) < s.hw {
                    continue;
                }
                let at = pt(v);
                let track = (pt(q) != at).then(|| Track { a: pt(q), b: at, width: r.width, layer: Layer::FCu, net: s.net });
                return Some((track, Via { at, diameter: r.via_d, drill: r.via_drill, net: s.net }));
            }
            gap += 250_000.0;
        }
        None
    }

    /// Write committed copper into the board, nets in index order.
    pub fn finish(&mut self, board: &mut Board) -> Vec<NetStat> {
        let mut stats = Vec::new();
        for (n, c) in self.committed.iter().enumerate() {
            let Some(c) = c else { continue };
            let len: f64 = c.tracks.iter().map(|t| t.a.dist(t.b)).sum();
            let w = self.rules[n].width;
            let minw = c.tracks.iter().map(|t| t.width).min().unwrap_or(w);
            let necks: Vec<&Track> = c.tracks.iter().filter(|t| t.width < w).collect();
            let body = c.tracks.iter().filter(|t| t.width >= w).map(|t| t.width).min().unwrap_or(w);
            stats.push(NetStat {
                net: n,
                name: self.netlist.nets[n].name.clone(),
                width_mm: self.rules[n].width.as_mm(),
                tracks: c.tracks.len(),
                vias: c.vias.len(),
                length_mm: len / 1e6,
                min_width_mm: minw.as_mm(),
                body_width_mm: body.as_mm(),
                necks: necks.len(),
                longest_neck_mm: necks.iter().map(|t| t.a.dist(t.b)).fold(0.0, f64::max) / 1e6,
            });
            board.tracks.extend(c.tracks.iter().cloned());
            board.vias.extend(c.vias.iter().cloned());
        }
        stats
    }

    pub fn pads(&self) -> &[PadItem] {
        &self.pads
    }
}

fn centre(pads: &[PadItem], group: &[usize]) -> P {
    pads[group[0]].centre
}

/// Pads of `net`, grouped where their copper already touches (a thermal
/// pad and the vias inside it are one terminal).
pub fn terminal_groups(pads: &[PadItem], net: usize) -> Vec<Vec<usize>> {
    let idx: Vec<usize> = (0..pads.len()).filter(|&i| pads[i].net == Some(net)).collect();
    let mut parent: Vec<usize> = (0..idx.len()).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        p[x] = r;
        r
    }
    for a in 0..idx.len() {
        for b in a + 1..idx.len() {
            let (pa, pb) = (&pads[idx[a]], &pads[idx[b]]);
            if pa.layers & pb.layers != 0 && pa.shape.dist_shape(&pb.shape) <= 0.0 && pa.part == pb.part {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, &pad) in idx.iter().enumerate() {
        let r = find(&mut parent, k);
        groups.entry(r).or_default().push(pad);
    }
    groups.into_values().collect()
}

/// Prim's MST over group centres from group 0: edges (from, to).
fn prim_edges(groups: &[Vec<usize>], pads: &[PadItem]) -> Vec<(usize, usize)> {
    let n = groups.len();
    let c: Vec<P> = groups.iter().map(|g| centre(pads, g)).collect();
    let mut inside = vec![false; n];
    let mut best = vec![(f64::MAX, 0usize); n];
    inside[0] = true;
    for k in 1..n {
        best[k] = (c[0].dist(c[k]), 0);
    }
    let mut edges = Vec::new();
    for _ in 1..n {
        let k = (0..n).filter(|&k| !inside[k]).min_by(|&a, &b| best[a].0.total_cmp(&best[b].0).then(a.cmp(&b))).unwrap();
        inside[k] = true;
        edges.push((best[k].1, k));
        for m in 0..n {
            if !inside[m] {
                let d = c[k].dist(c[m]);
                if d < best[m].0 {
                    best[m] = (d, k);
                }
            }
        }
    }
    edges
}

/// Order in which to join terminals: the MST's Prim order, starting from
/// the group with the largest pad (a connector or regulator pin is a better
/// trunk root than a 0402).
fn prim_order(groups: &[Vec<usize>], pads: &[PadItem]) -> Vec<usize> {
    let area = |g: &Vec<usize>| {
        g.iter()
            .map(|&i| {
                let (lo, hi) = pads[i].shape.bbox();
                (hi.x - lo.x) * (hi.y - lo.y)
            })
            .fold(0.0, f64::max)
    };
    let root = (0..groups.len()).max_by(|&a, &b| area(&groups[a]).total_cmp(&area(&groups[b])).then(b.cmp(&a))).unwrap_or(0);
    let mut reordered: Vec<Vec<usize>> = Vec::with_capacity(groups.len());
    reordered.push(groups[root].clone());
    let mut map = vec![root];
    for (i, g) in groups.iter().enumerate() {
        if i != root {
            reordered.push(g.clone());
            map.push(i);
        }
    }
    let mut order = vec![map[0]];
    for (_, k) in prim_edges(&reordered, pads) {
        order.push(map[k]);
    }
    order
}

impl Router<'_> {
    /// Debug: legal entry nodes of each pad group of `net` given the copper
    /// committed so far (node, anchor), and the entry count per group.
    pub fn debug_entries(&mut self, net: usize) -> Vec<(String, usize)> {
        let s = self.spec(net, false);
        self.search.reset_cache();
        let groups = self.groups[net].clone();
        groups.iter().map(|g| (self.group_name(g), self.entries(&s, g).len())).collect()
    }
}

impl Router<'_> {
    /// Every drill on the board (centre, radius): drilled pads, NPTH holes,
    /// vias already on the board and every via the router has committed.
    /// `only` keeps one net's (pads and vias of that net).
    fn drills(&self, only: Option<usize>) -> Vec<(P, f64)> {
        let mut d: Vec<(P, f64)> = self.pads.iter().filter(|p| p.drilled && (only.is_none() || p.net == only)).map(|p| (p.centre, p.drill_r)).collect();
        for (n, c) in self.committed.iter().enumerate() {
            if only.is_some() && only != Some(n) {
                continue;
            }
            if let Some(c) = c {
                d.extend(c.vias.iter().map(|v| (P::new(v.at.x.0 as f64, v.at.y.0 as f64), v.drill.0 as f64 / 2.0)));
            }
        }
        d.extend(self.fixed_drills.iter().filter(|x| only.is_none() || x.2 == only).map(|x| (x.0, x.1)));
        d
    }
}
