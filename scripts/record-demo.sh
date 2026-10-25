#!/usr/bin/env bash
# Record real agent runs for the table demo (the desktop app replays them with
# no network). No budgets: Pat (10/24) wants the best design, cost is information. Runs on the signed-in Claude Code subscription by default so the
# weekend's API credits are kept for live judging:
#   scripts/record-demo.sh                     # all three vehicles
#   scripts/record-demo.sh balloon             # one
#   WRIGHT_TRANSPORT=api scripts/record-demo.sh aircraft   # on API credits
# Output: demo/<domain>/events.jsonl plus every file the run built.
set -euo pipefail
cd "$(dirname "$0")/.."
. "$(dirname "$0")/env.sh"   # WRIGHT_PYTHON, WRIGHT_SU2, PATH
export WRIGHT_TRANSPORT="${WRIGHT_TRANSPORT:-cli}"
cargo build -q --release -p wright-cli

req() {  # bash 3.2 (macOS) has no associative arrays
  case "$1" in
    aircraft) echo "A 3D-printed VTOL quadplane with the range of the 130-mile build (130 mi flown with no reserve is about 187 km with a 15 % reserve): at least 180 km with a 15 % reserve, 1.2 m wingspan, prints on a Bambu A1, 6S." ;;
    rocket) echo "A student liquid-bipropellant rocket for the Spaceport America Cup 10,000 ft category, pressure-fed, stable off a standard rail." ;;
    balloon) echo "A high-altitude weather balloon that photographs the curvature of the Earth and is reliably recovered: APRS tracking with ELRS backup, FAA Part 101 light payload." ;;
  esac
}
domains="${*:-aircraft rocket balloon}"
for dom in $domains; do
  {
    out="demo/$dom"
    rm -rf "$out"; mkdir -p "$out"
    echo "== $dom"
    ./target/release/wright run "$dom" "$(req "$dom")" -o "$out" --no-creao || echo "   (run ended without passing; the recording is still kept)"
    wc -l "$out/events.jsonl"
  }
done
