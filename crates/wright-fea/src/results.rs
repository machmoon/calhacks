//! Running ccx and reading what it writes.
//!
//! `.frd` (CalculiX GraphiX result format) is read the way FreeCAD reads it
//! (`src/Mod/Fem/feminout/importCcxFrdResults.py`, `read_frd_result`):
//! fixed columns, ` 2C` node block (` -1` id in [3,13), coordinates at 13,
//! 25, 37), ` 3C` element block (` -1` id + type, ` -2` node lines of I10),
//! result blocks opened by ` -4  DISP` / ` -4  STRESS` and closed by ` -3`,
//! a mode's frequency on the `  100CL` line at [12,24). Shell (S8R) models
//! come back expanded to 20-node bricks (ccx expands shells into C3D20R), so
//! the surfaces drawn and the stresses read are on the real top and bottom
//! faces of the shell.
//! `.dat` eigenvalues the way `importCcxDatResults.py` does (the table after
//! `E I G E N V A L U E   O U T P U T`), and buckling factors from the
//! table after `B U C K L I N G   F A C T O R   O U T P U T`.

use crate::deck::Model;
use crate::mesh::V3;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub name: String,
    /// Time (static), frequency in Hz (modal) or the buckling factor.
    pub value: f64,
    pub data: HashMap<usize, Vec<f64>>,
}

#[derive(Debug, Clone, Default)]
pub struct Frd {
    pub nodes: HashMap<usize, V3>,
    /// (frd element type, node ids): 4 = 20-node brick, 6 = 10-node tet.
    pub elements: Vec<(u32, Vec<usize>)>,
    pub blocks: Vec<Block>,
}

fn f(line: &str, a: usize, b: usize) -> Result<f64> {
    let s = line.get(a..b.min(line.len())).with_context(|| format!("short frd line {line:?}"))?.trim();
    s.parse::<f64>().with_context(|| format!("number {s:?} in {line:?}"))
}
fn u(line: &str, a: usize, b: usize) -> Result<usize> {
    let s = line.get(a..b.min(line.len())).with_context(|| format!("short frd line {line:?}"))?.trim();
    s.parse::<usize>().with_context(|| format!("integer {s:?} in {line:?}"))
}

pub fn read_frd(text: &str) -> Result<Frd> {
    let mut r = Frd::default();
    let mut section = 0; // 2 nodes, 3 elements, 4 results
    let mut value = 0.0;
    let mut cur: Option<Block> = None;
    let mut elem: Option<(u32, Vec<usize>)> = None;
    for line in text.lines() {
        if line.starts_with("    2C") {
            section = 2;
            continue;
        }
        if line.starts_with("    3C") {
            section = 3;
            continue;
        }
        if line.starts_with("  100CL") {
            value = f(line, 12, 24)?;
            continue;
        }
        if let Some(rest) = line.strip_prefix(" -4") {
            section = 4;
            cur = Some(Block { name: rest.split_whitespace().next().unwrap_or("").to_string(), value, data: HashMap::new() });
            continue;
        }
        if line.starts_with(" -3") {
            if let Some(e) = elem.take() {
                r.elements.push(e);
            }
            if let Some(b) = cur.take() {
                r.blocks.push(b);
            }
            section = 0;
            continue;
        }
        match section {
            2 if line.starts_with(" -1") => {
                r.nodes.insert(u(line, 3, 13)?, [f(line, 13, 25)?, f(line, 25, 37)?, f(line, 37, 49)?]);
            }
            3 if line.starts_with(" -1") => {
                if let Some(e) = elem.take() {
                    r.elements.push(e);
                }
                elem = Some((u(line, 13, 18)? as u32, vec![]));
            }
            3 if line.starts_with(" -2") => {
                if let Some((_, ns)) = elem.as_mut() {
                    let mut k = 3;
                    while k + 10 <= line.trim_end().len() {
                        ns.push(u(line, k, k + 10)?);
                        k += 10;
                    }
                }
            }
            4 if line.starts_with(" -1") => {
                if let Some(b) = cur.as_mut() {
                    let id = u(line, 3, 13)?;
                    let mut v = vec![];
                    let mut k = 13;
                    while k + 12 <= line.trim_end().len() {
                        v.push(f(line, k, k + 12)?);
                        k += 12;
                    }
                    b.data.insert(id, v);
                }
            }
            _ => {}
        }
    }
    if r.nodes.is_empty() {
        bail!("frd has no nodes");
    }
    Ok(r)
}

/// Von Mises from (SXX, SYY, SZZ, SXY, SYZ, SZX).
pub fn von_mises(s: &[f64]) -> f64 {
    let (a, b, c) = (s[0], s[1], s[2]);
    (0.5 * ((a - b).powi(2) + (b - c).powi(2) + (c - a).powi(2)) + 3.0 * (s[3].powi(2) + s[4].powi(2) + s[5].powi(2))).sqrt()
}

/// Rows of numbers following a `.dat` table header, until a non-row line.
fn dat_table(text: &str, header: &str) -> Vec<Vec<f64>> {
    let mut out = vec![];
    let mut on = false;
    let mut started = false;
    for line in text.lines() {
        if line.contains(header) {
            on = true;
            continue;
        }
        if !on {
            continue;
        }
        let nums: Option<Vec<f64>> = line.split_whitespace().map(|x| x.parse::<f64>().ok()).collect();
        match nums {
            Some(v) if !v.is_empty() && line.trim_start().chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) => {
                started = true;
                out.push(v);
            }
            _ if started => break,
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct Solved {
    pub dir: PathBuf,
    pub job: String,
    pub frd: Frd,
    /// Natural frequencies, Hz (frequency analyses).
    pub frequencies_hz: Vec<f64>,
    /// Buckling factors (buckle analyses).
    pub buckling_factors: Vec<f64>,
    pub seconds: f64,
    /// A *BUCKLE run: its first DISP/STRESS blocks are the preload step.
    pub buckle: bool,
}

impl Solved {
    /// Static (or first) block by name.
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.frd.blocks.iter().find(|b| b.name == name)
    }
    /// The k-th block of `name` (k-th mode; for a buckling run the k-th
    /// buckling mode, after the preload step's block).
    pub fn nth(&self, name: &str, k: usize) -> Option<&Block> {
        self.frd.blocks.iter().filter(|b| b.name == name).nth(k + self.buckle as usize)
    }
    pub fn max_von_mises(&self) -> f64 {
        self.block("STRESS").map(|b| b.data.values().map(|s| von_mises(s)).fold(0.0, f64::max)).unwrap_or(f64::NAN)
    }
    pub fn max_displacement(&self) -> f64 {
        self.block("DISP").map(|b| b.data.values().map(|d| (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()).fold(0.0, f64::max)).unwrap_or(f64::NAN)
    }
}

/// Write `<dir>/<job>.inp`, run ccx there, read `.frd` and `.dat`.
pub fn solve(model: &Model, dir: &Path, job: &str) -> Result<Solved> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{job}.inp")), model.inp())?;
    let t0 = std::time::Instant::now();
    let ccx = crate::tools::ccx()?;
    let log = crate::tools::run(Command::new(ccx).current_dir(dir).env("OMP_NUM_THREADS", "4").arg("-i").arg(job), &format!("ccx {job}"))?;
    std::fs::write(dir.join(format!("{job}.log")), &log)?;
    if log.contains("*ERROR") {
        let e: Vec<&str> = log.lines().filter(|l| l.contains("ERROR")).take(5).collect();
        bail!("ccx {job}: {}", e.join(" | "));
    }
    let frd = read_frd(&std::fs::read_to_string(dir.join(format!("{job}.frd"))).context("reading frd")?)?;
    let dat = std::fs::read_to_string(dir.join(format!("{job}.dat"))).unwrap_or_default();
    let frequencies_hz = dat_table(&dat, "E I G E N V A L U E   O U T P U T").iter().filter(|r| r.len() >= 4).map(|r| r[3]).collect();
    let buckling_factors = dat_table(&dat, "B U C K L I N G   F A C T O R   O U T P U T").iter().filter(|r| r.len() >= 2).map(|r| r[1]).collect();
    // scratch files ccx leaves behind
    for ext in ["12d", "cvg", "sta"] {
        let _ = std::fs::remove_file(dir.join(format!("{job}.{ext}")));
    }
    let _ = std::fs::remove_file(dir.join("spooles.out"));
    Ok(Solved { dir: dir.to_path_buf(), job: job.into(), frd, frequencies_hz, buckling_factors, seconds: t0.elapsed().as_secs_f64(), buckle: matches!(model.analysis, crate::deck::Analysis::Buckle { .. }) })
}
