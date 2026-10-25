//! Toolpaths as data, their G-code, and their time.
//!
//! G-code dialect: LinuxCNC (rs274ngc), which accepts the Fanuc subset used
//! here. Conventions taken from the LinuxCNC G-code reference
//! (docs/src/gcode/g-code.adoc) and its sample programs (nc_files/):
//! * mill preamble `G21 G17 G90 G40 G49 G80 G94 G54`, `T# M6` then
//!   `G43 H#` tool length offset, `S# M3`, `M8` coolant, `G80` after a
//!   canned cycle, `M5 M9`, `G53 G0 Z0` to the machine's top, `M30`;
//! * peck drilling as the canned `G83 X Y Z R Q F` with `G98` (retract to
//!   the initial level between holes), kept canned in the file and
//!   expanded only for the simulation and the time;
//! * lathe preamble `G18 G7 G21 G90 G95`: XZ plane, X in DIAMETER (G7),
//!   feed per revolution (G95); `G96 D<max rpm> S<m/min>` constant surface
//!   speed; `G97 S<rpm>` for drilling on the centre line.
//!
//! Lathe moves are stored with `x` = RADIUS (the simulation's natural
//! unit) and written as diameter.

use crate::rules;
use serde::Serialize;
use wright_geom::V3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Machine {
    Mill,
    Router,
    Lathe,
}

impl Machine {
    pub fn name(self) -> &'static str {
        match self {
            Machine::Mill => rules::MILL_NAME,
            Machine::Router => rules::ROUTER_NAME,
            Machine::Lathe => rules::LATHE_NAME,
        }
    }
    fn rapid(self) -> f64 {
        match self {
            Machine::Mill => rules::MILL_RAPID_MM_MIN,
            Machine::Router => rules::ROUTER_RAPID_MM_MIN,
            Machine::Lathe => rules::LATHE_RAPID_MM_MIN,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum ToolKind {
    FlatEnd,
    Drill,
    /// External turning insert, nose radius mm.
    Turning(f64),
    /// Boring bar, nose radius mm.
    Boring(f64),
    /// Grooving / parting blade, width mm.
    Groove(f64),
}

#[derive(Clone, Debug, Serialize)]
pub struct Tool {
    pub number: u32,
    pub name: String,
    pub dia: f64,
    pub kind: ToolKind,
}

#[derive(Clone, Debug, Serialize)]
pub enum Mv {
    Rapid(V3),
    /// Linear feed; `f` is mm/min on a mill, mm/rev on the lathe.
    Feed(V3, f64),
    /// Peck drill at (x, y) from `r_plane` to `bottom`, `peck` per peck.
    Drill { x: f64, y: f64, bottom: f64, r_plane: f64, peck: f64, feed: f64 },
    /// Rotary indexer (4th axis) to an angle, degrees.
    Index(f64),
}

#[derive(Clone, Debug, Serialize)]
pub struct Op {
    pub name: String,
    pub tool: Tool,
    /// Spindle rpm (mill; lathe drilling with G97).
    pub rpm: f64,
    /// Lathe constant surface speed, m/min (G96), when set.
    pub css: Option<f64>,
    pub moves: Vec<Mv>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Setup {
    pub name: String,
    /// Lathe: the part is turned end for end before this setup.
    pub flipped: bool,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, Serialize)]
pub enum Stock {
    /// Rectangular block / sheet, mm: min corner at origin.
    Block { x: f64, y: f64, z: f64 },
    /// Round bar slice standing on its face (mill) or held in a chuck
    /// (lathe), diameter and length mm. Mill: centred on x = y = 0.
    Round { d: f64, len: f64 },
    /// Round bar with a bore already in it (tube).
    Tube { od: f64, id: f64, len: f64 },
}

#[derive(Clone, Debug, Serialize)]
pub struct Program {
    pub name: String,
    pub part: String,
    pub machine: Machine,
    pub material: String,
    pub density: f64,
    pub stock: Stock,
    pub setups: Vec<Setup>,
    /// Notes written into the file header (what the toolpath simplifies).
    pub notes: Vec<String>,
    /// How many of this part the vehicle needs.
    pub qty: u32,
}

/// One straight piece of motion, for the simulation and the time.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub setup: usize,
    pub op: usize,
    pub from: V3,
    pub to: V3,
    pub rapid: bool,
    pub minutes: f64,
    /// Indexer angle (deg) during this step.
    pub a: f64,
}

impl Program {
    pub fn tools(&self) -> Vec<&Tool> {
        let mut out: Vec<&Tool> = Vec::new();
        for s in &self.setups {
            for o in &s.ops {
                if !out.iter().any(|t| t.number == o.tool.number) {
                    out.push(&o.tool);
                }
            }
        }
        out
    }

    /// Every move as straight steps with their duration. Arcs do not
    /// occur (curves are emitted as chords); G83 is expanded the way
    /// LinuxCNC runs it (rapid to R, feed one peck deeper, rapid back to
    /// R, rapid down to just above the last depth).
    pub fn steps(&self) -> Vec<Step> {
        let mut out = Vec::new();
        let lathe = self.machine == Machine::Lathe;
        let rapid_rate = self.machine.rapid();
        let mut pos = V3 { x: 0.0, y: 0.0, z: 50.0 };
        if lathe {
            pos = V3 { x: self.stock_radius() + 20.0, y: 0.0, z: 20.0 };
        }
        let mut a = 0.0;
        for (si, s) in self.setups.iter().enumerate() {
            for (oi, o) in s.ops.iter().enumerate() {
                let push = |from: V3, to: V3, rapid: bool, feed: f64, a: f64, out: &mut Vec<Step>| {
                    let len = (to - from).norm();
                    let rate = if rapid {
                        rapid_rate
                    } else if lathe {
                        // feed per rev at the spindle speed of this radius
                        let r = 0.5 * (from.x + to.x).abs();
                        let n = match o.css {
                            Some(vc) => rules::rpm(vc, 2.0 * r, rules::LATHE_MAX_RPM),
                            None => o.rpm,
                        };
                        feed * n
                    } else {
                        feed
                    };
                    out.push(Step { setup: si, op: oi, from, to, rapid, minutes: if rate > 0.0 { len / rate } else { 0.0 }, a });
                };
                for m in &o.moves {
                    match *m {
                        Mv::Rapid(p) => {
                            push(pos, p, true, 0.0, a, &mut out);
                            pos = p;
                        }
                        Mv::Feed(p, f) => {
                            push(pos, p, false, f, a, &mut out);
                            pos = p;
                        }
                        Mv::Index(ang) => {
                            // the indexer turns at about 60 deg/s; as a zero-length step
                            out.push(Step { setup: si, op: oi, from: pos, to: pos, rapid: true, minutes: (ang - a).abs() / 3600.0, a: ang });
                            a = ang;
                        }
                        Mv::Drill { x, y, bottom, r_plane, peck, feed } => {
                            let init = pos.z.max(r_plane);
                            let top = V3 { x, y, z: init };
                            push(pos, top, true, 0.0, a, &mut out);
                            let rp = V3 { x, y, z: r_plane };
                            push(top, rp, true, 0.0, a, &mut out);
                            let mut depth = r_plane;
                            let mut cur = rp;
                            while depth > bottom + 1e-9 {
                                let next = (depth - peck).max(bottom);
                                if depth < r_plane {
                                    let start = V3 { x, y, z: depth + 0.5 };
                                    push(rp, start, true, 0.0, a, &mut out);
                                    cur = start;
                                }
                                let d = V3 { x, y, z: next };
                                push(cur, d, false, feed, a, &mut out);
                                push(d, rp, true, 0.0, a, &mut out);
                                cur = rp;
                                depth = next;
                            }
                            push(rp, top, true, 0.0, a, &mut out);
                            pos = top;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn stock_radius(&self) -> f64 {
        match self.stock {
            Stock::Block { x, y, .. } => 0.5 * x.max(y),
            Stock::Round { d, .. } => d / 2.0,
            Stock::Tube { od, .. } => od / 2.0,
        }
    }

    /// Cutting + rapid minutes, tool changes and setups included.
    pub fn minutes(&self) -> f64 {
        let motion: f64 = self.steps().iter().map(|s| s.minutes).sum();
        let tool_changes: usize = self.setups.iter().map(|s| s.ops.len()).sum();
        motion + tool_changes as f64 * rules::TOOL_CHANGE_MIN + self.setups.len() as f64 * rules::SETUP_MIN
    }

    pub fn stock_grams(&self) -> f64 {
        let v = match self.stock {
            Stock::Block { x, y, z } => x * y * z,
            Stock::Round { d, len } => std::f64::consts::PI * d * d / 4.0 * len,
            Stock::Tube { od, id, len } => std::f64::consts::PI * (od * od - id * id) / 4.0 * len,
        };
        v * 1e-3 * self.density
    }

    /// The program as G-code (LinuxCNC; see the module notes).
    pub fn gcode(&self) -> String {
        use std::fmt::Write as _;
        let mut g = String::new();
        let lathe = self.machine == Machine::Lathe;
        let _ = writeln!(g, "%");
        let _ = writeln!(g, "({} -- {})", self.name.to_uppercase(), self.part);
        let _ = writeln!(g, "(machine: {})", self.machine.name());
        let _ = writeln!(g, "(material: {}; stock: {})", self.material, stock_text(&self.stock));
        let _ = writeln!(g, "(generated by wright-fab; units mm; verify in a simulator before cutting)");
        for n in &self.notes {
            let _ = writeln!(g, "({})", n.replace(['(', ')'], ""));
        }
        let _ = writeln!(g, "(tools:)");
        for t in self.tools() {
            let _ = writeln!(g, "(  T{} {} D{:.3})", t.number, t.name, t.dia);
        }
        let x = |v: f64| if lathe { 2.0 * v } else { v };
        for s in &self.setups {
            let _ = writeln!(g, "\n(=== SETUP: {}{} ===)", s.name, if s.flipped { " -- part turned end for end" } else { "" });
            if lathe {
                let _ = writeln!(g, "G18 G7 G21 G90 G95 G40 G49 G54");
            } else {
                let _ = writeln!(g, "G21 G17 G90 G40 G49 G80 G94 G54");
            }
            for o in &s.ops {
                let t = &o.tool;
                let _ = writeln!(g, "\n({})", o.name);
                let _ = writeln!(g, "T{} M6", t.number);
                let _ = writeln!(g, "G43 H{}", t.number);
                match o.css {
                    Some(vc) => {
                        let _ = writeln!(g, "G96 D{:.0} S{:.0} M3", rules::LATHE_MAX_RPM, vc);
                    }
                    None => {
                        if lathe {
                            let _ = writeln!(g, "G97 S{:.0} M3", o.rpm);
                        } else {
                            let _ = writeln!(g, "S{:.0} M3", o.rpm);
                        }
                    }
                }
                let _ = writeln!(g, "M8");
                let mut in_cycle = false;
                let mut last_f = f64::NAN;
                for m in &o.moves {
                    match *m {
                        Mv::Drill { x: dx, y: dy, bottom, r_plane, peck, feed } => {
                            if lathe {
                                // on the lathe a centre-line drill is fed with G1 pecks
                                let _ = writeln!(g, "G0 X0 Z{:.3}", r_plane);
                                let mut d = r_plane;
                                while d > bottom + 1e-9 {
                                    d = (d - peck).max(bottom);
                                    let _ = writeln!(g, "G1 Z{:.3} F{}", d, fmt_f(feed));
                                    let _ = writeln!(g, "G0 Z{:.3}", r_plane);
                                }
                            } else {
                                let fpm = feed;
                                if !in_cycle {
                                    let _ = writeln!(g, "G98 G83 X{:.3} Y{:.3} Z{:.3} R{:.3} Q{:.3} F{:.0}", dx, dy, bottom, r_plane, peck, fpm);
                                    in_cycle = true;
                                } else {
                                    let _ = writeln!(g, "X{:.3} Y{:.3}", dx, dy);
                                }
                            }
                        }
                        _ if in_cycle => {
                            let _ = writeln!(g, "G80");
                            in_cycle = false;
                            write_move(&mut g, m, &x, lathe, &mut last_f);
                        }
                        _ => write_move(&mut g, m, &x, lathe, &mut last_f),
                    }
                }
                if in_cycle {
                    let _ = writeln!(g, "G80");
                }
                let _ = writeln!(g, "M9 M5");
                if lathe {
                    let _ = writeln!(g, "G97 S0");
                    let _ = writeln!(g, "G0 X{:.3} Z{:.3}", x(self.stock_radius() + 25.0), 50.0);
                } else {
                    let _ = writeln!(g, "G53 G0 Z0");
                }
            }
            let _ = writeln!(g, "M0 (operator: setup done; refixture before continuing)");
        }
        let _ = writeln!(g, "M30");
        let _ = writeln!(g, "%");
        g
    }
}

fn fmt_f(f: f64) -> String {
    if f < 5.0 { format!("{f:.3}") } else { format!("{f:.0}") }
}

fn write_move(g: &mut String, m: &Mv, x: &dyn Fn(f64) -> f64, lathe: bool, last_f: &mut f64) {
    use std::fmt::Write as _;
    match *m {
        Mv::Rapid(p) => {
            let _ = if lathe { writeln!(g, "G0 X{:.3} Z{:.3}", x(p.x), p.z) } else { writeln!(g, "G0 X{:.3} Y{:.3} Z{:.3}", p.x, p.y, p.z) };
        }
        Mv::Feed(p, f) => {
            let fs = if (f - *last_f).abs() > 1e-9 { format!(" F{}", fmt_f(f)) } else { String::new() };
            *last_f = f;
            let _ = if lathe { writeln!(g, "G1 X{:.3} Z{:.3}{fs}", x(p.x), p.z) } else { writeln!(g, "G1 X{:.3} Y{:.3} Z{:.3}{fs}", p.x, p.y, p.z) };
        }
        Mv::Index(a) => {
            let _ = writeln!(g, "G0 A{a:.3}");
        }
        Mv::Drill { .. } => {}
    }
}

pub fn stock_text(s: &Stock) -> String {
    match *s {
        Stock::Block { x, y, z } => format!("block {x:.1} x {y:.1} x {z:.2} mm"),
        Stock::Round { d, len } => format!("round bar {d:.1} mm dia x {len:.1} mm"),
        Stock::Tube { od, id, len } => format!("tube {od:.1} OD x {id:.1} ID x {len:.1} mm"),
    }
}
