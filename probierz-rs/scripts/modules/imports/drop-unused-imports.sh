#!/bin/sh
# Remove the imports the compiler reports as unused, one pass per run.
#
# Splitting a module leaves some parts without a use for an import every other
# part needs. `cargo build` names each one with a file and a line; deleting
# them by hand across a dozen parts is slow and easy to get wrong by one line.
# This reads the compiler's own report and deletes exactly the lines it names.
#
# Usage: scripts/drop-unused-imports.sh
# Run from the crate root. Re-run until it reports nothing left to remove.
set -eu

report=$(cargo build 2>&1 | grep -A 2 'warning: unused import' | grep -- '-->' || true)
if [ -z "$report" ]; then
  printf 'no unused imports reported\n'
  exit 0
fi

# Delete from the bottom of each file upwards so earlier line numbers stay true.
printf '%s\n' "$report" |
  sed -E 's|^ *--> ([^:]+):([0-9]+):[0-9]+$|\1 \2|' |
  sort -u -k1,1 -k2,2nr |
  while read -r file line; do
    [ -f "$file" ] || continue
    sed -i '' "${line}d" "$file"
    printf 'removed %s:%s\n' "$file" "$line"
  done
