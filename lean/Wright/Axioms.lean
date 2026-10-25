-- Run `lake env lean Wright/Axioms.lean`: every theorem rests only on propext and Quot.sound (no sorryAx).
import Wright.Check
import Wright.Reward
import Wright.Example
import Wright.Incumbent
open Wright.Check
#print axioms finished_passes_iff_all_measured
#print axioms build_passes_iff
#print axioms failure_is_permanent
#print axioms record_keeps_failure
#print axioms order_irrelevant
#print axioms unevaluated_fails
#print axioms record_nonfinite_fails
#print axioms keep_margin_min
-- the reward (Wright/Reward.lean)
#print axioms Report.reward_full_iff_all_measured
#print axioms Report.reward_lt_total_of_slot_fails
#print axioms Report.reward_le_recorded
#print axioms Report.reward_record_le
#print axioms Report.reward_foldl_le
#print axioms Report.reward_perm
-- the incumbent rule (Wright/Incumbent.lean)
#print axioms Wright.Incumbent.offer_keeps_passing
#print axioms Wright.Incumbent.run_keeps_passing
#print axioms Wright.Incumbent.better_asymm
#print axioms Wright.Incumbent.failing_zero_iff_passed
