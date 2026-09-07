#!/usr/bin/env bash
# The Figma parity journey of @wisent-ai/components, as a Stado job.
#
# Stado unpacks the probierz checkout at work/probierz and the wisent-components
# checkout (git archive HEAD of the submitted repository) at work/wisent-components.
# The outer job script runs this one from work/probierz, so the package sits at
# ../wisent-components. `$JOB_ROOT` is a variable of the outer script and is not
# exported, so nothing here may depend on it.
#
# The package owns this suite: it holds the component inventory, the Figma
# exports and the test server, and its own Playwright config is what runs. This
# script's only job is to put the evidence where probierz collects it — the
# outer script archives work/probierz/test-results — and to return the suite's
# exit code as the job's verdict.
set -euo pipefail

ARTIFACTS="$PWD/test-results/wisent-components/figma-parity"
PACKAGE="$PWD/../wisent-components"
mkdir -p "$ARTIFACTS"

export PROBIERZ_ARTIFACTS="$ARTIFACTS"
export PROBIERZ_REPORT_PATH="$ARTIFACTS/report.json"

cd "$PACKAGE"
npm ci --no-audit --no-fund --loglevel=error
# The package pins its own Playwright; probierz's `setup web` installed the
# browser build that probierz's version wants, which is not necessarily this
# one. Ask this Playwright for the browser it needs.
npx playwright install chromium
# `dist/` is what the test server serves: the stylesheet, the bundled fonts and
# the Figma exports the reference page loads. `npm ci` already runs it through
# `prepare`; running it here is the statement that the suite depends on it.
npm run build
npx playwright test --config playwright.config.mjs
