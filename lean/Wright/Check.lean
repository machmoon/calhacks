/-
A model of `wright_core::check` (crates/wright-core/src/check.rs) and proofs
that a report cannot pass by omission.

Correspondence (Rust -> Lean):

* `trait CheckId { const ALL }`        -> `class CheckId` with `all` and a
                                          proof `complete` that `all` lists
                                          every value (Rust: a per-domain test).
* `f64` margin handed to `Report::margin` -> `Value` = `finite (m : Int)` |
                                          `nonfinite` (NaN, +inf, -inf).
* `enum Evidence { Margin, Missing }`  -> `Evidence.margin (m : Int)` | `.missing`.
                                          A margin in a `Report` is always
                                          finite: `Report::margin` turns a
                                          non-finite value into `Missing`.
* `Outcome::passed`                    -> `Evidence.passed`.
* `Outcome::margin`                    -> `Evidence.margin?`.
* `Report::put` (private)              -> `keep` / `put`.
* `Report { got: BTreeMap<C, Outcome> }` -> `Report.got : C -> Option Evidence`.
* `Report::margin` / `Report::missing` -> `Report.margin` / `Report.missing`.
* `Report::finish` / `Finished::passed` -> `Report.finish` / `Finished.passed`.

Units, detail strings, names and `Source` do not affect pass/fail and are
not modelled. Margins are exact integers (read: a margin in some fixed
fine unit); see docs/verification.md for what that leaves to the Rust tests.
-/

namespace Wright.Check

-- `CheckId.deq` is a field instance (the `DecidableEq` a Rust `Ord` gives).
set_option warn.classDefReducibility false

/-- The closed set of checks a domain declares. -/
class CheckId (C : Type) where
  [deq : DecidableEq C]
  all : List C
  complete : ∀ c : C, c ∈ all

attribute [instance] CheckId.deq

/-- A margin as Rust's `Report::margin` receives it (an `f64`). -/
inductive Value where
  | finite (m : Int)
  | nonfinite
  deriving DecidableEq, Repr

inductive Evidence where
  /-- Positive: inside the limit by this much. Negative: violated. -/
  | margin (m : Int)
  /-- The check could not be measured. -/
  | missing
  deriving DecidableEq, Repr

namespace Evidence

/-- `Outcome::passed`: a finite margin `>= 0`. -/
def passed : Evidence → Bool
  | margin m => decide (0 ≤ m)
  | missing => false

/-- `Outcome::margin`. -/
def margin? : Evidence → Option Int
  | margin m => some m
  | missing => none

/-- Passing means: a measured margin that is not negative. -/
theorem passed_iff (e : Evidence) : e.passed = true ↔ ∃ m, e = margin m ∧ 0 ≤ m := by
  cases e <;> simp [passed]

end Evidence

/-- `Report::put` when an outcome `old` is already recorded for the id:
the outcome it keeps. Line for line with the Rust `match`:
`Some(old) if !old.passed() && new.passed() => {}` keeps `old`;
`Some(old) if old.margin().zip(new.margin()).is_some_and(|(a, b)| a < b) => {}`
keeps `old`; otherwise `new` is inserted. -/
def keep (old new : Evidence) : Evidence :=
  if !old.passed && new.passed then old
  else
    match old.margin?, new.margin? with
    | some a, some b => if a < b then old else new
    | _, _ => new

/-- `Report::put` on the slot's previous content. -/
def put : Option Evidence → Evidence → Evidence
  | none, new => new
  | some old, new => keep old new

structure Report (C : Type) where
  got : C → Option Evidence

namespace Report

variable {C : Type} [CheckId C]

/-- `Report::new`. -/
def empty : Report C := ⟨fun _ => none⟩

/-- Record evidence for `c` (the body of `Report::put`). -/
def record (r : Report C) (c : C) (e : Evidence) : Report C :=
  ⟨fun d => if d = c then some (put (r.got c) e) else r.got d⟩

/-- `Report::margin`: a non-finite value is recorded as missing. -/
def margin (r : Report C) (c : C) : Value → Report C
  | .finite m => r.record c (.margin m)
  | .nonfinite => r.record c .missing

/-- `Report::missing`. -/
def missing (r : Report C) (c : C) : Report C := r.record c .missing

/-- `Report::finish`: every declared check in declaration order; any never
evaluated becomes `Missing`. -/
def finish (r : Report C) : List (C × Evidence) :=
  CheckId.all.map fun c => (c, (r.got c).getD .missing)

/-- Fold a sequence of recordings into a report. -/
def build (l : List (C × Evidence)) : Report C :=
  l.foldl (fun r x => r.record x.1 x.2) empty

end Report

/-- `Finished::passed`: every outcome passed. -/
def Finished.passed {C : Type} (f : List (C × Evidence)) : Bool :=
  f.all fun x => x.2.passed

/-- Pass status of one slot of a report: unevaluated counts as missing. -/
def slotPassed (o : Option Evidence) : Bool := (o.getD .missing).passed

/-! ## The algebra of `put` -/

/-- The kept outcome passes exactly when both outcomes pass. -/
theorem keep_passed (o n : Evidence) : (keep o n).passed = (o.passed && n.passed) := by
  cases o with
  | missing =>
    cases n with
    | missing => simp [keep, Evidence.passed, Evidence.margin?]
    | margin b => by_cases hb : 0 ≤ b <;> simp [keep, Evidence.passed, Evidence.margin?, hb]
  | margin a =>
    cases n with
    | missing => simp [keep, Evidence.passed, Evidence.margin?]
    | margin b =>
      simp only [keep, Evidence.passed, Evidence.margin?]
      by_cases ha : 0 ≤ a <;> by_cases hb : 0 ≤ b <;> by_cases hab : a < b <;>
        simp [ha, hb, hab] <;> omega

/-- When both outcomes are measured, `put` keeps the smaller margin: the
worse outcome wins. -/
theorem keep_margin_min (a b : Int) : keep (.margin a) (.margin b) = .margin (min a b) := by
  simp only [keep, Evidence.passed, Evidence.margin?]
  by_cases ha : 0 ≤ a <;> by_cases hb : 0 ≤ b <;> by_cases hab : a < b <;>
    simp [ha, hb, hab, Int.min_def] <;> first | omega | (congr 1; omega)

theorem slotPassed_put (o : Option Evidence) (e : Evidence) :
    slotPassed (some (put o e)) = ((o.map Evidence.passed).getD true && e.passed) := by
  cases o <;> simp [slotPassed, put, keep_passed]

/-! ## Recording -/

namespace Report

variable {C : Type} [CheckId C]

/-- The evidence recorded for `c`, in recording order. -/
def evs (l : List (C × Evidence)) (c : C) : List Evidence :=
  l.filterMap fun x => if x.1 = c then some x.2 else none

theorem record_got_self (r : Report C) (c : C) (e : Evidence) :
    (r.record c e).got c = some (put (r.got c) e) := by
  simp [record]

theorem record_got_other (r : Report C) {c d : C} (e : Evidence) (h : d ≠ c) :
    (r.record c e).got d = r.got d := by
  simp [record, h]

/-- A slot after a sequence of recordings is the fold of `put` over the
evidence recorded for that id; other ids' recordings do not touch it. -/
theorem foldl_got (l : List (C × Evidence)) (r : Report C) (c : C) :
    (l.foldl (fun r x => r.record x.1 x.2) r).got c
      = (evs l c).foldl (fun o e => some (put o e)) (r.got c) := by
  induction l generalizing r with
  | nil => simp [evs]
  | cons x xs ih =>
    rw [List.foldl_cons, ih]
    by_cases h : x.1 = c
    · subst h; simp [evs, record_got_self]
    · rw [record_got_other _ _ (Ne.symm h)]; simp [evs, h]

/-- Pass status after folding evidence `es` into slot `o`. -/
theorem slotPassed_fold (es : List Evidence) (o : Option Evidence) :
    slotPassed (es.foldl (fun o e => some (put o e)) o)
      = if es = [] then slotPassed o
        else ((o.map Evidence.passed).getD true && es.all Evidence.passed) := by
  induction es generalizing o with
  | nil => simp
  | cons e es ih =>
    rw [List.foldl_cons, ih]
    cases es with
    | nil => simp [slotPassed_put]
    | cons e' es' =>
      cases o <;> simp [put, keep_passed, Bool.and_assoc]

/-- A slot of a report built from recordings `l` passes exactly when `c`
was recorded at least once and every recording for `c` passed. -/
theorem build_slot (l : List (C × Evidence)) (c : C) :
    slotPassed ((build l).got c) = (!(evs l c).isEmpty && (evs l c).all Evidence.passed) := by
  unfold build
  rw [foldl_got, slotPassed_fold]
  cases h : evs l c <;> simp [empty, slotPassed, Evidence.passed]

theorem mem_evs {l : List (C × Evidence)} {c : C} {e : Evidence} :
    e ∈ evs l c ↔ (c, e) ∈ l := by
  simp only [evs, List.mem_filterMap]
  constructor
  · rintro ⟨⟨d, e'⟩, hm, h⟩
    by_cases hd : d = c
    · subst hd; simp at h; subst h; exact hm
    · simp [hd] at h
  · intro hm; exact ⟨(c, e), hm, by simp⟩

/-! ## Finish -/

theorem finish_passed_iff (r : Report C) :
    Finished.passed r.finish = true ↔ ∀ c, slotPassed (r.got c) = true := by
  simp only [Finished.passed, finish, List.all_eq_true, List.mem_map]
  constructor
  · intro h c
    exact h _ ⟨c, CheckId.complete c, rfl⟩
  · rintro h _ ⟨c, _, rfl⟩
    exact h c

end Report

open Report

/-! ## (a) A finished report passes only on measured, non-negative margins -/

/-- **(a)** A finished report passes iff every declared check holds a
measured margin `m` with `0 ≤ m`. -/
theorem finished_passes_iff_all_measured {C : Type} [CheckId C] (r : Report C) :
    Finished.passed r.finish = true ↔
      ∀ c, ∃ m : Int, r.got c = some (.margin m) ∧ 0 ≤ m := by
  rw [finish_passed_iff]
  refine forall_congr' fun c => ?_
  cases h : r.got c with
  | none => simp [slotPassed, Evidence.passed]
  | some e => cases e <;> simp [slotPassed, Evidence.passed]

/-- **(a)**, stated on the recordings: a report built from a sequence of
recordings passes iff every declared check was recorded at least once and
every recording passed. -/
theorem build_passes_iff {C : Type} [CheckId C] (l : List (C × Evidence)) :
    Finished.passed (build l).finish = true ↔
      ∀ c, (∃ e, (c, e) ∈ l) ∧ ∀ e, (c, e) ∈ l → ∃ m, e = .margin m ∧ 0 ≤ m := by
  rw [finish_passed_iff]
  refine forall_congr' fun c => ?_
  rw [build_slot]
  simp only [Bool.and_eq_true, Bool.not_eq_true', List.isEmpty_eq_false_iff_exists_mem,
    List.all_eq_true, mem_evs, ← Evidence.passed_iff]

/-! ## (b) A later pass never hides a failure -/

/-- **(b)** One recording: once `c` holds a failing outcome, recording any
evidence (in particular a passing one) leaves it failing. -/
theorem record_keeps_failure {C : Type} [CheckId C] (r : Report C) (c : C)
    (o e : Evidence) (h : r.got c = some o) (hf : o.passed = false) :
    slotPassed ((r.record c e).got c) = false := by
  rw [record_got_self, slotPassed_put, h]
  simp [hf]

/-- **(b)** Any number of later recordings, for any ids: a failing check
stays failing, and so does the finished report. -/
theorem failure_is_permanent {C : Type} [CheckId C] (r : Report C) (c : C)
    (o : Evidence) (h : r.got c = some o) (hf : o.passed = false)
    (later : List (C × Evidence)) :
    slotPassed ((later.foldl (fun r x => r.record x.1 x.2) r).got c) = false ∧
      Finished.passed (later.foldl (fun r x => r.record x.1 x.2) r).finish = false := by
  have hslot : slotPassed ((later.foldl (fun r x => r.record x.1 x.2) r).got c) = false := by
    rw [foldl_got, slotPassed_fold, h]
    split <;> simp [slotPassed, hf]
  refine ⟨hslot, ?_⟩
  cases hp : Finished.passed (later.foldl (fun r x => r.record x.1 x.2) r).finish
  · rfl
  · rw [finish_passed_iff] at hp
    rw [hp c] at hslot
    exact absurd hslot (by decide)

/-! ## (c) Missing evidence always fails -/

/-- **(c)** `Missing` is never a pass. -/
theorem missing_fails : Evidence.missing.passed = false := rfl

/-- **(c)** A check recorded as missing fails, whatever was recorded before. -/
theorem record_missing_fails {C : Type} [CheckId C] (r : Report C) (c : C) :
    slotPassed ((r.missing c).got c) = false := by
  simp [Report.missing, record_got_self, slotPassed_put, Evidence.passed]

/-- **(c)** A non-finite margin (NaN, infinity) fails. -/
theorem record_nonfinite_fails {C : Type} [CheckId C] (r : Report C) (c : C) :
    slotPassed ((r.margin c .nonfinite).got c) = false :=
  record_missing_fails r c

/-- **(c)** A check nobody evaluated fails the finished report. -/
theorem unevaluated_fails {C : Type} [CheckId C] (r : Report C) (c : C)
    (h : r.got c = none) : Finished.passed r.finish = false := by
  cases hp : Finished.passed r.finish
  · rfl
  · have := (finish_passed_iff r).1 hp c
    simp [h, slotPassed, Evidence.passed] at this

/-- **(c)** Any slot holding `Missing` fails the finished report. -/
theorem missing_slot_fails {C : Type} [CheckId C] (r : Report C) (c : C)
    (h : r.got c = some .missing) : Finished.passed r.finish = false := by
  cases hp : Finished.passed r.finish
  · rfl
  · have := (finish_passed_iff r).1 hp c
    simp [h, slotPassed, Evidence.passed] at this

/-! ## (d) Recording order does not change pass/fail -/

/-- **(d)** Two sequences of recordings that are permutations of each other
give the same pass/fail, per check and for the whole report. -/
theorem order_irrelevant {C : Type} [CheckId C] {l l' : List (C × Evidence)}
    (hp : l.Perm l') :
    (∀ c, slotPassed ((build l).got c) = slotPassed ((build l').got c)) ∧
      Finished.passed (build l).finish = Finished.passed (build l').finish := by
  have hslot : ∀ c, slotPassed ((build l).got c) = slotPassed ((build l').got c) := by
    intro c
    have hm : ∀ e, e ∈ evs l c ↔ e ∈ evs l' c := fun e => by
      rw [mem_evs, mem_evs]; exact hp.mem_iff
    rw [build_slot, build_slot]
    have h1 : (evs l c).isEmpty = (evs l' c).isEmpty := by
      cases h : (evs l c).isEmpty <;> cases h' : (evs l' c).isEmpty <;> try rfl
      · rw [List.isEmpty_eq_false_iff_exists_mem] at h
        rw [List.isEmpty_iff] at h'
        obtain ⟨e, he⟩ := h
        have := (hm e).1 he
        simp [h'] at this
      · rw [List.isEmpty_eq_false_iff_exists_mem] at h'
        rw [List.isEmpty_iff] at h
        obtain ⟨e, he⟩ := h'
        have := (hm e).2 he
        simp [h] at this
    have h2 : (evs l c).all Evidence.passed = (evs l' c).all Evidence.passed := by
      apply Bool.eq_iff_iff.2
      simp only [List.all_eq_true]
      exact ⟨fun h e he => h e ((hm e).2 he), fun h e he => h e ((hm e).1 he)⟩
    rw [h1, h2]
  refine ⟨hslot, ?_⟩
  apply Bool.eq_iff_iff.2
  rw [finish_passed_iff, finish_passed_iff]
  exact ⟨fun h c => (hslot c) ▸ h c, fun h c => (hslot c).symm ▸ h c⟩

end Wright.Check
