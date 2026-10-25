# CAD geometry evaluation

Run from the repository root with the CAD Python runtime:

```sh
PYTHONPATH=cad python evals/cad/openrocket_parts.py
```

Requires build123d 0.13.0 and cadquery-ocp-novtk 8.0.1.1.0 (the existing temporary
runtime is recorded in `docs/cad-progress.md`). The command exits **1** until every
source component is supported and passes. Do not turn unsupported components into
passing checks or use the supported-only ratio as an overall score.

| Pinned OpenRocket example | Component definitions passing | Unsupported | Numeric failures |
|---|---:|---:|---:|
| A simple model rocket | 1 | 10 | 0 |
| Dual parachute deployment | 4 | 14 | 0 |
| Two stage high power rocket | 41 | 6 | 0 |

The benchmark rebuilds explicit-dimension tubes, rings, bulkheads, and square
sheet fins including mounting tabs. It compares exact B-rep volume to independent
analytic volume, bounding dimensions to source dimensions, then reimports the
STEP file and verifies volume and validity. These are individual component
checks, not full assemblies. Repeated instances are not multiplied into the
count. Curved fin sections, automatic dimensions, noses, rail buttons and recovery
hardware currently remain unsupported. Missing source dimensions are not guessed.

Reference `.ork` files are unmodified OpenRocket examples, licensed separately
under GPL-3.0-or-later; see `references/LICENSE.OpenRocket.txt`. URLs, immutable
commit IDs and SHA-256 digests are in `references/sources.json`. The evaluation
verifies those digests before reading any reference. `result.json` includes every
source component, including exclusions and their reasons.

No flight, structural, fabrication, full-import or college-rocket completeness
claim follows from passing an individual part check.

## Native assembly screening

The separate native Rocket Workbench import study is recorded under
`out/freecad-openrocket-verified`. To screen every exported material solid with
Wright's production interference checker, run:

```sh
cad/freecad/run.sh evals/cad/openrocket_assembly_fit.py
python3 evals/cad/openrocket_source_overlap.py
```

The current fixtures contain 10, 19 and 50 solids, giving 45, 171 and 1,225 checked
pairs respectively. The screen detects 0, 4 and 23 material overlaps. Each case
also passes a negative control that deliberately duplicates a solid at the same
position. Source exports remain unchanged. Recovery and mass components remain
explicit omissions (3, 2 and 3); required connections are not specified or verified.
`out/openrocket-assembly-fit/result.json` contains all pairs and source hashes.
This is a screening result, not a passing full-design benchmark.

The source-overlap check independently reads the pinned original two-stage `.ork`
file. Its first coupler/bulkhead pair necessarily intersects by 1,300.373918 mm³
from the specified radii, thickness and bottom-relative position. That overlap
exists in the source dimensional definitions; silently shrinking or moving the
imported component would damage import fidelity. A manufacturing design requires
an explicitly derived interface. This finding does not classify all other overlaps.

## Derived stepped-seat proposal

Run `cad/freecad/run.sh evals/cad/rocket_stepped_bulkhead.py` to reproduce the original OpenRocket coupler/bulkhead clash and build a separate parameter-linked stepped seat with provisional0.15mm radial clearance. Evidence lives in `out/rocket-stepped-bulkhead/result.json`; four nominal/edit/restore/reopen states pass geometry/contact checks. Original source score and files remain unchanged. Checked two-solid AP242/native/GLB exports are in `out/rocket-stepped-bulkhead/export` (`export-result.json`). Positive axial retention, recovery attachment, load sizing and manufacturing tolerances are unresolved; this proposal does not turn the reference import into a complete or flight-ready rocket.
