//! What a person takes to the workbench: renders of the vehicle and one
//! self-contained build guide (`guide.html`) with what to buy, what to print,
//! the assembly in order, the wiring, every check with its margin, and the
//! domain's checklist for the first flight, launch or release.
//!
//! * [`render`]: a z-buffered software renderer (orthographic views, PNG).
//! * [`guide`]: the HTML guide writer.

pub mod artifacts;
pub mod guide;
pub mod render;

pub use artifacts::from_artifacts;
pub use guide::{default_steps, write_guide, Block, Guide, Section, Step};
pub use render::{rasterize, render, render_with, Frame, Options, View};
