#!/bin/sh
# Point a surface registry at the grouped journey modules.
#
# `scripts/group-spec-modules.sh` moves `stado_host_gates.rs` to
# `stado/host_gates.rs`; the registry still says `run: stado_host_gates::run`.
# This rewrites those entries to `run: stado::host_gates::run` for every
# prefix given, and declares each group's members public to the surface.
#
# Usage: scripts/repoint-spec-registry.sh <surface-entry> <prefix> [prefix...]
set -eu

if [ "$#" -lt 2 ]; then
  printf 'usage: %s <surface-entry> <prefix> [prefix...]\n' "$0" >&2
  exit 2
fi

entry=$1
shift

if [ ! -f "$entry" ]; then
  printf 'not a file: %s\n' "$entry" >&2
  exit 1
fi

for prefix in "$@"; do
  sed -i '' -E "s/run: ${prefix}_([a-z_0-9]+)::run,/run: ${prefix}::\1::run,/g" "$entry"
done
