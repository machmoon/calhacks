# Wright Studio: heuristic review

Reviewed 2026-10-24 against `wright-studio --replay crates/wright-studio/fixtures/demo/events.jsonl`
(at 127.0.0.1:7979, 1495x812 viewport, dark and light themes, mouse and keyboard).

Media (all in `docs/product/media/`):

- `wright-01-replay-full-run.gif`: restart, then the full replay. Shows findings 1 and 3: a stale model, then an extreme close-up.
- `wright-02-markup-send-to-agents.gif`: Show Airframe v2, then Iso, Mark up, circle the boom, arrow, note, Send. Shows finding 4: waiting forever.
- `shot-mid-run-v1-failing.jpg`: the strongest frame in the app. v1 is failing 3 of 11, the reviewer explains, and the user's markup is answered.
- `shot-end-of-run-boom-clamp.jpg`: the run ends on a 44 mm clamp, not the aircraft (finding 2).
- `shot-light-theme.jpg`: light theme, with blank render placeholders in the timeline.
- `markup-sample-sent.png`: the PNG the studio actually sends to the agents.

## What already works (keep it)

- The layout makes sense: timeline on the left, the object in the middle, checks at the bottom. The run request is in the header.
- Pass and fail use a glyph, a word, and an Okabe-Ito blue/vermillion pair, so colour-blind users can still read them. "no evidence" has its own glyph (Ø). This is the product's thesis, made visible.
- The check table has a CHANGE column with ▲/▼ deltas between reports, so the effect of the feedback can be measured.
- In the markup tool, the pen/highlight/arrow/erase tools and their keys come from excalidraw/tldraw (cited in `markup.js`). The canvas freezes the view and records azimuth/elevation. Cmd+Enter sends. Esc cancels.
- The orchestrator's message tagged "re: your markup", and "answered by orchestrator (#19)" on the feedback row, close the loop.
- Keyboard: every control is reachable, there are 2px focus outlines, M opens markup, 1-4 switch views, Space pauses the replay.

## Findings, ranked by impact on the 5-minute demo

| # | Severity | Finding | Heuristic | Fix (sent to main) |
|---|---|---|---|---|
| 1 | Critical | The camera never refits when the model changes (`viewer.js` `load()`: `keepCamera` is true whenever a model is showing). A clamp-sized camera on the 1.7 m airframe shows one grey face. | Visibility of system status | Refit unless the bbox radius is within ±30% of the previous one |
| 2 | Critical | Auto-follow ends the run on "Boom clamp (STL)", not Airframe v2. The comment says small parts are skipped, but the code doesn't skip them. | Match with the real world | Follow only meshes whose radius is at least 50% of the current one |
| 3 | High | Replay Restart leaves the previous run's model in the viewer. | Consistency | Clear the viewer and the picker on `run_started` |
| 4 | High | A markup sent after the run ends shows "waiting for a response" forever. In replay, the judge can't be the one who draws. | User control, feedback | Pause the replay at the recorded feedback event with "Your turn…" and substitute the user's sketch |
| 5 | High | There is no hand-off. The BOM (CSV) and G-code are small links in the timeline. There's no total and no wiring or params entry. The ending is a mono line, "endurance (min): 49.5". | Recognition over recall | A "Ready to build" panel on pass |
| 6 | High | The BOM links go to store homepages. | Trust | Product URLs plus a `retrieved` date |
| 7 | Medium | The fixture says `slicer=OrcaSlicer`. Orca isn't installed; the real slicer is PrusaSlicer 2.9.6. | Honesty | Re-record the fixture |
| 8 | Medium | Headline numbers (endurance, cost, mass, print time) are spread across check rows. | Aesthetic and minimalist design | A results strip under the header, each number against its requirement |
| 9 | Medium | Check names are snake_case ids. The tightest passing margin isn't marked. | Match with the real world | Human names, with the id secondary. A "tightest" tag. |
| 10 | Medium | The render PNGs and the recorded feedback PNG in the fixture are blank placeholders, which show as dark empty boxes. | Aesthetic | Real renders, or hide render rows |
| 11 | Low | Contrast: light `--faint` is 3.1:1 and dark `--faint` is 3.75:1 (they carry token counts, file paths, hints). Light FAIL text on the fail row is 4.19:1. All three are below AA 4.5:1. | Accessibility | `#6b747e` light (4.75), `#8a939c` dark (5.7), fail `#a64400` (5.3) |
| 12 | Low | Agent names are coloured (reviewer red, structures orange), close to the fail vermillion. A red "reviewer" reads as an error at a glance. | Consistency | Keep agent names neutral, or use hues away from orange/red |
| 13 | Low | Markup's "Clear" sits where "Mark up" was. A double-click clears the strokes (Undo does restore them). | Error prevention | Put Clear left of Undo, or ask before clearing more than 3 strokes |
| 14 | Low | Feedback is written into `fixtures/demo/feedback/`, which pollutes the repo fixture on every demo. | n/a | Point `--artifacts` at a temp copy for demos |

## First 5 minutes, as a newcomer

- 0:00. The header text is clear, and "AIR" plus the request tell me what is being built. With no run, the empty state shows a CLI command, which is right for a developer but meaningless to a judge. For the demo, start with the replay already loaded.
- 0:30. The timeline moves fast. Tool rows with mono args are dense. The agent sentences (orchestrator, reviewer) are the readable parts.
- 1:00. "Check report 1 · 8 pass × 3 fail" lands well, and the FAIL rows are easy to find.
- 2:00. Mark up is the obvious primary button (accent, top right) and the freeze-and-draw idea needs no explanation.
- 3:00. After the fix, the change column tells the story. The viewer, though, is showing a clamp or a close-up (findings 1 and 2), which undercuts the moment.
- 4:00. "run passed / endurance (min): 49.5", and then I don't know what I'd take home. That is finding 5.

## Accessibility summary

- Colour-blind: pass/fail is safe (glyph, word, blue/vermillion). The ▲/▼ deltas use direction, not only colour. Good.
- Contrast: three tokens fail AA (finding 11). Everything else measured 4.75 to 7.7:1.
- Keyboard: complete. Focus is visible. The markup canvas has no keyboard way to draw, which is acceptable as long as the text note alone can be sent (it can).
- Motion: `prefers-reduced-motion` disables the animations.
- Screen reader: the timeline is `aria-live="polite"`. During a replay that will announce constantly. Consider announcing only check reports and agent messages.

## Prior art note

Markup follows excalidraw/tldraw (sourced in `markup.js`). The "Ready to build" panel and the results strip are proposals that came from the user research (brief.md). No open-source design tool was found and read for them, so treat their layout as unprecedented and test it.
