#!/bin/sh
set -eu

version='0.19.3'
archive="cua-driver-rs-${version}-darwin-arm64.tar.gz"
url="https://github.com/trycua/cua/releases/download/cua-driver-rs-v${version}/${archive}"
sha256='4f147affe7015dffdb0faeecb784a72d4ff9808b571a2d888231ae11e7966034'
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

curl -fsSL "$url" -o "$tmp/$archive"
printf '%s  %s\n' "$sha256" "$tmp/$archive" | shasum -a 256 -c -
tar -xzf "$tmp/$archive" -C "$tmp"
source="$tmp/cua-driver-rs-${version}-darwin-arm64/CuaDriver.app"

"$HOME/.local/bin/cua-driver" stop --socket "$HOME/Library/Caches/cua-driver/probierz.sock" >/dev/null 2>&1 || true
rm -rf /Applications/CuaDriver.app
/usr/bin/ditto "$source" /Applications/CuaDriver.app
mkdir -p "$HOME/.local/bin"
ln -sfn /Applications/CuaDriver.app/Contents/MacOS/cua-driver "$HOME/.local/bin/cua-driver"
"$HOME/.local/bin/cua-driver" --version
