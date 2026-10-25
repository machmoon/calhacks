#!/usr/bin/env bash
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/scripts" "$fixture/target/release" "$fixture/run"
printf '%s\n' 'export WRIGHT_RUNS_URI="${WRIGHT_RUNS_URI:-mock}"' > "$fixture/scripts/env.sh"
cat > "$fixture/target/release/wright" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$3/archive-argv.txt"
exit "${ARCHIVE_TEST_EXIT:-0}"
FAKE
chmod +x "$fixture/target/release/wright"
set +e
bash "$repo/scripts/run-with-storage.sh" "$fixture" "$fixture/run" hardware 'literal ; $(not-a-command)' /bin/sh -c 'exit 7'
status=$?
set -e
[ "$status" = 7 ]
[ "$(sed -n '1p' "$fixture/run/archive-argv.txt")" = runs ]
[ "$(sed -n '2p' "$fixture/run/archive-argv.txt")" = sync ]
[ "$(sed -n '7p' "$fixture/run/archive-argv.txt")" = 'literal ; $(not-a-command)' ]
set +e
ARCHIVE_TEST_EXIT=9 bash "$repo/scripts/run-with-storage.sh" "$fixture" "$fixture/run" hardware test /bin/sh -c 'exit 0'
status=$?
set -e
[ "$status" = 9 ]
rm "$fixture/run/archive-argv.txt"
WRIGHT_RUNS_URI=off bash "$repo/scripts/run-with-storage.sh" "$fixture" "$fixture/run" hardware test /bin/sh -c 'exit 0'
[ ! -e "$fixture/run/archive-argv.txt" ]
printf '%s\n' 'PASS: failed producer still archived; cloud failure propagated; arguments preserved; opt-out honored'
