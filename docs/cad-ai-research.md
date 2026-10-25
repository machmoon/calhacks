# AI CAD research for Wright

Research snapshot: 2026-10-24. Primary papers, author repositories and official project pages were checked online. No model weights, datasets or new dependencies were downloaded. Recommendations below are proposals, not measured Wright improvements.

## Recommendation

Prioritize **explicit assembly interfaces, native constraint execution, and verified edit/search traces** over training a general text-to-shape model. Wright already has the difficult execution foundation: persistent FreeCAD documents, typed operations, rollback, dependent features, collision/contact checks, checkpoint recovery and independently checked exchange. Its present gap is design intent and dense assembly reasoning: a measured contact graph is not a joint graph, an editable solid is not a complete component, and a collision-free final pose does not prove that a part can be installed.

This assessment uses `docs/cad-progress.md`, `cad/freecad/native.py`, `cad/freecad/worker.py` and the evaluator inventory. The documented structural aircraft still has 28 clashes, 12 unmet direct contacts and 23 contact groups; OpenRocket coverage still omits recovery/mass definitions. Neither a new model nor a passing shape-validity metric can erase those omissions.

## What the research actually contributes

| Work / date | Verified contribution and availability | Wright use and boundary |
|---|---|---|
| [JoinABLe, CVPR 2022](https://github.com/AutodeskAILab/JoinABLe) | Predicts mating B-rep faces/edges, derives joint axes and searches pose. Code and five pretrained models are published. [Code license is MIT](https://raw.githubusercontent.com/AutodeskAILab/JoinABLe/main/LICENSE); dataset has separate terms. Authors report 79.53% against 80% human performance on their task. | Best direct assembly precedent. Borrow candidate-entity → axis → pose-search decomposition. This is **a given pair of parts**, not complete aircraft design, BOM recovery or dense global assembly success. |
| [Fusion 360 Gallery assembly data, 2022](https://github.com/AutodeskAILab/Fusion360GalleryDataset/blob/master/docs/assembly.md) | Hierarchy, contacts and joints support different graphs; cross-splits prevent overlap with joint examples. [Paper supplement](https://chuhang.github.io/files/publications/CVPR_22_1.pdf) specifies noncommercial research availability. | Keep physical, hierarchy and kinematic edges separate. JoinABLe lists 514 MB preprocessed joint data versus 2.8 GB original joint data; pose search requires original geometry. Neither is appropriate to download casually with ~3 GB disk free. Check current per-artifact terms before use. |
| [AutoMate, SIGGRAPH Asia 2021](https://github.com/deGravity/automate) | Released assembly mating code/data. README requires proprietary Parasolid plus OpenCascade/CMake; dataset hosted separately on Zenodo. | Useful representation precedent, poor immediate dependency fit. Do not assume public code implies an all-open executable stack or blanket dataset permission. License/model availability not independently resolved in this review. |
| [Multi-part kinematic constraint prediction, CAD 2025](https://www.sciencedirect.com/science/article/pii/S0010448524001325) | Graph-based sequential constraint prediction incorporates all parts and existing constraints, including cycles; postprocessing selects next constraints. | Supports global graph reasoning beyond independent pairs. Public runnable code/weights were not verified; treat as algorithmic research, not a drop-in implementation. |
| [Linkify, July/October 2026](https://arxiv.org/abs/2607.01205) | Interface-aware assembly graphs; recomputed contact regions; masked-part retrieval using graph attention. Authors announce corrected interfaces and training pipeline. | Supports retrieving a missing spacer, mount or bracket from its neighbors' interfaces. It predicts a part class for retrieval, not a newly engineered assembly. Release contents/license were not audited; no reproduction claim. |
| [SketchGraphs, 2020](https://github.com/PrincetonLIPS/SketchGraphs) | 15 million constraint-annotated sketches, baseline generation/autoconstraint code. MIT code, but original sketch copyright remains with creators. Full raw data ~43 GB; sequence file ~15 GB. | Learn the separation of geometry and constraints; create a small Wright-owned corpus instead of downloading this dataset. Sketch constraints do not provide 3D assembly joints. |
| [BRepNet, CVPR 2021](https://github.com/AutodeskAILab/BRepNet) | Topological message passing on native B-rep structure, with published code; CC BY-NC-SA 4.0. | Geometry-aware interface ranking is more grounded than screenshots alone. Not a CAD generator or assembly solver; don't copy restricted implementation into a commercial product without suitable permission. |
| [Text2CAD, NeurIPS 2024](https://github.com/SadilKhan/Text2CAD) | Text-conditioned CAD sequences; paper describes ~170k models and ~660k text annotations derived from DeepCAD. Repository links data/model workflow. [CC BY-NC-SA license](https://raw.githubusercontent.com/SadilKhan/Text2CAD/main/LICENSE). | Relevant to prompt→part history, not hidden internal component recovery. Annotations derived from a limited construction corpus cannot demonstrate broad joint/assembly engineering competence. Training compute was not verified here. |
| [CAD-Recode, ICCV 2025](https://github.com/filaPro/cad-recode) | Point cloud→CadQuery code; released inference demo, weights and synthetic data. Qwen2-1.5B plus projection; one million synthetic sequences. [Paper](https://orbilu.uni.lu/bitstream/10993/67036/1/CADRecode_ICCV_2025.pdf) reports ~12 h on one H100 for its training setup. [v1.5 weights](https://huggingface.co/filapro/cad-recode-v1.5) are CC BY-NC 4.0. | Strong evidence for procedurally generated executable training examples. This is reconstruction of supplied geometry, not inference of unseen internals. Repository presents inference; do not describe it as a fully reproduced training pipeline. 1.5B weights alone need roughly 3 GB at 16-bit before runtime/other files: not a sensible local install now. |
| [Assemble Them All, SIGGRAPH Asia 2022](https://github.com/yunshengtian/Assemble-Them-All) | Released physics-based assembly-by-disassembly planning code/dataset for already modeled assemblies. | Add insertion/removal-path checks after static fit. Planning successful motion does not design joints, set manufacturing tolerances or validate strength. Dependency/license audit remains before integration. |

Dates identify publication/release periods, not proof that every linked repository revision is immutable. Pin commit and artifact hashes before any experiment. Paper performance numbers are author-reported and are not comparable across unlike tasks.

## Ranked experiments

### 1. Semantic interface graph and deterministic mate placement

Implement ports for a plane, cylindrical axis/seat, bolt pattern and named clearance envelope. Each port should have a stable semantic ID, owning feature/property, local frame, dimensions, fit interval, permitted degrees of freedom and provenance. Resolve geometry from the feature definition, not a persistent assumption that `Face7` stays the same after a topology change. Ambiguous matches must fail explicitly.

Add `mate`/`inspect_interfaces` operations around the existing transactional worker. Enumerate compatible ports, derive transforms, recompute, then reject overlap, wrong fit, unintended motion and violated prior mates. Keep required functional joints separate from measured touching surfaces. First fixtures: tube/bulkhead, shaft/bearing, rib/spar and tray/standoff. These exercise internals using existing geometry instead of arbitrary new silhouettes. Use JoinABLe's decomposition, initially with analytic scoring rather than neural inference.

Acceptance: 20 held-out fixture configurations; correct mate frames and clearance intervals; invalid-axis, mirrored-pattern, wrong-diameter and duplicate-constraint negative controls; edits preserve intended mates or report explicit failure. Record candidates tried, kernel calls and time, not just a final screenshot. Proposed experiment, no results yet.

### 2. Dense internal assembly and edit regression suite

Create a small versioned benchmark with declared BOM, hidden/internal geometry, joint graph, functional envelopes and required edits. Begin with 5–20 part fixtures; add a 30–60 part integration case only after fixtures pass. Hold out template families, not merely random dimensions of the same template.

Score independent axes: required component/feature coverage; dimensional fit; collision-free static arrangement; correct joints/remaining DOFs; internal clearance; rebuild success after parameter perturbation; unchanged-part preservation; STEP round-trip; and (separately) insertion/tool access. Missing hardware must lower completeness even when all existing solids pass. Use deliberate removed bores, displaced spacers, obstructed access, swapped units and stale evidence as negative controls. Retain the current hash-based check freshness and fail-closed policy.

Benchmark references: [neuralCAD-Edit, April 2026](https://autodeskailab.github.io/neuralCAD-Edit/) provides 192 expert multimodal requests and 384 edits, including assemblies and different history availability; [CADGenBench](https://github.com/huggingface/cadgenbench) separates drawing generation and existing-STEP editing. Both are useful task designs, not proof of dense aerospace competence. [CADEngBench, August 2026 preprint](https://arxiv.org/abs/2608.09296) explicitly evaluates parameter perturbations and 150 joint pairs; release/grade implementation was not audited. Avoid conflating its pairwise assembly track with global assemblies.

### 3. Execution-guided repair and a compact Wright training corpus

Log requirement → typed action → native result → failure classification → repair → final verified state, including rollback. Generate parameter sweeps of Wright-owned fixtures and physically meaningful negative examples, then retain only correctly labeled traces. Distinguish syntax/kernel validity, fit, dependency and completeness failures. Keep provenance/license/source hashes with each example. Store JSON and regenerate B-reps on demand to stay small.

First compare the existing model with retrieved successful traces against the same model without retrieval at an equal kernel-call budget. Then evaluate a small reranker of candidate mates/actions. Only consider supervised fine-tuning after trace retrieval and verifier-guided search are measured. Fine-tuning a decoder on isolated sketch/extrude histories is unlikely to fix missing assembly representations. A later LoRA pilot needs a selected base-model license, measured GPU/storage budget and external training authorization; no such run was initiated.

Acceptance: improve held-out task pass rate and repair success without increasing false-positive validity or omitted components. Split by family and provenance to avoid procedural-template leakage. Include counterfactual repairs that make a shape look right while deleting its interior, so reward cannot be gamed by appearance.

### 4. Assembly feasibility and service access

For each critical internal component, declare insertion direction and service/tool envelope. Prototype conservative swept-volume checks with current OCC operations, then limited sampled motion search for cases that fail straight insertion. Use assembly-by-disassembly only for the already modeled geometry. Report sampling resolution and distinguish “path found” from “no path found within budget”; a coarse sample is not continuous collision certification.

Start with an avionics tray inside a tube, captive fasteners, bulkheads and removable access panels. This directly targets the failure mode where internally dense CAD passes final-position clash checks but cannot be built or serviced. No new model is required.

### 5. Learned interface ranking, only after a deterministic baseline

Run a bounded JoinABLe-inspired baseline on Wright-owned fixtures. Analytic features (surface type, radius, normals, coaxiality, pattern pitch, overlap and clearance) are enough to establish a useful baseline. If failure analysis shows ranking—not missing semantics or bad geometry—is the bottleneck, compare a small graph/ranking model. Evaluate pair retrieval and whole-assembly success separately; locally correct mates can still make a globally inconsistent cycle.

## Practical conclusion

The near-term training asset should be a compact, validated **assembly/edit trace corpus**, with executable geometry and explicit interfaces. The near-term product improvement should be a joint-aware native API and verifiable internal-design benchmark. Large public part corpora, point-cloud reconstruction weights and render-based scoring do not directly address Wright's present failures. Research suggests useful methods; none of the reviewed evidence establishes autonomous, fabrication-ready dense aerospace assembly generation.
