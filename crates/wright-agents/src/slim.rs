//! Prompt slimming for specialists (cost + latency, 10/24 night).
//!
//! A specialist fixes failing checks. It gets the design state with only
//! the checks that bear on its task: every failing one, and every passing
//! one within 10 % of its margin (a fix that breaks a nearly-failing check
//! is the common regression), each with its margin and unit. The whole
//! report stays one `full_report` call away. The orchestrator and the
//! reviewer keep the full report (`DesignState::for_prompt`).
//!
//! Precedent: SWE-agent's observation shaping (SWE-agent
//! `sweagent/agent/history_processors.py`, `LastNObservations`: older tool
//! output elided to a one-line "Old environment output: (n lines omitted)"
//! so the agent sees what bears on the next step, with the full text still
//! reachable through a tool). Here the elision is by relevance (margin),
//! not by age, and the line says how to get the rest.
//!
//! "Within 10 % of its margin": checks report an absolute margin in a unit
//! (`+0.198 m/s`), not the limit, so the scale is read from the check's own
//! detail text: the first number written with the margin's unit
//! (`launch 4.20 m/s` -> 0.198 / 4.20 = 4.7 %), else the largest number in
//! the detail. A zero margin is always near; a check with no number in its
//! detail is near only at zero. This is a heuristic on text the domains
//! write; it errs toward showing a check.

use crate::state::DesignState;
use wright_core::{Finished, Outcome};

pub const NEAR_FRACTION: f64 = 0.10;

/// Numbers in `text` with the word that follows each (`4.20 m/s` -> (4.2,
/// "m/s"); ranges such as `-10..38` give both ends).
fn numbers(text: &str) -> Vec<(f64, String)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        let neg = c[i] == '-' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && (i == 0 || !c[i - 1].is_alphanumeric());
        if c[i].is_ascii_digit() || neg {
            let start = i;
            i += 1;
            while i < c.len() && (c[i].is_ascii_digit() || (c[i] == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()))) {
                i += 1;
            }
            let v: f64 = c[start..i].iter().collect::<String>().parse().unwrap_or(f64::NAN);
            // the unit: the next whitespace-separated word
            let rest: String = c[i..].iter().collect();
            let unit = if rest.starts_with(' ') { rest.split_whitespace().next().unwrap_or("").trim_matches(|x: char| matches!(x, ',' | ';' | ')' | '(')).to_string() } else { String::new() };
            if v.is_finite() {
                out.push((v, unit));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The scale a margin is judged against (see the module doc).
pub fn scale(o: &Outcome) -> Option<f64> {
    let unit = match &o.evidence {
        wright_core::Evidence::Margin { unit, .. } => unit.as_str(),
        _ => "",
    };
    let nums = numbers(&o.detail);
    if !unit.is_empty() {
        if let Some((v, _)) = nums.iter().find(|(v, u)| u == unit && v.abs() > 0.0) {
            return Some(v.abs());
        }
    }
    nums.iter().map(|(v, _)| v.abs()).filter(|v| *v > 0.0).fold(None, |m, v| Some(m.map_or(v, |m: f64| m.max(v))))
}

/// A passing check within [`NEAR_FRACTION`] of failing.
pub fn near_failing(o: &Outcome) -> bool {
    let Some(m) = o.margin() else { return false };
    if !o.passed() {
        return false;
    }
    if m == 0.0 {
        return true;
    }
    scale(o).is_some_and(|s| m <= NEAR_FRACTION * s)
}

/// The failing and near-failing checks, as report lines, with a header
/// that says what was left out and how to get it.
pub fn specialist_report(f: &Finished) -> String {
    let fails: Vec<&Outcome> = f.failures().collect();
    let near: Vec<&Outcome> = f.outcomes.iter().filter(|o| near_failing(o)).collect();
    let shown = fails.len() + near.len();
    let mut s = format!(
        "Failing checks ({} of {}):\n",
        fails.len(),
        f.outcomes.len()
    );
    for o in &fails {
        s.push_str(&format!("{o}\n"));
    }
    if fails.is_empty() {
        s.push_str("(none)\n");
    }
    s.push_str(&format!("\nPassing within {:.0} % of their limit (do not break these):\n", NEAR_FRACTION * 100.0));
    for o in &near {
        s.push_str(&format!("{o}\n"));
    }
    if near.is_empty() {
        s.push_str("(none)\n");
    }
    s.push_str(&format!(
        "\n{} other checks pass with room to spare and are not shown; call full_report for every check with its margin.",
        f.outcomes.len() - shown
    ));
    s
}

/// [`DesignState::for_prompt`] with the check report cut to
/// [`specialist_report`].
pub fn specialist_prompt(state: &DesignState) -> String {
    let full = state.for_prompt();
    let Some(report) = &state.report else { return full };
    let head = full.split("\n\nLast check report:\n").next().unwrap_or(&full);
    format!("{head}\n\nLast check report (failing and nearly failing only):\n{}", specialist_report(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::{Evidence, Source};

    fn o(name: &str, value: f64, unit: &str, detail: &str) -> Outcome {
        Outcome { name: name.into(), evidence: Evidence::Margin { value, unit: unit.into() }, detail: detail.into(), source: Source::Geometry }
    }

    #[test]
    fn near_failing_reads_the_scale_from_the_detail() {
        // balloon demo report lines (demo/balloon/state.json)
        assert!(near_failing(&o("ascent_rate", 0.1986, "m/s", "launch 4.20 m/s, mean 4.96 m/s (band 4-6)")));
        assert!(!near_failing(&o("gas_supply", 2.30, "m^3", "need 5.31 m^3 at 21 C/1 atm, have 7.61 usable in 1 cylinder(s)")));
        assert!(!near_failing(&o("burst_altitude", 7828.9, "m", "burst at 37829 m (CUSF closed form says 38958 m), target 30000 m")));
        assert!(near_failing(&o("supply_voltage", 0.0, "V", "lightaprs-2 takes 2.7-6.0 V")));
        assert!(near_failing(&o("part_temperature", 0.122, "K", "box -10..38 C vs rated -10..40 C")));
        assert!(near_failing(&o("recovery_probability", 0.0062, "", "P = 0.906: APRS 0.90, LoRa 0.85")));
        assert!(!near_failing(&o("failing", -1.0, "m", "x 3 m")), "failing is not near-failing");
        assert!(!near_failing(&o("words", 3.0, "dB", "no numbers here")));
    }

    #[test]
    fn the_specialist_report_keeps_failures_and_near_misses_and_says_how_to_get_the_rest() {
        let f = Finished { outcomes: vec![
            o("a", -2.0, "mm", "span 98 mm vs 100 mm"),
            o("b", 0.1, "m/s", "4.20 m/s"),
            o("c", 50.0, "mm", "200 mm"),
        ] };
        let s = specialist_report(&f);
        assert!(s.contains("FAIL a") && s.contains(" b ") && !s.contains(" c "), "{s}");
        assert!(s.contains("1 other checks pass") && s.contains("full_report"), "{s}");
        let mut st = DesignState::new("t", "r", serde_json::json!({}));
        st.report = Some(f);
        let p = specialist_prompt(&st);
        assert!(p.contains("DesignState:") && p.contains("failing and nearly failing only") && !p.contains(" c "), "{p}");
    }
}
