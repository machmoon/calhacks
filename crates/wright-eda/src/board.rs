//! The physical board: footprints from the KiCad library, placements,
//! copper, design rules, and the router's honest report.
//!
//! The split follows tscircuit's circuit-json (`src/source/*` logical vs
//! `src/pcb/*` physical, joined by ids): [`crate::ir::Netlist`] is the
//! `source_*` half, this module the `pcb_*` half, joined by part index and
//! net index. Field names follow KiCad's s-expression tokens
//! (dev-docs.kicad.org/en/file-formats/sexpr-pcb) so the writer is a
//! transcription.

use crate::units::{Nm, Pt, Rect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Layer {
    #[serde(rename = "F.Cu")]
    FCu,
    #[serde(rename = "B.Cu")]
    BCu,
}

impl Layer {
    pub fn kicad(self) -> &'static str {
        match self {
            Layer::FCu => "F.Cu",
            Layer::BCu => "B.Cu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    Rect,
    /// `roundrect_rratio` (0..0.5).
    RoundRect(f64),
    Circle,
    Oval,
    /// Anything else (custom, trapezoid): treated as its bounding rect.
    Other,
}

/// A pad in footprint-local coordinates (KiCad `.kicad_mod` frame).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    /// Pad number (`"1"`, `"A5"`); `""` for mechanical / NPTH.
    pub number: String,
    pub at: Pt,
    /// Pad's own rotation in the footprint, degrees (KiCad `(at x y a)`).
    pub rot_deg: f64,
    /// Width, height before rotation.
    pub size: (Nm, Nm),
    pub shape: PadShape,
    /// Through-hole drill diameter; `None` = SMD on the footprint's side.
    pub drill: Option<Nm>,
    /// Plated (`thru_hole`) vs `np_thru_hole`.
    pub plated: bool,
    /// Copper layers in the footprint's own (top-side) frame, from the
    /// `.kicad_mod` `(layers ...)` (`*.Cu` = both). Empty = the old rule:
    /// the part's side, or all copper for a through-hole pad. Added
    /// 2026-10-24 for pads like TI PowerPAD pad 9, which has B.Cu-only
    /// copper the router must see. Use [`Placed::pad_layers`].
    #[serde(default)]
    pub layers: Vec<Layer>,
}

impl Pad {
    /// Axis-aligned extent in footprint frame (pad rotation applied; for a
    /// non-90-degree pad rotation, the bounding box of the rotated rect).
    pub fn local_bbox(&self) -> Rect {
        let (w, h) = (self.size.0 .0 as f64, self.size.1 .0 as f64);
        let a = self.rot_deg.to_radians();
        let (c, s) = (a.cos().abs(), a.sin().abs());
        let hw = ((w * c + h * s) / 2.0).round() as i64;
        let hh = ((w * s + h * c) / 2.0).round() as i64;
        Rect::from_corners(Pt::new(self.at.x.0 - hw, self.at.y.0 - hh), Pt::new(self.at.x.0 + hw, self.at.y.0 + hh))
    }
}

/// A library footprint as the board needs it. `source` keeps the library
/// `.kicad_mod` s-expression verbatim so the writer emits exactly what the
/// library ships (silk, fab, 3D model) instead of a re-drawn land pattern:
/// The Python baseline generated its own (`footprints.py`, the `silkscreen:` library) and
/// shipped mirrored pin 1s until 2026-09-05 (the baseline's CLAUDE.md, "Verifying
/// against real KiCad").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    /// `Library:Name`, e.g. `Resistor_SMD:R_0603_1608Metric`.
    pub lib_id: String,
    pub pads: Vec<Pad>,
    /// Union of `F.CrtYd` graphics; falls back to pad bbox + 0.25 mm
    /// (IPC-7351 courtyard excess) when the library has none.
    pub courtyard: Rect,
    /// `(model ...)` path, if the library names one (for STEP export).
    pub model3d: Option<String>,
    /// The library file's text (`.kicad_mod`), emitted as-is with ref,
    /// value, position and pad nets substituted.
    #[serde(default)]
    pub source: String,
}

/// One part on the board: netlist part index, footprint, pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    /// Index into `Netlist::parts`.
    pub part: usize,
    pub footprint: Footprint,
    /// Footprint origin in the board frame.
    pub at: Pt,
    /// Quarter turns, KiCad CCW (0..=3). Placement is restricted to 90-degree
    /// steps (the baseline's packer did the same, `packing.py` `rotatable_refs`).
    pub rot: i32,
    pub side: Side,
    /// Placed by a person / template, not to be moved by the placer.
    #[serde(default)]
    pub locked: bool,
}

impl Placed {
    /// The ONLY footprint-to-board transform. Bottom side mirrors X first
    /// (KiCad flips footprints about the Y axis: `FOOTPRINT::Flip`,
    /// pcbnew/footprint.cpp, left-right flip).
    pub fn to_board(&self, p: Pt) -> Pt {
        let p = if self.side == Side::Bottom { Pt { x: -p.x, y: p.y } } else { p };
        p.rot90(self.rot) + self.at
    }
    pub fn pad_abs(&self, pad: &Pad) -> Pt {
        self.to_board(pad.at)
    }
    /// Courtyard in the board frame.
    pub fn courtyard_abs(&self) -> Rect {
        let c = self.footprint.courtyard;
        Rect::from_corners(self.to_board(c.min), self.to_board(c.max))
    }
    /// Pad bbox in the board frame.
    pub fn pad_bbox_abs(&self, pad: &Pad) -> Rect {
        let b = pad.local_bbox();
        Rect::from_corners(self.to_board(b.min), self.to_board(b.max))
    }
    /// The board copper layers a pad occupies: its own `layers` (swapped
    /// F/B on a bottom-side part), or, when empty, the part's side for SMD
    /// and both layers for a drilled pad.
    pub fn pad_layers(&self, pad: &Pad) -> Vec<Layer> {
        if pad.layers.is_empty() {
            return if pad.drill.is_some() { vec![Layer::FCu, Layer::BCu] } else { vec![self.copper_layer()] };
        }
        let mut v: Vec<Layer> = pad
            .layers
            .iter()
            .map(|l| match (self.side, l) {
                (Side::Bottom, Layer::FCu) => Layer::BCu,
                (Side::Bottom, Layer::BCu) => Layer::FCu,
                (_, l) => *l,
            })
            .collect();
        v.sort();
        v.dedup();
        v
    }
    pub fn copper_layer(&self) -> Layer {
        if self.side == Side::Top {
            Layer::FCu
        } else {
            Layer::BCu
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub a: Pt,
    pub b: Pt,
    pub width: Nm,
    pub layer: Layer,
    pub net: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub at: Pt,
    pub diameter: Nm,
    pub drill: Nm,
    pub net: usize,
}

/// A copper pour. Emitted unfilled; KiCad fills it (`kicad-cli pcb drc
/// --refill-zones`, and the fill is what DRC and STEP export see).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub net: usize,
    pub layer: Layer,
    pub outline: Vec<Pt>,
    pub clearance: Nm,
    pub min_thickness: Nm,
    /// Thermal relief spoke width.
    pub thermal_spoke: Nm,
}

/// Track width and clearance for a class of nets (KiCad `.kicad_pro`
/// `net_settings.classes`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub track_width: Nm,
    pub clearance: Nm,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    /// Net indices in this class.
    pub nets: Vec<usize>,
}

/// Fab limits. Defaults are JLCPCB 2-layer standard (jlcpcb.com/capabilities;
/// the baseline's `fabhouse.py`, via ring 0.05 mm per commit 9373aa5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub min_clearance: Nm,
    pub min_track: Nm,
    pub min_via_diameter: Nm,
    pub min_via_drill: Nm,
    pub edge_clearance: Nm,
    pub copper_layers: u8,
    pub copper_oz: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            min_clearance: Nm::mm(0.2),
            min_track: Nm::mm(0.2),
            min_via_diameter: Nm::mm(0.6),
            min_via_drill: Nm::mm(0.3),
            edge_clearance: Nm::mm(0.5),
            copper_layers: 2,
            copper_oz: 1.0,
        }
    }
}

/// The whole physical board. Net indices are `Netlist::nets` indices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// `Edge.Cuts` rectangle (rectangular boards only in the slice).
    pub outline: Rect,
    pub parts: Vec<Placed>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub classes: Vec<NetClass>,
    pub rules: Rules,
    /// Mounting holes (centre, drill), NPTH.
    #[serde(default)]
    pub holes: Vec<(Pt, Nm)>,
}

impl Board {
    /// The class a net belongs to; the first class is the default.
    pub fn class_of(&self, net: usize) -> Option<&NetClass> {
        self.classes.iter().find(|c| c.nets.contains(&net)).or(self.classes.first())
    }
    pub fn copper_length(&self) -> Nm {
        Nm(self.tracks.iter().map(|t| t.a.dist(t.b).round() as i64).sum())
    }
}

/// A net the router did not finish, and why. The baseline's contract
/// (`routing.py` docstring): every refusal is named, an empty result never
/// reads as 100 %.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unrouted {
    pub net: usize,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RouteReport {
    /// Nets the router was asked to route (every net with >= 2 pads that
    /// is not carried entirely by a zone).
    pub attempted: Vec<usize>,
    pub routed: Vec<usize>,
    pub unrouted: Vec<Unrouted>,
    /// Nets left to a pour, with the stitching vias placed for them.
    pub poured: Vec<usize>,
    pub expansions: u64,
    pub seconds: f64,
}

impl RouteReport {
    /// Routed share of attempted nets; 0 when nothing was attempted but
    /// something should have been (callers check `attempted`).
    pub fn completion(&self) -> f64 {
        if self.attempted.is_empty() {
            return 0.0;
        }
        self.routed.len() as f64 / self.attempted.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp() -> Footprint {
        let pad = |n: &str, x: f64| Pad { number: n.into(), at: Pt::mm(x, 0.0), rot_deg: 0.0, size: (Nm::mm(0.8), Nm::mm(0.95)), shape: PadShape::RoundRect(0.25), drill: None, plated: false, layers: vec![] };
        Footprint {
            lib_id: "Resistor_SMD:R_0603_1608Metric".into(),
            pads: vec![pad("1", -0.825), pad("2", 0.825)],
            courtyard: Rect::from_corners(Pt::mm(-1.48, -0.73), Pt::mm(1.48, 0.73)),
            model3d: None,
            source: String::new(),
        }
    }

    #[test]
    fn rotation_swaps_courtyard_extents() {
        let p = Placed { part: 0, footprint: fp(), at: Pt::mm(10.0, 10.0), rot: 1, side: Side::Top, locked: false };
        let c = p.courtyard_abs();
        assert_eq!(c.width(), Nm::mm(1.46));
        assert_eq!(c.height(), Nm::mm(2.96));
        // pad 1 at -x rotates to +y (down on screen) for +90 CCW in Y-down.
        assert_eq!(p.pad_abs(&p.footprint.pads[0]), Pt::mm(10.0, 10.825));
    }

    #[test]
    fn bottom_side_mirrors_x() {
        let p = Placed { part: 0, footprint: fp(), at: Pt::mm(0.0, 0.0), rot: 0, side: Side::Bottom, locked: false };
        assert_eq!(p.pad_abs(&p.footprint.pads[0]), Pt::mm(0.825, 0.0));
    }
}
