# Sponsor tracks: Anthropic and Microsoft

Sources were read on 2026-10-24 in the Cal Hacks 13.0 Slack, read-only.
Anthropic: #cohost-anthropic, Arinze Obiezue, Fri 10/23 4:37 PM. Microsoft:
#spons-microsoft, Shiloah Vasquez, Wed 10/21 9:32 AM and Fri 10/23 3:37 PM
(pinned).

## Anthropic: "Claude for Public Benefit"

**What they ask for.** "build something your community needs… The narrower,
the better." The track has four parts:

- a named community ("'Students' is too broad");
- a real problem that we have lived, or have talked to someone about;
- a working, high-impact product ("not a concept of a plan") with a clear
  plan for measuring impact;
- thoughtful use of Claude, both in building the product and in how it works.

Prizes: $5K in Claude credits, Claude Max 20x for a year, office hours, and a
visit to HQ.

**What Wright does for it.** Claude is the engineering team itself, not a
chat layer. An orchestrator (Fable 5.1) hands work to domain specialists
(Opus 5.5). A hostile reviewer (Opus 5.5) can only
block a design two ways:

- by citing a failing check;
- by making an attack that the harness runs and confirms.

Every confirmed attack becomes a permanent regression case
(`crates/wright-agents/src/harness.rs`). No agent's word counts as evidence.
Numbers come only from deterministic tools, and the final state is judged by
checks that cannot pass by omission. A person can draw on a render, and the
orchestrator gets that sketch as an image in its next turn.

**Communities to name in the pitch.** Pick one; the first is recommended.

1. **First-time high-altitude-balloon teams at UC campuses, starting with
   UCSB's student HAB team (Pat's own).** We lived this problem.
   - First launches lose payloads to a wrong free-lift fill, a burst altitude
     past the GPS's altitude limit, or a tracker link that drops below the
     horizon.
   - FAA Part 101 weight rules turn mass mistakes into legal problems.
   - Impact is measurable against our own flight: predicted vs. actual burst
     altitude, ascent rate, and landing distance (the balloon eval in
     `wright-balloon`), and payload recovered or not.
2. **Collegiate rocketry clubs building their first student motor or liquid
   engine (the Berkeley rocket-club vehicle is our eval).**
   - The failure mode is the one the prototype showed: a check nobody listed
     (a chamber-wall stress, a fin flutter margin), so it "passes".
   - Wright's closed check sets fail on missing evidence.
   - Impact: defects caught before the static fire, counted against the
     club's own design-review findings.
   - Not yet validated with the club.
3. **Volunteer search-and-rescue drone units in fire-prone foothills (for
   example, Santa Barbara County SAR).** These units need cheap, printable
   VTOL airframes that carry a radio or a med kit a stated distance, priced
   from parts that are in stock.
   - Impact: range and payload margin against a real mission profile, and
     BOM cost against a commercial drone.
   - **We have not talked to them.** Don't claim this one until someone
     has, because the track asks for a problem we've lived or discussed with
     the people who have it.

**30-second pitch.**

> "Our UCSB balloon team nearly lost its first payload to a burst altitude
> past the GPS limit. Nobody had checked it, so nothing failed. Wright is a
> team of Claude agents that design a balloon, a rocket, or a VTOL drone you
> can actually build. Their only actions are engineering tools, a hostile
> Claude reviewer has to prove every objection by running it, and a design
> ships only when every check passes with evidence. We measure it against
> our own flight."

## Microsoft: "Built for the Real World"

**What they ask for (verbatim challenge):** "Build a production-minded AI
agent that solves a meaningful problem in finance, productivity, healthcare,
or another high-impact domain."

Judging criteria: (1) Customer Understanding, (2) Measurable usefulness,
(3) Path from prototype to production, (4) Responsible design, (5) End-to-end
functionality.

Judges are Microsoft's Forward Deployed Engineering team (Microsoft Delta and
Frontier Co.: "We build production AI systems, not slide decks"). The prize
is a Surface laptop per member, an FDE tech talk, and mentorship.

**No Microsoft technology is required.** Azure, Foundry, Azure OpenAI and
Copilot are never named as requirements. What they offer is Azure for
Students credits ($100, self sign-up). There's a workshop, "System Design for
Deployed Agents", Sat 10:00-10:45 AM in The Theatre.

**What Wright does for each criterion.**

| Criterion | Wright |
|---|---|
| Customer understanding | A named community and a lived failure (above). The prototype's 13 real defects drove the architecture (`docs/ARCHITECTURE.md`). |
| Measurable usefulness | Evals start from requirements-only specs and run against real builds: the 2025 130-mile VTOL, Pat's balloon, and the Berkeley rocket. We count defects caught before build. A run costs about $0.70-0.90, priced from `usage`. |
| Path to production | Who pays is a config choice: `WRIGHT_TRANSPORT=api` (Claude API), `foundry` (**Claude on Microsoft Foundry**, Azure billing), or `cli` (a signed-in Claude login). Every step is an event, and `harness::spawn_event_log` writes a replayable JSON-lines audit trail. Regression cases persist across runs. The tests are offline and need no key, so CI needs no secrets. (The Container Apps plan below was for the web studio, which was removed when Wright became a desktop app; it is kept only as history.) |
| Responsible design | Every call is priced before it is sent and recorded in a ledger on disk that survives restarts; hard caps are one environment variable away (`WRIGHT_RUN_BUDGET_USD`, `WRIGHT_GLOBAL_BUDGET_USD`). Checks fail on missing evidence. The reviewer cannot block on opinion. Build files (CAD, prints) are written only for a design that passed. A person's sketch on a render steers the agents. Refusals stop the agent instead of being read as an answer. |
| End-to-end | Spec, then agents, then checks, then reviewer, then judged final state, then BOM and build files, watched live (and replayed) in the native desktop app (`wright-desktop`). |

**What was integrated (in `crates/wright-agents`).**

- `HttpTransport::foundry` / `foundry_from_env`
  (`crates/wright-agents/src/client.rs`) sends the same Messages request body
  to Claude on Microsoft Foundry. The endpoint shape, the
  `ANTHROPIC_FOUNDRY_API_KEY` / `ANTHROPIC_FOUNDRY_RESOURCE` /
  `ANTHROPIC_FOUNDRY_BASE_URL` variables, and sending the key as both
  `x-api-key` and `api-key` all follow Anthropic's own SDK
  (anthropics/anthropic-sdk-python @202db72, `src/anthropic/lib/foundry.py`).
- Server-side `fallbacks` is turned off on Foundry, where it isn't offered.
- Foundry calls count against the global ledger, the same as API calls.
- Strict tools, effort and prompt caching are all supported on Foundry (claude-api skill, `shared/platform-availability.md`).

**Deploy plan (not executed; obsolete since the web studio and Dockerfile were removed 10/24).** Pat's `az` login is on "Azure for Students".
It has no resource groups or AI resources yet, and `Microsoft.App` isn't
registered. Creating anything bills the student credit, so ask before running
these:

```sh
az provider register -n Microsoft.App --wait
az group create -n wright-rg -l westus2
# The studio needs a Dockerfile at the repo root (a Rust binary is not buildpack-detected).
az containerapp up -n wright-studio -g wright-rg --source . --ingress external \
  --target-port <studio port> --env-vars WRIGHT_TRANSPORT=foundry \
  ANTHROPIC_FOUNDRY_RESOURCE=<resource> ANTHROPIC_FOUNDRY_API_KEY=secretref:foundry-key
```

Claude on Foundry is bought through the Microsoft Marketplace. Whether a
student subscription is allowed to deploy it is **unverified**. If it isn't,
the studio still deploys to Container Apps with `WRIGHT_TRANSPORT=api`.

**30-second pitch.**

> "Wright is an engineering agent built like production software. Each run has
> a hard budget it checks before every call. Every claim is a tool result. A
> reviewer has to prove its attacks, and every proven attack becomes a
> permanent regression test. Every step is in an audit log. The same agent
> runs on the Claude API or on Claude in Microsoft Foundry by changing one
> variable. We measure it
> against vehicles real students built and flew."

## Running the agents

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p wright-agents       # offline: scripted transport, no key, no spend
WRIGHT_LIVE_CLI=1 cargo test -p wright-agents --test harness live_cli -- --ignored --nocapture
                                   # live on the signed-in `claude` login (subscription, not credits)
```

Settings come from the environment:

- `WRIGHT_TRANSPORT`: `api`, `foundry` or `cli`.
- `WRIGHT_RUN_BUDGET_USD` and `WRIGHT_GLOBAL_BUDGET_USD`: the spend limits.
- `WRIGHT_LEDGER`: the ledger file (default `~/.wright/ledger.json`).
- `WRIGHT_FALLBACKS=0`: turns off server-side refusal fallbacks.
