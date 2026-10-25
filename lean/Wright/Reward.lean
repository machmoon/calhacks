/-
The reward post-training reads off a finished report, and what a policy can
and cannot do to it.

`crates/wright-train` labels every recorded trajectory with
`checks_passed` (the number of declared checks that passed) and `passed`
(`Finished::passed`), and the rejection-sampling filter keeps only
`passed` trajectories. `reward` here is `checks_passed` on the model of
`crates/wright-core/src/check.rs` in `Wright.Check`. The theorems say the
reward cannot be gamed through the report:

* `reward_full_iff_all_measured`: the full reward needs every declared
  check measured with a non-negative margin (no credit for omission);
* `reward_lt_total_of_slot_fails`: one failing or unevaluated check costs
  at least one point, however the rest went;
* `reward_le_recorded`: at most one point per declared check that was
  actually recorded;
* `reward_record_le`: measuring a check again never raises the reward
  (the worse outcome is kept), so retrying a check buys nothing;
* `reward_perm`: the order in which checks were recorded does not matter.

What this does not cover: the numbers themselves. A margin that is wrong
before it reaches the report is the Haskell oracle's job (haskell/,
docs/verification.md).
-/
import Wright.Check

namespace Wright.Check

/-- `checks_passed`: how many outcomes of a finished report passed. -/
def reward {C : Type} (f : List (C × Evidence)) : Nat :=
  f.countP fun x => x.2.passed

namespace Report

variable {C : Type} [CheckId C]

/-- The reward of a finished report is the number of declared checks whose
slot passes (unevaluated counts as missing). -/
theorem reward_finish (r : Report C) :
    reward r.finish = (CheckId.all (C := C)).countP fun c => slotPassed (r.got c) := by
  simp only [reward, finish, List.countP_map]
  rfl

theorem finish_length (r : Report C) : r.finish.length = (CheckId.all (C := C)).length := by
  simp [finish]

/-- The reward never exceeds the number of declared checks. -/
theorem reward_le_total (r : Report C) : reward r.finish ≤ r.finish.length :=
  List.countP_le_length

/-- The full reward is exactly a passing report. -/
theorem reward_eq_total_iff (r : Report C) :
    reward r.finish = r.finish.length ↔ Finished.passed r.finish = true := by
  simp [reward, Finished.passed, List.countP_eq_length, List.all_eq_true]

/-- The full reward needs every declared check measured, with a
non-negative margin: there is no credit for a check nobody ran. -/
theorem reward_full_iff_all_measured (r : Report C) :
    reward r.finish = r.finish.length ↔
      ∀ c, ∃ m : Int, r.got c = some (.margin m) ∧ 0 ≤ m := by
  rw [reward_eq_total_iff, finished_passes_iff_all_measured]

/-- One failing slot (a negative margin, `Missing`, or never evaluated)
costs at least one point, whatever the other checks did. -/
theorem reward_lt_total_of_slot_fails (r : Report C) (c : C)
    (h : slotPassed (r.got c) = false) : reward r.finish < r.finish.length := by
  refine Nat.lt_of_le_of_ne (reward_le_total r) fun heq => ?_
  have hp := (reward_eq_total_iff r).1 heq
  rw [finish_passed_iff] at hp
  rw [hp c] at h
  exact absurd h (by decide)

/-- A check nobody evaluated costs at least one point. -/
theorem reward_lt_total_of_unevaluated (r : Report C) (c : C) (h : r.got c = none) :
    reward r.finish < r.finish.length :=
  reward_lt_total_of_slot_fails r c (by simp [slotPassed, h, Evidence.passed])

/-- At most one point per declared check that was recorded at least once:
a report built from recordings `l` scores no more than the number of
declared checks with some recording in `l`. -/
theorem reward_le_recorded (l : List (C × Evidence)) :
    reward (build l).finish ≤ (CheckId.all (C := C)).countP fun c => !(evs l c).isEmpty := by
  rw [reward_finish]
  refine List.countP_mono_left fun c _ hc => ?_
  rw [build_slot, Bool.and_eq_true] at hc
  exact hc.1

/-- Measuring an already-measured check again never raises the reward: the
slot keeps the worse outcome, every other slot is untouched. -/
theorem reward_record_le (r : Report C) (c : C) (o e : Evidence) (h : r.got c = some o) :
    reward (r.record c e).finish ≤ reward r.finish := by
  rw [reward_finish, reward_finish]
  refine List.countP_mono_left fun d _ hd => ?_
  by_cases hdc : d = c
  · subst hdc
    rw [record_got_self, slotPassed_put, h] at hd
    simp only [Option.map_some, Option.getD_some, Bool.and_eq_true] at hd
    rw [h]
    simpa [slotPassed] using hd.1
  · rwa [record_got_other _ _ hdc] at hd

/-- Any number of further recordings of already-measured checks never
raise the reward. -/
theorem reward_foldl_le (r : Report C) (later : List (C × Evidence))
    (hall : ∀ x ∈ later, ∃ o, r.got x.1 = some o) :
    reward (later.foldl (fun r x => r.record x.1 x.2) r).finish ≤ reward r.finish := by
  induction later generalizing r with
  | nil => exact Nat.le_refl _
  | cons x xs ih =>
    rw [List.foldl_cons]
    obtain ⟨o, ho⟩ := hall x (List.mem_cons_self ..)
    refine Nat.le_trans (ih (r.record x.1 x.2) fun y hy => ?_) (reward_record_le r x.1 o x.2 ho)
    obtain ⟨o', ho'⟩ := hall y (List.mem_cons_of_mem _ hy)
    by_cases hyx : y.1 = x.1
    · exact ⟨_, by rw [hyx, record_got_self]⟩
    · exact ⟨o', by rw [record_got_other _ _ hyx, ho']⟩

/-- The order of recordings does not change the reward. -/
theorem reward_perm {l l' : List (C × Evidence)} (hp : l.Perm l') :
    reward (build l).finish = reward (build l').finish := by
  have hslot := (order_irrelevant (C := C) hp).1
  rw [reward_finish, reward_finish]
  exact Nat.le_antisymm
    (List.countP_mono_left fun c _ hc => by rwa [← hslot c])
    (List.countP_mono_left fun c _ hc => by rwa [hslot c])

end Report

end Wright.Check
