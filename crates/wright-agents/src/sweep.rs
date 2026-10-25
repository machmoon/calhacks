//! `sweep`: a design-of-experiments tool for the specialists and the lead.
//!
//! An agent names one to three design parameters (JSON pointers into the
//! current design), a range or a list for each, and which objective
//! headlines to trade off. The host evaluates every point with the domain's
//! deterministic `check` (every declared check plus every confirmed
//! regression case, exactly as the `check` tool does) and hands back a
//! compact table, the Pareto front, and two files under `<run>/sweeps/`.
//! The agent then edits the design to a point it can see, instead of
//! probing blindly one `edit_design` + `check` at a time.
//!
//! Precedent, followed file by file:
//! * Point generation: OpenMDAO `openmdao/drivers/doe_generators.py`.
//!   `FullFactorialGenerator` builds each variable's levels with
//!   `np.linspace(lower, upper, num=levels)` and takes the product of the
//!   level indices ([`levels`], [`grid`]); `LatinHypercubeGenerator` draws
//!   a unit-cube LHS and maps it with `val = lower + sample * (upper -
//!   lower)` ([`lhs`], [`Range::at`]). The LHS itself is pyDOE
//!   `pyDOE/doe_lhs.py::_lhsclassic` (one uniform draw per stratum of
//!   `linspace(0, 1, samples + 1)`, each column independently permuted).
//! * Enumeration order and exhaustion: Optuna
//!   `optuna/samplers/_grid.py::GridSampler` (`itertools.product` over the
//!   sorted parameter names; a point visited once is never re-evaluated;
//!   the sweep stops when the grid is exhausted, or here, when the time
//!   budget is).
//! * Pareto front: Optuna `optuna/study/_multi_objective.py`
//!   (`_normalize_value` negates MAXIMIZE so everything is minimised,
//!   `_dominates` is "no worse on every objective and not identical", and
//!   the front is taken over feasible trials first). Here "feasible" is
//!   "fewest failing checks" so a sweep where nothing passes still ranks.
//!
//! Deviations, stated: the LHS uses a seeded splitmix64 instead of numpy
//! (no `rand` dependency in this crate; the lock file is shared by many
//! sessions), and the point budget is a hard cap rather than Optuna's
//! unbounded `n_trials`, because every point is a full check and the agent
//! is waiting on the result.

use crate::agent::ToolOutput;
use crate::state::DesignState;
use crate::tools::DomainTools;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use wright_core::{Bound, Event, Finished};

/// Hard cap on points per sweep, whatever the agent asks for.
pub const MAX_POINTS: usize = 200;
/// Wall-clock budget for the checks of one sweep.
pub const TIME_BUDGET: Duration = Duration::from_secs(20);
/// Rows shown in the tool result; the CSV always has every point.
const TABLE_ROWS: usize = 60;
/// Grid when the parameter count is at most this; LHS above it.
const GRID_MAX_PARAMS: usize = 2;

/// What one parameter sweeps over.
#[derive(Debug, Clone, PartialEq)]
pub enum Range {
    /// Explicit values (part ids, counts, flags): the grid's levels.
    List(Vec<Value>),
    /// `np.linspace(min, max, steps)`.
    Linear { min: f64, max: f64, steps: usize },
}

impl Range {
    /// How many distinct levels a grid would take.
    pub fn levels(&self) -> usize {
        match self {
            Range::List(v) => v.len(),
            Range::Linear { steps, .. } => *steps,
        }
    }
    /// The `i`th grid level (OpenMDAO `_pyDOE_Generator.__call__`: the
    /// design's index selects one linspace value).
    pub fn level(&self, i: usize) -> Value {
        match self {
            Range::List(v) => v[i].clone(),
            Range::Linear { min, max, steps } => {
                let t = if *steps <= 1 { 0.0 } else { i as f64 / (*steps - 1) as f64 };
                json!(min + t * (max - min))
            }
        }
    }
    /// The value at unit-cube coordinate `u` in `[0, 1)`
    /// (`LatinHypercubeGenerator`: `lower + sample * (upper - lower)`; a
    /// list is stratified by index).
    pub fn at(&self, u: f64) -> Value {
        match self {
            Range::List(v) => v[((u * v.len() as f64) as usize).min(v.len() - 1)].clone(),
            Range::Linear { min, max, .. } => json!(min + u * (max - min)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    /// RFC 6901 pointer into the design (`/envelope/balloon_g`).
    pub pointer: String,
    pub range: Range,
}

/// Which way an objective is good (Optuna `StudyDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Maximize,
    Minimize,
}

#[derive(Debug, Clone)]
pub struct Objective {
    pub label: String,
    pub unit: String,
    pub direction: Direction,
    /// False for an `Info` headline the agent did not name: recorded as a
    /// column, never part of the dominance test.
    pub in_front: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Grid,
    LatinHypercube,
}

/// One evaluated design point.
#[derive(Debug, Clone)]
pub struct Point {
    pub values: Vec<Value>,
    /// `None` when the patched design was invalid or check could not run.
    pub error: Option<String>,
    pub passing: usize,
    pub total: usize,
    pub failing: Vec<String>,
    /// One per [`Sweep::objectives`]; NaN when unavailable.
    pub objectives: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Sweep {
    pub params: Vec<Param>,
    pub objectives: Vec<Objective>,
    pub method: Method,
    pub points: Vec<Point>,
    /// Indices into `points` on the Pareto front.
    pub front: Vec<usize>,
    /// Points planned but not evaluated before the time budget ran out.
    pub skipped: usize,
}

// ------------------------------------------------------------- planning

/// Every grid point, in `itertools.product` order (Optuna `GridSampler`:
/// `self._all_grids = list(itertools.product(*self._search_space.values()))`).
pub fn grid(params: &[Param]) -> Vec<Vec<Value>> {
    let mut out: Vec<Vec<Value>> = vec![vec![]];
    for p in params {
        let mut next = Vec::with_capacity(out.len() * p.range.levels());
        for prefix in &out {
            for i in 0..p.range.levels() {
                let mut row = prefix.clone();
                row.push(p.range.level(i));
                next.push(row);
            }
        }
        out = next;
    }
    out
}

/// splitmix64: a small seeded generator so a sweep is reproducible.
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// Latin hypercube with `samples` rows (pyDOE `_lhsclassic`): the unit
/// interval is cut into `samples` strata, one uniform draw per stratum,
/// and each column is permuted independently.
pub fn lhs(params: &[Param], samples: usize, seed: u64) -> Vec<Vec<Value>> {
    let mut rng = Rng(seed);
    let n = params.len();
    let mut cols: Vec<Vec<f64>> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut col: Vec<f64> = (0..samples).map(|k| (k as f64 + rng.unit()) / samples as f64).collect();
        rng.shuffle(&mut col);
        cols.push(col);
    }
    (0..samples).map(|r| params.iter().enumerate().map(|(j, p)| p.range.at(cols[j][r])).collect()).collect()
}

// ------------------------------------------------------------- running

/// Resolve a dotted path (`a.b[0]`, what `catalog` uses) or an RFC 6901
/// pointer into a pointer.
pub fn pointer_of(path: &str) -> String {
    let p = path.trim();
    if p.starts_with('/') {
        return p.to_string();
    }
    let mut out = String::new();
    for seg in p.split('.') {
        let seg = seg.replace('[', "/").replace(']', "");
        out.push('/');
        out.push_str(&seg);
    }
    out
}

/// Which objectives a sweep records and how each is judged: the domain's
/// objective (maximised) and every headline with a bound (`AtLeast`
/// maximised, `AtMost` minimised, `Info` recorded only). `maximize` /
/// `minimize` name headline labels to override or add.
fn objectives(domain: &dyn DomainTools, spec: &Value, design: &Value, maximize: &[String], minimize: &[String]) -> anyhow::Result<Vec<Objective>> {
    let mut out = vec![];
    let (_, label) = domain.objective(design)?;
    out.push(Objective { label: label.to_string(), unit: String::new(), direction: Direction::Maximize, in_front: true });
    for h in domain.headlines(spec, design)? {
        if out.iter().any(|o| o.label == h.label) {
            continue;
        }
        let direction = match h.bound {
            Bound::AtLeast => Some(Direction::Maximize),
            Bound::AtMost => Some(Direction::Minimize),
            Bound::Info => None,
        };
        // an Info headline the agent names becomes an objective
        let direction = if maximize.iter().any(|m| m == &h.label) {
            Some(Direction::Maximize)
        } else if minimize.iter().any(|m| m == &h.label) {
            Some(Direction::Minimize)
        } else {
            direction
        };
        out.push(Objective { label: h.label, unit: h.unit, direction: direction.unwrap_or(Direction::Maximize), in_front: direction.is_some() });
    }
    for m in maximize.iter().chain(minimize) {
        if !out.iter().any(|o| &o.label == m) {
            anyhow::bail!(
                "no headline named `{m}`; the headlines are: {}",
                out.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(out)
}

/// Evaluate one point: patch the design, validate, run the full check
/// (regression cases included), read the objectives.
fn evaluate(domain: &dyn DomainTools, state: &DesignState, spec: &Value, base: &Value, params: &[Param], objs: &[Objective], values: Vec<Value>) -> Point {
    let mut design = base.clone();
    let nan = vec![f64::NAN; objs.len()];
    for (p, v) in params.iter().zip(&values) {
        match design.pointer_mut(&p.pointer) {
            Some(slot) => *slot = v.clone(),
            None => return Point { values, error: Some(format!("no field at {}", p.pointer)), passing: 0, total: 0, failing: vec![], objectives: nan },
        }
    }
    let design = match domain.validate_design(&design) {
        Ok(d) => d,
        Err(e) => return Point { values, error: Some(format!("{e:#}")), passing: 0, total: 0, failing: vec![], objectives: nan },
    };
    let fin: Finished = match state.full_check_of(domain, &design) {
        Ok(f) => f,
        Err(e) => return Point { values, error: Some(format!("check could not run: {e:#}")), passing: 0, total: 0, failing: vec![], objectives: nan },
    };
    let mut objectives = nan;
    if let Ok((v, _)) = domain.objective(&design) {
        objectives[0] = v;
    }
    if let Ok(hs) = domain.headlines(spec, &design) {
        for h in hs {
            if let Some(i) = objs.iter().position(|o| o.label == h.label) {
                objectives[i] = h.value;
            }
        }
    }
    let total = fin.outcomes.len();
    let failing: Vec<String> = fin.failures().map(|o| o.name.clone()).collect();
    Point { values, error: None, passing: total - failing.len(), total, failing, objectives }
}

/// Optuna `_dominates` after `_normalize_value`: `a` dominates `b` when it
/// is no worse on every objective in the front and the two differ. NaN
/// (an objective the point could not report) never dominates.
fn dominates(a: &[f64], b: &[f64], objs: &[Objective]) -> bool {
    let norm = |v: f64, d: Direction| if d == Direction::Maximize { -v } else { v };
    let mut any_better = false;
    for (i, o) in objs.iter().enumerate() {
        if !o.in_front {
            continue;
        }
        let (x, y) = (norm(a[i], o.direction), norm(b[i], o.direction));
        if x.is_nan() || y.is_nan() || x > y {
            return false;
        }
        if x < y {
            any_better = true;
        }
    }
    any_better
}

/// The Pareto front over the points with the fewest failing checks
/// (Optuna takes the front over feasible trials first).
fn pareto(points: &[Point], objs: &[Objective]) -> Vec<usize> {
    let Some(best_fail) = points.iter().filter(|p| p.error.is_none()).map(|p| p.failing.len()).min() else {
        return vec![];
    };
    let tier: Vec<usize> = (0..points.len()).filter(|&i| points[i].error.is_none() && points[i].failing.len() == best_fail).collect();
    tier.iter()
        .copied()
        .filter(|&i| !tier.iter().any(|&j| j != i && dominates(&points[j].objectives, &points[i].objectives, objs)))
        .collect()
}

/// Run a sweep over the current design. `max_points` is clamped to
/// [`MAX_POINTS`]; the checks stop at `deadline` and the rest are counted
/// in [`Sweep::skipped`].
#[allow(clippy::too_many_arguments)] // one call site plus tests; a struct would only rename the arguments
pub fn run(domain: &dyn DomainTools, state: &DesignState, params: Vec<Param>, maximize: &[String], minimize: &[String], max_points: usize, seed: u64, deadline: Instant) -> anyhow::Result<Sweep> {
    anyhow::ensure!(!params.is_empty() && params.len() <= 3, "sweep takes 1 to 3 parameters, got {}", params.len());
    for p in &params {
        anyhow::ensure!(p.range.levels() >= 1, "{}: a range needs at least one value", p.pointer);
    }
    let design = state.design.as_ref().ok_or_else(|| anyhow::anyhow!("no design yet: call design first"))?;
    let spec = domain.validate_spec(&state.effective_spec())?;
    for p in &params {
        anyhow::ensure!(design.pointer(&p.pointer).is_some(), "the design has no field at `{}`", p.pointer);
    }
    let objs = objectives(domain, &spec, design, maximize, minimize)?;

    let max_points = max_points.clamp(1, MAX_POINTS);
    let product: usize = params.iter().map(|p| p.range.levels()).fold(1usize, |a, b| a.saturating_mul(b));
    let (method, plan) = if params.len() <= GRID_MAX_PARAMS && product <= max_points {
        (Method::Grid, grid(&params))
    } else {
        (Method::LatinHypercube, lhs(&params, product.min(max_points), seed))
    };

    let mut points = Vec::with_capacity(plan.len());
    let mut skipped = 0;
    for values in plan {
        if Instant::now() >= deadline && !points.is_empty() {
            skipped += 1;
            continue;
        }
        points.push(evaluate(domain, state, &spec, design, &params, &objs, values));
    }
    let front = pareto(&points, &objs);
    Ok(Sweep { params, objectives: objs, method, points, front, skipped })
}

// ------------------------------------------------------------- output

fn fmt_val(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map(|f| if f.fract() == 0.0 && f.abs() < 1e9 { format!("{f}") } else { format!("{f:.4}") }).unwrap_or_else(|| n.to_string()),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn fmt_num(f: f64) -> String {
    if f.is_nan() {
        "-".into()
    } else if f.abs() >= 1000.0 {
        format!("{f:.0}")
    } else {
        format!("{f:.3}")
    }
}

fn short(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

impl Sweep {
    /// The compact table the agent reads: one row per point, the front
    /// starred, failing checks named, then the front summarised.
    pub fn table(&self) -> String {
        let mut s = String::new();
        let method = match self.method {
            Method::Grid => "full grid",
            Method::LatinHypercube => "Latin hypercube",
        };
        s.push_str(&format!(
            "sweep: {} points ({method}) over {}; {} skipped (time budget)\n",
            self.points.len(),
            self.params.iter().map(|p| p.pointer.as_str()).collect::<Vec<_>>().join(", "),
            self.skipped
        ));
        let dir = |o: &Objective| match o.direction {
            Direction::Maximize => "max",
            Direction::Minimize => "min",
        };
        s.push_str(&format!(
            "objectives: {}\n",
            self.objectives.iter().map(|o| if o.unit.is_empty() { format!("{} ({})", o.label, dir(o)) } else { format!("{} [{}] ({})", o.label, o.unit, dir(o)) }).collect::<Vec<_>>().join(", ")
        ));
        s.push_str(&format!(
            "front | # | {} | pass | failing | {}\n",
            self.params.iter().map(|p| short(&p.pointer)).collect::<Vec<_>>().join(" | "),
            self.objectives.iter().map(|o| o.label.clone()).collect::<Vec<_>>().join(" | ")
        ));
        for (i, p) in self.points.iter().enumerate().take(TABLE_ROWS) {
            let star = if self.front.contains(&i) { "*" } else { " " };
            let vals = p.values.iter().map(fmt_val).collect::<Vec<_>>().join(" | ");
            match &p.error {
                Some(e) => s.push_str(&format!("{star} | {i} | {vals} | error: {e}\n")),
                None => s.push_str(&format!(
                    "{star} | {i} | {vals} | {}/{} | {} | {}\n",
                    p.passing,
                    p.total,
                    if p.failing.is_empty() { "-".to_string() } else { p.failing.join(",") },
                    p.objectives.iter().map(|&f| fmt_num(f)).collect::<Vec<_>>().join(" | ")
                )),
            }
        }
        if self.points.len() > TABLE_ROWS {
            s.push_str(&format!("... {} more rows in the CSV\n", self.points.len() - TABLE_ROWS));
        }
        let passing = self.points.iter().filter(|p| p.error.is_none() && p.failing.is_empty()).count();
        let best_fail = self.front.first().map(|&i| self.points[i].failing.len());
        match best_fail {
            None => s.push_str("no point could be evaluated\n"),
            Some(0) => s.push_str(&format!("{passing} point(s) pass every check; Pareto front (*): {:?}\n", self.front)),
            Some(k) => s.push_str(&format!("no point passes every check; the front (*) is over the points with {k} failing: {:?}\n", self.front)),
        }
        s
    }

    pub fn csv(&self) -> String {
        let mut s = String::from("point,front");
        for p in &self.params {
            s.push(',');
            s.push_str(&p.pointer);
        }
        s.push_str(",passing,total,failing,error");
        for o in &self.objectives {
            s.push(',');
            s.push_str(&o.label.replace(',', ";"));
        }
        s.push('\n');
        for (i, p) in self.points.iter().enumerate() {
            s.push_str(&format!("{i},{}", if self.front.contains(&i) { 1 } else { 0 }));
            for v in &p.values {
                s.push(',');
                s.push_str(&csv_cell(&fmt_val(v)));
            }
            s.push_str(&format!(",{},{},{},{}", p.passing, p.total, csv_cell(&p.failing.join(";")), csv_cell(p.error.as_deref().unwrap_or(""))));
            for &f in &p.objectives {
                s.push(',');
                s.push_str(&if f.is_nan() { String::new() } else { format!("{f}") });
            }
            s.push('\n');
        }
        s
    }

    /// A small scatter plot. Two or more objectives: the first two
    /// objectives, front joined. One objective: objective against the first
    /// parameter. Passing points are filled; failing are hollow.
    pub fn svg(&self) -> String {
        let (w, h, m) = (480.0, 320.0, 44.0);
        let numeric = |v: &Value| v.as_f64();
        let (xs, ys, xlabel, ylabel): (Vec<f64>, Vec<f64>, String, String) = if self.objectives.len() >= 2 {
            (
                self.points.iter().map(|p| p.objectives[0]).collect(),
                self.points.iter().map(|p| p.objectives[1]).collect(),
                self.objectives[0].label.clone(),
                self.objectives[1].label.clone(),
            )
        } else {
            (
                self.points.iter().map(|p| numeric(&p.values[0]).unwrap_or(f64::NAN)).collect(),
                self.points.iter().map(|p| p.objectives.first().copied().unwrap_or(f64::NAN)).collect(),
                short(&self.params[0].pointer),
                self.objectives.first().map(|o| o.label.clone()).unwrap_or_default(),
            )
        };
        fn extent(v: &[f64]) -> (f64, f64) {
            v.iter().copied().filter(|f| f.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), f| (lo.min(f), hi.max(f)))
        }
        let (x0, x1) = extent(&xs);
        let (y0, y1) = extent(&ys);
        let span = |a: f64, b: f64| if (b - a).abs() < 1e-12 { 1.0 } else { b - a };
        let sx = |x: f64| m + (x - x0) / span(x0, x1) * (w - 2.0 * m);
        let sy = |y: f64| h - m - (y - y0) / span(y0, y1) * (h - 2.0 * m);
        let mut s = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"sans-serif\" font-size=\"11\">\n<rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\n<line x1=\"{m}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#444\"/>\n<line x1=\"{m}\" y1=\"{m}\" x2=\"{m}\" y2=\"{}\" stroke=\"#444\"/>\n",
            h - m, w - m, h - m, h - m
        );
        s.push_str(&format!("<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n", w / 2.0, h - 10.0, esc(&xlabel)));
        s.push_str(&format!("<text x=\"12\" y=\"{}\" text-anchor=\"middle\" transform=\"rotate(-90 12 {})\">{}</text>\n", h / 2.0, h / 2.0, esc(&ylabel)));
        if x0.is_finite() {
            s.push_str(&format!("<text x=\"{m}\" y=\"{}\" font-size=\"9\">{}</text><text x=\"{}\" y=\"{}\" font-size=\"9\" text-anchor=\"end\">{}</text>\n", h - m + 12.0, fmt_num(x0), w - m, h - m + 12.0, fmt_num(x1)));
            s.push_str(&format!("<text x=\"{}\" y=\"{}\" font-size=\"9\" text-anchor=\"end\">{}</text><text x=\"{}\" y=\"{m}\" font-size=\"9\" text-anchor=\"end\">{}</text>\n", m - 3.0, h - m, fmt_num(y0), m - 3.0, fmt_num(y1)));
        }
        // front, in x order
        let mut front = self.front.clone();
        front.sort_by(|&a, &b| xs[a].partial_cmp(&xs[b]).unwrap_or(std::cmp::Ordering::Equal));
        let path: Vec<String> = front.iter().filter(|&&i| xs[i].is_finite() && ys[i].is_finite()).map(|&i| format!("{:.1},{:.1}", sx(xs[i]), sy(ys[i]))).collect();
        if path.len() >= 2 {
            s.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"#d9480f\" stroke-width=\"1.5\"/>\n", path.join(" ")));
        }
        for (i, p) in self.points.iter().enumerate() {
            if !(xs[i].is_finite() && ys[i].is_finite()) {
                continue;
            }
            let pass = p.error.is_none() && p.failing.is_empty();
            let on_front = self.front.contains(&i);
            let (fill, stroke) = match (pass, on_front) {
                (true, true) => ("#d9480f", "#d9480f"),
                (true, false) => ("#1c7ed6", "#1c7ed6"),
                (false, true) => ("white", "#d9480f"),
                (false, false) => ("white", "#868e96"),
            };
            s.push_str(&format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.5\"><title>#{i}: {}</title></circle>\n", sx(xs[i]), sy(ys[i]), esc(&p.values.iter().map(fmt_val).collect::<Vec<_>>().join(", "))));
        }
        s.push_str(&format!("<text x=\"{}\" y=\"{}\" font-size=\"9\" text-anchor=\"end\" fill=\"#555\">filled = passes every check; orange = Pareto front</text>\n", w - m, m - 8.0));
        s.push_str("</svg>\n");
        s
    }

    /// Write `sweep-NNN.csv` and `.svg` under `dir/sweeps/`.
    pub fn write(&self, dir: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
        let d = dir.join("sweeps");
        std::fs::create_dir_all(&d)?;
        let n = std::fs::read_dir(&d)?.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "csv")).count();
        let csv = d.join(format!("sweep-{n:03}.csv"));
        let svg = d.join(format!("sweep-{n:03}.svg"));
        std::fs::write(&csv, self.csv())?;
        std::fs::write(&svg, self.svg())?;
        Ok((csv, svg))
    }
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ------------------------------------------------------------- the tool

/// The `sweep` tool definition (strict: every property required; the
/// ranges travel as a JSON string, as the edit tools' patches do).
pub fn tool_def() -> Value {
    json!({
        "name": "sweep",
        "description": "Design of experiments on the current design, read-only: for 1 to 3 design parameters, evaluate every check (including regression cases) at each point of a grid (or a Latin hypercube with 3 parameters or a grid over the point cap) and return a table with each point's passing count, failing checks and objective headlines, plus the Pareto front among the best points. Writes a CSV and an SVG plot under the run's sweeps/. Changes nothing: pick a point from the table and apply it with edit_design, then check. Bounded to 200 points and 20 s.",
        "strict": true,
        "input_schema": {
            "type": "object",
            "properties": {
                "params_json": {"type": "string", "description": "JSON object string: design path -> range. A path is dotted (\"envelope.balloon_g\") or a JSON pointer (\"/envelope/balloon_g\"). A range is a list of values ([600, 800, 1200] or part ids) or {\"min\": 0.8, \"max\": 2.0, \"steps\": 7}. 1 to 3 paths, e.g. {\"envelope.balloon_g\": [600,800,1000], \"envelope.fill_target_mps\": {\"min\":4,\"max\":6,\"steps\":5}}"},
                "maximize": {"type": "array", "items": {"type": "string"}, "description": "headline labels to maximize in the Pareto front; [] keeps the defaults (the domain objective, and each headline by its own bound)"},
                "minimize": {"type": "array", "items": {"type": "string"}, "description": "headline labels to minimize in the Pareto front; [] keeps the defaults"},
                "max_points": {"type": "integer", "description": "cap on evaluated points, 1-200; 60 is plenty for a first look"}
            },
            "required": ["params_json", "maximize", "minimize", "max_points"],
            "additionalProperties": false
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    params_json: String,
    maximize: Vec<String>,
    minimize: Vec<String>,
    max_points: usize,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RangeJson {
    List(Vec<Value>),
    Linear { min: f64, max: f64, steps: usize },
}

/// Parse the tool's `params_json` into ordered parameters (sorted by path,
/// as Optuna sorts `_param_names`, so the same request gives the same grid).
pub fn parse_params(params_json: &str) -> anyhow::Result<Vec<Param>> {
    let map: serde_json::Map<String, Value> = serde_json::from_str(params_json).map_err(|e| anyhow::anyhow!("params_json is not a JSON object: {e}"))?;
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out = vec![];
    for k in keys {
        let r: RangeJson = serde_json::from_value(map[k].clone()).map_err(|_| anyhow::anyhow!("{k}: a range is a list of values or {{\"min\",\"max\",\"steps\"}}"))?;
        let range = match r {
            RangeJson::List(v) if v.is_empty() => anyhow::bail!("{k}: empty value list"),
            RangeJson::List(v) => Range::List(v),
            RangeJson::Linear { min, max, steps } => {
                anyhow::ensure!((1..=MAX_POINTS).contains(&steps), "{k}: steps must be 1-{MAX_POINTS}");
                anyhow::ensure!(min.is_finite() && max.is_finite() && min <= max, "{k}: need finite min <= max");
                Range::Linear { min, max, steps }
            }
        };
        out.push(Param { pointer: pointer_of(k), range });
    }
    Ok(out)
}

/// Handle one `sweep` tool call from the workbench. `out_dir` is the run
/// directory; without one the files go under the system temp dir.
pub fn tool_call(domain: &dyn DomainTools, state: &DesignState, events: &tokio::sync::broadcast::Sender<Event>, run: &str, out_dir: Option<&Path>, input: &Value) -> ToolOutput {
    let a: Args = match serde_json::from_value(input.clone()) {
        Ok(a) => a,
        Err(e) => return ToolOutput::malformed(format!("sweep: {e}")),
    };
    let params = match parse_params(&a.params_json) {
        Ok(p) => p,
        Err(e) => return ToolOutput::malformed(format!("sweep: {e:#}")),
    };
    let sweep = match run_tool(domain, state, params, &a.maximize, &a.minimize, a.max_points) {
        Ok(s) => s,
        Err(e) => return ToolOutput::err(format!("sweep failed: {e:#}")),
    };
    let dir = out_dir.map(Path::to_path_buf).unwrap_or_else(|| std::env::temp_dir().join(format!("wright-{run}")));
    let mut text = sweep.table();
    match sweep.write(&dir) {
        Ok((csv, svg)) => {
            let _ = events.send(Event::Artifact { run: run.into(), label: "sweep plot".into(), path: svg.display().to_string() });
            let _ = events.send(Event::Artifact { run: run.into(), label: "sweep table (CSV)".into(), path: csv.display().to_string() });
            text.push_str(&format!("files: {} {}\n", csv.display(), svg.display()));
        }
        Err(e) => text.push_str(&format!("(could not write the CSV/SVG: {e})\n")),
    }
    ToolOutput::ok(text)
}

fn run_tool(domain: &dyn DomainTools, state: &DesignState, params: Vec<Param>, maximize: &[String], minimize: &[String], max_points: usize) -> anyhow::Result<Sweep> {
    run(domain, state, params, maximize, minimize, max_points, 0, Instant::now() + TIME_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::{Evidence, Headline, Outcome, Source};

    /// A toy domain: design {a, b}; checks a_ok (a <= 10), b_ok (b >= 1);
    /// objective a*b; headlines "cost" (a + b, at most 12) and "mass" (info).
    struct Toy;
    fn margin(name: &str, v: f64) -> Outcome {
        Outcome { name: name.into(), evidence: Evidence::Margin { value: v, unit: "".into() }, detail: String::new(), source: Source::Physics }
    }
    impl DomainTools for Toy {
        fn name(&self) -> &'static str {
            "toy"
        }
        fn spec_schema(&self) -> Value {
            json!({})
        }
        fn checks(&self) -> Vec<(&'static str, &'static str)> {
            vec![("a_ok", ""), ("b_ok", "")]
        }
        fn consistency_checks(&self) -> &'static [&'static str] {
            &[]
        }
        fn validate_spec(&self, s: &Value) -> anyhow::Result<Value> {
            Ok(s.clone())
        }
        fn validate_design(&self, d: &Value) -> anyhow::Result<Value> {
            anyhow::ensure!(d["a"].is_number() && d["b"].is_number(), "a and b must be numbers");
            Ok(d.clone())
        }
        fn design(&self, _: &Value) -> anyhow::Result<Value> {
            Ok(json!({"a": 2.0, "b": 2.0}))
        }
        fn design_under(&self, s: &Value, _: &[wright_core::Condition<Value>]) -> anyhow::Result<Value> {
            self.design(s)
        }
        fn check(&self, _: &Value, d: &Value) -> anyhow::Result<Finished> {
            let (a, b) = (d["a"].as_f64().unwrap(), d["b"].as_f64().unwrap());
            Ok(Finished { outcomes: vec![margin("a_ok", 10.0 - a), margin("b_ok", b - 1.0)] })
        }
        fn bom(&self, _: &Value) -> anyhow::Result<wright_core::Bom> {
            unimplemented!()
        }
        fn build(&self, _: &Value, _: &Path) -> anyhow::Result<wright_core::Artifacts> {
            unimplemented!()
        }
        fn objective(&self, d: &Value) -> anyhow::Result<(f64, &'static str)> {
            Ok((d["a"].as_f64().unwrap() * d["b"].as_f64().unwrap(), "product"))
        }
        fn headlines(&self, _: &Value, d: &Value) -> anyhow::Result<Vec<Headline>> {
            let (a, b) = (d["a"].as_f64().unwrap(), d["b"].as_f64().unwrap());
            Ok(vec![
                Headline { label: "cost".into(), value: a + b, unit: "$".into(), limit: Some(12.0), bound: Bound::AtMost, check: None },
                Headline { label: "mass".into(), value: a, unit: "g".into(), limit: None, bound: Bound::Info, check: None },
            ])
        }
        fn catalog_paths(&self) -> Vec<(&'static str, &'static str)> {
            vec![]
        }
        fn options(&self, _: &Value, _: &str, _: Option<&str>) -> anyhow::Result<Vec<wright_core::CatalogOption>> {
            Ok(vec![])
        }
    }

    fn state() -> DesignState {
        let mut s = DesignState::new("toy", "test", json!({}));
        s.design = Some(json!({"a": 2.0, "b": 2.0}));
        s
    }

    #[test]
    fn grid_is_the_cartesian_product_in_sorted_param_order() {
        let params = parse_params(r#"{"b": [1, 2], "a": {"min": 0, "max": 10, "steps": 3}}"#).unwrap();
        assert_eq!(params[0].pointer, "/a");
        let g = grid(&params);
        assert_eq!(g.len(), 6);
        assert_eq!(g[0], vec![json!(0.0), json!(1)]);
        assert_eq!(g[1], vec![json!(0.0), json!(2)]);
        assert_eq!(g[5], vec![json!(10.0), json!(2)]);
    }

    #[test]
    fn lhs_puts_one_sample_in_each_stratum() {
        let params = vec![
            Param { pointer: "/x".into(), range: Range::Linear { min: 0.0, max: 1.0, steps: 10 } },
            Param { pointer: "/y".into(), range: Range::Linear { min: 0.0, max: 1.0, steps: 10 } },
            Param { pointer: "/z".into(), range: Range::List((0..10).map(|i| json!(i)).collect()) },
        ];
        let pts = lhs(&params, 10, 7);
        assert_eq!(pts.len(), 10);
        for j in 0..2 {
            let mut strata: Vec<usize> = pts.iter().map(|p| (p[j].as_f64().unwrap() * 10.0) as usize).collect();
            strata.sort();
            assert_eq!(strata, (0..10).collect::<Vec<_>>(), "column {j} not Latin");
        }
        let mut zs: Vec<i64> = pts.iter().map(|p| p[2].as_i64().unwrap()).collect();
        zs.sort();
        assert_eq!(zs, (0..10).collect::<Vec<_>>());
        assert_eq!(lhs(&params, 10, 7), pts, "seeded: reproducible");
    }

    #[test]
    fn sweep_scores_every_point_and_finds_the_front() {
        let st = state();
        let params = parse_params(r#"{"a": [2, 6, 12], "b": [0.5, 3]}"#).unwrap();
        let s = run(&Toy, &st, params, &[], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(s.method, Method::Grid);
        assert_eq!(s.points.len(), 6);
        assert_eq!(s.objectives.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["product", "cost", "mass"]);
        // a=12 fails a_ok; b=0.5 fails b_ok
        let nums = |p: &Point| (p.values[0].as_f64().unwrap(), p.values[1].as_f64().unwrap());
        let p = |a: f64, b: f64| s.points.iter().find(|p| nums(p) == (a, b)).unwrap();
        assert_eq!(p(12.0, 3.0).failing, vec!["a_ok".to_string()]);
        assert_eq!(p(2.0, 0.5).failing, vec!["b_ok".to_string()]);
        assert!(p(6.0, 3.0).failing.is_empty());
        assert_eq!(p(6.0, 3.0).objectives[0], 18.0);
        // passing points: (2,3) product 6 cost 5; (6,3) product 18 cost 9:
        // neither dominates (max product, min cost) -> both on the front
        let mut front: Vec<(f64, f64)> = s.front.iter().map(|&i| nums(&s.points[i])).collect();
        front.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(front, vec![(2.0, 3.0), (6.0, 3.0)]);
        let t = s.table();
        assert!(t.contains("2 point(s) pass every check"), "{t}");
        assert!(t.contains("a_ok"), "{t}");
        let csv = s.csv();
        assert_eq!(csv.lines().count(), 7);
        assert!(csv.starts_with("point,front,/a,/b,passing,total,failing,error,product,cost,mass\n"));
        assert!(s.svg().contains("<polyline"));
    }

    #[test]
    fn info_headline_enters_the_front_only_when_named() {
        let st = state();
        let params = parse_params(r#"{"a": [2, 6], "b": [3]}"#).unwrap();
        // default: mass is Info -> both pass, (6,3) has more product and more cost: both on front
        let s = run(&Toy, &st, params.clone(), &[], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(s.front.len(), 2);
        // minimize cost only matters already; naming mass as minimize keeps both too;
        // maximizing mass with cost... use maximize=["mass"] and see (6,3) dominate on product+mass but not cost
        let s = run(&Toy, &st, params, &["mass".into()], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(s.front.len(), 2);
        let err = run(&Toy, &state(), parse_params(r#"{"a": [2]}"#).unwrap(), &["nope".into()], &[], 10, 0, Instant::now() + Duration::from_secs(5)).unwrap_err();
        assert!(err.to_string().contains("no headline named `nope`"), "{err}");
    }

    #[test]
    fn three_params_or_too_many_points_use_a_latin_hypercube_within_the_cap() {
        let st = state();
        let params = parse_params(r#"{"a": {"min": 0, "max": 10, "steps": 50}, "b": {"min": 1, "max": 5, "steps": 50}}"#).unwrap();
        let s = run(&Toy, &st, params, &[], &[], 30, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(s.method, Method::LatinHypercube);
        assert_eq!(s.points.len(), 30);
        assert!(s.points.iter().all(|p| p.error.is_none() && p.failing.is_empty()));
        let s = run(&Toy, &st, parse_params(r#"{"a": [1,2], "b": [2,3], "c": [1]}"#).unwrap(), &[], &[], 100, 0, Instant::now() + Duration::from_secs(5));
        assert!(s.unwrap_err().to_string().contains("no field at `/c`"));
    }

    #[test]
    fn a_spent_time_budget_skips_the_rest_but_evaluates_one() {
        let st = state();
        let params = parse_params(r#"{"a": {"min": 0, "max": 9, "steps": 10}}"#).unwrap();
        let s = run(&Toy, &st, params, &[], &[], 100, 0, Instant::now() - Duration::from_secs(1)).unwrap();
        assert_eq!(s.points.len(), 1);
        assert_eq!(s.skipped, 9);
        assert!(s.table().contains("9 skipped"));
    }

    #[test]
    fn when_nothing_passes_the_front_is_over_the_fewest_failures() {
        let st = state();
        let params = parse_params(r#"{"a": [12, 20], "b": [0.5]}"#).unwrap();
        let s = run(&Toy, &st, params, &[], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(s.points.iter().all(|p| p.failing.len() == 2));
        assert!(!s.front.is_empty());
        assert!(s.table().contains("no point passes every check; the front (*) is over the points with 2 failing"));
    }

    #[test]
    fn a_bad_value_type_is_a_per_point_error_not_a_crash() {
        let st = state();
        let params = parse_params(r#"{"a": [2, "six"]}"#).unwrap();
        let s = run(&Toy, &st, params, &[], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(s.points[1].error.as_deref().unwrap().contains("numbers"));
        assert_eq!(s.front, vec![0]);
    }

    #[test]
    fn tool_call_writes_files_and_emits_artifacts() {
        let st = state();
        let dir = std::env::temp_dir().join(format!("wright-sweep-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let input = json!({"params_json": r#"{"a": [2, 6]}"#, "maximize": [], "minimize": [], "max_points": 10});
        let out = tool_call(&Toy, &st, &tx, "t1", Some(&dir), &input);
        assert!(!out.is_error, "{}", out.content);
        assert!(dir.join("sweeps/sweep-000.csv").is_file());
        assert!(dir.join("sweeps/sweep-000.svg").is_file());
        let mut labels = vec![];
        while let Ok(Event::Artifact { label, .. }) = rx.try_recv() {
            labels.push(label);
        }
        assert_eq!(labels, vec!["sweep plot".to_string(), "sweep table (CSV)".to_string()]);
        // a second sweep gets the next number
        let out = tool_call(&Toy, &st, &tx, "t1", Some(&dir), &input);
        assert!(out.content.contains("sweep-001.csv"), "{}", out.content);
        let bad = tool_call(&Toy, &st, &tx, "t1", Some(&dir), &json!({"params_json": "nope", "maximize": [], "minimize": [], "max_points": 10}));
        assert!(bad.malformed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test -p wright-agents --lib print_example -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_example_table() {
        let params = parse_params(r#"{"a": [2, 6, 12], "b": {"min": 0.5, "max": 3, "steps": 3}}"#).unwrap();
        let s = run(&Toy, &state(), params, &[], &[], 100, 0, Instant::now() + Duration::from_secs(5)).unwrap();
        println!("{}", s.table());
    }

    #[test]
    fn dotted_paths_become_pointers() {
        assert_eq!(pointer_of("envelope.balloon_g"), "/envelope/balloon_g");
        assert_eq!(pointer_of("fins[2].span_mm"), "/fins/2/span_mm");
        assert_eq!(pointer_of("/already/a/pointer"), "/already/a/pointer");
    }
}
