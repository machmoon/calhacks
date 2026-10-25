//! wright-cfd: running real open-source flow solvers on Wright's designs.
//!
//! Nothing in this crate solves a flow. It turns a domain's design into a
//! solver case, runs the solver as a subprocess (the way
//! `wright_geom::slice` runs PrusaSlicer), parses the coefficients, and
//! draws the fields:
//!
//! * [`avl`]: Mark Drela's AVL vortex-lattice code through OptVL
//!   (github.com/joanibal/OptVL, AVL 3.x compiled as a Python module), for
//!   the VTOL's wing + tail: CL, CDi, Cm against alpha, the neutral point,
//!   span loading, and Cp on every panel.
//! * [`su2`]: SU2 (github.com/su2code/SU2, v8.5.0) compressible Euler on
//!   axisymmetric meshes written here, for the rocket: the nose's pressure
//!   drag against Mach and the nozzle's thrust coefficient.
//!
//! Every run writes `<out>/cfd/manifest.json` ([`Manifest`]), which the
//! desktop app reads; [`CfdStage`] is the pipeline stage.

pub mod avl;
pub mod mesh;
pub mod render;
pub mod rocket;
pub mod su2;
pub mod taylor_maccoll;
pub mod validate;
pub mod video;
pub mod vtol;

pub mod stage;

pub use stage::CfdStage;
/// The manifest is the pipeline's (`wright_core::stage`): `{ "kind": "cfd",
/// "title", "items": [...], "checks"? }`.
pub use wright_core::stage::{Item, Manifest};

use std::path::{Path, PathBuf};

/// The Python that has OptVL: `WRIGHT_PYTHON`, else the shared venv.
pub fn python() -> PathBuf {
    if let Ok(p) = std::env::var("WRIGHT_PYTHON") {
        return PathBuf::from(p);
    }
    PathBuf::from("/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/venv312/bin/python")
}

/// SU2_CFD: `WRIGHT_SU2`, else the scratchpad install, else `SU2_CFD` on PATH.
pub fn su2_cfd() -> PathBuf {
    if let Ok(p) = std::env::var("WRIGHT_SU2") {
        return PathBuf::from(p);
    }
    let p = PathBuf::from("/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/tools/su2/bin/SU2_CFD");
    if p.exists() {
        p
    } else {
        PathBuf::from("SU2_CFD")
    }
}

/// ffmpeg: `WRIGHT_FFMPEG`, else Homebrew's, else PATH.
pub fn ffmpeg() -> PathBuf {
    if let Ok(p) = std::env::var("WRIGHT_FFMPEG") {
        return PathBuf::from(p);
    }
    let p = PathBuf::from("/opt/homebrew/bin/ffmpeg");
    if p.exists() {
        p
    } else {
        PathBuf::from("ffmpeg")
    }
}

/// Write a CSV from a header and rows.
pub fn write_csv(path: &Path, header: &[&str], rows: &[Vec<f64>]) -> anyhow::Result<()> {
    let mut s = header.join(",");
    s.push('\n');
    for r in rows {
        s.push_str(&r.iter().map(|v| format!("{v:.6}")).collect::<Vec<_>>().join(","));
        s.push('\n');
    }
    std::fs::write(path, s)?;
    Ok(())
}
