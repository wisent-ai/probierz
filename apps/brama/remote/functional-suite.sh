#!/bin/bash
# Runs Brama's product-owned end-to-end integration tests inside a Probierz job.
# The app source is staged beside the Probierz checkout by the Stado bridge.
set -euo pipefail

BRAMA_ROOT="${BRAMA_ROOT:-../brama}"
RESULTS_DIR="${RESULTS_DIR:-test-results/brama-functional-suite}"
mkdir -p "$RESULTS_DIR"

cd "$BRAMA_ROOT"
printf '%s\n' "$(rustc --version)" > "../probierz/$RESULTS_DIR/toolchain.txt"
printf '%s\n' "$(cargo --version)" >> "../probierz/$RESULTS_DIR/toolchain.txt"

set +e
cargo test --locked --tests -- --test-threads=1 2>&1 | tee "../probierz/$RESULTS_DIR/cargo-test.log"
status=${PIPESTATUS[0]}
set -e

printf '{"command":"cargo test --locked --tests -- --test-threads=1","exitCode":%d}\n' "$status" \
  > "../probierz/$RESULTS_DIR/result.json"
exit "$status"
