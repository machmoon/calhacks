//! Spec elicitation and the feasibility probe: the step before the team
//! starts. The plain-language request becomes a requirements-only spec
//! with every assumption written down and sourced, units normalised
//! through `wright_core::units`, at most three clarifying questions (asked
//! of a person or answered from defaults when unattended), and a
//! deterministic probe that runs the domain's own `design` on the spec and
//! on relaxed variants to name the binding requirement ("range 300 km is
//! infeasible with 6S; 210 km passes") before any specialist burns tokens.
//!
//! Precedent (read, not remembered):
//!
//! * AutoGen 0.2 `autogen/agentchat/conversable_agent.py`: an agent's
//!   `human_input_mode` is `"ALWAYS"` or `"NEVER"` (`"TERMINATE"` is a
//!   mix); the one override point is `get_human_input(prompt) -> str`,
//!   which reads `iostream.input(prompt)` and appends every reply to
//!   `self._human_input`; when no reply function answers,
//!   `generate_reply` returns `self._default_auto_reply`. [`HumanInput`],
//!   [`Asker::get_human_input`], [`Answered`] and the per-question
//!   `default` that answers unattended follow that file one to one.
//! * Aider `aider/coders/architect_coder.py` `reply_completed`: the plan is
//!   shown first and `io.confirm_ask("Edit the files?")` gates the editor
//!   unless `auto_accept_architect`; the editor then runs from the plan
//!   text alone. Here the analyst's spec and the probe are shown (as
//!   events) before the orchestrator runs, and [`HumanInput::Never`] is the
//!   `auto_accept` path.
//!
//! Deviation, stated: AutoGen's human reply is free text appended to the
//! chat. Here an answer is a value at a dotted spec path, because the
//! harness locks the person's values (`DesignState::stated`) and the
//! reviewer attacks the rest; a free-text answer could not be locked.

use crate::agent::{run_agent, Conversation, Handler, ModelCfg, ToolOutput};
use crate::client::{BoxFut, Client};
use crate::state::DesignState;
use crate::tools::{merge_patch, DomainTools};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wright_core::units::{Length, Mass, Money, Speed, Temperature, Time};
use wright_core::Event;

/// AutoGen's `human_input_mode`, without `TERMINATE` (there is one
/// question round, so "ask only at the end" is "ask").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanInput {
    /// Every clarifying question goes to [`Asker::get_human_input`].
    Always,
    /// Nobody is asked: each question is answered by its own default and
    /// recorded as an [`Origin::UnattendedDefault`] assumption (AutoGen's
    /// `_default_auto_reply`).
    Never,
}

/// Where a person's answer comes from: AutoGen's `get_human_input`, the
/// one method a host overrides (a terminal, the desktop app, a test).
pub trait Asker: Send {
    /// Show `prompt` and return the reply; an empty reply means "take the
    /// default".
    fn get_human_input(&mut self, prompt: &str) -> String;
}

/// An asker that never answers: every question takes its default.
pub struct NoHuman;
impl Asker for NoHuman {
    fn get_human_input(&mut self, _prompt: &str) -> String {
        String::new()
    }
}

/// Where a spec value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Stated in the request (locked: `DesignState::stated`).
    Request,
    /// The schema's documented default, or a field left to the domain's
    /// `serde(default)`.
    SchemaDefault,
    /// A stated engineering convention the analyst chose (an ISA+15 K hot
    /// day, a 1200 mm span cap) and must name in `note`.
    Convention,
    /// A person's answer to a clarifying question.
    Answer,
    /// A clarifying question answered from its default because nobody was
    /// there to ask.
    UnattendedDefault,
}

/// One value the request did not state, with where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assumption {
    /// Dotted spec path (`requirements.hot_day_dk`).
    pub path: String,
    pub value: Value,
    pub source: Origin,
    /// The convention or default named, in one line.
    pub note: String,
}

/// One clarifying question the analyst may ask (at most
/// [`ElicitConfig::max_questions`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub path: String,
    pub question: String,
    /// Values to choose from, or empty for a free value.
    #[serde(default)]
    pub options: Vec<Value>,
    /// What answers it unattended.
    pub default: Value,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answered {
    pub question: Question,
    pub answer: Value,
    pub source: Origin,
}

/// A quantity found in the request and normalised to the unit the specs
/// use, through `wright_core::units` (never a bare conversion factor at
/// the use site).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    /// As written ("300 miles").
    pub text: String,
    /// length, mass, time, speed, money, temperature, cells.
    pub kind: String,
    /// The SI value (m, kg, s, m/s, USD, K; cells as a count).
    pub si: f64,
    /// Ready for the spec ("482.8 km").
    pub normalised: String,
}

/// One relaxation of one requirement the probe tried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relaxation {
    pub path: String,
    pub from: f64,
    pub to: f64,
    pub passes: bool,
    /// Checks still failing under the relaxed spec (empty when it passes).
    pub failing: Vec<String>,
}

/// What the deterministic probe found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Probe {
    pub feasible: bool,
    /// Checks the spec's own design fails (or the design error).
    pub failing: Vec<String>,
    pub error: Option<String>,
    pub designs_run: usize,
    pub relaxations: Vec<Relaxation>,
    /// The requirement whose relaxation alone makes the spec pass, with
    /// the passing value: "range_km 300 -> 210".
    pub binding: Option<Relaxation>,
}

impl Probe {
    /// One paragraph for the desktop and the orchestrator's first turn.
    pub fn summary(&self) -> String {
        if let Some(e) = &self.error {
            return format!("probe: the domain could not design this spec ({e}); {} designs run", self.designs_run);
        }
        if self.feasible {
            return format!("probe: feasible as asked (the domain's own design passes every check; {} design run)", self.designs_run);
        }
        let mut s = format!("probe: infeasible as asked; failing {}", self.failing.join(", "));
        match &self.binding {
            Some(b) => s.push_str(&format!(
                "; binding requirement {} = {} ({} passes); {} designs run",
                b.path,
                fmt(b.from),
                fmt(b.to),
                self.designs_run
            )),
            None => {
                let best = self.relaxations.iter().min_by_key(|r| r.failing.len());
                match best {
                    Some(r) => s.push_str(&format!(
                        "; no single relaxation passes (closest: {} = {} still fails {}); {} designs run",
                        r.path,
                        fmt(r.to),
                        r.failing.join(", "),
                        self.designs_run
                    )),
                    None => s.push_str(&format!("; no relaxation tried ({} designs run)", self.designs_run)),
                }
            }
        }
        s
    }
}

fn fmt(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e9 {
        format!("{x:.0}")
    } else {
        format!("{x:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The elicited spec and everything that explains it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Elicited {
    pub spec: Value,
    /// The values the request stated: what the harness locks.
    pub stated: Value,
    pub assumptions: Vec<Assumption>,
    pub answered: Vec<Answered>,
    pub quantities: Vec<Quantity>,
    pub probe: Probe,
    /// The analyst's one-line reading of the request.
    pub reading: String,
}

impl Elicited {
    /// The assumptions block, one line per value, sourced.
    pub fn assumptions_block(&self) -> String {
        let mut out = vec![format!("Request read as: {}", self.reading)];
        if !self.quantities.is_empty() {
            out.push("Quantities normalised:".into());
            for q in &self.quantities {
                out.push(format!("- {} -> {} ({})", q.text, q.normalised, q.kind));
            }
        }
        out.push("Locked from the request:".into());
        for (p, v) in leaves(&self.stated) {
            out.push(format!("- {p} = {v}"));
        }
        out.push("Assumptions (each with its source):".into());
        if self.assumptions.is_empty() {
            out.push("- none".into());
        }
        for a in &self.assumptions {
            out.push(format!("- {} = {} [{}] {}", a.path, a.value, origin_name(a.source), a.note));
        }
        if !self.answered.is_empty() {
            out.push("Clarifying questions:".into());
            for a in &self.answered {
                out.push(format!("- {}? {} -> {} [{}]", a.question.path, a.question.question, a.answer, origin_name(a.source)));
            }
        }
        out.push(self.probe.summary());
        out.join("\n")
    }

    /// Install the spec in a fresh state: the stated values locked, every
    /// assumption on the list the reviewer reads.
    pub fn apply(&self, state: &mut DesignState) {
        state.spec = self.spec.clone();
        state.stated = self.stated.clone();
        for a in &self.assumptions {
            state.assumptions.push(format!("{{\"{}\": {}}} ({}: {})", a.path, a.value, origin_name(a.source), a.note));
        }
        state.notes.push(format!("elicit: {}", self.probe.summary()));
    }
}

fn origin_name(o: Origin) -> &'static str {
    match o {
        Origin::Request => "request",
        Origin::SchemaDefault => "schema default",
        Origin::Convention => "convention",
        Origin::Answer => "answer",
        Origin::UnattendedDefault => "unattended default",
    }
}

// ------------------------------------------------------------- quantities

/// Every quantity written in `request`, normalised through
/// `wright_core::units`: "300 miles" -> "482.8 km", "2 lb" -> "907.2 g",
/// "10,000 ft" -> "3048 m", "$400" -> "400 USD", "6S" -> "6 cells".
pub fn quantities(request: &str) -> Vec<Quantity> {
    let chars: Vec<char> = request.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        let dollar = chars[i] == '$';
        let start = i;
        if dollar {
            i += 1;
        }
        let num_start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || (chars[i] == ',' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())) {
            i += 1;
        }
        if i == num_start || !chars[num_start].is_ascii_digit() {
            i = start + 1;
            continue;
        }
        let num_text: String = chars[num_start..i].iter().filter(|c| **c != ',').collect();
        let Ok(v) = num_text.parse::<f64>() else { continue };
        let mut j = i;
        let attached = j < chars.len() && chars[j] != ' ';
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let unit_start = j;
        while j < chars.len() && (chars[j].is_alphabetic() || matches!(chars[j], '°' | '/' | '\'' | '"')) {
            j += 1;
        }
        let unit: String = chars[unit_start..j].iter().collect();
        let text: String = chars[start..if unit.is_empty() { i } else { j }].iter().collect();
        if dollar {
            out.push(Quantity { text: chars[start..i].iter().collect(), kind: "money".into(), si: Money::usd(v).value(), normalised: format!("{} USD", fmt(v)) });
            continue;
        }
        if let Some(q) = classify(&unit, v, attached, text.trim().to_string()) {
            out.push(q);
            i = j;
        }
    }
    out
}

fn classify(unit: &str, v: f64, attached: bool, text: String) -> Option<Quantity> {
    let u = unit.trim_end_matches(['.', ',']);
    let lower = u.to_ascii_lowercase();
    let q = |kind: &str, si: f64, normalised: String| Some(Quantity { text, kind: kind.into(), si, normalised });
    match lower.as_str() {
        "mile" | "miles" | "mi" => {
            let l = Length::ft(v * 5280.0); // 1 mile = 5280 ft (international)
            q("length", l.value(), format!("{} km", fmt(l.as_km())))
        }
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => q("length", Length::km(v).value(), format!("{} km", fmt(v))),
        "ft" | "feet" | "foot" | "'" => {
            let l = Length::ft(v);
            q("length", l.value(), format!("{} m", fmt(l.as_m())))
        }
        "in" | "inch" | "inches" | "\"" => {
            let l = Length::inch(v);
            q("length", l.value(), format!("{} mm", fmt(l.as_mm())))
        }
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => q("length", Length::mm(v).value(), format!("{} mm", fmt(v))),
        "cm" => q("length", Length::mm(v * 10.0).value(), format!("{} mm", fmt(v * 10.0))),
        "meter" | "meters" | "metre" | "metres" => q("length", Length::m(v).value(), format!("{} m", fmt(v))),
        "m" if attached => q("length", Length::m(v).value(), format!("{} m", fmt(v))),
        "lb" | "lbs" | "pound" | "pounds" => {
            let m = Mass::lb(v);
            q("mass", m.value(), format!("{} g", fmt(m.as_g())))
        }
        "oz" | "ounce" | "ounces" => {
            let m = Mass::lb(v / 16.0); // 16 oz = 1 lb avoirdupois
            q("mass", m.value(), format!("{} g", fmt(m.as_g())))
        }
        "kg" | "kilogram" | "kilograms" => q("mass", Mass::kg(v).value(), format!("{} kg", fmt(v))),
        "g" | "gram" | "grams" if attached || lower != "g" => q("mass", Mass::g(v).value(), format!("{} g", fmt(v))),
        "s" if attached && u == "S" => q("cells", v, format!("{} cells in series", fmt(v))),
        "min" | "mins" | "minute" | "minutes" => q("time", Time::minutes(v).value(), format!("{} min", fmt(v))),
        "h" | "hr" | "hrs" | "hour" | "hours" => {
            let t = Time::h(v);
            q("time", t.value(), format!("{} min", fmt(t.as_minutes())))
        }
        "s" | "sec" | "secs" | "second" | "seconds" if attached || lower != "s" => q("time", Time::s(v).value(), format!("{} s", fmt(v))),
        "mph" => {
            let s = Speed::mps(v * 0.44704); // 1 mph = 0.44704 m/s exactly
            q("speed", s.value(), format!("{} m/s", fmt(s.as_mps())))
        }
        "kt" | "kts" | "knot" | "knots" => {
            let s = Speed::mps(v * 1852.0 / 3600.0); // 1 kt = 1852 m/h
            q("speed", s.value(), format!("{} m/s", fmt(s.as_mps())))
        }
        "km/h" | "kph" | "kmh" => {
            let s = Speed::kmh(v);
            q("speed", s.value(), format!("{} m/s", fmt(s.as_mps())))
        }
        "m/s" | "mps" => q("speed", Speed::mps(v).value(), format!("{} m/s", fmt(v))),
        "usd" | "dollars" | "bucks" => q("money", Money::usd(v).value(), format!("{} USD", fmt(v))),
        "°f" | "degf" | "fahrenheit" => {
            let t = Temperature::celsius((v - 32.0) * 5.0 / 9.0);
            q("temperature", t.value(), format!("{} C", fmt(t.as_celsius())))
        }
        "°c" | "degc" | "celsius" => q("temperature", Temperature::celsius(v).value(), format!("{} C", fmt(v))),
        _ => None,
    }
}

/// The quantities as lines for the analyst's prompt.
pub fn quantities_block(qs: &[Quantity]) -> String {
    if qs.is_empty() {
        return "Quantities in the request: none found by the unit parser.".into();
    }
    let lines: Vec<String> = qs.iter().map(|q| format!("- {} = {} ({})", q.text, q.normalised, q.kind)).collect();
    format!("Quantities in the request, normalised by wright_core::units (use these values, not your own conversions):\n{}", lines.join("\n"))
}

// ---------------------------------------------------------------- probe

/// Run the domain's own design and checks on `spec`; if it fails, relax
/// one numeric requirement at a time and report the one whose relaxation
/// alone passes. At most `max_designs` designs are run. Deterministic:
/// no model is called.
pub fn probe(domain: &dyn DomainTools, spec: &Value, max_designs: usize) -> Probe {
    let schema = domain.spec_schema();
    let mut p = Probe::default();
    let mut budget = max_designs;
    match try_design(domain, spec, &mut p.designs_run, &mut budget) {
        Ok(f) if f.is_empty() => {
            p.feasible = true;
            return p;
        }
        Ok(f) => p.failing = f,
        Err(e) => {
            p.error = Some(e);
            return p;
        }
    }
    // Phase 1: one step each way for every numeric leaf, stated-looking
    // requirements first (top-level and `requirements.*` before nested
    // conditions).
    let mut leaves = leaves(spec).into_iter().filter_map(|(path, v)| v.as_f64().map(|x| (path, x))).collect::<Vec<_>>();
    leaves.sort_by_key(|(p, _)| p.matches('.').count());
    let mut best: Option<(String, f64, f64, usize)> = None; // path, from, to, failing count
    for (path, from) in &leaves {
        for factor in [0.7, 1.5] {
            if budget == 0 {
                break;
            }
            let Some(to) = step(&schema, path, *from, factor) else { continue };
            let r = relax(domain, spec, path, *from, to, &mut p.designs_run, &mut budget);
            let Some(r) = r else { continue };
            if r.passes {
                if p.binding.as_ref().is_none_or(|b| rel(b) > rel(&r)) {
                    p.binding = Some(r.clone());
                }
            } else if best.as_ref().is_none_or(|b| b.3 > r.failing.len()) && r.failing.len() < p.failing.len() {
                best = Some((path.clone(), *from, to, r.failing.len()));
            }
            p.relaxations.push(r);
        }
    }
    if p.binding.is_some() {
        tighten(domain, spec, &mut p, &mut budget);
        return p;
    }
    // Phase 2: keep going in the direction that helped most.
    if let Some((path, from, to, _)) = best {
        let dir = if to > from { 1 } else { -1 };
        let mut last = to;
        for factor in [2.0, 3.0, 5.0] {
            if budget == 0 {
                break;
            }
            let f = if dir > 0 { factor } else { 1.0 / factor };
            let Some(next) = step(&schema, &path, from, f) else { break };
            if (next - last).abs() < 1e-12 {
                break;
            }
            last = next;
            let Some(r) = relax(domain, spec, &path, from, next, &mut p.designs_run, &mut budget) else { break };
            let passes = r.passes;
            p.relaxations.push(r.clone());
            if passes {
                p.binding = Some(r);
                tighten(domain, spec, &mut p, &mut budget);
                break;
            }
        }
    }
    p
}

/// Two bisection steps between the spec's value and the passing one, so
/// "210 passes" is close to the edge rather than a coarse 0.7x.
fn tighten(domain: &dyn DomainTools, spec: &Value, p: &mut Probe, budget: &mut usize) {
    let Some(b) = p.binding.clone() else { return };
    let (mut lo_fail, mut hi_pass) = (b.from, b.to);
    for _ in 0..2 {
        if *budget == 0 {
            break;
        }
        let mid = (lo_fail + hi_pass) / 2.0;
        let Some(r) = relax(domain, spec, &b.path, b.from, mid, &mut p.designs_run, budget) else { break };
        if r.passes {
            hi_pass = mid;
            p.binding = Some(r.clone());
        } else {
            lo_fail = mid;
        }
        p.relaxations.push(r);
    }
}

fn rel(r: &Relaxation) -> f64 {
    if r.from == 0.0 {
        r.to.abs()
    } else {
        ((r.to - r.from) / r.from).abs()
    }
}

fn relax(domain: &dyn DomainTools, spec: &Value, path: &str, from: f64, to: f64, run: &mut usize, budget: &mut usize) -> Option<Relaxation> {
    let mut s = spec.clone();
    set_path(&mut s, path, json!(to));
    if domain.validate_spec(&s).is_err() {
        return None;
    }
    match try_design(domain, &s, run, budget) {
        Ok(failing) => Some(Relaxation { path: path.into(), from, to, passes: failing.is_empty(), failing }),
        Err(e) => Some(Relaxation { path: path.into(), from, to, passes: false, failing: vec![format!("design: {e}")] }),
    }
}

/// Design and check once; `Ok(failing check names)`.
fn try_design(domain: &dyn DomainTools, spec: &Value, run: &mut usize, budget: &mut usize) -> Result<Vec<String>, String> {
    if *budget == 0 {
        return Err("probe budget exhausted".into());
    }
    *budget -= 1;
    *run += 1;
    let design = domain.design(spec).map_err(|e| format!("{e:#}"))?;
    let fin = domain.check(spec, &design).map_err(|e| format!("{e:#}"))?;
    Ok(fin.failures().map(|o| o.name.clone()).collect())
}

/// `from * factor`, clamped into the schema's range for `path`; `None`
/// when that leaves the value where it was (a value at its bound), or the
/// field is an integer (cell counts are a question, not a relaxation).
fn step(schema: &Value, path: &str, from: f64, factor: f64) -> Option<f64> {
    let (lo, hi, integer) = range_at(schema, path);
    if integer {
        return None;
    }
    let (floor, _) = crate::harness::unit_bounds(path.rsplit('.').next().unwrap_or(path));
    let mut to = if from == 0.0 { if factor > 1.0 { floor } else { return None } } else { from * factor };
    if let Some(l) = lo {
        to = to.max(l);
    }
    if let Some(h) = hi {
        to = to.min(h);
    }
    ((to - from).abs() > 1e-9 * from.abs().max(1.0)).then_some(to)
}

/// `(minimum, maximum, is_integer)` of the schema node at a dotted path.
fn range_at(schema: &Value, path: &str) -> (Option<f64>, Option<f64>, bool) {
    let mut node = schema;
    for k in path.split('.') {
        let Some(n) = node.get("properties").and_then(|p| p.get(k)) else { return (None, None, false) };
        node = n;
    }
    let ty = match node.get("type") {
        Some(Value::String(t)) => t == "integer",
        Some(Value::Array(a)) => a.iter().any(|t| t == "integer"),
        _ => false,
    };
    (
        node.get("minimum").or_else(|| node.get("exclusiveMinimum")).and_then(Value::as_f64),
        node.get("maximum").or_else(|| node.get("exclusiveMaximum")).and_then(Value::as_f64),
        ty,
    )
}

/// Every scalar leaf of `v` as `(dotted path, value)`.
pub fn leaves(v: &Value) -> Vec<(String, Value)> {
    fn walk(v: &Value, at: String, out: &mut Vec<(String, Value)>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    walk(x, if at.is_empty() { k.clone() } else { format!("{at}.{k}") }, out);
                }
            }
            Value::Null => {}
            other => out.push((at, other.clone())),
        }
    }
    let mut out = vec![];
    walk(v, String::new(), &mut out);
    out
}

fn set_path(v: &mut Value, path: &str, value: Value) {
    let mut patch = value;
    for k in path.rsplit('.') {
        patch = json!({ k: patch });
    }
    merge_patch(v, &patch);
}

fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut v = v;
    for k in path.split('.') {
        v = v.get(k)?;
    }
    Some(v)
}

/// The sub-object of `spec` at exactly `paths`.
fn pick(spec: &Value, paths: &[String]) -> Value {
    let mut out = json!({});
    for p in paths {
        if let Some(x) = get_path(spec, p) {
            set_path(&mut out, p, x.clone());
        }
    }
    out
}

// -------------------------------------------------------------- analyst

#[derive(Debug, Clone)]
pub struct ElicitConfig {
    pub model: ModelCfg,
    /// Model calls allowed to the analyst (each rejected spec costs one).
    pub turns: u32,
    pub human: HumanInput,
    pub max_questions: usize,
    /// Designs the probe may run.
    pub max_designs: usize,
}

impl ElicitConfig {
    pub fn new(model: ModelCfg, human: HumanInput) -> Self {
        ElicitConfig { model, turns: 4, human, max_questions: 3, max_designs: 12 }
    }
}

#[derive(Deserialize)]
struct ProposeArgs {
    reading: String,
    spec_json: String,
    from_request: Vec<String>,
    assumptions: Vec<ProposedAssumption>,
    questions: Vec<ProposedQuestion>,
}

#[derive(Deserialize)]
struct ProposedAssumption {
    path: String,
    source: String,
    note: String,
}

#[derive(Deserialize)]
struct ProposedQuestion {
    path: String,
    question: String,
    options_json: String,
    default_json: String,
    why: String,
}

/// The analyst's one tool.
pub fn analyst_tools() -> Vec<Value> {
    vec![json!({
        "name": "propose_spec",
        "description": "Write the requirements-only spec from the person's request. Rejected (with the reason) if it does not fit the schema, if a from_request path is not in the spec, or if there are more questions than allowed; then fix and call again.",
        "strict": true,
        "input_schema": {
            "type": "object",
            "properties": {
                "reading": {"type": "string", "description": "one line: what the person is asking for, in engineering terms"},
                "spec_json": {"type": "string", "description": "the spec as a JSON object string; requirements only, never design choices; numbers in the schema's units, taken from the normalised quantities"},
                "from_request": {"type": "array", "items": {"type": "string"}, "description": "dotted spec paths whose values the request states (or follow from it by unit conversion alone). These are LOCKED as the person's requirements"},
                "assumptions": {"type": "array", "items": {"type": "object", "properties": {
                    "path": {"type": "string", "description": "dotted spec path"},
                    "source": {"type": "string", "enum": ["schema_default", "convention"], "description": "schema_default: the schema's documented default; convention: an engineering convention you chose"},
                    "note": {"type": "string", "description": "one line naming the default or convention and why it fits this request"}
                }, "required": ["path", "source", "note"], "additionalProperties": false},
                    "description": "every value in spec_json that the request did not state"},
                "questions": {"type": "array", "items": {"type": "object", "properties": {
                    "path": {"type": "string", "description": "the spec path the answer sets"},
                    "question": {"type": "string"},
                    "options_json": {"type": "string", "description": "JSON array of values to pick from, or [] for a free value"},
                    "default_json": {"type": "string", "description": "JSON value used when nobody answers"},
                    "why": {"type": "string", "description": "why the design depends on it"}
                }, "required": ["path", "question", "options_json", "default_json", "why"], "additionalProperties": false},
                    "description": "at most three; only where the request is ambiguous AND the answer changes the design (a 6S vs 4S pack, a budget that may or may not include the battery). Never ask what the schema defaults well"}
            },
            "required": ["reading", "spec_json", "from_request", "assumptions", "questions"],
            "additionalProperties": false
        }
    })]
}

pub fn analyst_system(domain: &dyn DomainTools, max_questions: usize) -> String {
    let envelope = crate::harness::envelope_lines(&domain.spec_schema());
    let env = if envelope.is_empty() { String::new() } else { format!("\nDeclared envelope (a value outside it is rejected):\n- {}\n", envelope.join("\n- ")) };
    format!(
        "You are the requirements analyst on a team designing a {name}. Before the team starts you turn the person's plain-language request into a requirements-only spec, with one call to propose_spec.\n\n\
Rules:\n\
- Requirements only: what the person needs (payload, range, budget, site, altitude), never design choices (a motor, a span, a tube). Leave every choice open for the search.\n\
- Use the schema's units. The user message lists every quantity in the request already normalised by wright_core::units; copy those values, do not convert yourself.\n\
- from_request lists exactly the paths the request states. Everything else you write is an assumption: list each with its source. A schema default is `schema_default`; a value you chose by engineering convention is `convention` and the note names the convention. Never leave a value unsourced.\n\
- Ask at most {max_questions} clarifying questions, only where the request is ambiguous and the answer changes the design. Each has a default that answers it when nobody is there. Do not ask about anything the schema defaults well.\n\
- Do not pad the spec: a field the schema defaults and the request does not touch may be left out (it then counts as a schema default).\n\n\
Domain: {name}.\n\nSpec JSON schema:\n{schema}\n{env}",
        name = domain.name(),
        schema = serde_json::to_string_pretty(&domain.spec_schema()).unwrap_or_default(),
    )
}

struct Analyst<'a> {
    domain: &'a dyn DomainTools,
    max_questions: usize,
    proposal: Option<(ProposeArgs, Value)>,
}

impl Handler for Analyst<'_> {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput> {
        Box::pin(async move {
            if name != "propose_spec" {
                return ToolOutput::malformed(format!("unknown tool {name}"));
            }
            let a: ProposeArgs = match serde_json::from_value(input.clone()) {
                Ok(a) => a,
                Err(e) => return ToolOutput::malformed(format!("input does not match the tool schema: {e}")),
            };
            let spec: Value = match serde_json::from_str(&a.spec_json) {
                Ok(Value::Object(m)) => Value::Object(m),
                Ok(_) => return ToolOutput::malformed("spec_json must be a JSON object"),
                Err(e) => return ToolOutput::malformed(format!("spec_json is not JSON: {e}")),
            };
            let spec = match self.domain.validate_spec(&spec) {
                Ok(v) => v,
                Err(e) => return ToolOutput::err(format!("spec rejected, write it again: {e:#}")),
            };
            if a.questions.len() > self.max_questions {
                return ToolOutput::err(format!("{} questions; at most {} are allowed: keep the ones whose answer changes the design", a.questions.len(), self.max_questions));
            }
            if let Some(p) = a.from_request.iter().find(|p| get_path(&spec, p).is_none()) {
                return ToolOutput::err(format!("from_request path `{p}` is not in the spec"));
            }
            for q in &a.questions {
                if serde_json::from_str::<Value>(&q.default_json).is_err() {
                    return ToolOutput::err(format!("question `{}`: default_json is not JSON", q.path));
                }
                if serde_json::from_str::<Vec<Value>>(&q.options_json).is_err() {
                    return ToolOutput::err(format!("question `{}`: options_json is not a JSON array", q.path));
                }
            }
            self.proposal = Some((a, spec));
            ToolOutput::ok("spec accepted").stop()
        })
    }
}

/// Elicit the spec for `request` (with `hint`, a partial spec the person
/// supplied, or `null`), answer the clarifying questions through `asker`
/// or from defaults, run the probe, and emit the assumptions and probe as
/// events (`AgentMessage` from "elicit", and a `feasibility_probe`
/// `ToolCall`/`ToolResult` pair whose `ok` is feasibility; an `Event`
/// variant was not added because three exhaustive matches in
/// `wright-desktop` would have to change in a tree others are editing).
pub async fn elicit(client: &Client, domain: &dyn DomainTools, request: &str, hint: &Value, cfg: &ElicitConfig, asker: &mut dyn Asker) -> anyhow::Result<Elicited> {
    let qs = quantities(request);
    let mut conv = Conversation::new("elicit", cfg.model.clone(), analyst_system(domain, cfg.max_questions), analyst_tools());
    let mut task = format!("A person asked: {request}\n\n{}", quantities_block(&qs));
    if !hint.is_null() {
        task.push_str(&format!("\n\nThey also supplied this partial spec; keep its values and list them under from_request:\n{hint}"));
    }
    let mut h = Analyst { domain, max_questions: cfg.max_questions, proposal: None };
    let end = run_agent(client, &mut conv, vec![json!({"type": "text", "text": task})], &mut h, cfg.turns).await?;
    let Some((a, mut spec)) = h.proposal else {
        anyhow::bail!("the analyst did not propose a spec ({end:?})");
    };

    // Clarifying questions: AutoGen's get_human_input, or the default.
    let mut answered = vec![];
    for q in &a.questions {
        let options: Vec<Value> = serde_json::from_str(&q.options_json).unwrap_or_default();
        let default: Value = serde_json::from_str(&q.default_json).unwrap_or(Value::Null);
        let question = Question { path: q.path.clone(), question: q.question.clone(), options: options.clone(), default: default.clone(), why: q.why.clone() };
        let (answer, source) = match cfg.human {
            HumanInput::Never => (default.clone(), Origin::UnattendedDefault),
            HumanInput::Always => {
                let opts = if options.is_empty() { String::new() } else { format!(" options: {}", serde_json::to_string(&options).unwrap_or_default()) };
                let reply = asker.get_human_input(&format!("{} ({}){opts} [default {default}]: ", q.question, q.why));
                let reply = reply.trim();
                if reply.is_empty() {
                    (default.clone(), Origin::UnattendedDefault)
                } else {
                    (serde_json::from_str(reply).unwrap_or_else(|_| Value::String(reply.to_string())), Origin::Answer)
                }
            }
        };
        let mut next = spec.clone();
        set_path(&mut next, &q.path, answer.clone());
        match domain.validate_spec(&next) {
            Ok(v) => spec = v,
            Err(e) => anyhow::bail!("answer {answer} to `{}` leaves the spec invalid: {e:#}", q.path),
        }
        answered.push(Answered { question, answer, source });
    }

    // Lock what the request stated; source everything else.
    let stated = pick(&spec, &a.from_request);
    let mut assumptions: Vec<Assumption> = vec![];
    for pa in &a.assumptions {
        if a.from_request.contains(&pa.path) {
            continue;
        }
        let value = get_path(&spec, &pa.path).cloned().unwrap_or(Value::Null);
        let source = if pa.source == "convention" { Origin::Convention } else { Origin::SchemaDefault };
        assumptions.push(Assumption { path: pa.path.clone(), value, source, note: pa.note.clone() });
    }
    for ans in &answered {
        assumptions.retain(|x| x.path != ans.question.path);
        assumptions.push(Assumption {
            path: ans.question.path.clone(),
            value: ans.answer.clone(),
            source: ans.source,
            note: format!("{} ({})", ans.question.question, ans.question.why),
        });
    }
    // a leaf nobody sourced is still an assumption, said so
    for (path, value) in leaves(&spec) {
        if a.from_request.iter().any(|p| path == *p || path.starts_with(&format!("{p}."))) || assumptions.iter().any(|x| x.path == path) {
            continue;
        }
        assumptions.push(Assumption { path, value, source: Origin::SchemaDefault, note: "written by the analyst without a stated source; treated as a schema default".into() });
    }

    let probe = probe(domain, &spec, cfg.max_designs);
    let out = Elicited { spec, stated, assumptions, answered, quantities: qs, probe, reading: a.reading };
    let run = client.run.clone();
    let _ = client.events.send(Event::AgentMessage { run: run.clone(), agent: "elicit".into(), text: out.assumptions_block() });
    let _ = client.events.send(Event::ToolCall {
        run: run.clone(),
        agent: "elicit".into(),
        tool: "feasibility_probe".into(),
        input: json!({"spec": out.spec, "assumptions": out.assumptions, "probe": out.probe}),
    });
    let _ = client.events.send(Event::ToolResult { run, agent: "elicit".into(), tool: "feasibility_probe".into(), ok: out.probe.feasible, summary: out.probe.summary() });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_normalise_through_units() {
        let qs = quantities("fly 300 miles with a 2 lb payload for under $400, 10,000 ft field, 45 minutes, 6S pack, 20 mph wind, 95 °F");
        let by: std::collections::BTreeMap<&str, &Quantity> = qs.iter().map(|q| (q.text.as_str(), q)).collect();
        assert_eq!(by["300 miles"].normalised, "482.803 km");
        assert_eq!(by["2 lb"].normalised, "907.185 g");
        assert_eq!(by["$400"].normalised, "400 USD");
        assert_eq!(by["10,000 ft"].normalised, "3048 m");
        assert_eq!(by["45 minutes"].normalised, "45 min");
        assert_eq!(by["6S"].normalised, "6 cells in series");
        assert_eq!(by["20 mph"].normalised, "8.941 m/s");
        assert_eq!(by["95 °F"].normalised, "35 C");
        assert!((by["300 miles"].si - 482_803.2).abs() < 1e-6);
    }

    #[test]
    fn bare_numbers_and_single_letters_are_not_quantities() {
        let qs = quantities("a 4 motor plane with 2 cameras, version 3 of the design");
        assert!(qs.is_empty(), "{qs:?}");
        // "a" after a number is not a unit; "3 of" neither
    }

    #[test]
    fn step_respects_schema_bounds_and_skips_integers() {
        let schema = json!({"type": "object", "properties": {"requirements": {"type": "object", "properties": {
            "range_km": {"type": "number", "minimum": 0, "maximum": 1000},
            "cells_s": {"type": "integer"},
            "altitude_m": {"type": "number", "minimum": -378, "maximum": 4411}
        }}}});
        assert_eq!(step(&schema, "requirements.range_km", 800.0, 1.5), Some(1000.0));
        assert_eq!(step(&schema, "requirements.range_km", 1000.0, 1.5), None);
        assert_eq!(step(&schema, "requirements.range_km", 300.0, 0.7), Some(210.0));
        assert_eq!(step(&schema, "requirements.cells_s", 6.0, 0.7), None);
        // a zero steps up by its unit floor (harness::unit_bounds), never down
        assert_eq!(step(&schema, "requirements.altitude_m", 0.0, 0.7), None);
        assert_eq!(step(&schema, "requirements.altitude_m", 0.0, 1.5), Some(1000.0));
    }

    #[test]
    fn pick_and_leaves_round_trip() {
        let spec = json!({"requirements": {"range_km": 300, "payload_g": 500, "hot_day_dk": 15}, "name": "x"});
        let stated = pick(&spec, &["requirements.range_km".into(), "name".into()]);
        assert_eq!(stated, json!({"requirements": {"range_km": 300}, "name": "x"}));
        let mut ls = leaves(&spec).into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        ls.sort();
        assert_eq!(ls, ["name", "requirements.hot_day_dk", "requirements.payload_g", "requirements.range_km"]);
    }

    #[test]
    fn probe_summary_names_the_binding_requirement() {
        let p = Probe {
            feasible: false,
            failing: vec!["range".into()],
            error: None,
            designs_run: 5,
            relaxations: vec![],
            binding: Some(Relaxation { path: "requirements.range_km".into(), from: 300.0, to: 210.0, passes: true, failing: vec![] }),
        };
        assert_eq!(p.summary(), "probe: infeasible as asked; failing range; binding requirement requirements.range_km = 300 (210 passes); 5 designs run");
    }
}
