# Harness + orchestration work (10 agents, started 10/24 ~15:10)

Rules for every agent on this list (from Pat and ~/.claude/CLAUDE.md):
- Code in Rust (Haskell/Lean/C++ fine; no Python). Research a real open-source project first and name the file the design came from in a comment and in docs/TODO.md.
- New logic goes in YOUR new module under crates/wright-agents/src/ (listed below). Edits to shared files (harness.rs, prompts.rs, tools.rs, lib.rs, wright-cli/src/main.rs) must be small, additive, and re-read the file right before each Edit: six other sessions edit this tree concurrently. Never `cargo clean`, never reformat a whole file, never delete others' code.
- The cargo lock is shared by many sessions; a build may queue for 10+ minutes. Wait, don't kill other cargo processes. Keep disk > 3 GB free (df -g /System/Volumes/Data); no downloads without asking the main session.
- Do not touch: lean/, haskell/, crates/wright-train, docs/post-training.md (peer session); crates/wright-cad, cad/ (Astra); crates/wright-air, crates/wright-cfd (running workers); crates/wright-rocket, crates/wright-balloon (domain owners; ask via the main session).
- Done means: `cargo test -p wright-agents` and `cargo clippy -p wright-agents --all-targets -- -D warnings` pass, offline tests cover the new behaviour (ScriptedTransport in src/fake.rs), a TODO.md line under "Harness work" with [x], and a final report naming files + the OSS precedent.

| # | agent lane | module |
|---|---|---|
| 1 | engineering plan / task graph for the orchestrator | src/plan.rs |
| 2 | sweep / design-of-experiments tool | src/sweep.rs |
| 3 | cross-run lessons memory | src/memory.rs |
| 4 | hostile reviewer upgrade (attack taxonomy, debate) | src/review.rs |
| 5 | tool-call reliability + tracing | src/trace.rs |
| 6 | agent-team bench (offline scenarios) | src/bench.rs, tests/bench.rs |
| 7 | spec elicitation + feasibility probe | src/elicit.rs |
| 8 | sourcing tools: catalog search, alternates, BOM diff | src/sourcing.rs |
| 9 | parallel specialists + design merge | src/merge.rs |
| 10 | checkpoints, resume, pause/approve | src/checkpoint.rs |
