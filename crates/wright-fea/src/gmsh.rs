//! Solid meshes from gmsh (`py/gmsh_mesh.py`), read back from gmsh's own
//! `.inp` writer.

use crate::mesh::{read_gmsh_inp, Mesh};
use anyhow::Result;
use std::path::Path;

/// A flat disk of radius `r`, thickness `t` (z from 0 to t), 20-node bricks
/// with `layers` through the thickness and in-plane size `h`.
pub fn disk(dir: &Path, r: f64, t: f64, h: f64, layers: usize, set: &str) -> Result<Mesh> {
    std::fs::create_dir_all(dir)?;
    let out = dir.join("disk_mesh.inp");
    crate::tools::run_python(
        &[&crate::tools::script("gmsh_mesh.py").to_string_lossy(), "disk", &r.to_string(), &t.to_string(), &h.to_string(), &layers.to_string(), &out.to_string_lossy()],
        "gmsh disk",
    )?;
    let m = read_gmsh_inp(&std::fs::read_to_string(&out)?, set)?;
    std::fs::remove_file(&out).ok();
    Ok(m)
}

/// A rectangular plate lx x ly x t (corner at the origin) with through
/// holes (x, y, r): 10-node tets of size `h`.
pub fn plate(dir: &Path, lx: f64, ly: f64, t: f64, h: f64, holes: &[(f64, f64, f64)], set: &str) -> Result<Mesh> {
    std::fs::create_dir_all(dir)?;
    let out = dir.join("plate_mesh.inp");
    let mut args: Vec<String> = vec![crate::tools::script("gmsh_mesh.py").to_string_lossy().into(), "plate".into(), lx.to_string(), ly.to_string(), t.to_string(), h.to_string(), out.to_string_lossy().into()];
    args.extend(holes.iter().map(|(x, y, r)| format!("{x},{y},{r}")));
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    crate::tools::run_python(&a, "gmsh plate")?;
    let m = read_gmsh_inp(&std::fs::read_to_string(&out)?, set)?;
    std::fs::remove_file(&out).ok();
    Ok(m)
}
