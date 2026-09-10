#!/bin/sh
# Prepend a module's shared prelude to each part of a split module.
#
# A module that was one file keeps one import list: the package entry
# re-exports it, and every part opens with `use crate::<module>::*;`. Splitting
# a 4000-line file produces a dozen parts, and adding that line by hand to each
# one is how a part ends up with the wrong prelude. This script is the reusable
# way to do it, and it refuses rather than guessing.
#
# Usage: scripts/add-module-prelude.sh <module-path> <file> [file...]
#   module-path  the Rust path of the package, e.g. `crate::stado`
#   file         a part of that package
#
# A file that already opens with the prelude is left alone, so re-running this
# after adding one more part is safe.
set -eu

if [ "$#" -lt 2 ]; then
  printf 'usage: %s <module-path> <file> [file...]\n' "$0" >&2
  exit 2
fi

module=$1
shift
prelude="use $module::*;"

for file in "$@"; do
  if [ ! -f "$file" ]; then
    printf 'not a file: %s\n' "$file" >&2
    exit 1
  fi
  if head -1 "$file" | grep -qxF "$prelude"; then
    continue
  fi
  printf '%s\n' "$prelude" >"$file.prelude"
  cat "$file" >>"$file.prelude"
  mv "$file.prelude" "$file"
done
