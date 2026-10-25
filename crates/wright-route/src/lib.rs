//! Routing (lane D of docs/electronics-plan.md). Replaces the Python baseline's `routing.py`
//! (two-layer octilinear A* on a 0.25 mm lattice, rip-up-and-retry,
//! budgets in node expansions) and the copper parts of `board.py`
//! (fan-out, ground pour, stitching). Differences that are the point:
//! per-net-class track widths (the baseline routed every net at one width,
//! `routing.py:79`, so a 5 A rail got 0.2 mm or the whole board got 1.0 mm,
//! `aircraft/electronics.py build_servo_board`), exact clearance instead of
//! rasterised discs, and Rust speed (the baseline: 100 s for 18 parts,
//! `docs/measurements`).
//!
//! Design sources, by file:
//! * search: the baseline's `engine/silkscreen/routing.py` `_astar` (octile 12/17,
//!   no acute turns, via cost, expansion budgets) -> [`astar`];
//! * obstacle model: KiCadRoutingTools `rust_router/src/obstacle_map.rs`
//!   (per-layer obstacle lookup the A* queries), here with exact shapes ->
//!   [`copper`];
//! * net order: Topola `src/autorouter/planar_preconfigurer.rs` (presort by
//!   ratline intersections and length) over MST ratlines
//!   (`src/autorouter/ratsnest.rs`) -> [`router`];
//! * rip-up: the baseline's `routing.py` rip-up loop (probe with a cost on foreign
//!   copper, lift what it crosses, replay from the base state) -> [`router`];
//! * pour and stitching: the baseline's `board.py:2263 stitch_filled_nets` -> [`router`];
//! * track width: IPC-2221 `I = 0.048 ΔT^0.44 A^0.725` -> [`ipc`].
//!
//! Topola itself (`src/router/navmesh.rs`, `thetastar.rs`) was read as the
//! reference for a later topological router and is not a dependency.

pub mod astar;
pub mod copper;
pub mod geom;
pub mod ipc;
pub mod quick;
pub mod router;
pub mod verify;

pub use quick::{judge, judge_and_route, QuickScore};
pub use router::{NetStat, RouteStats};
use wright_eda::{Board, Netlist, RouteReport};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RouteOptions {
    pub grid_nm: i64,
    pub max_expansions: u64,
    pub max_expansions_per_net: u64,
    pub max_ripups: u32,
    /// Pour ground on B.Cu (and stitch with vias) as well as routing it.
    pub pour_ground: bool,
    /// Weighted-A* factor on the heuristic (1.0 = optimal A*).
    #[serde(default = "one")]
    pub heuristic_weight: f64,
    /// KiCadRoutingTools `route.py --quick LEVEL` (`py_router/routing_config.py`
    /// `quick: int = 0`): 0 routes to ship; 1 skips the recovery tail
    /// (fewer rip-ups, smaller budgets); 2, the seed judge's level, also
    /// skips the copper polish (pour and stitching).
    #[serde(default)]
    pub quick: u8,
}

fn one() -> f64 {
    1.0
}

impl Default for RouteOptions {
    fn default() -> Self {
        // Budgets from the baseline's routing.py:168,175,182 scaled for a 0.125 mm
        // lattice (the baseline used 0.25 mm; the finer grid fits 0.2 mm tracks
        // between 2.54 mm header pins at any phase).
        RouteOptions { grid_nm: 125_000, max_expansions: 4_000_000, max_expansions_per_net: 1_500_000, max_ripups: 8, pour_ground: true, heuristic_weight: 1.5, quick: 0 }
    }
}

impl RouteOptions {
    /// The seed judge's fast route (`--quick 2`): the baseline's original budgets
    /// (400k total, 150k per net, `routing.py:168,175`), two rip-ups, the
    /// heuristic at KiCadRoutingTools' 2.3, no pour or stitching.
    pub fn quick() -> Self {
        RouteOptions { max_expansions: 400_000, max_expansions_per_net: 150_000, max_ripups: 2, pour_ground: false, heuristic_weight: 2.3, quick: 2, ..Default::default() }
    }
}

/// Lay copper on `board` in place. Every attempted net ends in `routed`,
/// `poured` or `unrouted` (with a reason): never silently dropped.
pub fn route(netlist: &Netlist, board: &mut Board, opts: &RouteOptions) -> RouteReport {
    route_with_stats(netlist, board, opts).0
}

/// [`route`] plus per-net widths, via counts and lengths.
///
/// `attempted` is every net with two or more netlist pins (ground
/// included). A net is reported `routed` only when [`verify::connected`]
/// finds every one of its pads joined by tracks and vias on the board as
/// written; the ground net is routed with tracks like any other and is also
/// listed in `poured` when its B.Cu zone is added.
pub fn route_with_stats(netlist: &Netlist, board: &mut Board, opts: &RouteOptions) -> (RouteReport, RouteStats) {
    let t0 = std::time::Instant::now();
    // The quick levels hold whatever else the options say.
    let mut eff = opts.clone();
    if opts.quick >= 1 {
        eff.max_ripups = eff.max_ripups.min(2);
        eff.max_expansions = eff.max_expansions.min(400_000);
        eff.max_expansions_per_net = eff.max_expansions_per_net.min(150_000);
    }
    if opts.quick >= 2 {
        eff.pour_ground = false;
    }
    let opts = &eff;
    let mut report = RouteReport::default();
    let mut stats = RouteStats { grid_mm: opts.grid_nm as f64 / 1e6, ..Default::default() };
    let mut missing: Vec<(usize, Vec<String>)> = Vec::new();
    let mut to_route = Vec::new();
    for (i, n) in netlist.nets.iter().enumerate() {
        if n.pins.len() < 2 {
            continue;
        }
        report.attempted.push(i);
        let m = verify::missing_pads(netlist, board, i);
        if m.is_empty() {
            to_route.push(i);
        } else {
            missing.push((i, m));
        }
    }
    if opts.grid_nm <= 0 || board.outline.width().0 <= 0 || board.outline.height().0 <= 0 {
        for &i in &report.attempted {
            report.unrouted.push(wright_eda::Unrouted { net: i, name: netlist.nets[i].name.clone(), reason: "no routing area (empty outline or grid)".into() });
        }
        return (report, stats);
    }
    let mut r = router::Router::new(netlist, board, opts);
    let failures = r.run(&to_route);
    let pour = r.pour(board);
    stats.nets = r.finish(board);
    stats.ripups = r.ripups;
    if let Some((zone, stitched)) = pour {
        report.poured.push(zone.net);
        board.zones.push(zone);
        stats.stitching_vias = stitched;
    }
    report.expansions = r.expansions;
    let joined = verify::connected(netlist, board);
    for &i in &report.attempted {
        let name = netlist.nets[i].name.clone();
        if let Some((_, m)) = missing.iter().find(|(n, _)| *n == i) {
            report.unrouted.push(wright_eda::Unrouted { net: i, name, reason: format!("pads not on the board: {}", m.join(", ")) });
        } else if joined[i] {
            report.routed.push(i);
        } else {
            let reason = failures.get(&i).cloned().unwrap_or_else(|| "copper does not join every pad (router self-check)".into());
            report.unrouted.push(wright_eda::Unrouted { net: i, name, reason });
        }
    }
    report.seconds = t0.elapsed().as_secs_f64();
    (report, stats)
}
