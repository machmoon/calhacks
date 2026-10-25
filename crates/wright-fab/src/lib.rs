//! Fabrication: process plans, CAM toolpaths (mill, lathe, cut lists),
//! material-removal simulation, and the timelapse videos of every process,
//! the assembly and a turntable, from a build output directory.
//!
//! Entry points: [`fab::FabStage`] (the pipeline stage, `<out>/fab`) and
//! the `wright-fab BUILD_DIR` binary.
pub mod assembly;
pub mod cut;
pub mod fab;
pub mod film;
pub mod lathe;
pub mod mill;
pub mod print;
pub mod render;
pub mod rocket;
pub mod rules;
pub mod toolpath;
pub mod video;

pub use fab::FabStage;
