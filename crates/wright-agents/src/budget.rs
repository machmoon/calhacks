//! Spend limits: one per run, one across every run on this machine.
//!
//! The shape is SWE-agent's two cost limits, `per_instance_cost_limit` and
//! `total_cost_limit` (SWE-agent @3ea751c, `sweagent/agent/models.py:73-78`),
//! and the Claude Agent SDK's `max_budget_usd` (claude-agent-sdk-python
//! @0e89b29, `types.py:1368`). Two differences, both because the weekend has
//! $50 of credits in total:
//!
//! * the check is made **before** a call, on its worst case
//!   ([`crate::price::worst_case_usd`]), so a call that could cross a limit is
//!   never sent; SWE-agent checks after the fact and can overshoot by one
//!   call;
//! * the global total lives in a small JSON ledger re-read before and
//!   written after every call, so restarting the process (or running two)
//!   keeps the count. Writes go to a temp file and are renamed into place,
//!   and every read-modify-write holds an exclusive `flock` on a sidecar
//!   `<ledger>.lock` (std `File::lock`), the way Cargo serializes access to
//!   its package cache (rust-lang/cargo `src/cargo/util/flock.rs`, a lock
//!   file beside the data rather than on it, since the rename replaces the
//!   data file's inode).
//!
//! Concurrency inside one process: [`Budget::reserve`] books the worst case
//! as *reserved* before the call goes out, so parallel calls on one
//! [`crate::Client`] each see the others' worst cases; [`Budget::settle`]
//! swaps the reservation for the real cost and [`Budget::release`] drops it
//! when the call failed. The same reserve-then-settle shape as a card
//! authorization hold.

use crate::price::{worst_case_usd, Usage};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Pat (10/24): no spend cap for the demo, Fable 5.1 + Opus 5.5 throughout.
/// The defaults are high enough never to stop a run; every call is still
/// priced and written to the ledger, and the env variables still set a cap.
pub const DEFAULT_RUN_USD: f64 = 1000.0;
pub const DEFAULT_GLOBAL_USD: f64 = 10000.0;

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum BudgetError {
    #[error("run budget: spent ${spent:.4} of ${limit:.2}; the next call could cost up to ${next:.4}")]
    Run { spent: f64, next: f64, limit: f64 },
    #[error("global budget: ledger shows ${spent:.4} of ${limit:.2}; the next call could cost up to ${next:.4}")]
    Global { spent: f64, next: f64, limit: f64 },
    /// A limit that cannot be compared (NaN, infinite, negative) or an
    /// unparseable `WRIGHT_*_BUDGET_USD`: every call is refused.
    #[error("budget misconfigured: {0}")]
    Config(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub spent_usd: f64,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Ledger {
    /// A missing file is an empty ledger; a corrupt one is an error (a
    /// ledger that cannot be read must not silently restart at $0).
    pub fn load(path: &Path) -> anyhow::Result<Ledger> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
    /// Load, apply `f`, save, all under an exclusive lock on
    /// `<path>.lock`, so concurrent writers (threads or processes) never
    /// lose each other's updates. The lock is released when the file
    /// handle drops.
    pub fn update<T>(path: &Path, f: impl FnOnce(&mut Ledger) -> T) -> anyhow::Result<T> {
        let _lock = lock_exclusive(path)?;
        let mut l = Ledger::load(path)?;
        let out = f(&mut l);
        l.save(path)?;
        Ok(out)
    }
}

fn lock_exclusive(path: &Path) -> anyhow::Result<File> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    let f = OpenOptions::new().create(true).truncate(false).write(true).open(PathBuf::from(name))?;
    f.lock()?;
    Ok(f)
}

/// `$WRIGHT_LEDGER`, else `~/.wright/ledger.json`.
pub fn default_ledger_path() -> PathBuf {
    if let Ok(p) = std::env::var("WRIGHT_LEDGER") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".wright").join("ledger.json")
}

/// A limit must be a finite, non-negative number of dollars.
fn check_limit(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() && v >= 0.0 {
        Ok(v)
    } else {
        Err(format!("{name} must be a finite, non-negative number of USD, got {v}"))
    }
}

/// `$name` as a dollar limit: the default only when the variable is absent;
/// anything set but unusable (`nan`, `inf`, `-1`, `ten`) is an error.
pub fn env_usd(name: &str, default: f64) -> Result<f64, String> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid UTF-8")),
        Ok(v) => match v.trim().parse::<f64>() {
            Ok(x) => check_limit(name, x).map_err(|e| format!("{e} ({name}={v:?})")),
            Err(_) => Err(format!("{name}={v:?} is not a number of USD")),
        },
    }
}

#[derive(Debug, Clone)]
pub struct Budget {
    pub run_limit: f64,
    pub global_limit: f64,
    pub run_spent: f64,
    pub run_input_tokens: u64,
    pub run_output_tokens: u64,
    /// Worst cases of calls authorized but not yet settled, counted against
    /// the run limit (all of them) and the global limit (`reserved_global`).
    pub reserved: f64,
    pub reserved_global: f64,
    /// None: no global ledger (tests, or a transport that does not bill
    /// API credits never touches it anyway).
    pub ledger: Option<PathBuf>,
    /// Set by [`Budget::from_env`] when a limit variable is unusable; every
    /// call is then refused with this message.
    pub config_error: Option<String>,
}

impl Budget {
    pub fn new(run_limit: f64, global_limit: f64, ledger: Option<PathBuf>) -> Self {
        Budget {
            run_limit,
            global_limit,
            run_spent: 0.0,
            run_input_tokens: 0,
            run_output_tokens: 0,
            reserved: 0.0,
            reserved_global: 0.0,
            ledger,
            config_error: None,
        }
    }

    /// Limits from `WRIGHT_RUN_BUDGET_USD` / `WRIGHT_GLOBAL_BUDGET_USD`
    /// (defaults $1000 / $10000 when unset: effectively no cap) and the default ledger path. A set
    /// but unusable value does not fall back to the default: the budget
    /// refuses every call with [`BudgetError::Config`] naming the variable
    /// ([`Budget::try_from_env`] returns the error instead).
    pub fn from_env() -> Self {
        match Self::try_from_env() {
            Ok(b) => b,
            Err(e) => {
                let mut b = Budget::new(0.0, 0.0, Some(default_ledger_path()));
                b.config_error = Some(e);
                b
            }
        }
    }

    pub fn try_from_env() -> Result<Self, String> {
        Ok(Budget::new(
            env_usd("WRIGHT_RUN_BUDGET_USD", DEFAULT_RUN_USD)?,
            env_usd("WRIGHT_GLOBAL_BUDGET_USD", DEFAULT_GLOBAL_USD)?,
            Some(default_ledger_path()),
        ))
    }

    fn check_config(&self) -> Result<(), BudgetError> {
        if let Some(e) = &self.config_error {
            return Err(BudgetError::Config(e.clone()));
        }
        check_limit("run limit", self.run_limit).map_err(BudgetError::Config)?;
        check_limit("global limit", self.global_limit).map_err(BudgetError::Config)?;
        Ok(())
    }

    /// Refuse `body` if its worst case could cross either limit; reserves
    /// nothing (see [`Budget::reserve`]). `global` is false for transports
    /// that do not spend API credits.
    pub fn authorize(&self, model: &str, body: &serde_json::Value, global: bool) -> Result<f64, BudgetError> {
        let next = worst_case_usd(model, body);
        self.check(next, global)?;
        Ok(next)
    }

    /// Whether a call costing up to `next` fits both limits, counting the
    /// worst cases already reserved by calls in flight.
    pub fn check(&self, next: f64, global: bool) -> Result<(), BudgetError> {
        self.check_config()?;
        if !next.is_finite() || next < 0.0 {
            return Err(BudgetError::Config(format!("worst case of the next call is {next}")));
        }
        let committed = self.run_spent + self.reserved;
        if committed + next > self.run_limit {
            return Err(BudgetError::Run { spent: committed, next, limit: self.run_limit });
        }
        if global {
            if let Some(path) = &self.ledger {
                // An unreadable ledger refuses: we cannot prove we are under.
                let spent = Ledger::load(path).map(|l| l.spent_usd).unwrap_or(f64::INFINITY) + self.reserved_global;
                if spent + next > self.global_limit {
                    return Err(BudgetError::Global { spent, next, limit: self.global_limit });
                }
            }
        }
        Ok(())
    }

    /// Check `next` against both limits and, if it fits, hold it as
    /// reserved until [`Budget::settle`] or [`Budget::release`]. Call with
    /// the budget's mutex held across check and reserve (as
    /// [`crate::Client`] does) so two calls cannot both fit the same room.
    pub fn reserve(&mut self, next: f64, global: bool) -> Result<f64, BudgetError> {
        self.check(next, global)?;
        self.reserved += next;
        if global {
            self.reserved_global += next;
        }
        Ok(next)
    }

    /// Drop a reservation whose call never completed.
    pub fn release(&mut self, reserved: f64, global: bool) {
        self.reserved = (self.reserved - reserved).max(0.0);
        if global {
            self.reserved_global = (self.reserved_global - reserved).max(0.0);
        }
    }

    /// Replace a reservation with what the call cost: `usage` priced at
    /// `model`, or, when the response carried no readable usage, the whole
    /// reservation (the worst case it was authorized at). Returns the
    /// amount booked. The reservation is released even if the ledger write
    /// fails, and the run total is updated first, so an error never leaves
    /// spend uncounted in this process.
    pub fn settle(&mut self, reserved: f64, model: &str, usage: Option<&Usage>, global: bool) -> anyhow::Result<f64> {
        self.release(reserved, global);
        match usage {
            Some(u) => self.record(model, u, global),
            None => self.book(reserved, 0, 0, global),
        }
    }

    /// Book a response's cost; returns it. Ledger write failures are
    /// returned so the caller can stop rather than spend uncounted.
    pub fn record(&mut self, model: &str, usage: &Usage, global: bool) -> anyhow::Result<f64> {
        self.book(usage.usd(model), usage.prompt(), usage.output, global)
    }

    fn book(&mut self, usd: f64, input: u64, output: u64, global: bool) -> anyhow::Result<f64> {
        self.run_spent += usd;
        self.run_input_tokens += input;
        self.run_output_tokens += output;
        if global {
            if let Some(path) = &self.ledger {
                Ledger::update(path, |l| {
                    l.spent_usd += usd;
                    l.calls += 1;
                    l.input_tokens += input;
                    l.output_tokens += output;
                })?;
            }
        }
        Ok(usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wright-budget-{}-{name}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn ledger_survives_a_restart_and_stops_the_next_run() {
        let path = tmp("restart");
        let body = json!({"max_tokens": 1000, "messages": []});
        let mut a = Budget::new(10.0, 0.05, Some(path.clone()));
        a.record("claude-sonnet-5-5", &Usage { output: 4000, ..Default::default() }, true).unwrap();
        // A fresh process: run spend is zero, the ledger still says $0.04.
        let b = Budget::new(10.0, 0.05, Some(path.clone()));
        assert_eq!(b.run_spent, 0.0);
        assert!(matches!(b.authorize("claude-sonnet-5-5", &body, true), Err(BudgetError::Global { .. })));
        // A transport that does not bill API credits is not held to it.
        assert!(b.authorize("claude-sonnet-5-5", &body, false).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corrupt_ledger_refuses() {
        let path = tmp("corrupt");
        std::fs::write(&path, "not json").unwrap();
        let b = Budget::new(10.0, 45.0, Some(path.clone()));
        assert!(b.authorize("claude-haiku-5-5", &json!({"max_tokens": 10}), true).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reservations_hold_room_until_settled_or_released() {
        let body = json!({"max_tokens": 8000, "messages": []});
        let mut b = Budget::new(0.10, 45.0, None);
        let next = b.authorize("claude-sonnet-5-5", &body, false).unwrap();
        assert!(next > 0.05 && next < 0.10);
        b.reserve(next, false).unwrap();
        // A second call while the first is in flight does not fit.
        assert!(matches!(b.reserve(next, false), Err(BudgetError::Run { .. })));
        // Settling at the real (small) cost frees the difference.
        let usd = b.settle(next, "claude-sonnet-5-5", Some(&Usage { output: 100, ..Default::default() }), false).unwrap();
        assert!((usd - 0.001).abs() < 1e-12 && b.reserved == 0.0);
        let r = b.reserve(next, false).unwrap();
        b.release(r, false);
        assert_eq!(b.reserved, 0.0);
        assert!((b.run_spent - 0.001).abs() < 1e-12);
        // No usage: the reservation is booked whole.
        let r = b.reserve(next, false).unwrap();
        assert_eq!(b.settle(r, "claude-sonnet-5-5", None, false).unwrap(), r);
        assert!((b.run_spent - 0.001 - r).abs() < 1e-12);
    }

    #[test]
    fn ledger_keeps_every_update_across_writers() {
        // The reviewer's probe: 4 writers x 100 records used to leave ~168.
        let path = tmp("concurrent");
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let p = path.clone();
                std::thread::spawn(move || {
                    let mut b = Budget::new(1e9, 1e9, Some(p));
                    for _ in 0..100 {
                        b.record("claude-sonnet-5-5", &Usage { output: 1000, ..Default::default() }, true).unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let l = Ledger::load(&path).unwrap();
        assert_eq!(l.calls, 400);
        assert!((l.spent_usd - 4.0).abs() < 1e-9);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.lock"));
    }

    #[test]
    fn non_finite_or_negative_limits_refuse() {
        let body = json!({"max_tokens": 1_000_000});
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let b = Budget::new(bad, 45.0, None);
            assert!(matches!(b.authorize("claude-sonnet-5-5", &body, false), Err(BudgetError::Config(_))), "{bad}");
            let b = Budget::new(10.0, bad, None);
            assert!(matches!(b.authorize("claude-sonnet-5-5", &body, true), Err(BudgetError::Config(_))), "{bad}");
        }
    }

    #[test]
    fn env_limits_reject_garbage_and_default_only_when_absent() {
        // Unique names: tests run in parallel and share the environment.
        let name = "WRIGHT_TEST_BUDGET_ENV_USD";
        std::env::remove_var(name);
        assert_eq!(env_usd(name, 1.25), Ok(1.25));
        for (v, ok) in [("nan", false), ("inf", false), ("-1", false), ("ten", false), ("", false), ("2.5", true), ("0", true)] {
            std::env::set_var(name, v);
            assert_eq!(env_usd(name, 1.25).is_ok(), ok, "{v:?}");
        }
        std::env::set_var(name, "nan");
        assert!(env_usd(name, 1.25).unwrap_err().contains(name));
        std::env::remove_var(name);
        // from_env with a bad value refuses every call with that message.
        let mut b = Budget::new(10.0, 45.0, None);
        b.config_error = Some("WRIGHT_RUN_BUDGET_USD=\"nan\" is bad".into());
        assert!(matches!(b.authorize("claude-haiku-5-5", &json!({"max_tokens": 1}), false), Err(BudgetError::Config(m)) if m.contains("WRIGHT_RUN_BUDGET_USD")));
    }
}
