#!/bin/sh
set -eu

version='0.2.0'
archive="cua-driver-${version}-darwin-arm64.tar.gz"
url="https://github.com/trycua/cua/releases/download/cua-driver-v${version}/${archive}"
sha256='18c9fb20dcddfe703a55ed99aede4ca3d8fe5aee38afd20c3731acb10f6f4478'
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

curl -fsSL "$url" -o "$tmp/$archive"
printf '%s  %s\n' "$sha256" "$tmp/$archive" | shasum -a 256 -c -
tar -xzf "$tmp/$archive" -C "$tmp"
source="$tmp/CuaDriver.app"

"$HOME/.local/bin/cua-driver" stop --socket "$HOME/Library/Caches/cua-driver/probierz.sock" >/dev/null 2>&1 || true
rm -rf /Applications/CuaDriver.app
/usr/bin/ditto "$source" /Applications/CuaDriver.app
mkdir -p "$HOME/.local/bin"
ln -sfn /Applications/CuaDriver.app/Contents/MacOS/cua-driver "$HOME/.local/bin/cua-driver"
"$HOME/.local/bin/cua-driver" --version
