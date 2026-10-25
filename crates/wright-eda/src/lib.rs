//! Shared electronics IR for Wright's PCB pipeline (the electronics rewrite,
//! `docs/electronics-plan.md`). Every lane crate depends on this one and on
//! nothing else of each other's, so the lanes build in parallel:
//!
//! ```text
//! Circuit (ir)  --validate-->  Netlist
//!   --wright-kicad::library-->  Vec<Footprint>          (board)
//!   --wright-place-->           Board with placements   (board)
//!   --wright-route-->           Board with copper + RouteReport
//!   --wright-kicad::pcb / wright-sch-->  .kicad_pcb / .kicad_sch / .kicad_pro
//!   --wright-kicad::cli-->      KicadReport (ERC / DRC / parity)
//! ```
//!
//! Rules this crate fixes for every lane:
//!
//! * **Integer nanometres** ([`units::Nm`]), KiCad's internal unit (the Python baseline's
//!   `engine/silkscreen/units.py:13`). No float length crosses a crate
//!   boundary.
//! * **One frame**: KiCad's board frame, Y down, rotation in degrees
//!   counter-clockwise as KiCad displays it. The baseline flipped Y at two
//!   boundaries (`kicad.py` "Coordinate frames"; `schematic.py`
//!   `_pin_on_sheet`) and shipped the bug class it guards against
//!   (known issue 9, rotated anchors). Here there is no solver frame;
//!   [`board::Placed::pad_abs`] is the only footprint-to-board transform.
//! * **Honest results**: a net the router did not finish is in
//!   [`board::RouteReport::unrouted`] with a reason, never dropped.

pub mod board;
pub mod ir;
pub mod sexpr;
pub mod units;

pub use board::{Board, Footprint, Layer, NetClass, Pad, PadShape, Placed, RouteReport, Rules, Side, Track, Unrouted, Via, Zone};
pub use ir::{Circuit, Netlist, Part, PartKind, PinRef, ValidationError};
pub use units::{Nm, Pt, Rect};
