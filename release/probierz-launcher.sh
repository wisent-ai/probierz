#!/bin/bash
# Probierz command-line release @@VERSION@@ (@@PLATFORM@@) — self-extracting launcher.
#
# Probierz's command line is inseparable from its tree. Every module under
# agent/ resolves ROOT as the directory above itself and reads apps/,
# packages/ and test-results/ from there, and `probierz setup` provisions the
# workspace with `npm install` in that same ROOT. A lone executable copied into
# ~/.stado/bin would compute ROOT as ~/.stado and discover nothing, so the
# released artifact carries the tree with it: the gzipped tar below the marker
# is unpacked once per version under the Stado runtime root, and every later
# invocation only execs node against it.
set -euo pipefail

version="@@VERSION@@"
platform="@@PLATFORM@@"
payload_sha256="@@PAYLOAD_SHA256@@"
marker="__PROBIERZ_PAYLOAD__"

# The payload lives inside this file, so the launcher has to find the file
# rather than the name it was invoked by: ~/.stado/bin entries are reached
# through PATH and through symlinks.
self="$0"
while [ -L "$self" ]; do
  link="$(readlink "$self")"
  case "$link" in
    /*) self="$link" ;;
    *) self="$(dirname "$self")/$link" ;;
  esac
done
self="$(cd "$(dirname "$self")" && pwd -P)/$(basename "$self")"

host="$(uname -s):$(uname -m)"
if [ "$host" != "Darwin:arm64" ]; then
  printf 'probierz %s is a %s release; this host is %s\n' "$version" "$platform" "$host" >&2
  exit 1
fi

node_bin="${PROBIERZ_NODE:-}"
if [ -z "$node_bin" ]; then
  node_bin="$(command -v node 2>/dev/null || true)"
fi
if [ -z "$node_bin" ] && [ -x /opt/homebrew/bin/node ]; then
  node_bin=/opt/homebrew/bin/node
fi
if [ -z "$node_bin" ] || [ ! -x "$node_bin" ]; then
  printf 'probierz needs Node on PATH, or PROBIERZ_NODE pointing at it\n' >&2
  exit 1
fi
node_major="$("$node_bin" -p 'process.versions.node.split(".")[0]')"
if [ "$node_major" -lt 22 ]; then
  printf 'probierz needs Node 22 or newer (package.json engines); %s is %s\n' \
    "$node_bin" "$("$node_bin" -v)" >&2
  exit 1
fi

runtime_root="${PROBIERZ_RUNTIME_ROOT:-$HOME/.stado/probierz}"
runtime="$runtime_root/$version-$(printf '%.12s' "$payload_sha256")"
if [ ! -f "$runtime/.probierz-release" ]; then
  # Unpack beside the destination and move into place, so a run interrupted
  # mid-extraction never leaves a half tree that the next run would trust.
  staging="$runtime.staging.$$"
  rm -rf "$staging"
  mkdir -p "$staging"
  trap 'rm -rf "$staging"' EXIT
  sed -n "/^$marker\$/,\$p" "$self" | tail -n +2 | base64 -d | tar -xzf - -C "$staging"
  for required in agent/cli.mjs agent/mcp.mjs package.json node_modules/yaml node_modules/@wisent/errors; do
    if [ ! -e "$staging/$required" ]; then
      printf 'probierz release payload is incomplete: %s is missing\n' "$required" >&2
      exit 1
    fi
  done
  printf 'version=%s\nplatform=%s\npayload_sha256=%s\n' \
    "$version" "$platform" "$payload_sha256" > "$staging/.probierz-release"
  mkdir -p "$runtime_root"
  rm -rf "$runtime"
  mv "$staging" "$runtime"
  trap - EXIT
fi

export PROBIERZ_RELEASE_VERSION="$version"
export PROBIERZ_RELEASE_PLATFORM="$platform"
export PROBIERZ_RELEASE_SHA256="$payload_sha256"
exec "$node_bin" "$runtime/${PROBIERZ_ENTRY:-agent/cli.mjs}" "$@"
exit 0
