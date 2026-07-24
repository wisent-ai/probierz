#!/bin/sh
# nightly-probierz: nightly hygiene + overview report.
# Scans key repositories with find-violations (report only) and writes a
# unified overview (journeys + eligibility + fleet) to ~/.stado/nightly/.
set -eu

PROBIERZ="/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/probierz"
TAMA_CLI="/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/hooks-rotator/src/cli.mjs"
OUT_DIR="${HOME}/.stado/nightly"
STAMP="$(date +%Y-%m-%dT%H-%M-%S)"
mkdir -p "$OUT_DIR"

REPOS="${PROBIERZ_NIGHTLY_REPOS:-/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/jeden /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/hooks-rotator /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/oko /Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/tama-desktop}"

{
    echo "== nightly ${STAMP} =="
    for repo in $REPOS; do
        echo "--- find-violations ${repo}"
        node "$TAMA_CLI" find-violations --repo "$repo" --json 2>/dev/null \
            | node -e "let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{try{const r=JSON.parse(d);const t=(r.repos||[])[0]||{};console.log('violations:',(t.violations||[]).length,'skipped:',(t.skippedFiles||[]).length,'errors:',(t.errors||[]).length)}catch(e){console.log('scan failed')}})"
    done
    echo "--- overview"
    cd "$PROBIERZ"
    node agent/cli.mjs overview --text
} >> "$OUT_DIR/nightly.log" 2>&1
