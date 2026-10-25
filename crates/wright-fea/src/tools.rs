//! The external programs: CalculiX `ccx` (solver), gmsh through Python
//! (solid meshes), matplotlib through Python (contour pictures) and ffmpeg
//! (mode animations). Found the way `wright_geom::slice` finds PrusaSlicer:
//! an environment variable first, then the place this machine installed it,
//! then PATH.
//!
//! * `WRIGHT_CCX`: CalculiX 2.23, conda-forge build `calculix`
//!   (installed 10/24 under the session scratchpad `tools/calculix`).
//! * `WRIGHT_FEA_PYTHON`: a Python with `gmsh` 4.15.2 and matplotlib
//!   (the shared `venv312`).
//! * `WRIGHT_FFMPEG`: ffmpeg (`/opt/homebrew/bin/ffmpeg`).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const SCRATCH: &str = "/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad";

fn find(env: &str, candidates: &[PathBuf], name: &str) -> Result<PathBuf> {
    if let Ok(p) = std::env::var(env) {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
        bail!("{env}={} does not exist", p.display());
    }
    for c in candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    bail!("{name} not found: set {env}")
}

pub fn ccx() -> Result<PathBuf> {
    find("WRIGHT_CCX", &[PathBuf::from(SCRATCH).join("tools/calculix/bin/ccx")], "ccx")
}

pub fn python() -> Result<PathBuf> {
    find("WRIGHT_FEA_PYTHON", &[PathBuf::from(SCRATCH).join("venv312/bin/python")], "python3")
}

pub fn ffmpeg() -> Result<PathBuf> {
    find("WRIGHT_FFMPEG", &[PathBuf::from("/opt/homebrew/bin/ffmpeg")], "ffmpeg")
}

/// The crate's Python helpers (`py/`), next to the sources.
pub fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("py").join(name)
}

/// Run a command, failing with its stderr tail when it fails.
pub fn run(cmd: &mut Command, what: &str) -> Result<String> {
    let out = cmd.output().with_context(|| format!("starting {what}"))?;
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let se = String::from_utf8_lossy(&out.stderr);
        let tail: String = format!("{so}\n{se}").lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        bail!("{what} failed ({}):\n{tail}", out.status);
    }
    Ok(so)
}

/// Run Python in isolated mode (`-I`: no script-dir or cwd imports).
pub fn run_python(args: &[&str], what: &str) -> Result<String> {
    let py = python()?;
    run(Command::new(py).arg("-I").args(args), what)
}
