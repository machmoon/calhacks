//! A small quadplane-shaped part set, one plate sliced with PrusaSlicer, a
//! made-up BOM and check report, written as a build guide.
//!
//! cargo run -p wright-guide --example demo

use std::f64::consts::PI;
use wright_core::check::{CheckId, Report, Source};
use wright_core::units::Money;
use wright_core::{Bom, BomLine};
use wright_geom::shapes::{cuboid, cylinder, loft};
use wright_geom::slice::{self, Recipe};
use wright_geom::{v3, Part, PartKind, V3};
use wright_guide::guide::{Block, Guide, Section, Step};

fn part(name: &str, kind: PartKind, material: &str, mesh: wright_geom::Mesh, touches: &[&str]) -> Part {
    Part { name: name.into(), kind, material: material.into(), mesh, touches: touches.iter().map(|s| s.to_string()).collect() }
}

/// NACA 0012-ish section at span station `y`: chord `c` from x0, z up.
fn section(y: f64, x0: f64, c: f64, z0: f64) -> Vec<V3> {
    let n = 24;
    let half = |x: f64| 0.6 * c * (0.2969 * x.sqrt() - 0.126 * x - 0.3516 * x * x + 0.2843 * x.powi(3) - 0.1036 * x.powi(4));
    let mut pts = Vec::new();
    for i in 0..n {
        let x = 0.5 * (1.0 - (PI * i as f64 / n as f64).cos()); // 0 -> 1
        pts.push(v3(x0 + (1.0 - x) * c, y, z0 + half(1.0 - x)));
    }
    for i in 0..n {
        let x = 0.5 * (1.0 - (PI * i as f64 / n as f64).cos());
        pts.push(v3(x0 + x * c, y, z0 - half(x)));
    }
    pts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Demo {
    PartsDoNotCollide,
    HoverCurrent,
    StaticMargin,
    PrintFitsBed,
    BudgetPriced,
}

impl CheckId for Demo {
    const ALL: &'static [Self] = &[Demo::PartsDoNotCollide, Demo::HoverCurrent, Demo::StaticMargin, Demo::PrintFitsBed, Demo::BudgetPriced];
    fn name(self) -> &'static str {
        match self {
            Demo::PartsDoNotCollide => "parts_do_not_collide",
            Demo::HoverCurrent => "hover_current",
            Demo::StaticMargin => "static_margin",
            Demo::PrintFitsBed => "print_fits_bed",
            Demo::BudgetPriced => "budget_priced",
        }
    }
    fn describe(self) -> &'static str {
        "demo"
    }
}

fn main() -> anyhow::Result<()> {
    let out = std::env::temp_dir().join("wright-guide-demo");
    std::fs::create_dir_all(&out)?;

    // frame: x aft, y starboard, z up (wright-air's layout frame)
    let mut parts = vec![
        part("fuselage", PartKind::Printed, "pla_aero", cuboid(v3(-120.0, -45.0, -40.0), v3(380.0, 45.0, 40.0)), &["wing", "battery", "flight_controller", "receiver"]),
        part("wing", PartKind::Printed, "pla_aero", loft(&[section(-700.0, 120.0, 150.0, 30.0), section(0.0, 80.0, 220.0, 30.0), section(700.0, 120.0, 150.0, 30.0)]), &["fuselage", "left_boom", "right_boom"]),
        part("battery", PartKind::Bought, "lipo_4s_5000", cuboid(v3(-60.0, -24.0, -30.0), v3(90.0, 24.0, 12.0)), &["fuselage"]),
        part("flight_controller", PartKind::Bought, "matek_h743", cuboid(v3(120.0, -18.0, -10.0), v3(156.0, 18.0, 0.0)), &["fuselage"]),
        part("receiver", PartKind::Bought, "elrs_rx", cuboid(v3(180.0, -10.0, -10.0), v3(200.0, 10.0, -4.0)), &["fuselage"]),
        part("h_stab", PartKind::Printed, "pla_aero", cuboid(v3(720.0, -270.0, 22.0), v3(800.0, 270.0, 28.0)), &["left_boom", "right_boom"]),
        part("pusher_motor", PartKind::Bought, "motor_2814", cylinder(v3(380.0, 0.0, 0.0), v3(410.0, 0.0, 0.0), 18.0, 32), &["fuselage", "pusher_prop"]),
        part("pusher_prop", PartKind::Bought, "prop_10x6", cylinder(v3(412.0, 0.0, 0.0), v3(416.0, 0.0, 0.0), 127.0, 48), &["pusher_motor"]),
    ];
    for (side, y) in [("left", -260.0), ("right", 260.0)] {
        parts.push(part(&format!("{side}_boom"), PartKind::Bought, "cf_tube_16mm", cylinder(v3(-230.0, y, 22.0), v3(810.0, y, 22.0), 8.0, 24), &["wing", "h_stab", &format!("{side}_front_motor"), &format!("{side}_rear_motor"), &format!("{side}_boom_clamp")]));
        parts.push(part(&format!("{side}_boom_clamp"), PartKind::Printed, "petg", cuboid(v3(150.0, y - 16.0, 10.0), v3(190.0, y + 16.0, 34.0)), &["wing", &format!("{side}_boom")]));
        for (end, x) in [("front", -215.0), ("rear", 795.0)] {
            parts.push(part(&format!("{side}_{end}_motor"), PartKind::Bought, "motor_2814", cylinder(v3(x, y, 30.0), v3(x, y, 58.0), 17.0, 32), &[&format!("{side}_boom"), &format!("{side}_{end}_prop")]));
            parts.push(part(&format!("{side}_{end}_prop"), PartKind::Bought, "prop_10x6", cylinder(v3(x, y, 60.0), v3(x, y, 64.0), 127.0, 48), &[&format!("{side}_{end}_motor")]));
        }
    }

    // slice the clamp (on its bed, as it would be printed)
    let mut plates = Vec::new();
    let clamp = parts.iter().find(|p| p.name == "left_boom_clamp").unwrap();
    let stl = out.join("left_boom_clamp.stl");
    clamp.mesh.on_bed().write_stl(&stl)?;
    match slice::slice(&stl, &out.join("left_boom_clamp.gcode"), slice::printer("prusa_mk4")?, slice::material("petg")?, Recipe::Structural { perimeters: 3, infill_pct: 30 }) {
        Ok(s) => plates.push(("left_boom_clamp".to_string(), s)),
        Err(e) => eprintln!("not sliced: {e}"),
    }

    let line = |cat: &str, id: &str, name: &str, qty: f64, price: Option<f64>, url: &str| BomLine {
        category: cat.into(),
        id: id.into(),
        name: name.into(),
        qty,
        unit: "ea".into(),
        unit_price: price.map(Money::usd),
        url: url.into(),
        note: String::new(),
    };
    let bom = Bom {
        lines: vec![
            line("propulsion", "motor_2814", "T-Motor AT2814 900KV", 5.0, Some(39.99), "https://store.tmotor.com/product/at2814-long-shaft-fixed-wing-motor.html"),
            line("propulsion", "prop_10x6", "APC 10x6E propeller", 5.0, Some(4.95), "https://www.apcprop.com/product/10x6e/"),
            line("power", "lipo_4s_5000", "4S 5000 mAh LiPo", 1.0, None, "https://www.getfpv.com/"),
            line("avionics", "matek_h743", "Matek H743-WING V3", 1.0, Some(84.99), "https://www.mateksys.com/?portfolio=h743-wing-v3"),
            line("avionics", "elrs_rx", "RadioMaster RP1 ELRS receiver", 1.0, Some(17.99), "https://www.radiomasterrc.com/products/rp1-expresslrs-2-4ghz-nano-receiver"),
            line("structure", "cf_tube_16mm", "Carbon tube 16 x 14 mm, 1 m", 2.0, Some(18.50), "https://www.rockwestcomposites.com/round-tube"),
        ],
    };

    let mut r = Report::<Demo>::new();
    r.margin(Demo::PartsDoNotCollide, 0.0, "mm^3", Source::Geometry, "all 20 parts, all pairs, declared contacts excluded");
    r.margin(Demo::HoverCurrent, -6.2, "A", Source::Physics, "68.2 A at hover vs 62 A pack limit (4S 5000 mAh, 12C continuous)");
    r.margin(Demo::StaticMargin, 4.1, "% MAC", Source::Physics, "neutral point 4.1 % MAC behind the CG");
    r.margin(Demo::PrintFitsBed, 2.0, "mm", Source::Slicer, "every plate inside its printer's bed");
    r.missing(Demo::BudgetPriced, Source::Requirement, "the 4S pack has no listed price");
    let checks = r.finish();

    let wiring = out.join("wiring.svg");
    std::fs::write(&wiring, WIRING_SVG)?;
    let params = out.join("quadplane.param");
    std::fs::write(&params, "Q_ENABLE,1\nQ_FRAME_CLASS,1\nQ_FRAME_TYPE,1\n")?;

    let mut g = Guide::new("Demo quadplane, 1.4 m", &parts, &bom, &checks);
    g.summary = "A 1.4 m span quadplane with a printed fuselage and wing on two carbon booms. Demo data: the numbers here are made up.".into();
    g.plates = plates;
    g.wiring = Some(wiring);
    g.files = vec![("ArduPilot parameters (load in Mission Planner)".into(), params)];
    g.steps = Some(vec![
        Step { title: "Fuselage and electronics".into(), parts: vec!["fuselage".into(), "flight_controller".into(), "receiver".into()], text: "Glue the two fuselage halves. Mount the flight controller on its standoffs, arrow pointing forward, and stick the receiver beside it.".into() },
        Step { title: "Wing".into(), parts: vec!["wing".into()], text: "Join the wing panels on the spar and slide the wing onto the fuselage saddle.".into() },
        Step { title: "Booms".into(), parts: vec!["left_boom".into(), "right_boom".into(), "left_boom_clamp".into(), "right_boom_clamp".into()], text: "Clamp each boom under the wing with its printed clamp. Tighten the M3 screws until the boom no longer turns.".into() },
        Step { title: "Tail".into(), parts: vec!["h_stab".into()], text: "Glue the stabiliser across the boom ends. Check it sits square to the wing.".into() },
        Step { title: "Motors".into(), parts: vec!["left_front_motor".into(), "left_rear_motor".into(), "right_front_motor".into(), "right_rear_motor".into(), "pusher_motor".into()], text: "Bolt each motor to its mount with threadlocker. Route the leads to the ESCs.".into() },
        Step { title: "Battery".into(), parts: vec!["battery".into()], text: "Strap the pack in the bay. Do not connect it yet.".into() },
    ]);
    g.before_use = Some(Section::new(
        "Before the first flight",
        vec![
            Block::Para("Do these with the propellers off unless a line says otherwise.".into()),
            Block::Checklist(vec![
                "Load quadplane.param and reboot the flight controller.".into(),
                "In Q_STABILIZE, tilt the plane: each motor should speed up on the low side.".into(),
                "Check every motor spins the way the arrow on its mount shows.".into(),
                "Move the sticks: ailerons and elevator move the right way in FBWA.".into(),
                "Balance on the CG mark with the battery in.".into(),
                "Fit the propellers last, then hover at 2 m for 30 s before any transition.".into(),
            ]),
        ],
    ));
    g.extra_sections = vec![Section::new("Notes", vec![Block::Para("This is the wright-guide demo; the real guides come from each domain's build().".into())])];

    let path = wright_guide::write_guide(&g, &out)?;
    println!("{}", path.display());
    Ok(())
}

const WIRING_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="260" viewBox="0 0 640 260" font-family="sans-serif" font-size="13">
<rect width="640" height="260" fill="#fff"/>
<rect x="20" y="100" width="120" height="60" rx="6" fill="#606670"/><text x="80" y="135" fill="#fff" text-anchor="middle">4S LiPo</text>
<rect x="260" y="90" width="130" height="80" rx="6" fill="#1d2129"/><text x="325" y="135" fill="#fff" text-anchor="middle">H743-WING</text>
<rect x="500" y="30" width="120" height="50" rx="6" fill="#78a6d6"/><text x="560" y="60" text-anchor="middle">ESC x5</text>
<rect x="500" y="180" width="120" height="50" rx="6" fill="#78a6d6"/><text x="560" y="210" text-anchor="middle">RP1 receiver</text>
<path d="M140 130 H260" stroke="#b42318" stroke-width="4"/><text x="200" y="122" text-anchor="middle">XT60, 16 V</text>
<path d="M390 115 L500 55" stroke="#1d2129" stroke-width="2"/><text x="440" y="75">S1-S5</text>
<path d="M390 145 L500 205" stroke="#1d2129" stroke-width="2"/><text x="420" y="195">UART2 (CRSF)</text>
</svg>"##;
