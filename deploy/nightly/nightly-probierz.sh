#!/bin/sh
# nightly-probierz: nightly hygiene + overview report.
# Scans key repositories with find-violations (report only) and writes a
# unified overview (journeys + eligibility + fleet) to ~/.stado/nightly/.
set -eu

PROBIERZ="/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/probierz"
PROBIERZ_CLI="${PROBIERZ_CLI:-probierz}"
# Tama is a product with a command, not a file in a checkout. This used to run
# `node .../hooks-rotator/src/cli.mjs`, which stopped existing when Tama became
# a Rust binary — every nightly scan since has printed "scan failed".
TAMA_CLI="${TAMA_CLI:-tama}"
OUT_DIR="${HOME}/.stado/nightly"
STAMP="$(date +%Y-%m-%dT%H-%M-%S)"
mkdir -p "$OUT_DIR"

REPOS="${PROBIERZ_NIGHTLY_REPOS:-/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/jeden /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/hooks-rotator /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/oko /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/tama-desktop}"

{
    echo "== nightly ${STAMP} =="
    for repo in $REPOS; do
        echo "--- find-violations ${repo}"
        if command -v "$TAMA_CLI" >/dev/null 2>&1; then
            "$TAMA_CLI" find-violations --repo "$repo" 2>&1 || echo "scan refused: $repo"
        else
            echo "scan skipped: ${TAMA_CLI} is not installed; install Tama or set TAMA_CLI"
        fi
    done
    echo "--- overview"
    cd "$PROBIERZ"
    "$PROBIERZ_CLI" overview --text
    echo "--- seo"
    if [ -n "${PROBIERZ_SEO_BASE_URL:-}" ]; then
        : "${PROBIERZ_SEO_PRIMARY_MODEL:?nightly SEO needs PROBIERZ_SEO_PRIMARY_MODEL}"
        : "${PROBIERZ_SEO_SECONDARY_MODEL:?nightly SEO needs PROBIERZ_SEO_SECONDARY_MODEL}"
        : "${PROBIERZ_SEO_ADJUDICATOR_MODEL:?nightly SEO needs PROBIERZ_SEO_ADJUDICATOR_MODEL}"
        "$PROBIERZ_CLI" stado seo landing-page \
            --base-url "$PROBIERZ_SEO_BASE_URL" \
            --mode nightly \
            --primary-model "$PROBIERZ_SEO_PRIMARY_MODEL" \
            --secondary-model "$PROBIERZ_SEO_SECONDARY_MODEL" \
            --adjudicator-model "$PROBIERZ_SEO_ADJUDICATOR_MODEL" \
            --agent-id "${PROBIERZ_MODEL_AGENT_ID:-probierz}" \
            --host "${PROBIERZ_SEO_HOST:-stado:mini}"
    else
        echo "seo: not configured (PROBIERZ_SEO_BASE_URL is empty)"
    fi
} >> "$OUT_DIR/nightly.log" 2>&1
