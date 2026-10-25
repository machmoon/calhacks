/-
The harness's incumbent rule, and what it can never do.

`crates/wright-agents/src/state.rs` keeps the best verified design so far
(`Incumbent`) and offers every checked design to it (`DesignState::offer`):
the candidate replaces the incumbent only when `better(candidate, incumbent)`,

    better(a, b) = a.failing < b.failing
                 || (a.failing == b.failing && obj(a) > obj(b))

where `failing` is the number of failing checks (regression cases included)
and `obj` maps a missing or NaN objective to `-inf`. The shape is FunSearch's
programs database (google-deepmind/funsearch
`implementation/programs_database.py`, `_register_program_in_island`:
replace only on a strictly better score) ranked feasibility-first as
Optuna's `Study._get_best_trial` does (`optuna/study/study.py`).

Model: `Score.failing : Nat`; `Score.objective : Option Int`, where `none`
stands for the Rust's `None` *and* for NaN (both are mapped to `-inf` before
the comparison, so they are the same point of the order). The theorems:

* `better_failing_le`: whatever beats an incumbent has no more failing
  checks than it;
* `better_passing`, `offer_keeps_passing`: a candidate that replaces a
  passing incumbent (zero failing) is itself passing, so the incumbent
  rule never trades a passing design for a failing one;
* `offer_failing_le`, `run_failing_le`, `run_keeps_passing`: over one
  offer or any sequence of offers the incumbent's failing count never
  rises;
* `better_irrefl`, `offer_tie`, `better_asymm`: a tie keeps the earlier
  design (FunSearch's strict `>`), and two designs never both beat each
  other;
* `failing_zero_iff_passed`: in the check model (`Wright.Check`), zero
  failing outcomes is exactly `Finished.passed`, which is what `wright-train`
  labels `passed` (`lean/Wright/Reward.lean`).

Not modelled: `rescore_best` (the incumbent's score is recomputed under a
changed spec or new regression cases; the rule above is then applied to
the new scores, which is what these theorems are about), and the objective's
real-number value beyond its order.
-/
import Wright.Check

namespace Wright.Incumbent

/-- A checked design as the harness ranks it. -/
structure Score where
  failing : Nat
  objective : Option Int
  deriving DecidableEq, Repr

/-- `obj(a) > obj(b)` with a missing (or NaN) objective at `-inf`: it is
beaten by every number and beats nothing. -/
def objGt : Option Int → Option Int → Bool
  | some a, some b => decide (b < a)
  | some _, none => true
  | none, _ => false

/-- `state.rs` `better`: strictly fewer failing checks, or as many and a
strictly higher objective. -/
def better (a b : Score) : Bool :=
  a.failing < b.failing || (a.failing == b.failing && objGt a.objective b.objective)

/-- `DesignState::offer`: no incumbent yet takes the candidate; otherwise
the candidate replaces the incumbent only when strictly better. -/
def offer (best : Option Score) (cand : Score) : Score :=
  match best with
  | none => cand
  | some b => if better cand b then cand else b

/-- A run's worth of offers, in order. -/
def run (best : Option Score) (cands : List Score) : Option Score :=
  cands.foldl (fun b c => some (offer b c)) best

theorem objGt_irrefl (o : Option Int) : objGt o o = false := by
  cases o <;> simp [objGt]

theorem objGt_asymm {x y : Option Int} (h : objGt x y = true) : objGt y x = false := by
  cases x <;> cases y <;> simp_all [objGt] <;> omega

/-- Whatever beats an incumbent has no more failing checks than it. -/
theorem better_failing_le {a b : Score} (h : better a b = true) : a.failing ≤ b.failing := by
  simp only [better, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq, beq_iff_eq] at h
  rcases h with h | ⟨h, _⟩
  · exact Nat.le_of_lt h
  · exact Nat.le_of_eq h

/-- A candidate that beats a passing incumbent is passing. -/
theorem better_passing {a b : Score} (h : better a b = true) (hb : b.failing = 0) : a.failing = 0 :=
  Nat.eq_zero_of_le_zero (hb ▸ better_failing_le h)

/-- One offer never raises the incumbent's failing count. -/
theorem offer_failing_le (b cand : Score) : (offer (some b) cand).failing ≤ b.failing := by
  by_cases h : better cand b = true
  · simpa [offer, h] using better_failing_le h
  · simp [offer, h]

/-- The incumbent rule never replaces a passing design with a failing one. -/
theorem offer_keeps_passing (b cand : Score) (hb : b.failing = 0) :
    (offer (some b) cand).failing = 0 :=
  Nat.eq_zero_of_le_zero (hb ▸ offer_failing_le b cand)

/-- Over any sequence of offers the failing count never rises. -/
theorem run_failing_le (b : Score) (cands : List Score) :
    ∃ r, run (some b) cands = some r ∧ r.failing ≤ b.failing := by
  induction cands generalizing b with
  | nil => exact ⟨b, rfl, Nat.le_refl _⟩
  | cons c cs ih =>
    obtain ⟨r, hr, hle⟩ := ih (offer (some b) c)
    exact ⟨r, hr, Nat.le_trans hle (offer_failing_le b c)⟩

/-- A passing incumbent stays passing through any sequence of offers. -/
theorem run_keeps_passing (b : Score) (cands : List Score) (hb : b.failing = 0) :
    ∃ r, run (some b) cands = some r ∧ r.failing = 0 := by
  obtain ⟨r, hr, hle⟩ := run_failing_le b cands
  exact ⟨r, hr, Nat.eq_zero_of_le_zero (hb ▸ hle)⟩

/-- A design never beats itself: a tie keeps the earlier one. -/
theorem better_irrefl (a : Score) : better a a = false := by
  simp [better, objGt_irrefl]

/-- Offering the incumbent's own score changes nothing. -/
theorem offer_tie (b : Score) : offer (some b) b = b := by
  simp [offer, better_irrefl]

/-- Two designs never both beat each other. -/
theorem better_asymm {a b : Score} (h : better a b = true) : better b a = false := by
  simp only [better, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq, beq_iff_eq] at h
  simp only [better, Bool.or_eq_false_iff, Bool.and_eq_false_iff, decide_eq_false_iff_not,
    beq_eq_false_iff_ne, ne_eq]
  rcases h with h | ⟨h, h2⟩
  · exact ⟨Nat.not_lt.2 (Nat.le_of_lt h), Or.inl (Nat.ne_of_gt h)⟩
  · exact ⟨by omega, Or.inr (objGt_asymm h2)⟩

/-- Zero failing outcomes of a finished report is exactly `Finished.passed`:
the harness's `failing == 0` and `wright-train`'s `passed` agree. -/
theorem failing_zero_iff_passed {C : Type} (f : List (C × Wright.Check.Evidence)) :
    (f.countP fun x => !x.2.passed) = 0 ↔ Wright.Check.Finished.passed f = true := by
  induction f with
  | nil => exact ⟨fun _ => rfl, fun _ => rfl⟩
  | cons x xs ih =>
    rw [List.countP_cons, Wright.Check.Finished.passed, List.all_cons, Bool.and_eq_true,
      ← Wright.Check.Finished.passed, ← ih]
    generalize x.2.passed = b
    cases b <;> simp

end Wright.Incumbent
