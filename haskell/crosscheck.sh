#!/usr/bin/env bash
# Cross-check the Rust domain crates against the independent Haskell oracle.
# Builds both, feeds them the same JSON cases, and prints the max relative
# difference per (function, output). Exit 1 if any exceeds its tolerance.
#
#   haskell/crosscheck.sh            # all domains whose modules are public
#   KEEP=1 haskell/crosscheck.sh     # keep the case/result JSON in $OUT
set -euo pipefail
export PATH="$HOME/.ghcup/bin:$HOME/.cargo/bin:$PATH"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="${OUT:-$(mktemp -d)}"

# --- build the oracle ---------------------------------------------------
(cd "$HERE" && cabal build -v0 exe:wright-oracle)
ORACLE="$(cd "$HERE" && cabal list-bin wright-oracle)"

# --- build the Rust probe with every domain that exposes its modules ------
feat=()
grep -q '^pub mod atmosphere' "$ROOT/crates/wright-air/src/lib.rs" && feat+=(air)
grep -q '^pub mod ideal' "$ROOT/crates/wright-rocket/src/lib.rs" && grep -q '^pub mod atmosphere' "$ROOT/crates/wright-rocket/src/lib.rs" && feat+=(rocket)
grep -q '^pub mod physics' "$ROOT/crates/wright-balloon/src/lib.rs" && grep -q '^pub mod atmosphere' "$ROOT/crates/wright-balloon/src/lib.rs" && feat+=(balloon)
# the PCB oracle against wright-eda (the frozen board contract)
grep -q '^pub mod board' "$ROOT/crates/wright-eda/src/lib.rs" && feat+=(electronics)
FEATURES="$(IFS=,; echo "${feat[*]:-}")"
echo "rust domains exposed: ${FEATURES:-none}"
(cd "$HERE/crosscheck-rs" && CARGO_TARGET_DIR="$ROOT/target" cargo build -q --offline --features "$FEATURES")
PROBE="$ROOT/target/debug/wright-crosscheck"

# --- cases, run, compare ------------------------------------------------
# PCB: the probe writes SYNTHETIC boards (real netlists from
# evals/electronics/fixtures, made-up placement and copper) in the
# wright_eda serde format; the case list embeds them
if [[ ",$FEATURES," == *",electronics,"* ]]; then
  "$PROBE" gen-pcb "$ROOT/evals/electronics/fixtures" "$OUT/pcb"
  # REAL boards too: PCB_REAL=DIR adds every DIR/**/board.json (the
  # `(netlist, board, route)` wright-electronics' build writes), placed by
  # lane C and routed by lane D
  if [ -n "${PCB_REAL:-}" ]; then
    find "$PCB_REAL" -name board.json | while read -r b; do cp "$b" "$OUT/pcb/real-$(basename "$(dirname "$b")").json"; done
  fi
  export PCB_DIR="$OUT/pcb"
fi
python3 -I "$HERE/crosscheck_cases.py" > "$OUT/cases.json"
"$ORACLE" < "$OUT/cases.json" > "$OUT/oracle.json"
"$PROBE" < "$OUT/cases.json" > "$OUT/rust.json"
status=0
python3 -I "$HERE/crosscheck_compare.py" "$OUT/cases.json" "$OUT/oracle.json" "$OUT/rust.json" || status=$?
if [ -n "${KEEP:-}" ]; then echo "results in $OUT"; else rm -rf "$OUT"; fi
exit $status
