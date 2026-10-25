# CAD-only reference corpus for the SO-ARM101 benchmark

No weights have been trained. No model success rate is measured here. This initial
corpus teaches/tests native geometric construction and dependency-preserving edits;
it contains no robot policy, manipulation video, firmware or physical experiment.

`generate.py` produces two training references for single-sleeve coaxial joints and
one double-sleeve regression holdout. These are authored analytic examples, **not
SO101 component replicas or dimensions**. Each example creates native features,
binds bore/pin radii to shared dimensions, then changes the pin radius. Its complete
actual typed requests/results are saved. Independent measurements check radial gap,
zero common volume, analytic sleeve volume, live expression and native reopen.
An undersized bore and a removed dependency are retained as detected negative
controls, never as successful operation traces. This is nominal geometry, not a
solved revolute joint or mechanical readiness claim.

```sh
python3 -m unittest discover -s evals/cad/training/so_arm101 -v
cad/freecad/run.sh evals/cad/training/so_arm101/generate.py
```

Output: `out/so-arm101-cad-corpus/{train,validation,test}.jsonl`, a manifest and
editable per-case FCStd files. Validation is currently empty. Train and held-out
references use separate topology groups. They share a generator, so this public
regression holdout is a weak generalization test, not a secret benchmark.

Each v1 record contains `id`, `split`, `lineage_group`, `origin`, `model`, `task`,
`source`, `trace` and `verification`. `trace` contains ordered `{action,arguments,
result}` values returned by the real worker; it is not an invented chat. Source
identity hashes the semantic case specification independently of record ID/split;
the generator hash is separate. Verification records measurements, binary checks,
FreeCAD version, final native path/hash and explicit scope. Curation rejects failed
checks, missing results, duplicate IDs, cross-split source/family overlap, an
invented model attribution and SO101 benchmark geometry in training.

## Sources and separation

`sources.json` records the discovery/rights decisions. The official RobotStudio
SO101 STEP repository is Apache-2.0 at the pinned revision; upstream license and
hash inventory are maintained by the CAD-source importer under
`cad/projects/so-arm101/upstream`. All exact SO101 assets, source-derived variants,
assembly targets and their successful solutions remain benchmark-only. No native
parametric FreeCAD history has been established for those upstream STEP files.

GenCAD-Code/CAD-Coder, DeepCAD and SketchGraphs are discovery candidates, not
silently imported training data. Their representation differs from Wright's native
assembly actions, and repository code permission is not a substitute for dataset
rights. SketchGraphs explicitly says original creators retain sketch copyright.
No bulk datasets were downloaded. Analytic references copy no external geometry;
workspace-generated code/data still require redistribution/provider-use review.

## Existing Wright training integration

`crates/wright-train` already exports genuine harness `messages` and `tools`, with
`model`, `lossy`, and run-level verifier/reviewer labels. Its `resolved` label needs
both final engineering checks and harness completion. That exporter is retained
unchanged: these scripted reference traces must not masquerade as real Astra
transcripts or fabricate `state.json`/`run_finished` events.

For actual subscription Astra experiments, preserve the harness's exact transcript,
then attach a CAD-specific evidence record and enforce the same source/family split.
A run-level pass alone is insufficient to label every CAD action correct. The next
reusable step is a reviewed adapter joining genuine transcript IDs to independent
CAD checks; provider training conversion is intentionally not emitted here. No
training/upload, GPU purchase, or API-billing fallback was performed.
