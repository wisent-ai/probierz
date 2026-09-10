#!/bin/sh
# Prepend one import line to each file that does not already have it.
#
# The companion of scripts/add-module-prelude.sh: a split module's parts often
# also need one item that cannot travel through a glob re-export, such as the
# `serde_json::json` macro. Adding it by hand to a dozen parts is how one part
# ends up missing it and the build fails on the last file.
#
# Usage: scripts/add-import.sh 'use serde_json::json;' <file> [file...]
set -eu

if [ "$#" -lt 2 ]; then
  printf 'usage: %s <import line> <file> [file...]\n' "$0" >&2
  exit 2
fi

line=$1
shift

for file in "$@"; do
  if [ ! -f "$file" ]; then
    printf 'not a file: %s\n' "$file" >&2
    exit 1
  fi
  if grep -qxF "$line" "$file"; then
    continue
  fi
  printf '%s\n' "$line" >"$file.import"
  cat "$file" >>"$file.import"
  mv "$file.import" "$file"
done
