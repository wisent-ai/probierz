#!/bin/bash
# Build the Probierz command-line release for darwin-arm64.
#
# Writes, under $WISENT_OUTPUT_DIR and nowhere else:
#   bin/probierz       the product binary
#   bin/probierz-mcp   the stdio MCP server
#   evidence/DIGESTS   sha256 of both
#
# Those three are exactly the stage keys in .wisent-release.json.
#
# Probierz is a Rust product. It used to ship as a self-extracting shell
# launcher carrying a tarball of JavaScript plus two node_modules trees, which
# is why this file once staged dependencies and verified an unpack. A compiled
# binary needs none of that: what is built here is what runs on the host.
set -euo pipefail

: "${WISENT_SOURCE_DIR:?WISENT_SOURCE_DIR is required}"
: "${WISENT_OUTPUT_DIR:?WISENT_OUTPUT_DIR is required}"
: "${WISENT_VERSION:?WISENT_VERSION is required}"
: "${WISENT_PLATFORM:?WISENT_PLATFORM is required}"

# darwin-arm64 is the only coordinate this repository can honestly claim. The
# binary is compiled by the runner that builds it, and probierz's own surfaces —
# the mac2 and cua drivers, the iOS simulator, the launchd nightly in deploy/ —
# are macOS. Cross-building a coordinate this script cannot produce is how a
# manifest starts lying, so it refuses instead.
host="$(uname -s):$(uname -m)"
if [ "$host" != "Darwin:arm64" ] || [ "$WISENT_PLATFORM" != "darwin-arm64" ]; then
  printf 'probierz releases darwin-arm64 built on darwin-arm64; runner is %s and %s was requested\n' \
    "$host" "$WISENT_PLATFORM" >&2
  exit 1
fi

work="$WISENT_OUTPUT_DIR/work"
source_tree="$work/source"
rm -rf "$work"
# Throwaway state is removed by the code that made it: the unpacked commit
# and its target directory end with the run.
trap 'rm -rf "$work"' EXIT
mkdir -p "$source_tree" "$WISENT_OUTPUT_DIR/bin" "$WISENT_OUTPUT_DIR/evidence"

# The commit, not the working tree. The installer files HEAD as the revision it
# installed and reports the product stale against origin/main from it, so the
# artifact has to be HEAD or that provenance is fiction.
revision="$(git -C "$WISENT_SOURCE_DIR" rev-parse HEAD)"
git -C "$WISENT_SOURCE_DIR" archive --format=tar HEAD | tar -x -C "$source_tree"

# `.wisent-release.json` reads the version out of probierz-rs/Cargo.toml, so
# there is one version and nothing here to reconcile. What still has to be
# proven is that the binary reports it, and that check is below, after it is
# built.

# Locked, offline, release profile: a release that resolves a different
# dependency graph than the checkout was tested against is a different product.
export CARGO_TERM_COLOR=never
export SOURCE_DATE_EPOCH="$(git -C "$WISENT_SOURCE_DIR" show -s --format=%ct HEAD)"
cargo build --locked --release --manifest-path "$source_tree/probierz-rs/Cargo.toml" --bins

built="$source_tree/probierz-rs/target/release"
for binary in probierz probierz-mcp; do
  if [ ! -x "$built/$binary" ]; then
    printf 'cargo did not produce %s from %s\n' "$binary" "$revision" >&2
    exit 1
  fi
  install -m 0755 "$built/$binary" "$WISENT_OUTPUT_DIR/bin/$binary"
done
wisent-products signing sign --product probierz "$WISENT_OUTPUT_DIR/bin/probierz" "$WISENT_OUTPUT_DIR/bin/probierz-mcp"

# An artifact that cannot start is not a release. Each binary answers a
# read-only question: the product prints its command surface, and the MCP
# server answers one discovery call over stdio.
if ! "$WISENT_OUTPUT_DIR/bin/probierz" list >/dev/null; then
  printf 'the built probierz could not answer `list` at %s\n' "$revision" >&2
  exit 1
fi
reported="$("$WISENT_OUTPUT_DIR/bin/probierz" --version | tr -d '\n')"
case "$reported" in
  *"$WISENT_VERSION"*) : ;;
  *)
    printf 'the built probierz reports %s and the release is %s\n' "$reported" "$WISENT_VERSION" >&2
    exit 1
    ;;
esac
discovery='{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
if ! printf '%s\n' "$discovery" \
  | "$WISENT_OUTPUT_DIR/bin/probierz-mcp" 2>/dev/null \
  | grep -q '"tools"'; then
  printf 'the built probierz-mcp did not answer tools/list at %s\n' "$revision" >&2
  exit 1
fi

/usr/bin/shasum -a 256 \
  "$WISENT_OUTPUT_DIR/bin/probierz" \
  "$WISENT_OUTPUT_DIR/bin/probierz-mcp" > "$WISENT_OUTPUT_DIR/evidence/DIGESTS"
