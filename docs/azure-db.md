# Run history and artifact storage

`wright-agents/src/runs_db.rs` keeps one record per prior effort: request,
domain, spec (with an exact-match key), chosen design (the verified
incumbent), check results, score, artifact paths and failure reasons. At the
start of `wright run` the harness finds the nearest prior runs of the domain
(an exact spec match first, then BM25 over request + spec + failures) and adds
a "Prior efforts" block to the orchestrator's and every specialist's system
prompt: the runs, the best prior design to seed from, and the known failures.
An empty store, or no match, adds nothing. At the end the run's own record is
written. Like lessons, the block is advisory: nothing in it passes a check.

The store interface follows MLflow's tracking store
(`mlflow/store/tracking/abstract_store.py`: `get_run`, `create_run` +
`log_batch`, concrete `search_runs` over the backend's `_search_runs`), and the
backend is picked by URI scheme as MLflow's `TrackingStoreRegistry` does
(`mlflow/tracking/_tracking_service/registry.py`, `utils.py`).

| `WRIGHT_RUNS_URI` | backend |
|---|---|
| unset | `data/runs-db.jsonl` (append-only JSON lines; the last line for an id wins) |
| a path, or `file:PATH` | that JSONL file |
| `cosmos://ACCOUNT/DATABASE/CONTAINER` | Azure Cosmos DB for NoSQL (build with `--features azure`) |
| `off` | no store: no prior-efforts block, nothing recorded |

```sh
wright runs import                 # backfill from demo/ out/ runs/ demo-prev/ + data/lessons-*.jsonl, data/regressions-*.jsonl
wright runs search "VTOL quadplane 130 miles" [--domain aircraft] [--spec spec.json] [-n 5] [--json]
```

Why a JSONL file locally and not SQLite: CLAUDE.md keeps storage as files
until after Cal Hacks, and the shared cargo lock has no SQLite crate.

## Why Cosmos DB for NoSQL and not Azure Database for PostgreSQL

The Rust path is simpler. Cosmos is a REST API, and the crate already uses
`reqwest` with rustls, so the backend adds no crates. PostgreSQL would add
`tokio-postgres` and a TLS adapter, because Azure PostgreSQL requires TLS.
Auth uses Microsoft Entra ID tokens
(`Authorization: type=aad&ver=1.0&sig=TOKEN`) and not the account key. The
key would need HMAC-SHA256 signing, and no crate for that is in the lock.
Microsoft also recommends turning key auth off. The free tier (1000 RU/s,
25 GB) is far more than this needs.

Records are upserted (`x-ms-documentdb-is-upsert`) with partition key
`/domain`. Search loads the domain's records with one query and ranks them in
Rust, so both backends rank the same way. Local working files remain the source
of truth. Cloud persistence stores run summaries in Cosmos and copies the
explicit artifact manifest to private Azure Blob Storage.

## Private Blob Storage (implemented 2026-10-24)

Account `wrightfilesfb102aa8`, container `wright-runs`, resource group
`wright-ai-student`, region `westus2`, Standard LRS. Public blob access and
shared-key authorization are disabled; HTTPS and TLS 1.2 are required. The
signed-in user has Storage Blob Data Contributor on this account.

`scripts/env.sh`, sourced by the desktop launcher, sets:

```sh
export WRIGHT_RUNS_URI=cosmos://wright-runs-pl4676/wright/runs
export WRIGHT_BLOB_URL=https://wrightfilesfb102aa8.blob.core.windows.net/wright-runs
```

`azure_blob.rs` uses the existing Azure CLI credential cache with scope
`https://storage.azure.com/.default`. Tokens are refreshed before expiration;
no account key or SAS is saved in configuration. The implementation follows
[Azure Put Blob](https://learn.microsoft.com/en-us/rest/api/storageservices/put-blob)
and [Microsoft Entra authorization](https://learn.microsoft.com/en-us/rest/api/storageservices/authorize-with-azure-active-directory).

The normal run-summary save and `wright runs import` upload artifacts before
upserting their private URLs into Cosmos. Supported discovery includes CAD
(FreeCAD documents, STL, STEP, GLB), KiCad schematics and boards, Gerbers, drill/placement files,
G-code, parts-list CSVs, reports, previews and ZIP build packages. Discovery
excludes arbitrary JSON and environment files. Explicit artifact events also
contribute files, so workers must list only intended deliverables.

Discovery scans up to three directory levels and 200 artifacts per run.
Limits: 128 MiB per file; 120-second HTTP timeout per request; no redirects.
Missing files, symlinks, oversize files and HTTP errors fail visibly. Local
files are never deleted. An upload failure prevents that run's Cosmos upsert;
retry with `wright runs import /absolute/run/directory`. Successful partial
uploads are overwritten safely on retry. Keys encode run ID and full artifact
path without lossy replacement, avoiding collisions between same-name CAD
and PCB files. Relocating a local run creates new keys; this is a result archive,
not continuous sync or a full project backup. Checkpoints, event logs and
lessons remain local. There is no automatic background retry queue or retention
job yet. Importing the same ID replaces the cloud run record.

`BlobStore::download` authenticates private reads and only accepts URLs in the
configured container. URLs alone do not grant public access. A desktop restore
button is not part of this implementation. `WRIGHT_BLOB_URL=off` disables file
uploads while keeping Cosmos history; `WRIGHT_RUNS_URI=off` disables both.
Legacy `WRIGHT_BLOB_SAS_URL` remains supported if `WRIGHT_BLOB_URL` is unset.

```sh
source scripts/env.sh
# Explicit live test. Writes three tiny infrastructure fixtures, no model call.
cargo run --offline -p wright-agents --features azure --example azure_storage_smoke
# Save an existing run and its generated artifacts:
./target/release/wright runs import /absolute/run/directory
```

The smoke example writes a labeled `storage-test` record, reads it from
Cosmos, downloads every file using the Rust client, and compares exact bytes.
It is infrastructure evidence, not a successful hardware design.

## If Azure credits run out: AWS

The local JSONL store always works and needs no cloud. Pat's AWS Activate
credits are the fallback. Amazon Bedrock serves Claude, so it is a model
transport, not a database. For a shared store on AWS, the same `RunStore`
trait would take a DynamoDB backend: table `wright-runs`, partition key
`domain`, sort key `id`. Its REST calls need SigV4 signing, and that needs
HMAC-SHA256, so the backend would add `hmac` + `sha2` or the AWS SDK. It is
not written yet. A no-code stopgap is to sync `data/runs-db.jsonl` to S3 with
`aws s3 cp data/runs-db.jsonl s3://<bucket>/runs-db.jsonl` and pull it before a
run. A Postgres backend (Azure Database for PostgreSQL or AWS RDS) fits the
same trait too, at the cost of `tokio-postgres` + TLS.

## Live account (created 2026-10-24 ~21:20 PT)
Subscription "Azure for Students", resource group `wright-ai-student` (westus2), free-tier Cosmos account `wright-runs-pl4676`, database `wright`, container `runs` (partition `/domain`, 1000 RU/s shared, inside the free tier). Pat's UCSB identity has the Data Contributor role. All records from `data/runs-db.jsonl` imported; `wright runs search` works against it.

Default since 2026-10-24 22:05: `scripts/env.sh` exports
`WRIGHT_RUNS_URI=cosmos://wright-runs-pl4676/wright/runs` (the desktop app
sources it), and `wright-cli` builds the `azure` feature by default. The token
comes from the signed-in Azure CLI the way azure-sdk-for-rust's
`AzureCliCredential` gets it (`sdk/identity/azure_identity/src/azure_cli_credential.rs`:
`az account get-access-token -o json --scope https://ACCOUNT.documents.azure.com/.default`),
cached until 5 minutes before `expires_on`, so a long session never holds an
expired token. `WRIGHT_COSMOS_TOKEN` still overrides. If `az` is missing or
signed out, wright prints one line and uses `data/runs-db.jsonl`.

    source scripts/env.sh && wright runs search "VTOL quadplane"

## Verification on 2026-10-24

The live Rust smoke test passed: one Cosmos record plus three private Blob
artifacts uploaded, downloaded, and compared byte-for-byte. The rebuilt release
CLI also imported the test run and returned its three private URLs through
`runs search`. A desktop-like minimal PATH successfully found Azure CLI after
sourcing `scripts/env.sh`. All six storage and run-history tests pass, and Azure all-target Clippy passes.
The two stale harness model assertions were corrected to the normal-mode
Opus default. The full agent suite now passes; demo-mode Fable retains its
separate role-selection test.

## Desktop completion archive (2026-10-24)

The Tauri `wright_start` command now launches `scripts/run-with-storage.sh` for
both `wright run` and the native CAD project runner. After the producer exits,
the wrapper calls `wright runs sync RUN_DIR --domain DOMAIN --request TEXT`.
This catches native CAD jobs that bypass the harness and files written after
the harness summary, including the build guide. The archive adapter accepts
`state.json` or the native runner's `run_started`/`artifact` events, so native
CAD runs do not need a synthetic design state. Files outside the run directory
are refused. Cloud saves read the record back and check the artifact URL count.

Each attempt writes `storage-status.json` and a storage message into the run's
`events.jsonl`. A cloud failure keeps local files and causes a nonzero exit; a
producer failure retains its original exit code even if archiving succeeds.
`WRIGHT_RUNS_URI=off` explicitly skips the completion archive. If Blob uploads
are disabled while artifacts exist, final cloud synchronization reports a count
mismatch instead of claiming those files were saved. A forced process-group
kill, power loss or app crash can interrupt archiving; retry with `wright runs
sync RUN_DIR`. This is completion-time archiving, not continuous file sync.

Offline launcher regression: `bash scripts/test-run-with-storage.sh`. The live
launcher verification uses an existing 5.3 MB STEP model and a KiCad board fixture,
not a newly generated design; evidence is in
`docs/evidence/azure-storage-2026-10-24.json`.

The desktop dev binary was rebuilt and confirmed to contain the new launcher
path. The full agent suite passed (181 tests, 12 ignored), the CLI archive
regression and shell launcher regressions passed, and agent/CLI all-target
Clippy passed. The optional existing Tauri helper-test rebuild was stopped
after recompiling unrelated native audio dependencies; no pass is claimed for
that helper suite. The live archive check exercised the exact production
launcher script and downloaded both outputs for SHA-256 comparison.

Isolated merge validation against main: 179 agent/CLI tests passed, 12 ignored;
normal/demo model selection and both original failing harness tests are included.
