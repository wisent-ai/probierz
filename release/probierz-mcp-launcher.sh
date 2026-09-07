#!/bin/bash
# Probierz stdio MCP server — the second command package.json declares.
#
# It shares the released tree with `probierz`, so it shares the launcher too:
# the payload is unpacked once and this only chooses the other entry point.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd -P)"
export PROBIERZ_ENTRY="agent/mcp.mjs"
exec "$here/probierz" "$@"
