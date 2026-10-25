# Harness research (2026-10-24)

Read from source; commits as checked that day.

## Claude API (claude-api skill table dated 2026-10-06; platform.claude.com docs)

| model | id | $/M in / out |
|---|---|---|
| Opus 5.5 | claude-opus-5-5 | 4 / 20 (cache read 0.20) |
| Sonnet 5.5 | claude-sonnet-5-5 | 2 / 10 (cache read 0.20) |
| Haiku 5.5 | claude-haiku-5-5 | 0.10 / 0.50 up to 100K prompt (verify on the pricing page) |
| Fable 5.1 | claude-fable-5-1 | 10 / 50 |

* Opus 5.5 cannot disable thinking; control is `output_config: {effort}` (low..max). Thinking tokens bill as output.
* Forced `tool_choice` (any/tool) is a 400 on Opus/Sonnet 5.5 and Fable 5.1: use `auto`, `strict: true` tools, name the tool in the prompt. No assistant prefill.
* Histories are append-only (edited thinking blocks are rejected).
* Tool loop: append the whole assistant `content` array; reply with one user message of `tool_result` blocks (`is_error: true` for a failed check, with numbers).
* stop_reason: end_turn, tool_use, max_tokens, stop_sequence, pause_turn, refusal.
* Caching: one breakpoint after the fixed system+tools, plus top-level `cache_control` auto-caching; min prefix 512 tokens; no timestamps or run ids in the system prompt; verify via `usage.cache_read_input_tokens`.
* `POST /v1/messages/count_tokens` is free. Batches API is 50 % off but single-shot (no tool loop): use for offline reviews only.

Budget plan: specialists Haiku 5.5 low, orchestrator Sonnet 5.5 medium, reviewer Sonnet 5.5 high (Opus 5.5 for the demo); about $0.70-0.90 per run; stop a run at $1.25 and everything at $45.

## Patterns, from source

* Claude Agent SDK (anthropics/claude-agent-sdk-python @0e89b29): `max_turns`, `max_budget_usd`, `AgentDefinition` sub-agents whose output returns as one tool result (`types.py:108, 1368, 2405-2415`).
* OpenHands (software-agent-sdk @acd96e6): tool errors go back to the model (`agent/agent.py:1495-1557`); delegate runs sub-agents in their own conversations and returns only their final response (`tools/delegate/impl.py:287-380`); condenser keeps first events and summarises the middle (`context/condenser/llm_summarizing_condenser.py`).
* SWE-agent (@3ea751c): malformed actions requeried up to 3 times (`agents.py` `forward_with_handling`); per-instance and total cost limits (`models.py:73-78`).
* Anthropic: "Building effective agents" (orchestrator-workers; evaluator-optimizer when criteria are clear); "multi-agent research system" (sub-agents return condensed findings; judge the end state).

Pattern: evaluator-optimizer around orchestrator-workers. Specialists share a typed `DesignState`, not conversation history; each returns a <=300-token summary plus its edits.

## Rust

No official SDK. Hand-roll a ~150-line reqwest client passing `serde_json::Value` (rig-core 0.46 is the maintained alternative). Live UI: axum SSE (`examples/sse`, axum 0.8.9 @86aa615) + `tokio::sync::broadcast` (`examples/chat`).
