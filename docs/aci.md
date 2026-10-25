# Headless runs on Azure Container Instances

The bench, the evals and the rocket propellant sweep run headless in
containers, fanned out over Azure Container Instances (ACI). Each shard or
eval gets its own container group. Results land on an Azure Files share and
are pulled back and merged on the laptop. The Azure sponsor's advice
(10/24, 19:32) was: "docker + ACI/kubernetes; ACI is fun, it's burstable...
and has no orchestrator layer". ACI fits that: one `az container create`
per job, billed per second, deleted when the job finishes.

Status (2026-10-24): the pieces below are written and the sharding is
tested, but **no Azure resource has been created and the image has not been
built**. Disk had 3.9 GB free, under the 5 GB rule, and the Docker daemon
(colima) was not running. Spending Azure money needs Pat's OK.

## Pieces

| file | what |
|---|---|
| `docker/Dockerfile` | multi-stage: `builder` (Rust 1.98), `bench` (debian-slim + `wright` and the eval/sweep binaries, no solvers), `solvers` (bench + Python venv with jsbsim, optvl, rocketpy, cantera, gmsh, plus CalculiX) |
| `.dockerignore` | keeps target/, out/, runs/, demo*/, .git, .lake, dist-newstyle, node_modules, .env*, docs/ (incl. docs/creao.md), CAD meshes out of the context |
| `wright bench --shard i/n` | runs one contiguous slice of the scenarios (`wright_core::shard`, after Playwright's `filterForShard`, `packages/playwright/src/runner/testGroups.ts`) |
| `rocket_matrix -- --shard i/n --json OUT` | the same for the 20-case propellant sweep; writes `{"kind":"rocket_matrix","rows":[...]}` |
| `wright merge A.json B.json ... -o OUT.json` | joins shards. Bench results are recomputed with the same `results_json`, so 3 merged shards are byte-identical to an unsharded run (tested). A duplicated id is an error. |
| `scripts/aci-fanout.sh` | build → push to ACR → N × `az container create` → poll → logs → download → merge → delete. **Dry run by default**; `--run` executes and asks before creating anything. |

Prior art, by file: the Azure CLI reference for `az container`
(learn.microsoft.com/cli/azure/container, ms.date 2026-09-01) for every
flag. `Azure-Samples/aci-docs-sample-python` `src/aci_docs_sample.py::run_task_based_container`
for the task pattern: restart policy Never, wait, read the logs, delete the
group. SWE-bench `swebench/harness/run_evaluation.py` for one isolated
container per instance, each writing its own report, then one aggregation
step and cleanup in a `finally`. Here that is `wright merge` and an EXIT
trap. The image stages follow the layering in `swebench/harness/docker_build.py`
(base, env, instance).

Not in the image (each stage that needs one reports "not run", which is
never a pass): FreeCAD (over 1 GB, and CAD belongs to Astra), SU2 (no Debian
package of v8.5), PrusaSlicer (a GUI AppImage), AeroSandbox (an audit
cross-check), usd-core/build123d (CAD only), and ffmpeg and GPU drivers
(renders and films are optional stages). CalculiX is Debian's 2.20; the
local runs used 2.23.

Linux build note: `wright-desktop` (egui) is linked into the CLI with
eframe's default features off, and winit will not compile on Linux without
a windowing backend. The builder stage therefore appends a Linux-only
`eframe` dependency with the `x11` feature to the in-container
`crates/wright-desktop/Cargo.toml`. x11 is dlopen'ed, so it is never opened
headless. This is not done in the shared tree because it would rewrite
Cargo.lock (it adds `glutin_glx_sys`) for every session on macOS. It has
not been compiled yet, so the first `docker build` is the test.

## One-time setup (Pat runs these after he OKs the spend)

```bash
az login
az account set --subscription "<sponsor subscription>"
az provider register --namespace Microsoft.ContainerInstance
az provider register --namespace Microsoft.ContainerRegistry
az group create --name wright-rg --location eastus
# registry: Basic SKU; admin user so the script can pull with a username and password
# (alternative: a user-assigned identity with AcrPull plus --acr-identity/--assign-identity)
az acr create --resource-group wright-rg --name wrightacr --sku Basic --admin-enabled true
# results share
az storage account create --resource-group wright-rg --name wrightstore --location eastus --sku Standard_LRS --kind StorageV2
az storage share-rm create --resource-group wright-rg --storage-account wrightstore --name wright-results --quota 10
```

Names are the script defaults. Override them with `AZ_RG`, `AZ_LOCATION`,
`AZ_ACR`, `AZ_STORAGE` and `AZ_SHARE`. ACR and storage names must be
globally unique.

## Running

```bash
scripts/aci-fanout.sh --job bench --shards 4             # dry run: prints every command, secrets masked
scripts/aci-fanout.sh --run --job bench --shards 4       # bench, 4 groups; out/aci/<run>/results.json
scripts/aci-fanout.sh --run --job rocket_matrix --shards 5
scripts/aci-fanout.sh --run --job evals --spot           # vtol, rocket, balloon evals, one group each
```

The image is built with `az acr build`, which builds in Azure and needs no
local disk or Docker. `--local-build` uses `docker build` + `docker push`
instead, and `--skip-build` reuses a pushed tag. Each group mounts the
share at `/results`. Logs go to `out/aci/<run>/logs/` and results to
`out/aci/<run>/<run>/...`, then are merged into `out/aci/<run>/results.json`
(bench) or `rows.json` (sweep). The EXIT trap deletes every group, also on
failure or Ctrl-C.

Secrets come only from the calling shell. Set `ANTHROPIC_API_KEY`, or
`WRIGHT_TRANSPORT=foundry` with `ANTHROPIC_FOUNDRY_API_KEY` and
`ANTHROPIC_FOUNDRY_RESOURCE` (Microsoft Foundry on the sponsor's
subscription). They are passed with `--secure-environment-variables`, which
ACI does not show in `az container show`. The script never reads `.env`.
The bench and the sweep need no key, since the bench uses scripted models.
A live `wright run` in a container cannot use the Claude CLI subscription
(no logged-in `claude` in the image), so it needs the API or Foundry
transport.

## Cost

ACI Linux pay-as-you-go prices in East US, from the Azure Retail Prices API
(`prices.azure.com/api/retail/prices`, serviceName 'Container Instances',
queried 2026-10-24):

| meter | per hour | per second |
|---|---|---|
| Standard vCPU | $0.0405 / vCPU-h | $0.00001125 / vCPU-s |
| Standard memory | $0.00445 / GB-h | $0.00000124 / GB-s |
| Spot vCPU | $0.01215 / vCPU-h | |
| Spot memory | $0.001335 / GB-h | |

Billing is per second, from the start of the image pull until the
container stops. A per-run estimate is
groups × hours × (vCPU × 0.0405 + GB × 0.00445):

| run | groups | size | wall time (est.) | cost |
|---|---|---|---|---|
| bench (scripted, under 1 s of work; pull and start dominate) | 4 | 1 vCPU, 1.5 GB | ~2 min | ~$0.006 |
| rocket_matrix sweep | 4 | 1 vCPU, 1.5 GB | ~10 min | ~$0.03 |
| evals (vtol, rocket ~7 CPU-min, balloon) | 3 | 2 vCPU, 4 GB | ~15 min | ~$0.07 (spot ~$0.02) |

Fixed costs: ACR Basic is $0.1666/day (about $5/month), plus $0.10/GB-month
storage. The Files share holding a few MB of JSON costs pennies. The
script prints its own estimate before `--run` asks to go ahead. Model spend
(a live agent run on the API) is separate and much larger than the compute.

Spot groups can be evicted and support fewer features. Check the current
ACI Spot limits (VNet, volume support) before relying on `--spot` with the
Files mount. The evals are the job where Spot pays.

## AWS fallback (Activate credits)

If the Azure credits run out or Pat prefers AWS, Pat's AWS Activate credits
(Org ID in memory) cover the same pattern on ECS Fargate, which also has no
cluster to manage:

| ACI | ECS Fargate |
|---|---|
| ACR + `az acr build` | ECR (`aws ecr create-repository`, `docker push`, or CodeBuild) |
| `az container create --restart-policy Never` | `aws ecs run-task --launch-type FARGATE` with a one-container task definition, one task per shard, `--overrides '{"containerOverrides":[{"name":"wright","command":["wright","bench","--shard","2/4","-o","/results/..."]}]}'` |
| `--secure-environment-variables` | task definition `secrets` from Secrets Manager / SSM Parameter Store |
| Azure Files at `/results` | EFS volume in the task definition, or have the container `aws s3 cp` to S3 |
| poll `az container show` | `aws ecs wait tasks-stopped`, then `describe-tasks` for `exitCode` |
| `az container logs` | the `awslogs` driver → CloudWatch Logs (`aws logs get-log-events`) |
| `az container delete` | nothing: stopped Fargate tasks cost nothing |

Fargate prices in us-east-1 (aws.amazon.com/fargate/pricing, 2026-10-24):
Linux/x86 is $0.000011244 per vCPU-s and $0.000001235 per GB-s (about the
same as ACI), and Linux/ARM is $0.0000089944 and $0.0000009889. Billing is
per second with a 1-minute minimum. Fargate Spot is up to 70% off. The same
image runs unchanged on Fargate. An ARM build (`--platform linux/arm64`) is
about 20% cheaper but needs the Python wheels for aarch64.
