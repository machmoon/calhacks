/-
The three unit tests of crates/wright-core/src/check.rs, replayed on the
model and checked by the kernel (`decide`).
-/
import Wright.Check
import Wright.Reward

namespace Wright.Check.Example

open Wright.Check Report

inductive T where
  | a
  | b
  deriving DecidableEq, Repr

instance : CheckId T where
  all := [.a, .b]
  complete := by intro c; cases c <;> decide

def passes (r : Report T) : Bool := Finished.passed r.finish

/-- `a_check_nobody_ran_fails`. -/
example : passes ((empty : Report T).margin .a (.finite 1)) = false := by decide

/-- `missing_evidence_and_nan_fail`. -/
example : passes (((empty : Report T).missing .a).margin .b .nonfinite) = false := by decide

/-- `a_later_pass_cannot_hide_a_failure`: and the kept margin is -2. -/
example :
    let r := (((empty : Report T).margin .a (.finite (-2))).margin .a (.finite 5)).margin .b (.finite 1)
    passes r = false ∧ r.got .a = some (.margin (-2)) := by decide

/-- Both checks measured and clear: passes. -/
example : passes (((empty : Report T).margin .a (.finite 0)).margin .b (.finite 3)) = true := by
  decide

/-! The reward `wright-train` labels a trajectory with (`checks_passed`). -/

/-- One check measured and passing, one never evaluated: 1 of 2. -/
example : reward ((empty : Report T).margin .a (.finite 1)).finish = 1 := by decide

/-- Re-measuring `a` with a pass after a failure does not buy the point
back; `b` passes: 1 of 2. -/
example :
    reward ((((empty : Report T).margin .a (.finite (-2))).margin .a (.finite 5)).margin .b (.finite 1)).finish
      = 1 := by decide

/-- Running `a` ten times does not score ten: the reward is per declared
check, 2 of 2 at most. -/
example :
    reward (((List.replicate 10 (T.a, Evidence.margin 1)).foldl (fun r x => r.record x.1 x.2)
      ((empty : Report T).margin .b (.finite 0))).finish) = 2 := by decide

end Wright.Check.Example
