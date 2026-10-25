# Wright: 5-minute table demo

**Replay a REAL recorded run in the desktop app (`wright-desktop`; there is
no web studio any more).** Every number below is quoted from
`demo/aircraft/events.jsonl` (run `aircraft-1792847422`, 44 events) or from
the files that run wrote. If a judge asks "is it real?", the answer is yes.
Rewritten 10/24 after review 4 (finding 7): the old script narrated the
fixture's numbers (8 pass / 3 fail, $628.70, 7 plates), which no real run
produced.

## What this recording does and does not have

- It passed in one round: the orchestrator wrote the spec, ran `design`, got
  44 of 44 checks, and submitted. The reviewer made three attacks and none
  was confirmed. Total spend $0.16 (the last Spend event: $0.1559 of a $5.00
  run limit).
- **It has no markup step.** No person drew on it. The judge-draws pause
  needs a recording that contains a `feedback` event. The attempt at one,
  `demo/aircraft-markup/` (run `aircraft-1792851007`), did not pass: it stopped
  at the round limit after 4 rounds, $1.65, 194.9 km. So **re-record a markup
  run that passes** before showing the pause. Until then, the app refuses
  a judge's markup on this replay and says why: "This is a recorded run:
  markup is replayed at the recorded step." (On this recording there is no
  such step, so do not invite the judge to draw.)
- It has no Render events: the run was recorded before the guide renders
  were sent as events. The 3D view shows `cad/assembly.stl` from the Artifact
  events (orbit, click a piece to select it). The build guide was regenerated
  from this run's recorded build files with `wright report demo/aircraft` (no
  model calls); its pictures and checklists are in the Guide tab, and "Open
  the printable guide" opens `guide/guide.html`.
- Caveat to keep in mind, not to narrate: review 4 found that at recording
  time the aircraft's checks read the requirements from the design, so the
  reviewer's spec attacks could not change an outcome. Its own verdict says
  so: "The model apparently doesn't vary stall or range with those
  environment fields". The fix is in progress (harness side done: edit_design
  can no longer touch `requirements`). If a judge asks why the attacks failed,
  say that.

Setup (before judges arrive):
`cargo run --release -p wright-desktop -- --replay demo/aircraft/events.jsonl`
(run from the repo root; recorded paths also resolve against the
recording's folder). Window full screen, press Pause, then Restart so it
waits at event 0. Have PrusaSlicer open in a second window
with one of `demo/aircraft/print/*.gcode` loaded, and `demo/aircraft/bom.csv`
in a third. No wifi or API needed.

## 0:00 to 0:30: the problem (talk, no clicks)

"People who build drones, student rockets and weather balloons lose them on
the first flight to things a calculator didn't check: a boom that flexes, a
CG that's 2 cm off, a GPS that stops at 12 km. Wright designs the vehicle with
a team of Claude agents, but the agents can act only through deterministic
engineering tools. Every claim has to pass a check, and a check with no
evidence fails."

## 0:30 to 1:45: the run

Press Play at 2x (speed menu by the timeline). The timeline's ticks mark each check report (green pass, red fail) and the finish. Point at these as they arrive:

1. The request in the header: "A 3D-printed VTOL quadplane that flies farther
   than 130 miles with a 15 % reserve: 1.2 m wingspan, prints on a Bambu A1,
   6S, under $2000 including the battery."
2. The orchestrator's first message: "The spec uses 210 km for 'farther than
   130 miles', which is a little over 209 km." The person's numbers are locked
   from here on. No agent can lower them.
3. **The check report: 44 pass, 0 fail.** Read two rows aloud, both thin:
   - range: "211.2 km with 15 % reserve", a +1.2 km margin on 210 km;
   - stall_speed: "stall 14.0 m/s (CLmax 1.47), at most 14", a +0.026 m/s
     margin.
   "Thin margins are shown as thin. Nothing is rounded up."
4. The headlines (top left): Range 211.2 km (need 210), Cost $1611.93 (cap $2000),
   Mass 4.53 kg.

## 1:45 to 2:45: the hostile reviewer

The reviewer's message: "Attacking the thin margins first: stall_speed (0.026
m/s) and range (1.2 km on 210 km)." It makes three attacks: a 1500 m field
altitude, a 1000 m field altitude and a +45 K hot day. Each comes back "not
confirmed" with the numbers. "The reviewer can't block on opinion. It must
cite a failing check or make an attack the host runs. A confirmed attack
becomes a permanent regression case for every later design."

(If asked: this aircraft model did not vary stall or range with those fields
when this was recorded. That is review 4's finding and is being fixed; the
reviewer said so itself in its verdict.)

## 2:45 to 4:00: a design you can go build

The Artifact rows at the end, then the build guide:
- **Parts**: "BOM ($1612)", 29 lines, each linking to a product page (the
  first is the AOS Supernova 2810 900KV lift motor, 4 at $45.54). The budget
  check: "BOM $1612 over 29 lines; budget $2000".
- **Prints**: "printed pieces: 28 STL + G-code, 61.5 h, 562 g". Switch to
  PrusaSlicer: "real G-code, sliced by PrusaSlicer 2.9.6. Print time and grams
  come from the slicer."
- **Flight files**: "ArduPilot parameters" (`vtol.param`), the wiring
  "harness" (`harness.csv`) and the "first-flight checklist".
- **The build guide** (`guide/guide.html`): the assembled aircraft rendered
  from `cad/assembly.stl`, 28 plates and every check with its margin.
- The geometry row: interference, "50 solids, 1225 pairs, none overlapping".
  "Every pair, not a hand-picked list."

## 4:00 to 5:00: why believe it

- "Every check names its source, unevaluated means fail, and NaN fails. The
  Lean proofs show that no check passes on missing evidence. The verify script
  now also fails if any theorem rests on an axiom beyond the standard three."
- "We ran each domain against a real vehicle under the same strict checks."
  Use the eval table in the README as it stands; it is marked "under
  revision" until review 3's fixes are re-run. Don't quote numbers from
  memory.
- Close: "Describe the vehicle; get one you can build, and the reasons it
  flies."

## If something breaks

- The 3D view shows a close-up: press "fit" (or "iso") above the view.
- A judge draws anyway: the app answers "markup is replayed at the
  recorded step". Say "this recording has no markup step; the live agents take
  a drawing in a real run".
- No wifi: nothing changes. The replay is local.

## Likely judge questions (short answers)

- "Is it real or scripted?" It is a real recorded run: the agents' actual
  calls, recorded by `wright run`, $0.16.
- "What stops the AI from making up a number?" Agents can only call tools,
  and checks are a closed enum. Unevaluated means fail. The person's stated
  numbers are locked, and that includes the design's copy of them.
- "Who is it for?" Hobby VTOL builders, student rocket teams and school
  balloon launches. See brief.md for the evidence.
