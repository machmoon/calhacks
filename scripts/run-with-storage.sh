#!/usr/bin/env bash
# Shared desktop completion path; argv is never interpolated into shell code.
# ROOT RUN_DIR DOMAIN REQUEST PRODUCER [ARG...]
set -u
root=$1
run_dir=$2
domain=$3
request=$4
shift 4
source "$root/scripts/env.sh"
"$@"
producer_status=$?
if [ "${WRIGHT_RUNS_URI:-}" != off ]; then
  "$root/target/release/wright" runs sync "$run_dir" --domain "$domain" --request "$request"
  storage_status=$?
  if [ "$storage_status" -ne 0 ] && [ "$producer_status" -eq 0 ]; then
    exit "$storage_status"
  fi
fi
if [ "$producer_status" -ne 0 ]; then
  printf 'Design process exited with status %s; see earlier errors in this run log.\n' "$producer_status" >&2
fi
exit "$producer_status"
