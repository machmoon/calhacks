# Astra CAD subscription integration

Wright's CAD specialist uses `gpt-6-astra` at low reasoning effort through the
existing ChatGPT-authenticated Codex CLI when the run uses the default Claude
subscription transport. Orchestrator, other specialists and reviewer keep their
existing model/transport. Explicit API and fake transports retain their routing.
Set `WRIGHT_CAD_TRANSPORT=codex` to explicitly select Astra for CAD with any lead
transport, or `WRIGHT_CAD_TRANSPORT=inherit` to explicitly retain the lead's route.
Unknown values fail instead of silently choosing a provider.

`crates/wright-agents/src/codex.rs` performs each turn as stateless structured
output. It accepts only offered host actions and object-shaped JSON inputs.
Wright's existing agent tool dispatcher and persistent native CAD session execute
and validate those proposals. Codex never receives a direct CAD execution tool.
Native tool events in its stream are rejected. The adapter passes an explicit
read-only sandbox, disables shell/apps/plugins/hooks/browser/multi-agent tools,
and uses a disposable working directory. It ignores user config without touching
saved authentication or global settings.

The adapter checks `codex login status` for ChatGPT authentication, removes API
key/base URL environment overrides in the child, and pins the OpenAI provider and
ChatGPT login method. It never reads/copies credential files. A failed login,
model request, malformed response, native tool attempt, or timeout fails the
specialist; there is no model or API billing fallback.

The official bundled macOS CLI is preferred when present because PATH may point
to a wrapper that injects flags. `WRIGHT_CODEX_BIN` explicitly overrides the binary.
`WRIGHT_CODEX_TIMEOUT_S` bounds each turn including authentication preflight
(default 300 seconds); the existing specialist turn limit bounds the host loop.
PNG/JPEG screenshots and CAD renders are attached through the CLI image flag.
Only base64 image blocks are accepted, including images inside tool results.
Remote URLs are rejected without fetching. Attachments are limited to four images,
8 MiB each, 16 MiB combined and 32 million pixels each. Declared MIME type must
match a recognized PNG/JPEG header; malformed base64 and unsupported formats fail.
The CLI image decoder remains responsible for complete image decoding. Numbered
markers preserve transcript ordering while pixels are passed as attachments.
Files live in a private temporary directory (mode 0700 on Unix), removed when
the request finishes or fails. The CLI usage is
mapped into Wright's token accounting; run USD values remain conservative
engineering budget estimates using the existing unknown-model rate and **are not
subscription charges or a claim about Astra pricing**. The API credit ledger is
not charged for this transport. Account usage limits still apply.

Official behavior reference: [Codex non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
documents saved CLI authentication, JSONL events, structured output schemas,
ephemeral runs, read-only sandbox and ignoring user config.

Verification commands (from repository root):

```sh
cargo test -p wright-agents codex::tests --lib
cargo test -p wright-agents codex::tests::live_astra_subscription --lib -- --ignored
cargo test -p wright-agents codex::tests::live_astra_visual_cad --lib -- --ignored
```

The ignored test consumes signed-in subscription usage and writes a normalized
response to `out/astra-subscription/rust-response.json` (override with
`WRIGHT_ASTRA_SMOKE_OUT`). The live test creates a native box, asks Astra to propose its length edit, executes
the proposal through Wright’s typed native session, checks exact volume and
parameter values, and saves FCStd/STEP/GLB. It verifies the connection and edit
path; it does not establish complex-assembly CAD proficiency.

## Verified on 2026-10-24

The official bundled CLI `0.164.0-alpha.11.1` reported ChatGPT login and accepted
explicit `gpt-6-astra` requests. The real Rust adapter test passed: Astra proposed
`set_parameter` for `AstraBox.Length = 20`, Wright executed it in native FreeCAD,
and inspection returned bounds `[0,0,0]`–`[20,10,10]` mm and volume 2,000 mm³ within
1e-6. FCStd, checked STEP, and GLB were saved under
`out/astra-subscription/native-78598-1/`; the normalized proposal, usage and native
result are in `out/astra-subscription/rust-response.json`. The request used 14,198
input and 68 output tokens. This shows successful explicit-model request handling;
the CLI stream does not provide independent server-side model attestation.

Six adapter tests pass, including fake subprocess execution, timeout, API-key
login rejection, action/parser safeguards, and routing. The entire agent library
passes 27 tests with the live test normally ignored; that live test was separately
run and passed. Tests used `/tmp/wright-astra-target` with debug info disabled to
avoid contention with unrelated shared builds. No desktop rebuild is claimed.

The image extension also passed a live Astra request using the existing Pi fixture
PNG. Astra identified four upright standoffs and proposed `VisualBox.Length = 40`
for a requested count-times-ten test. Wright executed the typed proposal, verified
4,000 mm³, and saved native/export artifacts. Evidence is
`out/astra-subscription/visual-response.json` and `visual-native-*/` alongside it.
This single annotated preview is a transport smoke, not a measurement of general
visual counting, hidden-part recognition, or reconstruction accuracy. JPEG headers
are unit-tested; the live visual request used PNG.

After image support, eight adapter tests and all 29 regular agent-library tests
pass; two live subscription tests are ignored in ordinary runs and were explicitly
run separately. Image tests cover nested result ordering, private directory mode,
cleanup, remote URL rejection, format mismatch, corrupt base64, attachment count,
encoded byte limit, pixel bounds and JPEG frame parsing. The official
[CLI command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli)
and installed `codex exec --help` document the image flag used by the adapter.

## Images in actual CAD delegation

`RunConfig.reference_blocks` accepts initial user reference text/image blocks.
They enter the lead's first turn and the delegated CAD context. The existing
studio `Event::Feedback` route also forwards its loaded sketch/render image and
view/note text to the next CAD delegation; it no longer stops at the lead.

The harness retains only the newest user-image batch and newest successful CAD
render batch, in source arrival order, with provenance labels. It does not copy
unrelated tool images or the full conversation history. A newer reference batch
explicitly replaces the previous batch; text-only feedback does not erase the
reference. Geometry-changing CAD actions invalidate retained CAD render snapshots.
Only CAD delegates receive this retained visual context. Both per-batch and
combined limits use the adapter's same image validator; invalid or over-limit
context returns an explicit delegate error instead of dropping attachments.

`ToolOutput::with_blocks` transports genuine multimodal tool results while legacy
text-only results remain strings. Native CAD action results may supply an optional
`content_blocks` array containing image blocks; these are delivered to the current
agent and retained for subsequent CAD delegation. Image bytes are excluded from
the textual result serialization to avoid duplicating base64 in the prompt. The native `render` action now produces this contract directly from selected
FreeCAD shapes using bounded, depth-buffered tessellation. It returns PNG pixels
and a geometry fingerprint without editing the source shapes. Existing studio
render/markup screenshots also use the feedback route above.

A focused offline test executes the real harness delegation twice and routes the
CAD request through `CodexTransport` with a fake CLI process. It proves that initial
reference pixels reach the actual `--image` argument, and that later studio
feedback replaces the older reference with its note preserved. Separate tests
cover CAD render provenance/order, invalidation after edits, combined limits,
unsupported images, and backward-compatible tool-result serialization. After this
extension 32 regular library tests pass (two subscription tests remain ignored by
default and have separately passed earlier).

The complete live render loop also passed: the real CAD delegate made two explicit
Astra requests. It first proposed native `render` for `VisionBox` at 320×240; the
next request contained the real PNG plus its geometry fingerprint and dimensions.
Astra then reported the visible purple rectangular block and explicitly separated
the image from fit/hidden-geometry verification. Evidence:
`out/astra-subscription/native-render-loop.json` and
`out/astra-subscription/render-loop-82230/native-render.png`. The test is
`cargo test -p wright-agents live_cad_delegate_requests_native_render --lib -- --ignored`.
It uses the signed-in subscription and is ignored in ordinary test runs.

The existing harness integration fixture still asserted Haiku/low for the default
generalist despite the existing `Models::standard` choosing Opus/medium. Its
scripted specialist replies and explicit assertions were aligned with that current
default; no production model default was changed by this correction. The native
CAD integration and all eight regular harness integration tests now pass.

## First actual VTOL diagnosis

A read-only, task-specific evaluation used the real reference
`out/tsung-vtol-reference/assembly.FCStd`, selecting `BoomRight`, `FinRight`, and
`TopStabilizer`. The host opened the document; Astra independently requested
`inspect`, `render`, and `check`, then reported its findings. Expected findings
were not included in its task. The evaluation wrapper restricted offered actions
and rejected any mutating proposal before execution.

Astra correctly reported the two native material penetrations:
BoomRight–FinRight **404.8847174196096 mm³** and FinRight–TopStabilizer
**354.6282067990029 mm³**. It also correctly described the other pair's
**138.07107770999687 mm separation lower bound**, without calling it an exact
measured distance. It did not mislabel the checker's disconnected components as
air gaps at penetrating interfaces, or infer joint approval from zero required
contacts. A fresh native session reproduced the pair evidence; the source FCStd
remained byte-for-byte unchanged.

The run used two Astra turns (45,341 input tokens and 412 output tokens). Evidence
under `out/astra-vtol-diagnosis/run-84035/` includes the full request/response trace,
events, original report, model-visible native PNG, independent check, and explicit
claim comparison. This is one initial task-specific diagnosis, not a broad model
quality benchmark, a repair, or aircraft/flight completion. The unrelated design
optimizer reports `RoundLimit` because this test deliberately does not run a
complete design/review cycle; the diagnostic assertions pass independently.

Reproduce with:

```sh
cargo test -p wright-agents live_astra_vtol_right_tail_diagnosis --lib -- --ignored --nocapture
```

This consumes the signed-in subscription and writes a new per-process evidence
directory. Its independent native oracle runs only after the model report and is
never supplied as task context. The trace is retained as evaluation evidence; no
training dataset or model weights are updated by this test.

The held-out v1 joint assessment also completed without supplied answers. Astra
identified the approximately 0.2 mm boom clearance, the already-touching split
faces, and the resulting lack of rigid closing travel; it explicitly did not
claim a working clamp. It chose to require both boom/clamp contacts, and an
independent replay reproduced zero overlaps but two unmet contacts. Detached native
shape probing confirmed that a 0.05 mm closing translation already creates
28.692120 mm³ interference between the halves while 0.15 mm boom clearance remains.
The source was unchanged. The original v1 public report was read only after the
model finished and agrees with this limitation. Full evidence and original report:
`out/astra-vtol-clamp-v1-diagnosis/run-87326/`. This assessment used three Astra turns;
it is one mechanical reasoning case, not broad CAD proficiency evidence.

A subsequent bounded repair attempt on that v1 file passed an independent native
oracle. With only target dimensions and protected geometry supplied, Astra chose
two existing parameter edits, inspected/rendered the result and saved four outputs
in six turns (168.21 seconds). Fresh reopening measured coaxial R5 seats, zero
boom overlap, boom gaps of zero to numerical tolerance, and a 0.500000 mm split
gap. Boolean material comparisons found no change to the boom, stabilizer or fin
outside the allowed root region, including its top seat. The original input was
unchanged. Both final cuts retained live editable dependencies; transient radius
probes changed their volumes and restored them without saving the artifact.

Evidence is in `out/astra-vtol-clamp-repair/run-88837/`: full transcript, events,
model-visible PNG, original report, native files, `evaluation.json` and
`independent-repair-oracle.json`. No manual correction was applied to model output.
The reproducible checker is `evals/cad/training/verify_clamp_repair.py`; the ignored
Rust test is `live_astra_vtol_clamp_constrained_repair_attempt`. Its execution test
alone does not mean repair success—the independent oracle supplies that verdict.
This is one successful nominal geometry repair, not general complex-assembly
proficiency or a fully functional joint. Fasteners, preload, friction, compliance,
load capacity and flight readiness remain unverified. The six calls reported
327,564 input and 1,089 output tokens; they used full-document inspection before
the newer lightweight/scoped inspection workflow, so this is not its cost measure.

The desktop dependency integration also passed `cargo check -p wright-desktop
--offline` on 2026-10-24, using the isolated `/tmp/wright-astra-target` target,
two build jobs and disabled development debug information. This is a compile
check, not a launched desktop or an end-to-end GUI verification.

The typed CAD API now includes native linked physical mirrors. Source dimensions
propagate to the reflected instance after reopening; source and mirror remain
independently selectable physical outputs. This lets the CAD delegate construct
bilateral parts through the bounded API instead of requiring a custom script.

The subsequent read-only full-aircraft case used the stable 23-part bilateral tail
assembly and completed in four Astra turns. It used two lightweight discovery
pages, two scoped inspections and two native renders. After the report, a fresh
native replay matched all three model-selected checks: 253 whole-assembly pairs
with zero overlaps and three disconnected groups; ten wing/boom/pod pairs with
four unmet model-declared contacts; and 28 right-tail pairs with zero overlaps,
no unmet declared contacts and one connected group. The source SHA-256 remained
`356348efc7990cc9896862d8c636c689c710799924d3bd06f57bc5020f0444e0`.

Astra correctly qualified the wing–boom 20.3536 mm and wing–pod 0.353595 mm values
as separation lower bounds under an explicit direct-attachment assumption. It
separated nominal hardware/contact from retention and left detailed left-side
retention, preload and service access unassessed. This was not exhaustive: open
nut-pocket capture and incomplete exterior-envelope internal structure remain
additional limitations. The original assembly review was read only after the
model finished. Evidence is in
`out/astra-vtol-full-assembly-diagnosis/run-93531/`, including the full transcript,
report, independent replay, manual claim review and two model-visible renders.
Reported usage was 159,517 uncached plus 24,832 cached input tokens and 1,743 output
tokens. This differs in task scope from earlier cases and is not a controlled
cost-reduction comparison. The full 253-pair check responses were still replayed
in context. The ignored case is `live_astra_vtol_full_assembly_diagnosis`; native
replay uses `evals/cad/training/replay_diagnosis_checks.py` after model completion.

A fresh compact-check-guided run on the same source also passed independent full
replay: `out/astra-vtol-full-assembly-compact-diagnosis/run-96086/`. The replay
always computes all native pairs, then verifies the complete required compact
subset, omitted/contact counts, components and measured values. Negative controls
reject omitted clashes or declared contacts, false counts/components, duplicate
pairs and altered measurements; default full-response comparison still requires
every pair. Nine corpus/replay tests and 32 agent tests pass (eight live cases
ignored by default).

Both full-aircraft runs used four Astra turns. Input usage including cache was
184,349 in the prior run and 114,474 in the compact run, 37.9% lower for this pair.
Output increased from 1,743 to 2,170 tokens; live test plus initial native oracle
time increased from 217.05 to 259.31 seconds. These are two task-specific runs,
not a controlled or general efficiency comparison: the compact run checked both
clamps, while the prior targeted check covered the right tail. Compact responses
retained 0/12/16 pair details from full native sets of 253/78/91, with omissions
explicit. Astra correctly disclosed the omitted full-selection details and the
bilateral clamp check's disabled connectivity requirement. All three compact
responses match fresh full native replay; source hash is unchanged. The report
continues to distinguish geometric contact from retention and does not establish
preload, internal structure, serviceability or flight readiness.

Reproduce with ignored test `live_astra_vtol_full_assembly_compact_diagnosis`, then
run `replay_diagnosis_checks.py` on its new evidence directory. Execution success
alone is not the independent replay verdict. Comparison and manual claim review
are stored beside the complete trace and renders in the evidence directory.

Native `set_parameter` and `set_profile` now accept optional `summary:true` at the
worker boundary. The underlying native operation still validates the full
supported document and commits or rolls back transactionally. Omitting summary
preserves the full legacy description list. Compact output identifies the edit,
actual parameter value or resulting profile geometry count, validated-object
count, and transitive reverse-dependency names. Those names are inspection
candidates, not proof each shape changed. `assembly_contacts_checked:false` is
explicit: scoped-inspect final physical outputs and run a fresh fit check.

`cad/freecad/test_edit_summary.py` verifies actual metadata, dependent cuts and
mirrors, stale contact-evidence invalidation, invalid dependent-cut rollback,
unrelated invalid geometry rejection, profile rollback and legacy return types.
Evidence: `out/freecad-edit-summary/result.json`. This change has offline native
verification; no new paid model run was used to measure repair-context savings.

Existing polygon lofts can now be converted without retransmitting their section
coordinates through typed `convert_spline_loft`, for example:

```json
{"source":"WingRight","name":"SmoothWingRight","split_index":31,"sampled_tolerance_mm":0.2}
```

The conversion preserves the source and creates a separately named native spline
loft. It reads actual world-coordinate boundary points from fully constrained,
line-only Wright sketch sections of a rooted solid ruled `Part::Loft`. The
existing 2–16 section, 8–256 points/section and 2,048 total-point bounds still
apply. It copies supported linear shared-dimension translation expressions onto
corresponding new sections and records source BREP/coordinate hashes, source
section names, preserved expressions and sampled fit evidence. Unsupported source
geometry, transformed parent/loft placements, expressions outside that supported
subset, collisions in result/section names and invalid spline fits are rejected;
creation and full-document validation are transactional.

This is a coordinate snapshot with copied layout controls, not an associative
link to later source profile-point edits. Source point edits do not regenerate the
spline curves. The sampled curve-to-source-polyline deviation is not a certified
continuous bound, and the smoothed solid is not exactly the original polygon
solid. Inspect/check the intended new physical outputs before export.

Offline validation uses the actual reference wing: 11 sections and 660 polygon
edges become 22 native spline section edges; all 11 span expressions survive,
span edits update both versions after checkpoint/reopen, source-coordinate edits
do not change the converted snapshot, and the original FCStd bytes stay unchanged.
`cad/freecad/test_spline_conversion.py` covers this plus rollback and unsupported
inputs; `actual_reference_wing_converts_and_resumes_through_typed_session` covers
the Rust Session path. Evidence: `out/freecad-spline-conversion/result.json` and
`converted.FCStd`. No subscription model run has yet exercised this conversion.
