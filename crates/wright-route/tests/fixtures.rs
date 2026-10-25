//! Every baseline eval fixture, footprints from the installed KiCad library,
//! shelf-placed, routed with class widths. Skipped (with a message) when
//! the KiCad library is not installed.

mod common;
use wright_eda::Nm;
use wright_route::{route_with_stats, verify, RouteOptions};

#[test]
fn every_fixture_routes_fully_with_class_widths() {
    let Some(root) = common::kicad_footprints() else {
        eprintln!("skipped: KiCad footprint library not found (set WRIGHT_KICAD_SHARE)");
        return;
    };
    let mut failures = Vec::new();
    for name in common::FIXTURES {
        let (nl, mut board) = common::fixture_board(&root, name);
        let (rep, stats) = route_with_stats(&nl, &mut board, &RouteOptions::default());
        // attempted covers every net with >= 2 pins
        for (i, n) in nl.nets.iter().enumerate() {
            if n.pins.len() >= 2 {
                assert!(rep.attempted.contains(&i), "{name}: {} not attempted", n.name);
            }
        }
        let widths: Vec<String> = stats.nets.iter().filter(|s| s.width_mm > 0.2).map(|s| format!("{} {:.2}mm/{}v/{}neck{}", s.name, s.body_width_mm, s.vias, s.necks, if s.necks > 0 { format!(" {:.2}mm<= {:.2}mm long", s.min_width_mm, s.longest_neck_mm) } else { String::new() })).collect();
        println!(
            "{name:18} {:3.0}% routed ({}/{}), {:5.1}x{:5.1} mm, {} tracks, {} vias, {} stitch, {} ripups, {} exp, {:.3} s; wide: {}",
            rep.completion() * 100.0,
            rep.routed.len(),
            rep.attempted.len(),
            board.outline.width().as_mm(),
            board.outline.height().as_mm(),
            board.tracks.len(),
            board.vias.len(),
            stats.stitching_vias,
            stats.ripups,
            rep.expansions,
            rep.seconds,
            widths.join(", ")
        );
        for u in &rep.unrouted {
            println!("   unrouted {}: {}", u.name, u.reason);
        }
        let v = verify::clearance_violations(&nl, &board);
        for x in v.iter().take(5) {
            println!("   clearance: {x}");
        }
        if rep.completion() < 1.0 || !v.is_empty() {
            failures.push(name);
        }
        // Every track is at its class width except neck-down stubs, which
        // touch an own-net pad and are short (pad to first lattice node).
        let (pads, _) = wright_route::copper::pads(&nl, &board);
        for t in &board.tracks {
            let want = wright_route::router::rule_for(&board, t.net).width;
            if t.width < want {
                let ends = [t.a, t.b].map(|p| wright_route::geom::P::new(p.x.0 as f64, p.y.0 as f64));
                let at_pad = pads.iter().any(|p| p.net == Some(t.net) && ends.iter().any(|&e| p.shape.contains(e)));
                assert!(at_pad && t.a.dist(t.b) <= 2.0 * want.0 as f64 + 300_000.0, "{name}: {} track {} mm < class {} away from a pad", nl.nets[t.net].name, t.width.as_mm(), want.as_mm());
            }
        }
        if name == "servo_power_6v5a" {
            for net in ["VBAT", "SW", "+6V"] {
                let s = stats.nets.iter().find(|s| s.name == net).unwrap_or_else(|| panic!("{net} has no copper"));
                assert!(s.body_width_mm >= 1.5, "{net} {} mm", s.body_width_mm);
            }
            for net in ["SW", "+6V"] {
                let s = stats.nets.iter().find(|s| s.name == net).unwrap();
                let amps = wright_route::ipc::current_a(Nm::mm(s.body_width_mm), 1.0, 20.0);
                assert!(amps >= 5.0, "{net} carries only {amps:.2} A");
            }
            assert!(rep.poured.contains(&nl.net_index("GND").unwrap()));
        }
        // Work is bounded in expansions (deterministic); wall-clock is
        // asserted only on request because parallel lanes load this machine.
        assert!(rep.expansions < 1_500_000, "{name}: {} expansions", rep.expansions);
        if !cfg!(debug_assertions) && std::env::var("WRIGHT_ROUTE_TIMING").is_ok() {
            assert!(rep.seconds < 5.0, "{name} took {} s", rep.seconds);
        }
        // the seed judge's quick route of the same placement
        let mut qb = common::fixture_board(&root, name).1;
        let (qr, _) = route_with_stats(&nl, &mut qb, &RouteOptions::quick());
        let qs = wright_route::quick::score(&nl, &qb, &qr);
        println!("   quick: {:?} {} exp {:.3} s", qs, qr.expansions, qr.seconds);
    }
    assert!(failures.is_empty(), "not fully routed and clean: {failures:?}");
}

#[test]
fn routing_is_deterministic() {
    let Some(root) = common::kicad_footprints() else {
        return;
    };
    let (nl, b0) = common::fixture_board(&root, "servo_power_6v5a");
    let (mut a, mut b) = (b0.clone(), b0);
    let ra = wright_route::route(&nl, &mut a, &RouteOptions::default());
    let rb = wright_route::route(&nl, &mut b, &RouteOptions::default());
    assert_eq!(a, b);
    assert_eq!(ra.routed, rb.routed);
    assert_eq!(ra.expansions, rb.expansions);
}
