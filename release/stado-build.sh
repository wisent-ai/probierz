#!/bin/bash
# Build the Probierz command-line release for darwin-arm64.
#
# Writes, under $WISENT_OUTPUT_DIR and nowhere else:
#   bin/probierz       self-extracting launcher carrying the released tree
#   bin/probierz-mcp   the same tree, entered at the stdio MCP server
#   evidence/DIGESTS   sha256 of both, plus of the payload they carry
#
# Those three are exactly the stage keys in .wisent-release.json.
set -euo pipefail

: "${WISENT_SOURCE_DIR:?WISENT_SOURCE_DIR is required}"
: "${WISENT_OUTPUT_DIR:?WISENT_OUTPUT_DIR is required}"
: "${WISENT_VERSION:?WISENT_VERSION is required}"
: "${WISENT_PLATFORM:?WISENT_PLATFORM is required}"

# darwin-arm64 is the only coordinate this repository can honestly claim. The
# payload is assembled from the checkout that builds it, and probierz's own
# surfaces — the mac2 and cua drivers, the iOS simulator, the launchd nightly
# in deploy/ — are macOS. Cross-building a coordinate this script cannot
# produce is how a manifest starts lying, so it refuses instead.
host="$(uname -s):$(uname -m)"
if [ "$host" != "Darwin:arm64" ] || [ "$WISENT_PLATFORM" != "darwin-arm64" ]; then
  printf 'probierz releases darwin-arm64 built on darwin-arm64; runner is %s and %s was requested\n' \
    "$host" "$WISENT_PLATFORM" >&2
  exit 1
fi

work="$WISENT_OUTPUT_DIR/work"
tree="$work/tree"
rm -rf "$work"
mkdir -p "$tree" "$WISENT_OUTPUT_DIR/bin" "$WISENT_OUTPUT_DIR/evidence"

# The commit, not the working tree. The installer files HEAD as the revision it
# installed and reports the product stale against origin/main from it, so the
# artifact has to be HEAD or that provenance is fiction.
git -C "$WISENT_SOURCE_DIR" archive --format=tar HEAD | tar -x -C "$tree"

# agent/ statically imports exactly two packages. Everything heavier —
# playwright, webdriverio, @wdio/globals — is imported dynamically by the
# commands that execute a suite, and every `probierz setup <target>` begins
# with `npm install` in the released tree, so the release ships the two and
# lets setup fetch the rest against the package-lock.json it carries.
for dependency in yaml @wisent/errors; do
  installed="$WISENT_SOURCE_DIR/node_modules/$dependency"
  if [ ! -d "$installed" ]; then
    printf 'runtime dependency %s is not installed; run npm ci in %s first\n' \
      "$dependency" "$WISENT_SOURCE_DIR" >&2
    exit 1
  fi
  mkdir -p "$tree/node_modules/$(dirname "$dependency")"
  rm -rf "${tree:?}/node_modules/$dependency"
  cp -R "$installed" "$tree/node_modules/$dependency"
done

payload="$work/probierz-runtime.tar.gz"
COPYFILE_DISABLE=1 tar --format=ustar -czf "$payload" -C "$tree" .
payload_sha256="$(/usr/bin/shasum -a 256 "$payload")"
payload_sha256="${payload_sha256%% *}"

launcher="$WISENT_OUTPUT_DIR/bin/probierz"
sed -e "s|@@VERSION@@|$WISENT_VERSION|g" \
    -e "s|@@PLATFORM@@|$WISENT_PLATFORM|g" \
    -e "s|@@PAYLOAD_SHA256@@|$payload_sha256|g" \
    "$WISENT_SOURCE_DIR/release/probierz-launcher.sh" > "$launcher"
printf '__PROBIERZ_PAYLOAD__\n' >> "$launcher"
# base64 rather than a raw append: the launcher stays a text file that `sed`
# can split at the marker on any host, with no byte offset to keep in step with
# the header above it.
base64 < "$payload" >> "$launcher"
chmod 0755 "$launcher"

install -m 0755 "$WISENT_SOURCE_DIR/release/probierz-mcp-launcher.sh" \
  "$WISENT_OUTPUT_DIR/bin/probierz-mcp"

# An artifact that cannot start is not a release. The launcher unpacks into a
# throwaway root here and answers a read-only command, which loads every module
# agent/cli.mjs imports statically — the one failure this cannot be allowed to
# ship past is a committed module importing a file that was never committed,
# because on the machine that built it the file is right there on disk.
if ! PROBIERZ_RUNTIME_ROOT="$work/verify" "$WISENT_OUTPUT_DIR/bin/probierz" list >/dev/null; then
  printf 'the built launcher could not run `probierz list` from a clean unpack of %s\n' \
    "$(git -C "$WISENT_SOURCE_DIR" rev-parse HEAD)" >&2
  exit 1
fi

/usr/bin/shasum -a 256 \
  "$WISENT_OUTPUT_DIR/bin/probierz" \
  "$WISENT_OUTPUT_DIR/bin/probierz-mcp" \
  "$payload" > "$WISENT_OUTPUT_DIR/evidence/DIGESTS"
