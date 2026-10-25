#!/usr/bin/env bash
# Orchestrator A/B: N live runs per config, cost per passed run.
#   scripts/ab-orchestrator.sh [-n N] [-d DOMAIN] [-r ROUNDS] CONFIG...
# CONFIG is a label=model:effort for the orchestrator role, e.g.
#   scripts/ab-orchestrator.sh -n 2 fable-low=claude-fable-5-1:low opus-med=claude-opus-5-5:medium
# Each run (WRIGHT_BIN, default ./target/release/wright): wright run DOMAIN "$REQUEST" --model orchestrator=MODEL:EFFORT
# -o runs/ab/<label>-<i>, on the transport in WRIGHT_TRANSPORT (default the CLI
# subscription). Cost comes from each run's trace.jsonl (`wright trace --json`),
# (Claude calls) plus the run ledger (`ledger_usd`, incl. Astra CAD estimates),
# pass/fail from the run's last line, wall time from the trace's span bounds.
# Precedent: SWE-bench's `run_evaluation` (resolved per instance, then a
# rate over the set) and inspect_ai's per-sample scores; here the unit is
# "USD per passed run", the number that decides the default (docs/TODO.md).
# Results: runs/ab/results.md (table) + runs/ab/results.jsonl (one row per run).
set -uo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
n=2; domain=balloon; rounds="${WRIGHT_MAX_ROUNDS:-2}"
while getopts "n:d:r:" o; do case $o in n) n=$OPTARG;; d) domain=$OPTARG;; r) rounds=$OPTARG;; *) exit 2;; esac; done
shift $((OPTIND-1))
[ $# -ge 1 ] || { sed -n 2,6p "$0"; exit 2; }
case "$domain" in
  aircraft) request="A 3D-printed VTOL quadplane with the range of the 130-mile build (130 mi flown with no reserve is about 187 km with a 15 % reserve): at least 180 km with a 15 % reserve, 1.2 m wingspan, prints on a Bambu A1, 6S." ;;
  rocket) request="A student liquid-bipropellant rocket for the Spaceport America Cup 10,000 ft category, pressure-fed, stable off a standard rail." ;;
  balloon) request="A high-altitude weather balloon that photographs the curvature of the Earth and is reliably recovered: APRS tracking with ELRS backup, FAA Part 101 light payload." ;;
  *) echo "unknown domain $domain"; exit 2;;
esac
bin="${WRIGHT_BIN:-./target/release/wright}"
export WRIGHT_TRANSPORT="${WRIGHT_TRANSPORT:-cli}" WRIGHT_MAX_ROUNDS="$rounds"
mkdir -p runs/ab
rows=runs/ab/results.jsonl
for cfg in "$@"; do
  label="${cfg%%=*}"; model="${cfg#*=}"
  for i in $(seq 1 "$n"); do
    out="runs/ab/$label-$i"; rm -rf "$out"; mkdir -p "$out"
    echo "== $label run $i ($model) -> $out"
    "$bin" run "$domain" "$request" --model "orchestrator=$model" -o "$out" --no-creao > "$out/stdout.txt" 2>&1
    last=$(grep -E "after [0-9]+ rounds" "$out/stdout.txt" | tail -1)
    passed=false; [[ "$last" == *": Passed after"* ]] && passed=true
    cost=$("$bin" trace "$out" --json 2>/dev/null | jq '[.models[].cost_usd] | add // 0')
    wall=$(jq -s '[.[].resourceSpans[].scopeSpans[].spans[] | (.startTimeUnixNano|tonumber), (.endTimeUnixNano|tonumber)] | (max - min) / 1e9' "$out/trace.jsonl" 2>/dev/null || echo 0)
    ledger=$(grep '"type":"spend"' "$out/events.jsonl" 2>/dev/null | tail -1 | jq '.usd // 0'); ledger=${ledger:-0}
    rl=$(grep -ciE "rate.?limit|429|usage limit" "$out/stdout.txt" "$out/events.jsonl" 2>/dev/null | awk -F: '{s+=$2} END {print s+0}')
    echo "{\"label\":\"$label\",\"model\":\"$model\",\"run\":$i,\"passed\":$passed,\"cost_usd\":${cost:-0},\"ledger_usd\":$ledger,\"wall_s\":${wall:-0},\"rate_limit_mentions\":$rl,\"last\":$(jq -Rn --arg s "$last" '$s')}" | tee -a "$rows"
  done
done
{
  echo "| config | runs | passed | total USD | USD per passed run | mean wall s |"
  echo "|---|---|---|---|---|---|"
  jq -rs 'group_by(.label)[] | {l: .[0].label, m: .[0].model, n: length, p: ([.[] | select(.passed)] | length), c: ([.[].cost_usd] | add), w: ([.[].wall_s] | add / length)} | "| \(.l) (\(.m)) | \(.n) | \(.p) | \(.c * 100 | round / 100) | \(if .p > 0 then (.c / .p * 100 | round / 100 | tostring) else "no pass" end) | \(.w | round) |"' "$rows"
} | tee runs/ab/results.md
