# CREAO AI in Wright: the Build Desk

Crate: `crates/wright-creao`. Status 2026-10-24: built and tested offline (20 tests,
clippy clean). Not yet run live, because the CREAO agent it calls has not been created.

## What CREAO is

CREAO (creao.ai) is an "AI super agent" platform. You chat with an agent that runs in
its own Linux sandbox. It can search the web, drive a logged-in browser, call MCP
connectors (Slack, Gmail, Sheets, GitHub, Notion and more), and produce files such as
HTML, PDF and dashboards. A chat that works can be saved as an **agent**: a versioned,
reusable workflow with a structured input form. You can run an agent by hand, on a
schedule, or from code. There are two ways to call it from code
(docs.creao.ai/developer-api/api-paths):

| | API Trigger | CREAO CLI |
|---|---|---|
| key | `capi_`, created on one agent's page, works for that agent only | `cr_sk_`, from developer.creao.ai, works across the account |
| start | `POST https://agent.creao.ai/api/v1/apps/{appId}/runs` with `{inputs}` | `POST https://developer.creao.ai/v1/runs` with `{agent_id, input}` |
| poll | `GET .../apps/{appId}/runs/{id}` until `completed` / `failed` / `cancelled` | `GET /v1/runs/{run_id}` |
| files | `result.artifacts[].url`. Fetch it with the key; it redirects (302) to storage | same |

Our `.env` key is a `capi_` key. It is valid: `agent.creao.ai` answers
`API_KEY_SCOPE_RESTRICTED`, not 401. It is scoped to one agent, so the client needs
that agent's `appId`, set as `CREAO_APP_ID`. The client supports both key types.

## What the Cal Hacks 13.0 track wants

Source: community.creao.ai/CalHacks2026, linked from #spons-creao-ai.

* **Rule for every track:** at least one CREAO agent does the core work of the project.
  It is built on agent.creao.ai. The Devpost README must include **a share link to one
  of the agent's runs and the agent ID**. The demo must **show the agent running**, and
  the project must select the CREAO track on Devpost.
* **Best AI-First Disruptor ($2,500):** "remove the agent, and the product stops
  working". The app runs on the agent's output from end to end. Suggested tools: API
  Trigger plus webhooks.
* **Best One-Person Company ($1,500):** an org chart of agents that hand work to each
  other, coordinated "by an orchestrator via API Trigger".
* **Best Team Empowerment ($1,000):** agents connect the people on a team, using
  Slack, Teams or Notion connectors.
* Winners also join CREAO Launchpad.

**Recommended entry: One-Person Company.** Wright already is an org chart. A Claude
designer agent proposes the design, deterministic solvers check it, and a hostile
reviewer agent attacks it. The CREAO Build Desk is the last role: procurement plus the
build packet. Wright's orchestrator hands work to it through the API Trigger, which
matches the track's own wording. AI-First Disruptor fits partly: without the desk,
uncatalogued parts stay unpriced and the budget check fails. But Wright's core is the
engineering, not the desk, so that claim would be weaker.

## Why the Build Desk (and not a bolt-on)

Wright stops at "every check passes from the catalog". To get from there to "a person
can order the parts and build it tonight", two jobs remain that need the live web
rather than a solver. These are what CREAO is best at: browsing vendor pages and
generating files in a sandbox.

1. **Sourcing.** For every BOM line, find a current listing that is in stock, with its
   URL, vendor, unit price and the date it was read.
2. **The builder's packet.** A single `build.html` with the summary, the sourced BOM
   and totals, the build steps, the check report and safety notes.

Wright does not trust the quotes. `desk::apply_quotes` treats each quote like a catalog
row (ARCHITECTURE.md: a catalog row carries `url` and `retrieved`):

* A quote is rejected if its id is not in the BOM, if the price is NaN, zero or
  negative, or above $20k, if the URL is not https, if the date is not ISO, or if the
  listing is out of stock.
* A catalog price is never overwritten. If a quote differs from it by more than 25%,
  the difference is reported as drift.
* A line the desk could not source **stays unpriced**, so the budget check still fails
  on it. This keeps Wright's rule that no check passes by omission.

The run emits `wright_core::Event`s for the studio: a `ToolCall`, a `ToolResult` with a
priced / rejected / drift / unpriced summary, `AgentMessage` and `Artifact`. Artifacts
are saved under the output directory along with `bom.sourced.csv`.

## API used (and safety)

* The key is read from `.env` or the environment at runtime. `ApiKey` redacts itself in
  `Debug`, and the key appears only in the `Authorization` header. It never goes in a
  URL: the documented `?creao-api-key=` form is not used.
* Ids are checked to be UUIDs before they are put in a path. Run input is capped at the
  documented 256 KB before anything is sent.
* An artifact is downloaded only from the CREAO origin. reqwest drops `Authorization`
  on a redirect to another host, as the docs require for the 302 to storage. Artifact
  file names are reduced to a safe basename.
* Errors are typed from the stable codes: `AUTHENTICATION_REQUIRED`,
  `API_KEY_SCOPE_RESTRICTED`, `INSUFFICIENT_CREDITS` (with available and required
  amounts), `RATE_LIMITED` (honours `Retry-After`), and failed or cancelled runs with
  their code.
* Polling starts at 3 s, backs off by 1.5x up to 20 s, and stops after a 20 min budget.
  There are no webhooks yet: they need a public URL, and polling is the documented
  fallback.

Prior art: the request and response shapes follow CREAO's own OpenAPI
(`docs.creao.ai/developer-api/developer-v1.openapi.yaml`, as rendered in
`developer-api/runs/create-async-run.md`) and `pro/api-trigger.md`. There is no
official Rust SDK to copy. The start → poll → download-artifacts loop is the one the
CREAO quickstart prescribes.

## Steps for Pat: create the agent (about 5 minutes, uses a few credits)

1. Go to agent.creao.ai (Patrick's team), then **New chat**.
2. Paste the **bootstrap prompt** below and send it. Let it finish: it should produce a
   `build.html` and end with a JSON block.
3. Click **Create Agent** in the chat input bar. Set the output format to **Html**.
   Paste the **agent instructions** below into Instructions, then click **Start
   building**.
4. Open the new agent, then the **Config** tab. Make sure the input form has exactly two
   text fields with ids **`vehicle`** and **`design_packet`**; Wright sends those two
   keys. If the names differ, send a follow-up message in the agent's chat: "Rename the
   inputs to exactly `vehicle` (text) and `design_packet` (long text, JSON)."
5. Name it **Wright Build Desk**. Set the default model to a Claude model.
6. On the agent's page, copy its **agent ID (appId)** and add `CREAO_APP_ID=<id>` to
   `wright/.env`. The ID is not secret, and the Devpost README needs it.
7. **API Trigger key:** if the existing `.env` key was made on a different agent, it
   will return `API_KEY_SCOPE_RESTRICTED`. In that case, create an API key on this
   agent's page and replace `CREAO_API_KEY` in `.env` yourself.
8. Test: `cargo run -p wright-creao --example desk_smoke`. It runs one small sourcing
   job of 2 BOM lines and prints the run id, credits and summary. Then open the run on
   agent.creao.ai and **share** it; that link goes in the Devpost README.

### Bootstrap prompt (paste into the new chat)

```
You are the Build Desk for Wright, a system that designs buildable vehicles (VTOL
drones, student rockets, high-altitude balloons). Wright sends you a design whose
engineering checks already pass. Your job is the part that needs the live web:

Inputs: vehicle (text) and design_packet (JSON: vehicle, domain, summary, bom.lines[]
with id/name/qty/unit/unit_price/url/note, checks[] with name/passed/detail,
build_steps[], files[]).

1. Source every BOM line. Prefer the line's own url if it still sells the part; else
   find an equivalent listing that matches every spec in the name/note (voltage,
   current, size, thread, mass class). Use US vendors that ship to California. Read
   the actual page: record vendor, https URL, unit price in USD for ONE unit, whether
   it is in stock, and today's date. Never estimate or invent a price; if you cannot
   find a real listing, list the line under "unsourced" with the reason.
2. Write build.html: a single self-contained page for the person who will build it:
   summary, a BOM table (part, qty, vendor link, unit price, line total, grand total;
   mark unsourced lines in red), build steps in order, the check report (pass/fail +
   detail), and safety notes for the domain (LiPo handling / rocket motor rules /
   balloon FAA Part 101).
3. End your reply with exactly one fenced ```json block:
   {"quotes":[{"bom_id":"...","vendor":"...","url":"https://...","unit_price_usd":0.0,
     "in_stock":true,"retrieved":"YYYY-MM-DD"}],
    "unsourced":[{"bom_id":"...","reason":"..."}],
    "build_page":"build.html","notes":"one sentence"}
   bom_id must be copied exactly from the input. One quote per line.

Try it now with: vehicle = "Wright smoke test" and design_packet =
{"vehicle":"Wright smoke test","domain":"air","summary":"two parts",
 "bom":{"lines":[{"category":"propulsion","id":"esc-30a","name":"30 A BLHeli_S ESC, 2-4S",
 "qty":1,"unit":"ea","unit_price":null,"url":"","note":""},{"category":"controls",
 "id":"servo-9g","name":"9 g micro servo, metal gear","qty":2,"unit":"ea",
 "unit_price":null,"url":"","note":""}]},"checks":[],"build_steps":["solder ESC"],"files":[]}
```

### Agent instructions (paste into the Create Agent dialog)

```
Inputs are exactly two text fields: vehicle and design_packet (JSON). Follow the
three steps from the session every run: source every BOM line from a real, in-stock
listing you actually read (never estimate a price; unsourced lines go in "unsourced"),
write build.html as a self-contained builder's page, and end the reply with the single
fenced json block {quotes, unsourced, build_page, notes} using bom_id values copied
exactly from the input. Do not ask questions; this agent is called from an API.
```

## Integration hook for the orchestrator

`wright-agents` (or `wright-cli`) calls the desk after a design's checks pass:

```rust
let client = wright_creao::Client::new(wright_creao::Config::load(".env".as_ref())?,
                                       wright_creao::transport::Reqwest::new()?);
let packet = wright_creao::DesignPacket { vehicle, domain, summary, bom, checks, build_steps, files };
let out = wright_creao::run_desk(&client, run_id, &packet, &out_dir, Default::default()).await?;
// out.events -> the event stream; out.sourcing.bom -> re-run the budget check
```

## 30-second pitch

"Wright's Claude agents design a drone, a rocket or a balloon, and deterministic
solvers check every claim. Then the design is handed down the org chart to a CREAO
agent, our Build Desk. It goes out to the live web, prices every part from real
in-stock listings, and writes the builder's packet. Wright checks every quote it
returns: a made-up price is rejected, and a part it can't source fails the budget. One
person, a team of agents, and a vehicle you can order tonight."

## Gaps

* Run live 2026-10-24: agent `35c8dff1-1512-4206-b667-f05514f40f85` (Claude Sonnet 5.5), smoke run `7bbbb877-660b-4083-b9c8-411419b7aeb1` priced 2/2 lines. That agent has a structured output, so `result.text` is a summary object and the fenced JSON block is in `result.chatText`; `run_desk` reads `chatText` first.
  Devpost also needs a shared run link plus the agent ID.
* No webhooks: they need a public URL. Polling is the documented fallback.
* Not wired into the orchestrator yet. The hook above is the whole call.
* The 25% drift and $20k caps are unsourced judgement calls (they are named constants
  in `desk.rs`).
