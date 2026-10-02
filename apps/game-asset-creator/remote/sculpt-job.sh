#!/bin/bash
# Run Glina's supported pipeline on the Stado-selected worker. The worker
# supplies the live Blender addon; this runner never patches vendor code or
# starts a second, unmanaged Blender process.
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
GAC_ROOT="${GAC_ROOT:-${PROBIERZ_APP_SOURCE:-$(dirname "$REPOSITORY_ROOT")/glina}}"
RESOLVED_CONFIG="${RESOLVED_CONFIG:-$GAC_ROOT/pipeline.config.json}"
SCULPT_PROMPT="${SCULPT_PROMPT:-low-poly boulder, Thronefall style}"
SCULPT_FILENAME="${SCULPT_FILENAME:-sculpt-output.glb}"
RESULTS_DIR="${RESULTS_DIR:-$REPOSITORY_ROOT/test-results/sculpt-job}"
SCULPT_OUT="${SCULPT_OUT:-$RESULTS_DIR/models}"

if [ ! -f "$GAC_ROOT/pipeline/cli.js" ]; then
  printf 'Glina CLI not found: %s/pipeline/cli.js\n' "$GAC_ROOT" >&2
  exit 1
fi
if [ ! -f "$RESOLVED_CONFIG" ]; then
  printf 'Glina configuration not found: %s\n' "$RESOLVED_CONFIG" >&2
  exit 1
fi

# Resolve caller-relative paths before changing to the product directory.
GAC_ROOT="$(cd "$GAC_ROOT" && pwd)"
RESOLVED_CONFIG="$(cd "$(dirname "$RESOLVED_CONFIG")" && pwd)/$(basename "$RESOLVED_CONFIG")"
mkdir -p "$SCULPT_OUT" "$RESULTS_DIR"
SCULPT_OUT="$(cd "$SCULPT_OUT" && pwd)"
RESULTS_DIR="$(cd "$RESULTS_DIR" && pwd)"
cd "$GAC_ROOT"

# Validate before provisioning. The config is passed read-only, not copied
# over the worker's configuration. check-config redacts resolved secrets.
node pipeline/cli.js check-config --config "$RESOLVED_CONFIG" | tee "$RESULTS_DIR/config-report.json"
node pipeline/cli.js setup | tee "$RESULTS_DIR/setup-report.json"
node pipeline/cli.js doctor --config "$RESOLVED_CONFIG" | tee "$RESULTS_DIR/doctor-report.json"

# No arbitrary round limit is introduced by the runner: Glina reads its
# configured policy unless the caller explicitly supplies SCULPT_ROUNDS.
set --
if [ -n "${SCULPT_ROUNDS:-}" ]; then
  set -- --rounds "$SCULPT_ROUNDS"
fi
node pipeline/cli.js sculpt "$SCULPT_PROMPT" \
  --config "$RESOLVED_CONFIG" --out "$SCULPT_OUT" \
  --filename "$SCULPT_FILENAME" "$@" | tee "$RESULTS_DIR/sculpt-result.json"
node pipeline/cli.js verify "$SCULPT_OUT/$SCULPT_FILENAME" \
  --config "$RESOLVED_CONFIG" | tee "$RESULTS_DIR/verify-report.json"

if [ "$SCULPT_OUT" != "$RESULTS_DIR/models" ]; then
  mkdir -p "$RESULTS_DIR/models"
  cp -R "$SCULPT_OUT/." "$RESULTS_DIR/models/"
fi
printf 'sculpt-job completed: %s\n' "$RESULTS_DIR"
