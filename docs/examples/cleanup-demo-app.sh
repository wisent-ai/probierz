#!/bin/sh
# cleanup-demo-app.sh — remove everything the demo examples created:
# the registration, the spec, the demo product, and the demo evidence.
# Run from the probierz checkout root: sh docs/examples/cleanup-demo-app.sh
set -eu

[ -f agent/cli.mjs ] || { echo "run from the probierz checkout root" >&2; exit 2; }

rm -rf apps/docs-demo
rm -f packages/tui/specs/docs-demo-hello.spec.mjs
rm -rf /tmp/probierz-demo-app
rm -rf test-results/docs-demo test-results/receipts/docs-demo
echo "demo application deregistered and its evidence removed"
