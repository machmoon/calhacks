/-
The electronics acceptance rule (docs/electronics-plan.md section 4, Lane F):
when a routed board may be accepted, and the ways it can never be.

Model of the evidence `wright-electronics` judges a board on
(crates/wright-electronics/src/checks.rs, crates/wright-eda/src/board.rs):

* `RouteReport { attempted, routed, unrouted, poured }` -> `Board.attempted`
  and `Board.status : Nat → NetStatus` (`routed | poured | unrouted reason`);
  a net index is an index into `Netlist::nets`.
* the per-net pad count (the `Netlist` pins) -> `Board.pads`; `Board.nets`
  lists every net index.
* every `Pin { net, no_connect }` of every part -> `Board.pins`
  (`onNet` = `net.is_some()`, `declaredNC` = `no_connect`).
* kicad-cli's `CliVerdict` for erc, drc and parity -> `ToolVerdict`
  (`ran n` = the tool ran and reported `n` errors; `didNotRun` = the tool
  was missing or crashed, Rust `ran == false`).

The rule, `accept`: every attempted net is routed or poured; every net
with two or more pads was attempted; every pin is on a net or declared
no-connect; ERC, DRC and parity each ran and reported 0. The coverage
conjunct is stricter than the plan's sketch: without it a router that
attempts nothing would satisfy "every attempted net is routed" vacuously
(the Python baseline's "an empty result reads as 100 %" bug), so it is in the rule rather
than only in the completeness theorem.

Theorems:

1. `tool_not_run_rejects` (and `tool_errors_reject`): a tool that did not
   run, or reported errors, means not accepted ("unverified never green").
2. `unrouted_rejects`: any attempted net left unrouted means not accepted.
3. `mark_unrouted_monotone`: marking one more net unrouted cannot turn a
   rejected board into an accepted one.
4. `floating_pin_rejects`: a pin on no net and not declared no-connect
   means not accepted.
5. `unattempted_rejects`: a net with two or more pads that the router was
   never asked to route means not accepted.
6. `complete_iff_none_unrouted`, `empty_attempted_not_complete`,
   `covered_nonempty`: with `attempted` covering every net with two or more
   pads, `completion = 1` exactly when no attempted net is unrouted, and an
   empty `attempted` never reads as complete.
7. `accept_iff`: `accept` is exactly the conjunction above, as
   propositions.

What this does not verify: the Rust code. These proofs pin the rule
`wright-electronics`'s `check` must implement; whether the Rust does so is
for its tests, and whether a net is really connected is recomputed
independently by the Haskell oracle (haskell/src/Oracle/Pcb.hs), which
also does not verify the Rust by itself.
-/

namespace Wright.Board

/-- A net's state after routing (`RouteReport`). -/
inductive NetStatus where
  | routed
  | poured
  | unrouted (reason : String)
  deriving DecidableEq, Repr

namespace NetStatus

def isUnrouted : NetStatus → Bool
  | unrouted _ => true
  | _ => false

end NetStatus

/-- kicad-cli's verdict (`CliVerdict`): it ran with `n` errors, or it did
not run at all. -/
inductive ToolVerdict where
  | ran (errors : Nat)
  | didNotRun
  deriving DecidableEq, Repr

namespace ToolVerdict

/-- Clean means: the tool ran and reported zero errors. -/
def clean : ToolVerdict → Bool
  | ran 0 => true
  | _ => false

theorem clean_iff (v : ToolVerdict) : v.clean = true ↔ v = ran 0 := by
  cases v with
  | didNotRun => simp [clean]
  | ran n => cases n <;> simp [clean]

end ToolVerdict

/-- One pin of one part. -/
structure Pin where
  onNet : Bool
  declaredNC : Bool
  deriving DecidableEq, Repr

def Pin.accounted (p : Pin) : Bool := p.onNet || p.declaredNC

structure Board where
  /-- Every net index of the netlist. -/
  nets : List Nat
  /-- Pads on each net. -/
  pads : Nat → Nat
  /-- `RouteReport.attempted`. -/
  attempted : List Nat
  status : Nat → NetStatus
  pins : List Pin
  erc : ToolVerdict
  drc : ToolVerdict
  parity : ToolVerdict

namespace Board

/-- Nets that need copper: two or more pads. -/
def needs (b : Board) : List Nat := b.nets.filter fun n => decide (2 ≤ b.pads n)

def netsOk (b : Board) : Bool := b.attempted.all fun n => !(b.status n).isUnrouted

def covered (b : Board) : Bool := b.needs.all fun n => decide (n ∈ b.attempted)

def pinsOk (b : Board) : Bool := b.pins.all Pin.accounted

def toolsOk (b : Board) : Bool := b.erc.clean && b.drc.clean && b.parity.clean

/-- The acceptance rule. -/
def accept (b : Board) : Bool := b.netsOk && b.covered && b.pinsOk && b.toolsOk

/-- `accept`, read as a proposition. -/
theorem accept_iff (b : Board) :
    b.accept = true ↔
      (∀ n ∈ b.attempted, (b.status n).isUnrouted = false) ∧
      (∀ n ∈ b.nets, 2 ≤ b.pads n → n ∈ b.attempted) ∧
      (∀ p ∈ b.pins, p.onNet = true ∨ p.declaredNC = true) ∧
      b.erc = .ran 0 ∧ b.drc = .ran 0 ∧ b.parity = .ran 0 := by
  simp only [accept, netsOk, covered, pinsOk, toolsOk, needs, Pin.accounted,
    Bool.and_eq_true, List.all_eq_true, Bool.not_eq_true', List.mem_filter,
    decide_eq_true_eq, Bool.or_eq_true, ToolVerdict.clean_iff]
  constructor
  · rintro ⟨⟨⟨h1, h2⟩, h3⟩, ⟨h4, h5⟩, h6⟩
    exact ⟨h1, fun n hn hp => h2 n ⟨hn, hp⟩, h3, h4, h5, h6⟩
  · rintro ⟨h1, h2, h3, h4, h5, h6⟩
    exact ⟨⟨⟨h1, fun n hn => h2 n hn.1 hn.2⟩, h3⟩, ⟨h4, h5⟩, h6⟩

/-! ## 1. Unverified never green -/

/-- A tool that did not run means the board is not accepted. -/
theorem tool_not_run_rejects (b : Board)
    (h : b.erc = .didNotRun ∨ b.drc = .didNotRun ∨ b.parity = .didNotRun) :
    b.accept = false := by
  rcases h with h | h | h <;> simp [accept, toolsOk, h, ToolVerdict.clean]

/-- A tool that reported errors means the board is not accepted. -/
theorem tool_errors_reject (b : Board) (n : Nat)
    (h : b.erc = .ran (n + 1) ∨ b.drc = .ran (n + 1) ∨ b.parity = .ran (n + 1)) :
    b.accept = false := by
  rcases h with h | h | h <;> simp [accept, toolsOk, h, ToolVerdict.clean]

/-- Accepted implies every tool ran clean. -/
theorem accept_tools_ran (b : Board) (h : b.accept = true) :
    b.erc = .ran 0 ∧ b.drc = .ran 0 ∧ b.parity = .ran 0 :=
  let ⟨_, _, _, r⟩ := (accept_iff b).1 h; r

/-! ## 2. An unrouted net blocks -/

theorem unrouted_rejects (b : Board) (n : Nat) (r : String)
    (hn : n ∈ b.attempted) (hs : b.status n = .unrouted r) : b.accept = false := by
  cases h : b.accept with
  | false => rfl
  | true =>
    have := ((accept_iff b).1 h).1 n hn
    simp [hs, NetStatus.isUnrouted] at this

/-! ## 3. Monotonicity -/

/-- The report with net `n` marked unrouted (for reason `r`). -/
def markUnrouted (b : Board) (n : Nat) (r : String) : Board :=
  { b with status := fun m => if m = n then .unrouted r else b.status m }

/-- Marking one more net unrouted cannot make a board accepted: if the
marked board is accepted, the original was too. -/
theorem mark_unrouted_monotone (b : Board) (n : Nat) (r : String)
    (h : b.accept = false) : (b.markUnrouted n r).accept = false := by
  cases hm : (b.markUnrouted n r).accept with
  | false => rfl
  | true =>
    obtain ⟨h1, h2, h3, h4⟩ := (accept_iff _).1 hm
    have : b.accept = true := by
      refine (accept_iff b).2 ⟨fun m hmem => ?_, h2, h3, h4⟩
      have hm1 := h1 m hmem
      by_cases e : m = n
      · simp [markUnrouted, e, NetStatus.isUnrouted] at hm1
      · simpa [markUnrouted, e] using hm1
    rw [h] at this; exact absurd this (by decide)

/-! ## 4. An undeclared floating pin blocks -/

theorem floating_pin_rejects (b : Board) (p : Pin) (hp : p ∈ b.pins)
    (hn : p.onNet = false) (hc : p.declaredNC = false) : b.accept = false := by
  cases h : b.accept with
  | false => rfl
  | true =>
    have := ((accept_iff b).1 h).2.2.1 p hp
    simp [hn, hc] at this

/-! ## 5. A net the router was never asked to route blocks -/

theorem unattempted_rejects (b : Board) (n : Nat) (hn : n ∈ b.nets)
    (hp : 2 ≤ b.pads n) (ha : n ∉ b.attempted) : b.accept = false := by
  cases h : b.accept with
  | false => rfl
  | true => exact absurd (((accept_iff b).1 h).2.1 n hn hp) ha

/-! ## 6. Completion (`RouteReport::completion`) -/

/-- Attempted nets that are not unrouted (`routed.len()` counting pours). -/
def doneCount (b : Board) : Nat :=
  (b.attempted.filter fun n => !(b.status n).isUnrouted).length

/-- `completion() == 1.0`: Rust returns 0 for an empty `attempted`,
otherwise `routed / attempted`. -/
def complete (b : Board) : Bool :=
  !b.attempted.isEmpty && b.doneCount == b.attempted.length

/-- The baseline's bug, ruled out: nothing attempted is never 100 %. -/
theorem empty_attempted_not_complete (b : Board) (h : b.attempted = []) :
    b.complete = false := by
  simp [complete, h]

/-- With `attempted` covering every net that needs copper, a board with
any such net has a non-empty `attempted`. -/
theorem covered_nonempty (b : Board) (hc : b.covered = true) (hn : b.needs ≠ []) :
    b.attempted ≠ [] := by
  intro ha
  obtain ⟨n, hmem⟩ := List.exists_mem_of_ne_nil _ hn
  simp only [covered, List.all_eq_true, decide_eq_true_eq] at hc
  have := hc n hmem
  simp [ha] at this

theorem doneCount_eq_iff (b : Board) :
    b.doneCount = b.attempted.length ↔ ∀ n ∈ b.attempted, (b.status n).isUnrouted = false := by
  simp only [doneCount]
  rw [List.length_filter_eq_length_iff]
  simp

/-- Completeness: under coverage, and with at least one net that needs
copper, `completion = 1` exactly when no attempted net is unrouted. -/
theorem complete_iff_none_unrouted (b : Board) (hc : b.covered = true) (hn : b.needs ≠ []) :
    b.complete = true ↔ ∀ n ∈ b.attempted, (b.status n).isUnrouted = false := by
  have hne := covered_nonempty b hc hn
  have : b.attempted.isEmpty = false := by
    cases h : b.attempted with
    | nil => exact absurd h hne
    | cons _ _ => rfl
  simp only [complete, this, Bool.not_false, Bool.true_and, beq_iff_eq]
  exact doneCount_eq_iff b

/-- An accepted board with any net that needs copper reads as complete. -/
theorem accept_complete (b : Board) (h : b.accept = true) (hn : b.needs ≠ []) :
    b.complete = true := by
  have hc : b.covered = true := by
    simp only [accept, Bool.and_eq_true] at h; exact h.1.1.2
  exact (complete_iff_none_unrouted b hc hn).2 ((accept_iff b).1 h).1

end Board

/-! ## A concrete board: the rule decides it -/

/-- Two nets, both routed, all tools clean: accepted. -/
def okBoard : Board :=
  { nets := [0, 1], pads := fun _ => 2, attempted := [0, 1], status := fun _ => .routed,
    pins := [⟨true, false⟩, ⟨false, true⟩], erc := .ran 0, drc := .ran 0, parity := .ran 0 }

theorem okBoard_accepted : okBoard.accept = true := by decide

/-- The same board with DRC missing (kicad-cli not installed): rejected. -/
theorem okBoard_no_drc : ({ okBoard with drc := .didNotRun } : Board).accept = false := by decide

end Wright.Board
