#!/bin/sh
# Move a surface's journey modules into one sub-package per product and
# rewrite the surface's module declarations to match.
#
# One file per journey is the right shape — a journey is read whole — but a
# surface with fifty of them in one directory is not navigable, and the
# repository's own audit refuses it. The product prefix each file already
# carries is the grouping: `stado_host_gates.rs` belongs with the other
# `stado` journeys. This script performs that move for every prefix given,
# so adding a journey later is one more file in an existing group rather
# than a fifty-line directory again.
#
# Usage: scripts/group-spec-modules.sh <surface-dir> <prefix> [prefix...]
#   surface-dir  e.g. src/specs/tui
#   prefix       the product prefix shared by a group, e.g. stado
#
# Each group's members stay separate modules — every journey declares its own
# `run`, so a glob re-export would make the name ambiguous — and the surface
# registry addresses one as `<prefix>::<journey>::run`.
set -eu

if [ "$#" -lt 2 ]; then
  printf 'usage: %s <surface-dir> <prefix> [prefix...]\n' "$0" >&2
  exit 2
fi

surface_dir=$1
shift
surface_entry="$surface_dir.rs"

if [ ! -d "$surface_dir" ] || [ ! -f "$surface_entry" ]; then
  printf 'not a surface: %s with %s\n' "$surface_dir" "$surface_entry" >&2
  exit 1
fi

for prefix in "$@"; do
  group_dir="$surface_dir/$prefix"
  mkdir -p "$group_dir"
  members=""
  for file in "$surface_dir/$prefix"_*.rs; do
    [ -f "$file" ] || continue
    base=$(basename "$file" .rs)
    member=${base#"${prefix}_"}
    git mv "$file" "$group_dir/$member.rs" 2>/dev/null || mv "$file" "$group_dir/$member.rs"
    members="$members $member"
    sed -i '' "/^mod ${base};$/d" "$surface_entry"
    sed -i '' "/^pub mod ${base};$/d" "$surface_entry"
    sed -i '' -E "s/run: ${base}::run,/run: ${prefix}::${member}::run,/" "$surface_entry"
  done
  if [ -z "$members" ]; then
    rmdir "$group_dir" 2>/dev/null || true
    continue
  fi
  {
    printf '//! The %s journeys this surface runs.\n\n' "$prefix"
    for member in $members; do
      printf 'pub(crate) mod %s;\n' "$member"
    done
  } >"$group_dir/mod.rs"
  printf 'mod %s;\n' "$prefix" >>"$surface_entry"
  printf 'grouped %s: %s\n' "$prefix" "$members"
done
