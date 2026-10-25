#!/usr/bin/env bash
# Electronics A/B eval: every fixture spec through `wright run electronics`
# twice, real kicad-cli and real Claude calls, no cached numbers.
#   b = Wright annealing only: --no-groups --single-seed (no model call)
#   c = Wright full: Claude groups, 12 seeds, 6 quick-routed, best routed
# Column a (the Python baseline) is read from evals/electronics/baseline/.
# Usage: evals/electronics/run.sh [OUT_DIR] [fixture ...]
set -u
cd "$(dirname "$0")/../.."
OUT=${1:-out/eval-electronics}
shift || true
FIX=${*:-servo_power ldo blinker blinker10 ref25 two_chip_usb esp32_devboard}
BIN=./target/release/wright
mkdir -p "$OUT"
for f in $FIX; do
  spec=evals/electronics/specs/$f.json
  for col in b c; do
    dir=$OUT/$f-$col
    rm -rf "$dir"
    flags=(--spec "$spec" --no-creao)
    [ "$col" = b ] && flags+=(--no-groups --single-seed)
    echo "== $f $col"
    "$BIN" run electronics "eval fixture $f" -o "$dir" "${flags[@]}" >"$dir.log" 2>&1 || echo "  exit $? (see $dir.log)"
    jq -c '{passed, seconds, model_calls, input_tokens, output_tokens, m: .metrics | {area_mm2, decap_total_mm, routed_pct, drc_errors, erc_errors, parity, power}}' "$dir/run.json" 2>/dev/null || echo "  no run.json"
  done
done
