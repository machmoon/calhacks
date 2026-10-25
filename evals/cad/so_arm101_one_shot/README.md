# One-prompt SO-ARM101 CAD attempt

This runner calls Wright's existing `CodexTransport` once with exact
`gpt-6-astra`, low reasoning effort and saved ChatGPT subscription authentication.
It offers one host tool, `submit_candidate`, which returns a FreeCAD Python script.
The model has no shell/file/network tools. There is no API-key or model fallback.

This track assembles and articulates **provided official STL mesh parts** using
their official URDF. It does not recreate original solid part feature histories,
and importing the preassembled official STEP is not allowed. Scripted reference
success is not model success. The first candidate is retained and executed once,
without oracle feedback or human repairs.

Reproduce from the Wright repository root (use a new output directory per attempt):

```sh
CARGO_TARGET_DIR=/tmp/wright-astra-target CARGO_BUILD_JOBS=2 \
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=0 \
  cargo build --manifest-path evals/cad/so_arm101_one_shot/Cargo.toml --offline
python3 evals/cad/so_arm101_one_shot/freeze.py out/so-arm101-one-shot/frozen
/tmp/wright-astra-target/debug/wright-so-arm101-one-shot \
  out/so-arm101-one-shot/frozen/request.json out/so-arm101-one-shot/attempt
python3 evals/cad/so_arm101_one_shot/execute.py \
  out/so-arm101-one-shot/frozen/inputs out/so-arm101-one-shot/attempt
```

`freeze.py` verifies pinned asset hashes and copies only the official URDF,
thirteen independent STL files, license, public contract and source manifest.
It hashes the entire prompt/request/input set before dispatch. It rejects reuse
of a frozen destination. The Rust executable refuses to overwrite an existing
attempt and saves request, response, usage-bearing response and submitted script.
The public contract is maintained separately by the evaluator owner under
`evals/cad/so_arm101/public_contract.json`; no other evaluator files enter inputs.

`execute.py` runs the script with the installed FreeCAD interpreter, a 300-second
limit and a macOS sandbox. Repository reads are denied except the public input
and current output directories. Source writes, network and child-process creation
are denied; all writes are confined to the attempt directory. The sandbox control
test verifies source-read/output-write access, hidden-sibling read denial, source
write denial and refusal of a second execution. Input hashes are rechecked after
execution. Syntax/runtime/timeout failures are retained, not repaired here.

Candidate execution exit zero is **not** an evaluation pass. The independent
fresh-process evaluator must verify coverage, native persisted controls, FK and
mesh preservation, including its negative controls. Limit metadata is checked;
native enforcement of limits is not assessed. Motion collision coverage must be
reported separately. The result is one task-specific attempt, not a success rate.

This is reusable evaluation-local integration with Wright's subscription transport
and native FreeCAD runtime. The minimal future CLI integration is a command that
calls this same frozen-request transport and isolated host executor; no alternate
agent/provider or broad platform rewrite is needed. It is not yet a production
`wright` CLI subcommand.

## Recorded result — 2026-10-24

Prompt-development iteration 1 failed: the model chose unsupported object type
`App::Feature`, so no native artifact was produced. A separate FreeCAD 1.1.4 probe
verified that `App::DocumentObjectGroup` supports the required custom Angle fields.
Only this factual API note was added to the prompt; the failed script was not
edited or supplied as a repaired solution.

Iteration 2 used one fresh Astra call and one unmodified script execution. It saved
`out/so-arm101-one-shot/attempt-2/assembly.FCStd` in 2.75 seconds. Independent
fresh-process evaluation passed all seven gates: coverage, source mesh geometry,
controls/limit metadata, sampled articulation, joint axes, fresh reopen/edit and
native dependencies without custom proxies. The original candidate stayed unchanged.
Full evidence is `out/so-arm101-evaluation/actual-attempt-2/result.json`; compact
hashes, outcomes, token counts and qualifications for both attempts are in the
checked-in `result.json`.

This is a successful **second prompt-development iteration**, not first-attempt
success or a measured generalization rate. It preserves 17 official mesh instances
and six persistent controls; it does not generate original solid CAD features.
One source-derived mesh is non-watertight (`Visual_shoulder_link_1`). Collision
clearance and native out-of-range enforcement were not assessed. The model's
headless preview was unavailable; any separately generated host preview is labeled
as such and does not change the submitted candidate.

Exact unmodified generated scripts are retained in `recorded_candidates/` for
review, including the failed first candidate. Their SHA256 values match the
compact result record. They are evaluation evidence, excluded from the public
model input bundle and from training data. Loading these recorded scripts is
not another model-generation attempt.
