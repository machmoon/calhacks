#!/usr/bin/env bash
# Everything that makes Wright's claims checkable, in one command:
#   scripts/verify.sh          # Rust tests, Lean proofs, Haskell oracle, cross-check, evals
#   scripts/verify.sh --quick  # Rust tests only
# Each stage prints PASS/FAIL/SKIP; a missing toolchain is a SKIP that says
# so (never a silent pass), and the script exits non-zero if any stage fails.
set -u
cd "$(dirname "$0")/.."
. "$(dirname "$0")/env.sh"   # WRIGHT_PYTHON, WRIGHT_SU2, PATH
fails=0
stage() {
  local name=$1; shift
  printf '== %-28s ' "$name"
  if out=$("$@" 2>&1); then echo PASS; else echo FAIL; echo "$out" | tail -25; fails=$((fails + 1)); fi
}
skip() { printf '== %-28s SKIP (%s)\n' "$1" "$2"; }

stage "rust tests" cargo test --workspace -q
stage "rust clippy" cargo clippy --workspace --all-targets -q -- -D warnings
[ "${1:-}" = "--quick" ] && exit $fails

if command -v lake >/dev/null; then
  stage "lean proofs (no sorry)" bash -c 'cd lean && out=$(lake build 2>&1) && ! grep -q "declaration uses .sorry" <<<"$out"'
  # every theorem's axioms via `#print axioms` (lake env lean): only propext,
  # Quot.sound, Classical.choice; a declared axiom or sorryAx fails
  stage "lean axioms (#print axioms)" python3 -I scripts/lean_axioms.py lean
else skip "lean proofs" "no lake on PATH; see docs/verification.md"; fi

stage "cross-check comparator tests" python3 -I haskell/crosscheck_compare_test.py

if command -v cabal >/dev/null; then
  stage "haskell oracle tests" bash -c 'cd haskell && cabal test -v0'
  stage "rust vs haskell cross-check" haskell/crosscheck.sh
else skip "haskell oracle" "no cabal on PATH; see docs/verification.md"; fi

for ex in vtol_eval rocket_eval balloon_eval; do
  for c in wright-air wright-rocket wright-balloon; do
    if [ -f "crates/$c/examples/$ex.rs" ]; then stage "eval $ex" cargo run -q -p "$c" --example "$ex"; fi
  done
done

echo "== $fails stage(s) failed"
exit $fails
