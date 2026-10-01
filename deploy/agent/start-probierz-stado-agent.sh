#!/bin/sh
set -eu

# The agent finds its own registry target, so this script names no host.
exec "$HOME/.stado/bin/stado" agent --auto
