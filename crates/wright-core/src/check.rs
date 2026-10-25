//! Acceptance checks that cannot pass by omission.
//!
//! Every defect found in the Python baseline's aircraft kernel on 2026-10-24 was a check
//! that passed without evidence: a clause skipped when its input was `None`
//! (no servo -> no `servo_fit` clause -> pass), a clause fed the same
//! inequality the sizing used (`wire_ampacity`), an infeasible operating
//! point reporting zero current as a full margin, a pair the loop never
//! visited (prop disc against prop disc). The rules here make each of those
//! impossible to write:
//!
//! 1. A domain declares its checks as a closed `enum` implementing
//!    [`CheckId`]; a [`Report`] built with [`Report::new`] knows every id via
//!    [`CheckId::ALL`] and [`Report::finish`] turns any id nobody evaluated
//!    into a failure. Skipping a check is a failed check.
//! 2. A check's outcome is [`Evidence`]: a measured margin, or `Missing`
//!    with the reason. Missing evidence fails, always; there is no
//!    "not applicable" that passes.
//! 3. A margin must be finite. NaN and infinity fail (a 0/0 is not a pass).
//! 4. Each check names its [`Source`]: what independent model measured it,
//!    so a reviewer can see when a check only restates the sizing rule.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The closed set of checks a domain must run. Implement on a fieldless
/// enum; `ALL` lists every variant (a test in each domain asserts it does).
pub trait CheckId: Copy + Ord + fmt::Debug + 'static {
    const ALL: &'static [Self];
    /// Stable snake_case name: the vocabulary agents repair against.
    fn name(self) -> &'static str;
    /// One line: what it checks and against which limit.
    fn describe(self) -> &'static str;
}

/// Where a check's number came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// A physics model independent of the sizing rule that chose the part.
    Physics,
    /// Measured on the built geometry (solids or meshes).
    Geometry,
    /// A part's published rating against a load.
    Rating,
    /// A requirement the user stated.
    Requirement,
    /// A slicer's own output (print time, filament, bed fit).
    Slicer,
    /// An external simulator.
    Simulation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Evidence {
    /// Positive: inside the limit by this much. Negative: violated.
    Margin { value: f64, unit: String },
    /// The check could not be measured; this fails.
    Missing { why: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub name: String,
    pub evidence: Evidence,
    pub detail: String,
    pub source: Source,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        matches!(self.evidence, Evidence::Margin { value, .. } if value.is_finite() && value >= 0.0)
    }
    pub fn margin(&self) -> Option<f64> {
        match self.evidence {
            Evidence::Margin { value, .. } if value.is_finite() => Some(value),
            _ => None,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.passed() { "ok  " } else { "FAIL" };
        match &self.evidence {
            Evidence::Margin { value, unit } => {
                write!(f, "{mark} {:<26} {:>+10.3} {:<6} {}", self.name, value, unit, self.detail)
            }
            Evidence::Missing { why } => {
                write!(f, "{mark} {:<26} {:>10} {:<6} missing: {why}", self.name, "-", "")
            }
        }
    }
}

/// Outcomes keyed by check; complete only after [`Report::finish`].
#[derive(Debug, Clone)]
pub struct Report<C: CheckId> {
    got: BTreeMap<C, Outcome>,
}

impl<C: CheckId> Default for Report<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: CheckId> Report<C> {
    pub fn new() -> Self {
        Self { got: BTreeMap::new() }
    }

    /// Record a measured margin. A non-finite value is recorded as missing.
    pub fn margin(&mut self, id: C, value: f64, unit: &str, source: Source, detail: impl Into<String>) {
        let evidence = if value.is_finite() {
            Evidence::Margin { value, unit: unit.to_string() }
        } else {
            Evidence::Missing { why: format!("the margin came out {value}") }
        };
        self.put(id, evidence, source, detail.into());
    }

    /// Record that the check could not be measured (it fails).
    pub fn missing(&mut self, id: C, source: Source, why: impl Into<String>) {
        self.put(id, Evidence::Missing { why: why.into() }, source, String::new());
    }

    /// A check written twice keeps the worse outcome: a later pass can
    /// never hide an earlier failure.
    fn put(&mut self, id: C, evidence: Evidence, source: Source, detail: String) {
        let new = Outcome { name: id.name().to_string(), evidence, detail, source };
        match self.got.get(&id) {
            Some(old) if !old.passed() && new.passed() => {}
            Some(old) if old.margin().zip(new.margin()).is_some_and(|(a, b)| a < b) => {}
            _ => {
                self.got.insert(id, new);
            }
        }
    }

    /// Every declared check, in declaration order; any never evaluated is a
    /// failure that says so.
    pub fn finish(mut self) -> Finished {
        for &id in C::ALL {
            self.got.entry(id).or_insert_with(|| Outcome {
                name: id.name().to_string(),
                evidence: Evidence::Missing { why: "never evaluated".into() },
                detail: String::new(),
                source: Source::Physics,
            });
        }
        // declared order, each id once (a duplicate in ALL must not panic)
        let mut outcomes: Vec<Outcome> = C::ALL.iter().filter_map(|id| self.got.remove(id)).collect();
        // an id recorded but missing from ALL is a domain bug; keep its
        // outcome rather than drop it (a dropped failure would read as a
        // pass -- found by the Lean model, docs/verification.md)
        outcomes.extend(self.got.into_values());
        Finished { outcomes }
    }
}

/// A complete report: one outcome per declared check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finished {
    pub outcomes: Vec<Outcome>,
}

impl Finished {
    /// Every outcome passed, and there was at least one: a report with no
    /// outcomes (a domain that declared no checks, a hand-built empty
    /// report) has shown nothing and does not pass (review 4, finding 12).
    pub fn passed(&self) -> bool {
        !self.outcomes.is_empty() && self.outcomes.iter().all(Outcome::passed)
    }
    pub fn failures(&self) -> impl Iterator<Item = &Outcome> {
        self.outcomes.iter().filter(|o| !o.passed())
    }
    /// A graded violation for search: 0 when everything passes, larger the
    /// further from passing (missing evidence counts as a large violation).
    pub fn violation(&self, scale: impl Fn(&str) -> f64) -> f64 {
        self.failures()
            .map(|o| match &o.evidence {
                Evidence::Margin { value, unit } if value.is_finite() => -value / scale(unit),
                _ => 10.0,
            })
            .sum()
    }
    pub fn get(&self, name: &str) -> Option<&Outcome> {
        self.outcomes.iter().find(|o| o.name == name)
    }
}

impl fmt::Display for Finished {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for o in &self.outcomes {
            writeln!(f, "{o}")?;
        }
        let failed = self.failures().count();
        write!(f, "{} checks, {} failing", self.outcomes.len(), failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum T {
        A,
        B,
    }
    impl CheckId for T {
        const ALL: &'static [Self] = &[T::A, T::B];
        fn name(self) -> &'static str {
            match self {
                T::A => "a",
                T::B => "b",
            }
        }
        fn describe(self) -> &'static str {
            "test"
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Nothing {}
    impl CheckId for Nothing {
        const ALL: &'static [Self] = &[];
        fn name(self) -> &'static str {
            match self {}
        }
        fn describe(self) -> &'static str {
            match self {}
        }
    }

    /// Review 4, finding 12: a report with no outcomes passed.
    #[test]
    fn an_empty_report_does_not_pass() {
        assert!(!Report::<Nothing>::new().finish().passed());
        assert!(!Finished { outcomes: vec![] }.passed());
    }

    #[test]
    fn a_check_nobody_ran_fails() {
        let mut r = Report::<T>::new();
        r.margin(T::A, 1.0, "mm", Source::Physics, "fine");
        let f = r.finish();
        assert!(!f.passed());
        assert_eq!(f.failures().next().unwrap().name, "b");
    }

    #[test]
    fn missing_evidence_and_nan_fail() {
        let mut r = Report::<T>::new();
        r.missing(T::A, Source::Rating, "no rating published");
        r.margin(T::B, f64::NAN, "A", Source::Physics, "0/0");
        let f = r.finish();
        assert_eq!(f.failures().count(), 2);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Sloppy {
        Listed,
        Forgotten,
    }
    impl CheckId for Sloppy {
        // `Forgotten` left out of ALL, and `Listed` listed twice
        const ALL: &'static [Self] = &[Sloppy::Listed, Sloppy::Listed];
        fn name(self) -> &'static str {
            match self {
                Sloppy::Listed => "listed",
                Sloppy::Forgotten => "forgotten",
            }
        }
        fn describe(self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn a_check_missing_from_all_still_counts_and_duplicates_do_not_panic() {
        let mut r = Report::<Sloppy>::new();
        r.margin(Sloppy::Listed, 1.0, "", Source::Physics, "");
        r.margin(Sloppy::Forgotten, -1.0, "", Source::Physics, "fails");
        let f = r.finish();
        assert!(!f.passed());
        assert_eq!(f.outcomes.len(), 2);
    }

    #[test]
    fn a_later_pass_cannot_hide_a_failure() {
        let mut r = Report::<T>::new();
        r.margin(T::A, -2.0, "mm", Source::Geometry, "pair 1 clashes");
        r.margin(T::A, 5.0, "mm", Source::Geometry, "pair 2 clear");
        r.margin(T::B, 1.0, "", Source::Physics, "");
        let f = r.finish();
        assert!(!f.passed());
        assert_eq!(f.get("a").unwrap().margin(), Some(-2.0));
    }
}
