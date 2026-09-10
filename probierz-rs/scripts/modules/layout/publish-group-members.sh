#!/bin/sh
# Declare every member of a grouped package visible to the crate.
#
# A journey group's entry lists its members with `mod <journey>;`, which keeps
# them private to the group; the surface registry addresses them directly, so
# each has to be `pub(crate) mod`. This rewrites the entries of the groups
# given, and is safe to re-run.
#
# Usage: scripts/modules/layout/publish-group-members.sh <group-dir> [group-dir...]
set -eu

if [ "$#" -lt 1 ]; then
  printf 'usage: %s <group-dir> [group-dir...]\n' "$0" >&2
  exit 2
fi

for dir in "$@"; do
  entry="$dir/mod.rs"
  [ -f "$entry" ] || continue
  sed -i '' -E 's/^mod ([a-z_0-9]+);$/pub(crate) mod \1;/' "$entry"
  sed -i '' -E '/^pub\(crate\) use [a-z_0-9]+::\*;$/d' "$entry"
  printf 'published %s\n' "$entry"
done
