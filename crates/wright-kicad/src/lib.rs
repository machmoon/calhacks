//! KiCad I/O for the electronics pipeline (lane A of docs/electronics-plan.md).
//!
//! * `library`: resolve a netlist part to a KiCad library footprint
//!   (`.kicad_mod` under the installed `footprints/`), parse pads and
//!   courtyard. Replaces the Python baseline's `kicadlib/` + `footprints.py` + `kicad.py`.
//! * `pcb`: write `.kicad_pcb` (+ `.kicad_pro` net classes). Replaces the baseline's
//!   `board.py emit_kicad_pcb`; follows atopile
//!   `src/faebryk/exporters/pcb/kicad/transformer.py` (insert_footprint /
//!   insert_track / insert_via / insert_zone) and the sexpr-pcb spec.
//! * `cli`: run `kicad-cli` ERC / DRC / parity / STEP export, parse its JSON.
//!   Replaces the baseline's `verify/kicad.py`.
//!
//! * `project`: `.kicad_pro` (net classes, design rules), following KiCad's
//!   own template `RaspberryPi-HAT.kicad_pro`.
//!
//! The function signatures below are the contract other lanes code
//! against; lane A fills the bodies and may add, never change them.

pub mod cli;
pub mod library;
pub mod pcb;
pub mod project;

pub use cli::{drc, export_glb, export_pcb_svg, export_pcb_svg_layers, export_sch_svg, fill_zones, tool_version};
pub use library::{find_symbol, footprint_id, load_footprint, parse_footprint};
pub use pcb::{pad_nets, symbol_uuid, unconnected_net};

use std::path::{Path, PathBuf};
use wright_eda::{Board, Footprint, Netlist};

/// Default KiCad install (macOS); `WRIGHT_KICAD_CLI` / `WRIGHT_KICAD_SHARE` override.
pub const KICAD_CLI: &str = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli";
pub const KICAD_SHARE: &str = "/Applications/KiCad/KiCad.app/Contents/SharedSupport";

pub fn kicad_cli() -> PathBuf {
    std::env::var_os("WRIGHT_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(KICAD_CLI))
}
pub fn kicad_share() -> PathBuf {
    std::env::var_os("WRIGHT_KICAD_SHARE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(KICAD_SHARE))
}

/// Footprint for every part of `netlist`, in part order. Errors name every
/// part that could not be resolved (batched, like `Circuit::validate`) and
/// every pin whose pad number the footprint lacks.
pub fn resolve_footprints(netlist: &Netlist, share: &Path) -> anyhow::Result<Vec<Footprint>> {
    library::resolve_footprints(netlist, share)
}

/// `.kicad_pcb` text for a placed (and maybe routed) board.
pub fn write_pcb(netlist: &Netlist, board: &Board, title: &str) -> anyhow::Result<String> {
    pcb::write_pcb(netlist, board, title)
}

/// `.kicad_pro` JSON text (net classes, design rules) tying sch and pcb.
pub fn write_project(netlist: &Netlist, board: &Board, name: &str) -> anyhow::Result<String> {
    project::write_project(netlist, board, name)
}

/// One kicad-cli verdict. `ran == false` means the tool did not run (not
/// installed, crashed): that is *unverified*, never a pass.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CliVerdict {
    /// "erc", "drc", "parity".
    pub check: String,
    pub ran: bool,
    pub tool_version: Option<String>,
    pub errors: usize,
    pub warnings: usize,
    /// DRC only: `unconnected_items`.
    pub unconnected: usize,
    /// First violations, `type: description`.
    pub details: Vec<String>,
    pub report: Option<PathBuf>,
}

/// ERC on the schematic, DRC (`--refill-zones --severity-error`) and
/// parity (`--schematic-parity`) on the board. Always three verdicts.
pub fn verify(pcb: &Path, sch: &Path) -> Vec<CliVerdict> {
    cli::verify(pcb, sch)
}

/// `kicad-cli pcb export step` (board + component models) for the CAD team.
pub fn export_step(pcb: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    cli::export_step(pcb, out)
}
