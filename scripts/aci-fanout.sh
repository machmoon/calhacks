#!/usr/bin/env bash
# Fan a bench / sweep / eval run out over Azure Container Instances: one
# container group per shard (or per eval), results on an Azure Files share
# mounted at /results, pulled back and merged here. docs/aci.md has the
# one-time setup and the cost.
#
#   scripts/aci-fanout.sh                         # DRY RUN (default): print every az command, run nothing
#   scripts/aci-fanout.sh --job bench --shards 4  # still a dry run
#   scripts/aci-fanout.sh --run --job bench       # really create resources (asks first; Pat's OK for spend)
#
# Jobs:
#   bench          wright bench --shard i/N            (image target `bench`, no solvers)
#   rocket_matrix  rocket_matrix --shard i/N --json    (the propellant sweep; pure Rust)
#   evals          one group per eval: vtol_eval, rocket_eval, balloon_eval (image target `solvers`)
#
# Precedent, followed:
#   * Azure-Samples/aci-docs-sample-python, src/aci_docs_sample.py
#     (`run_task_based_container`: restart policy Never, wait for the group,
#     read its logs, delete the group afterwards in `main`).
#   * az container create / show / logs / delete flags exactly as in the Azure
#     CLI reference (learn.microsoft.com/cli/azure/container, ms.date 2026-09-01).
#     There is no `az container wait`, so the state is polled with
#     `az container show --query containers[0].instanceView.currentState`.
#   * SWE-bench swebench/harness/run_evaluation.py: one isolated container per
#     instance, each writing its own report file, then one aggregation step
#     (`make_run_report`) -- here `wright merge`; and cleanup in a `finally`
#     (`cleanup_container`) -- here an EXIT trap that deletes every group.
#
# Secrets: only from this shell's environment (ANTHROPIC_API_KEY, or
# ANTHROPIC_FOUNDRY_API_KEY + ANTHROPIC_FOUNDRY_RESOURCE/BASE_URL), passed with
# --secure-environment-variables so ACI never shows them in `az container
# show`. This script never reads .env. The bench and rocket_matrix need no key.
set -euo pipefail
cd "$(dirname "$0")/.."

# ------------------------------------------------------------------ config
RG=${AZ_RG:-wright-rg}
LOCATION=${AZ_LOCATION:-eastus}
ACR=${AZ_ACR:-wrightacr}                 # registry name; login server $ACR.azurecr.io
STORAGE=${AZ_STORAGE:-wrightstore}       # storage account holding the share
SHARE=${AZ_SHARE:-wright-results}
TAG=${WRIGHT_IMAGE_TAG:-$(git rev-parse --short HEAD 2>/dev/null || echo dev)}
JOB=bench
SHARDS=4
CPU=1
MEMORY=1.5
SPOT=0
LOCAL_BUILD=0
SKIP_BUILD=0
DRY=1
YES=0
EST_MIN=""                               # minutes per group for the cost line
TIMEOUT_MIN=60

usage() { sed -n '2,30p' "$0"; cat <<EOF
Options:
  --run                 execute (default is a dry run that only prints)
  --yes                 with --run: do not ask before creating resources
  --job bench|rocket_matrix|evals
  --shards N            container groups for bench / rocket_matrix (default $SHARDS)
  --cpu C --memory GB   per group (default $CPU vCPU, $MEMORY GB)
  --spot                --priority Spot (about 70% cheaper; may be evicted)
  --local-build         docker build + docker push instead of az acr build
  --skip-build          reuse $ACR.azurecr.io/wright:<tag> already pushed
  --tag T               image tag (default: git short sha, now $TAG)
  --timeout MIN         give up waiting after MIN minutes (default $TIMEOUT_MIN)
Env: AZ_RG AZ_LOCATION AZ_ACR AZ_STORAGE AZ_SHARE (now $RG $LOCATION $ACR $STORAGE $SHARE)
EOF
exit 2; }

while [ $# -gt 0 ]; do
  case $1 in
    --run) DRY=0 ;;
    --dry-run) DRY=1 ;;
    --yes) YES=1 ;;
    --job) JOB=$2; shift ;;
    --shards) SHARDS=$2; shift ;;
    --cpu) CPU=$2; shift ;;
    --memory) MEMORY=$2; shift ;;
    --spot) SPOT=1 ;;
    --local-build) LOCAL_BUILD=1 ;;
    --skip-build) SKIP_BUILD=1 ;;
    --tag) TAG=$2; shift ;;
    --timeout) TIMEOUT_MIN=$2; shift ;;
    -h|--help) usage ;;
    *) echo "unknown option $1" >&2; usage ;;
  esac
  shift
done

case $JOB in
  bench) TARGET=bench; EST_MIN=${EST_MIN:-2} ;;
  rocket_matrix) TARGET=bench; EST_MIN=${EST_MIN:-10} ;;
  evals) TARGET=solvers; EST_MIN=${EST_MIN:-15}; [ "$CPU" = 1 ] && CPU=2; [ "$MEMORY" = 1.5 ] && MEMORY=4 ;;
  *) echo "unknown job $JOB" >&2; usage ;;
esac
[[ $SHARDS =~ ^[1-9][0-9]*$ ]] || { echo "--shards wants a positive integer" >&2; exit 2; }

RUN_ID=$(echo "wright-$JOB-$(date +%Y%m%d-%H%M%S)" | tr '_' '-')
IMAGE="$ACR.azurecr.io/wright:$TAG-$TARGET"
LOCAL_OUT="out/aci/$RUN_ID"

# --------------------------------------------------------------- execution
# `x CMD...` prints the command (secrets masked) and runs it unless dry.
# `cap VAR CMD...` does the same and stores stdout in VAR (a placeholder in
# a dry run). Masking: any argument after a --*password/--*key flag, and the
# value half of KEY=VALUE for the secret names below.
SECRET_NAMES='ANTHROPIC_API_KEY|ANTHROPIC_FOUNDRY_API_KEY'
show() {
  local out="" mask=0 a
  for a in "$@"; do
    if [ $mask = 1 ]; then a='***'; mask=0
    elif [[ $a =~ ^($SECRET_NAMES)= ]]; then a="${a%%=*}=***"; fi
    case $a in --registry-password|--azure-file-volume-account-key|--account-key) mask=1 ;; esac
    out+=" $(printf '%q' "$a")"
  done
  echo "+${out}"
}
x() { show "$@"; [ $DRY = 1 ] || "$@"; }
cap() { local __v=$1; shift; show "$@"; if [ $DRY = 1 ]; then printf -v "$__v" '%s' "<${__v}>"; else printf -v "$__v" '%s' "$("$@")"; fi; }

# Cost line: ACI Linux pay-as-you-go, East US, Azure Retail Prices API
# (prices.azure.com, 2026-10-24): $0.0405 per vCPU-hour, $0.00445 per
# GB-hour; Spot $0.01215 and $0.001335.
groups() { if [ "$JOB" = evals ]; then echo 3; else echo "$SHARDS"; fi; }
estimate() {
  awk -v n="$(groups)" -v c="$CPU" -v m="$MEMORY" -v t="$EST_MIN" -v s="$SPOT" 'BEGIN {
    vc = s ? 0.01215 : 0.0405; gb = s ? 0.001335 : 0.00445; h = t / 60
    printf "estimate: %d groups x %.1f vCPU x %.1f GB x ~%d min = $%.4f (%s)\n", n, c, m, t, n * h * (c * vc + m * gb), s ? "spot" : "pay-as-you-go" }'
}

echo "run $RUN_ID: job $JOB, image $IMAGE, $(groups) container groups in $RG ($LOCATION)"
estimate
if [ $DRY = 1 ]; then
  echo "DRY RUN: printing the commands only; nothing is created. Pass --run to execute."
else
  command -v az >/dev/null || { echo "az CLI not found" >&2; exit 1; }
  if [ $YES = 0 ]; then
    read -r -p "Create $(groups) container groups (billed per second) in $RG? type yes: " ok
    [ "$ok" = yes ] || { echo "stopped"; exit 1; }
  fi
fi

# 1. image -> ACR. `az acr build` builds in Azure (no local disk or Docker);
#    --local-build uses Docker here.
if [ $SKIP_BUILD = 0 ]; then
  if [ $LOCAL_BUILD = 1 ]; then
    x docker build -f docker/Dockerfile --target "$TARGET" -t "$IMAGE" .
    x az acr login --name "$ACR"
    x docker push "$IMAGE"
  else
    x az acr build --registry "$ACR" --image "wright:$TAG-$TARGET" --file docker/Dockerfile --target "$TARGET" .
  fi
fi

# 2. credentials the groups need (registry pull, share mount).
cap ACR_USER az acr credential show --name "$ACR" --query username -o tsv
cap ACR_PASS az acr credential show --name "$ACR" --query 'passwords[0].value' -o tsv
cap STORAGE_KEY az storage account keys list --resource-group "$RG" --account-name "$STORAGE" --query '[0].value' -o tsv

SECURE=()
for v in ANTHROPIC_API_KEY ANTHROPIC_FOUNDRY_API_KEY; do
  [ -n "${!v:-}" ] && SECURE+=("$v=${!v}")
done
PLAIN=("WRIGHT_RUN_ID=$RUN_ID")
for v in WRIGHT_TRANSPORT ANTHROPIC_FOUNDRY_RESOURCE ANTHROPIC_FOUNDRY_BASE_URL WRIGHT_RUN_BUDGET_USD; do
  [ -n "${!v:-}" ] && PLAIN+=("$v=${!v}")
done

# 3. one group per shard / eval: name<TAB>command line
R="/results/$RUN_ID"
jobs_list() {
  local i
  case $JOB in
    bench) for i in $(seq 1 "$SHARDS"); do
        printf '%s-%d\t%s\n' "$RUN_ID" "$i" "sh -c 'mkdir -p $R/bench && wright bench --shard $i/$SHARDS -o $R/bench/results-shard-$i-of-$SHARDS.json'"; done ;;
    rocket_matrix) for i in $(seq 1 "$SHARDS"); do
        printf '%s-%d\t%s\n' "$RUN_ID" "$i" "sh -c 'mkdir -p $R/rocket_matrix && rocket_matrix --shard $i/$SHARDS --json $R/rocket_matrix/rows-shard-$i-of-$SHARDS.json'"; done ;;
    evals) for e in vtol:vtol_eval rocket:rocket_eval balloon:balloon_eval; do
        printf '%s-%s\t%s\n' "$RUN_ID" "${e%%:*}" "sh -c '${e#*:}; s=\$?; mkdir -p $R/evals && cp -r evals/${e%%:*} $R/evals/; exit \$s'"; done ;;
  esac
}

NAMES=()
cleanup() {
  [ ${#NAMES[@]} -eq 0 ] && return
  echo "# deleting ${#NAMES[@]} container groups"
  for n in "${NAMES[@]}"; do x az container delete --resource-group "$RG" --name "$n" --yes -o none || true; done
}
trap cleanup EXIT

PRIORITY=()
[ $SPOT = 1 ] && PRIORITY=(--priority Spot)
while IFS=$'\t' read -r name cmd; do
  NAMES+=("$name")
  args=(az container create --resource-group "$RG" --name "$name" --location "$LOCATION"
    --image "$IMAGE" --os-type Linux --cpu "$CPU" --memory "$MEMORY" --restart-policy Never
    --registry-login-server "$ACR.azurecr.io" --registry-username "$ACR_USER" --registry-password "$ACR_PASS"
    --azure-file-volume-account-name "$STORAGE" --azure-file-volume-account-key "$STORAGE_KEY"
    --azure-file-volume-share-name "$SHARE" --azure-file-volume-mount-path /results
    --environment-variables "${PLAIN[@]}" --command-line "$cmd" --no-wait -o none)
  [ ${#SECURE[@]} -gt 0 ] && args+=(--secure-environment-variables "${SECURE[@]}")
  [ ${#PRIORITY[@]} -gt 0 ] && args+=("${PRIORITY[@]}")
  x "${args[@]}"
done < <(jobs_list)

# 4. wait for every group's container to terminate (restart policy Never).
[ $DRY = 1 ] || mkdir -p "$LOCAL_OUT/logs"
failed=0
for n in "${NAMES[@]}"; do
  q='containers[0].instanceView.currentState.[state,exitCode]'
  if [ $DRY = 1 ]; then
    show az container show --resource-group "$RG" --name "$n" --query "$q" -o tsv
    echo "  (poll every 15 s until state is Terminated; give up after $TIMEOUT_MIN min)"
  else
    deadline=$(( $(date +%s) + TIMEOUT_MIN * 60 ))
    while :; do
      st=$(az container show --resource-group "$RG" --name "$n" --query "$q" -o tsv 2>/dev/null | tr '\t\n' '  ')
      case $st in Terminated*) break ;; esac
      [ "$(date +%s)" -gt "$deadline" ] && { echo "$n: timed out ($st)"; st="Timeout 124"; break; }
      sleep 15
    done
    code=${st##* }; code=${code:-1}
    echo "$n: $st"
    [ "$code" = 0 ] || failed=$((failed + 1))
  fi
  # 5. logs, kept beside the results
  if [ $DRY = 1 ]; then show az container logs --resource-group "$RG" --name "$n"; echo "  > $LOCAL_OUT/logs/$n.log"
  else az container logs --resource-group "$RG" --name "$n" > "$LOCAL_OUT/logs/$n.log" 2>&1 || true; fi
done

# 6. results back from the share, then merged.
x az storage file download-batch --account-name "$STORAGE" --account-key "$STORAGE_KEY" \
  --source "$SHARE" --pattern "$RUN_ID/*" --destination "$LOCAL_OUT" -o none
W=$(command -v wright || ls target/release/wright target/debug/wright 2>/dev/null | head -1 || true)
W=${W:-wright}
case $JOB in
  bench) x "$W" merge $(for i in $(seq 1 "$SHARDS"); do echo "$LOCAL_OUT/$RUN_ID/bench/results-shard-$i-of-$SHARDS.json"; done) -o "$LOCAL_OUT/results.json" ;;
  rocket_matrix) x "$W" merge $(for i in $(seq 1 "$SHARDS"); do echo "$LOCAL_OUT/$RUN_ID/rocket_matrix/rows-shard-$i-of-$SHARDS.json"; done) -o "$LOCAL_OUT/rows.json" ;;
  evals) echo "# evals: per-eval result.json/result.md under $LOCAL_OUT/$RUN_ID/evals/ (compare with evals/*/result.md before quoting)" ;;
esac

# 7. groups deleted by the EXIT trap (also on failure or Ctrl-C).
[ $DRY = 1 ] && echo "DRY RUN done: nothing was created."
[ $failed = 0 ] || { echo "$failed container group(s) exited non-zero; see $LOCAL_OUT/logs" >&2; exit 1; }
