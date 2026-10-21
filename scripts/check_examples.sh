#!/usr/bin/env bash
# Runs every examples/*.jsonl through the release binary and diffs the output
# against the matching *.expected.jsonl file.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --quiet
bin=target/release/fleet-sim
status=0

for input in examples/*.jsonl; do
  case "$input" in *.expected.jsonl) continue ;; esac
  expected="${input%.jsonl}.expected.jsonl"
  if diff <("$bin" < "$input") "$expected" > /dev/null; then
    echo "PASS  $input"
  else
    echo "FAIL  $input"
    diff <("$bin" < "$input") "$expected" | head -20
    status=1
  fi
done
exit $status
