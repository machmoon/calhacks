//! Finite-element analysis of Wright's parts.
//!
//! CalculiX (`ccx`, www.calculix.de; the solver FreeCAD's FEM workbench
//! drives) solves; gmsh meshes solids; this crate writes the decks, reads
//! the results, draws them and compares them with the closed-form numbers
//! the domain checks use today. It wraps the solver the way
//! `wright_geom::slice` wraps PrusaSlicer: a found binary, a written input,
//! a parsed output.
//!
//! * [`mesh`], [`gmsh`]: structured S8R shells for tubes and fins; gmsh
//!   bricks and tets for plates;
//! * [`deck`]: the `.inp` (FreeCAD's card order);
//! * [`results`]: running ccx, reading `.frd` / `.dat` (FreeCAD's readers);
//! * [`render`]: contour PNGs and mode-shape mp4s;
//! * [`validate`]: closed-form cases the pipeline must reproduce;
//! * [`cases`]: the VTOL boom and spar, the rocket tank, bulkhead and fin,
//!   the balloon's rack shelf;
//! * [`cases::FeaStage`]: the `fea` stage (wright_core::Stage); its manifest is `<out>/fea/manifest.json`.

// geometry builders take many plain numbers; grids are indexed in two axes
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

pub mod cases;
pub mod deck;
pub mod gmsh;
pub mod mesh;
pub mod render;
pub mod results;
pub mod tools;
pub mod validate;
