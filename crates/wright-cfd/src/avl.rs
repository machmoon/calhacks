//! AVL (Drela and Youngren, web.mit.edu/drela/Public/web/avl) through OptVL.
//!
//! The geometry file is AVL's own format (`avl_doc.txt` in the AVL 3.40
//! distribution, "Geometry input file format"): a header with Mach, symmetry,
//! Sref/Cref/Bref, the moment reference point and CDp, then SURFACE blocks
//! (Nchord Cspace Nspan Sspace, YDUPLICATE, ANGLE, SECTION lines `Xle Yle Zle
//! Chord Ainc`, then NACA or AIRFOIL camber data) and BODY blocks (BFIL side
//! profile). The same file opens in stock AVL (`avl case.avl`, `OPER`, `x`).
//!
//! The run goes through OptVL (github.com/joanibal/OptVL v2.5.0, MDO Lab;
//! `optvl/optvl_class.py`: `OVLSolver`, `set_variable("alpha")`,
//! `execute_run`, `get_total_forces`, `get_stab_derivs`, `get_strip_forces`,
//! `get_cp_data`), because its wheel carries a compiled AVL for macOS arm64
//! and AVL's own plotting needs X11. `py/avl_sweep.py` only copies AVL's
//! numbers into JSON.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Camber {
    /// 4-digit NACA designation, e.g. "0009".
    Naca(String),
    /// Selig-ordered x/c, y/c (TE upper -> LE -> TE lower).
    Coords(Vec<(f64, f64)>),
    Flat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub xle: f64,
    pub yle: f64,
    pub zle: f64,
    pub chord: f64,
    pub ainc_deg: f64,
    pub camber: Camber,
    /// AVL CLAF: section lift slope over 2 pi (None = 1).
    #[serde(default)]
    pub claf: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Surface {
    pub name: String,
    pub n_chord: usize,
    /// AVL spacing parameter: 1 cosine, 0 equal, -2 -sine ...
    pub c_space: f64,
    pub n_span: usize,
    pub s_space: f64,
    pub y_duplicate: Option<f64>,
    pub angle_deg: f64,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub name: String,
    pub n_body: usize,
    pub b_space: f64,
    pub translate: [f64; 3],
    /// Side profile as (x, radius) from nose to tail, radius >= 0.
    pub profile: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geometry {
    pub title: String,
    pub mach: f64,
    pub sref: f64,
    pub cref: f64,
    pub bref: f64,
    pub xref: [f64; 3],
    pub cdp: f64,
    pub surfaces: Vec<Surface>,
    pub bodies: Vec<Body>,
}

impl Geometry {
    /// AVL geometry text; body profiles go to `<stem>_<body>.dat` beside it.
    pub fn write(&self, dir: &Path, stem: &str) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let mut s = String::new();
        writeln!(s, "{}", self.title)?;
        writeln!(s, "#Mach\n{}", self.mach)?;
        writeln!(s, "#IYsym IZsym Zsym\n0 0 0.0")?;
        writeln!(s, "#Sref Cref Bref\n{:.6} {:.6} {:.6}", self.sref, self.cref, self.bref)?;
        writeln!(s, "#Xref Yref Zref\n{:.6} {:.6} {:.6}", self.xref[0], self.xref[1], self.xref[2])?;
        writeln!(s, "#CDp\n{:.6}", self.cdp)?;
        for sf in &self.surfaces {
            writeln!(s, "#{}\nSURFACE\n{}", "=".repeat(60), sf.name)?;
            writeln!(s, "#Nchord Cspace Nspan Sspace\n{} {} {} {}", sf.n_chord, sf.c_space, sf.n_span, sf.s_space)?;
            if let Some(y) = sf.y_duplicate {
                writeln!(s, "YDUPLICATE\n{y}")?;
            }
            writeln!(s, "ANGLE\n{:.4}", sf.angle_deg)?;
            for sec in &sf.sections {
                writeln!(s, "SECTION\n#Xle Yle Zle Chord Ainc\n{:.6} {:.6} {:.6} {:.6} {:.4}", sec.xle, sec.yle, sec.zle, sec.chord, sec.ainc_deg)?;
                match &sec.camber {
                    Camber::Naca(d) => writeln!(s, "NACA\n{d}")?,
                    Camber::Coords(c) => {
                        writeln!(s, "AIRFOIL")?;
                        for (x, y) in c {
                            writeln!(s, "{x:.6} {y:.6}")?;
                        }
                    }
                    Camber::Flat => {}
                }
                if let Some(k) = sec.claf {
                    writeln!(s, "CLAF\n{k:.4}")?;
                }
            }
        }
        for b in &self.bodies {
            let f = dir.join(format!("{stem}_{}.dat", b.name));
            let mut t = format!("{}\n", b.name);
            // AVL reads a body file like an airfoil: closed side outline,
            // tail top -> nose -> tail bottom; radius = half the thickness
            for (x, r) in b.profile.iter().rev() {
                writeln!(t, "{x:.6} {r:.6}")?;
            }
            for (x, r) in b.profile.iter().skip(1) {
                writeln!(t, "{x:.6} {:.6}", -r)?;
            }
            std::fs::write(&f, t)?;
            writeln!(s, "#{}\nBODY\n{}", "=".repeat(60), b.name)?;
            writeln!(s, "#Nbody Bspace\n{} {}", b.n_body, b.b_space)?;
            writeln!(s, "TRANSLATE\n{:.6} {:.6} {:.6}", b.translate[0], b.translate[1], b.translate[2])?;
            writeln!(s, "BFIL\n{}", f.file_name().unwrap().to_string_lossy())?;
        }
        let path = dir.join(format!("{stem}.avl"));
        std::fs::write(&path, s)?;
        Ok(path)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Strip {
    pub y: Vec<Option<f64>>,
    pub chord: Vec<Option<f64>>,
    pub cl: Vec<Option<f64>>,
    pub lift_dist: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpSurface {
    /// (strips + 1) x (2 nchord + 1) points.
    pub xyz: Vec<Vec<[f64; 3]>>,
    /// strips x 2 nchord panel values.
    pub cp: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct Row {
    pub alpha: f64,
    pub CL: Option<f64>,
    pub CD: Option<f64>,
    pub CDi: Option<f64>,
    pub CDff: Option<f64>,
    pub Cm: Option<f64>,
    pub e: Option<f64>,
    pub CLa: Option<f64>,
    pub Cma: Option<f64>,
    pub Xnp: Option<f64>,
    #[serde(default)]
    pub strips: BTreeMap<String, Strip>,
    #[serde(default)]
    pub surfaces: Vec<CpSurface>,
}

impl Row {
    pub fn cl(&self) -> f64 {
        self.CL.unwrap_or(f64::NAN)
    }
    pub fn cdi(&self) -> f64 {
        self.CDff.or(self.CDi).unwrap_or(f64::NAN)
    }
    pub fn cm(&self) -> f64 {
        self.Cm.unwrap_or(f64::NAN)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sweep {
    pub reference: BTreeMap<String, Option<f64>>,
    pub rows: Vec<Row>,
}

impl Sweep {
    /// Least-squares CL slope per radian over rows with |alpha| <= `within` deg.
    pub fn cl_alpha_fit(&self, within: f64) -> f64 {
        let pts: Vec<(f64, f64)> = self.rows.iter().filter(|r| r.alpha.abs() <= within).map(|r| (r.alpha.to_radians(), r.cl())).collect();
        slope(&pts)
    }
}

pub fn slope(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len() as f64;
    if n < 2.0 {
        return f64::NAN;
    }
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    sxy / sxx
}

const SCRIPT: &str = include_str!("../py/avl_sweep.py");

/// Run an alpha sweep of `geom` (written to `dir`) and parse it.
pub fn run_sweep(geom: &Geometry, dir: &Path, stem: &str, alphas: &[f64], with_cp: bool) -> anyhow::Result<Sweep> {
    std::fs::create_dir_all(dir)?;
    let dir = &dir.canonicalize()?;
    let avl = geom.write(dir, stem)?;
    let script = dir.join("avl_sweep.py");
    std::fs::write(&script, SCRIPT)?;
    let case = dir.join(format!("{stem}_case.json"));
    let out = dir.join(format!("{stem}_sweep.json"));
    std::fs::write(
        &case,
        serde_json::to_vec(&serde_json::json!({
            "avl_file": avl.file_name().unwrap().to_string_lossy(),
            "alphas": alphas, "mach": geom.mach, "cp": with_cp,
        }))?,
    )?;
    let py = crate::python();
    // AVL resolves BFIL names against the working directory
    let o = Command::new(&py)
        .arg("-I")
        .arg(&script)
        .arg(&case)
        .arg(&out)
        .current_dir(dir)
        .output()
        .with_context(|| format!("running {} (OptVL); set WRIGHT_PYTHON to a Python with `optvl`", py.display()))?;
    std::fs::write(dir.join(format!("{stem}_avl.log")), [&o.stdout[..], &o.stderr[..]].concat())?;
    if !o.status.success() {
        bail!("OptVL/AVL failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).lines().next_back().unwrap_or(""));
    }
    let sweep: Sweep = serde_json::from_slice(&std::fs::read(&out)?).context("parsing the AVL sweep")?;
    if sweep.rows.iter().any(|r| r.CL.is_none()) {
        bail!("AVL returned a non-finite CL (see {stem}_avl.log)");
    }
    Ok(sweep)
}

/// Selig-ordered camber/thickness points from an upper/lower surface
/// function of x/c (cosine spacing, `n` per side).
pub fn coords_from(n: usize, surf: impl Fn(f64) -> (f64, f64)) -> Vec<(f64, f64)> {
    let xs: Vec<f64> = (0..=n).map(|i| 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / n as f64).cos())).collect();
    let mut v: Vec<(f64, f64)> = xs.iter().rev().map(|&x| (x, surf(x).0)).collect();
    v.extend(xs.iter().skip(1).map(|&x| (x, surf(x).1)));
    v
}
