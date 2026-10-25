# Post-training: Wright's runs as training data

Status 2026-10-24 (evening). What exists, what it is for, what was not done.
No model was trained.

## The idea

Every action a Wright agent takes is a deterministic engineering tool, and
the end of every run is a check report nobody can talk their way past:
`Finished::passed` needs every declared check measured, finite and
non-negative (`crates/wright-core/src/check.rs`; proved in
`lean/Wright/Check.lean`). That makes a Wright run a trajectory with a
verifiable reward: the setting of rejection-sampling fine-tuning (train on
the trajectories the verifier accepted) and, later, RL with verifiable
rewards. The reward is not a model's opinion of the design.

Prior art, read from source:

* **SWE-smith** (github.com/SWE-bench/SWE-smith, `main`):
  `swesmith/train/traj_mgr/collect_trajs.py` walks a folder of SWE-agent
  trajectories, reads each one's evaluation report, and writes one JSON
  object per line with `messages`, `instance_id`, `resolved`, `model`,
  `traj_id` and `patch`. It **tags** `resolved`; it does not filter (a
  trajectory is dropped only when its evaluation is missing or it fails to
  parse). `swesmith/train/traj_mgr/utils.py` offers three message styles:
  `ticks`, `xml` and `tool` (`transform_traj_toolcalls`, which keeps the
  native tool calls as they were). `docs/guides/train_swe_agent.md` is the
  rejection-sampling workflow: generate, evaluate, train on the resolved
  subset; `configs/train/full_ft_qwen_7b.yml` and
  `scripts/train.run_ft_torchtune.sh` are the training side. Their
  `train/README.md` credits SWE-Gym for the procedure.
* **SWE-agent** keeps the full history in its `.traj` files; SWE-smith's
  `get_messages` reads `trajectory[-1]["query"]`, the exact messages of the
  last step. Wright's `events.jsonl` had no equivalent: `Event::ToolResult`
  carries a 400-character summary (`crates/wright-agents/src/agent.rs`),
  so a run could not be replayed into what the model saw. The harness now
  writes transcripts (below).
* **OpenAI fine-tuning JSONL** puts `tools` beside `messages` in each
  record; the record here does the same, with the system prompt as the
  first message as in SWE-smith's output.

`docs/cad-ai-research.md` §3 already argued for a trace corpus with
execution-grounded labels and for measuring retrieval of successful traces
before any fine-tuning; this is the corpus half of that.

## What the harness records

With an output directory (`wright run ... -o DIR`, the desktop app, or
`RunConfig::out_dir`), the harness writes `DIR/transcripts/<name>.json`
after every agent loop (`crates/wright-agents/src/harness.rs`,
`transcript`; `agent.rs`, `save_transcript`):

| file | conversation | written |
|---|---|---|
| `orchestrator.json` | the lead, one conversation across rounds | after every round (append-only history, so each write is a superset) |
| `reviewer-r<N>.json` | the hostile reviewer of round N | once |
| `specialist-<role>-r<N>.json`, `-2`, `-3`... | each delegation in round N | once each, never overwriting |

Each file: `agent`, `model` (`id`, `effort`, `max_tokens`), `system`,
`tools`, `messages` (the Messages API history exactly as sent, including
`tool_use` / `tool_result` blocks, thinking blocks and image feedback),
and `pending` (tool results still owed to the model when a tool ended the
loop, kept apart so the history ends on the assistant's last turn).
`crates/wright-agents/tests/harness.rs::transcripts_record_what_each_agent_was_sent`
checks the record against the scripted transport's requests byte for byte.

## The exporter: `wright train export`

```sh
wright train export runs/* -o out/train             # everything, tagged
wright train export runs/* -o out/train --resolved-only   # the RFT set
```

Writes `out/train/trajectories.jsonl` (one record per agent conversation
per run) and `summary.json`. A run directory that does not read is
skipped and named, not fatal (as SWE-smith's collector does). Library:
`crates/wright-train` (`read_run`, `trajectories`, `reward`, `resolved`,
`reward_bin`, `export`), tests in `src/lib.rs`.

`summary.json` (also printed) carries, besides the counts SWE-smith's
`collect_trajs.py` prints (found / resolved): `full` vs `lossy`
trajectory counts, `per_domain`, a `reward_histogram` of the runs by
`checks_passed / checks_total` in tenths (index 10 is the full reward, a
passing report; runs without a report are counted in `no_report`), and
`run_summaries`, one row per run with `id`, `dir`, `domain`, `request`,
`agents` (the transcript names), `trajectories`, `resolved`, `lossy` and
the run's `reward`. The rows are what to read first: they say which runs
have real transcripts, which passed, and how far the others got, without
opening the trajectory file.

One record:

| field | meaning |
|---|---|
| `traj_id` | `<run id>.<transcript name>` |
| `instance_id` | the run id (SWE-smith's name for the task instance) |
| `domain`, `request` | what was asked, in the person's words |
| `agent`, `model` | `orchestrator` / `reviewer` / `specialist:<role>`; the model id that produced the assistant turns |
| `resolved` | the run's final report (`state.json`) passes under `Finished::passed` **and** the harness ended the run as passed (`run_finished.passed`: checks clean and the reviewer out of issues). Either alone is not enough |
| `lossy` | rebuilt from `events.jsonl` (a run recorded before transcripts): tool results cut to 400 characters, first user turn a placeholder, no system prompt, no thinking. For reading, not training |
| `reward.passed` | `Finished::passed` on the final report: the binary reward |
| `reward.checks_passed`, `checks_total` | declared checks that passed, out of all declared: the graded reward |
| `reward.harness_passed`, `objective`, `objective_label`, `spend_usd` | the harness's verdict, the domain objective (km, FAR-OUT score, burst altitude), cumulative cost |
| `reward.check_history` | `[passed, failed]` of every check report during the run, in order: the verifier's view of progress, for process rewards |
| `tools` | the strict tool definitions the agent had |
| `messages` | `[{"role":"system",...}, ...]` then the recorded history, Messages API shape |

The verdict is recomputed here from `state.json` with the same
`Finished::passed`; the harness's `run_finished.passed` is kept as a
separate field, so a trajectory is never resolved on the agent's say-so.

## Why the reward holds up

* `lean/Wright/Reward.lean` (every theorem rests on `propext` and
  `Quot.sound` only; `scripts/verify.sh` checks this): the full reward
  needs every declared check measured and non-negative
  (`reward_full_iff_all_measured`); one failing or unevaluated check costs
  at least one point (`reward_lt_total_of_slot_fails`); at most one point
  per declared check actually recorded (`reward_le_recorded`); measuring a
  check again never raises the reward, singly or in any number
  (`reward_record_le`, `reward_foldl_le`), so retrying checks buys a policy
  nothing; recording order is irrelevant (`reward_perm`).
  `lean/Wright/Example.lean` runs the cases by `decide` (ten recordings of
  one check score one point).
* `lean/Wright/Incumbent.lean`: the harness's incumbent rule
  (`state.rs` `better` / `DesignState::offer`, FunSearch's strict
  replace-on-better) never trades a passing design for a failing one
  (`offer_keeps_passing`, over any sequence of offers `run_keeps_passing`),
  never raises the failing count (`offer_failing_le`), keeps the earlier
  design on a tie (`offer_tie`), and `failing_zero_iff_passed` ties its
  "zero failing" to the same `Finished.passed` the reward uses. So once
  a run has a passing incumbent, no later offer can make the incumbent a
  failing design; whether the run's final `state.json` is that incumbent
  is the harness's `prefer_best` flow, which is not modelled.
* The numbers the margins are made of are cross-checked by the Haskell
  oracle: 842 cases over atmosphere, nozzle, balloon, the boom's bending
  and shell buckling, Lame tank walls, fin flutter, bulkheads, free-space
  loss and link geometry, GNSS coexistence, the box's warmer power and
  cell-capacity tables, nose pressure drag, chute descent and bulkhead
  closures; 2125 comparisons, 0 over tolerance (`haskell/crosscheck.sh`,
  docs/verification.md).

## Using it

A researcher's walk through the files, in order:

1. `wright train export runs/* -o out/train` and read the printed
   per-run rows (or `out/train/summary.json` → `run_summaries`). Keep the
   runs whose `lossy` is false: only those carry the exact prompts, tool
   definitions and full tool results. A lossy run tells you what happened
   (`events.jsonl` order, 400-character result summaries) but not what the
   model was shown; do not train on it.
2. Look at `reward_histogram`. Everything in the last bin with
   `resolved: true` is the RFT set; `--resolved-only` writes exactly that.
   The intermediate bins are the graded data (how many declared checks a
   run cleared) for a reward model or a curriculum; `no_report` runs have
   no label and should be dropped.
3. In `trajectories.jsonl`, one line is one agent's whole conversation:
   `messages[0]` is the system prompt, `tools` the strict schemas it had,
   and the rest alternates assistant turns (`text`, `thinking`,
   `tool_use` blocks) with user turns of `tool_result` blocks. Filter by
   `agent` (`orchestrator`, `reviewer`, `specialist:<role>`) if the
   student is to learn one role; the labels are run-level, so a reviewer
   in a resolved run is labelled resolved whether or not it found anything
   (see "Not done").
4. `reward.check_history` is the verifier's trace of the run: the
   `[passed, failed]` count after every check report, so a step-level
   signal can be aligned with the `tool_result` that followed each design
   change.
5. Convert `tool_use` / `tool_result` to the student's chat template and
   train as SWE-smith does (`scripts/train.run_ft_torchtune.sh`); nothing
   in this repository does that step.

* **RFT / SFT of an open-weights student** (what SWE-smith does with
  Qwen): filter `resolved`, drop or keep thinking blocks by policy, convert
  the Anthropic `tool_use` / `tool_result` blocks to the student's chat
  template (OpenAI-style `tool_calls`: a mechanical transform, not written
  here), train with torchtune or unsloth as in
  `SWE-smith/scripts/train.run_ft_torchtune.sh`.
* **Preference pairs**: the same request run twice with one resolved and
  one not is a DPO pair (SWE-smith ships `configs/train/dpo_qwen_7b.yml`).
  Not generated here; `runs/` holds one run per request so far.
* **Process rewards**: `reward.check_history` and the per-step `checks`
  events give a verifier-graded signal at every design change.
* **Retrieval before training**: per `docs/cad-ai-research.md` §3, the
  cheaper first use of this corpus is retrieving a resolved trajectory for
  a similar request into the orchestrator's context and measuring the gain
  at equal tool-call budget.

## Not done, and what to check before doing it

* No weights were trained: no GPU, no budget line, no authorization
  (`docs/cad-ai-research.md` says the same for the CAD corpus).
* Anthropic's commercial terms restrict using Claude's outputs to develop
  competing models. Before training any third-party model on these
  trajectories, read the current terms; the exporter does not decide this.
* Only runs recorded from now on carry transcripts. `demo/aircraft`,
  `demo/balloon`, `demo/rocket` and `runs/rocket-1792873213` export as
  `lossy`.
* Corpus size is three domains and a handful of runs; SWE-smith's point is
  that the task generator, not the trajectory format, is what scales.
  Wright's equivalent is the spec space of each domain (`Domain::design`
  from requirements-only specs) plus the reviewer's regression cases in
  `data/regressions-*.jsonl`.
* The reviewer's `attack` tool calls are part of the reviewer's
  trajectory, labelled by the same run-level reward; a reviewer that
  found a real regression in a run that then passed is a good reviewer in
  a resolved run, which the run-level label cannot tell apart from a
  reviewer that found nothing. A per-agent reward (confirmed attacks,
  refuted attacks) is the next label to add.
