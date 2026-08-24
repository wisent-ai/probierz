# Quick start

How do you go from a source checkout to one evidence-producing run? This page
is the one happy path: install, discover, preflight, run, and read the
result. There is no stable public binary; Probierz runs from source.

## Prerequisites

- Git
- Node.js 22 or newer (`package.json` declares `"node": ">=22.0.0"`)
- npm

## Install and discover

```bash
git clone https://github.com/wisent-ai/probierz.git
cd probierz
npm install
node agent/cli.mjs list
node agent/cli.mjs apps
```

`list` prints every test surface with its tool, npm script, targets, and the
environment variables it reads. `apps` prints the validated application
manifests registered under `apps/`. Both are read-only: no browser starts, no
driver installs, no test runs. (`npm install` links the `probierz` and
`probierz-mcp` bins declared in `package.json`; the examples below call
`node agent/cli.mjs` directly, which is the same entry point.)

## Preflight a target

```bash
node agent/cli.mjs check web
```

`check` reports whether the target's toolchain is ready and, for each missing
piece, exactly how to fix it: `probierz setup <target>` for the parts
Probierz owns (npm dependencies, Playwright browsers, Appium drivers) or a
one-line host install hint for the parts it never installs (Xcode, the
Android SDK, simulators, WinAppDriver). `check` is read-only.

```bash
node agent/cli.mjs setup web
```

`setup` installs only the Probierz-owned parts, in order, stopping at the
first failure.

## Run one target

```bash
node agent/cli.mjs run web --app APP_ID --record
```

`run` is preflight-gated: if the toolchain is not ready it returns the
blocking checks and their fixes without spawning anything. A ready run
executes the application's declared spec, records video/trace/screenshots
(`--record`), analyzes the report and media, and writes everything under
`test-results/<appId>/<target>/<date>/<runId>/`. A failing suite is a
recorded result, not a tool error. Run options are listed in
[cli](cli.md#execution); what the run directory contains is in
[evidence-model](evidence-model.md).

## Read the result

```bash
node agent/cli.mjs status APP_ID --text
node agent/cli.mjs history APP_ID
node agent/cli.mjs last-green APP_ID
```

`status` answers merge eligibility for the application: which manifest
journeys have evidence, whether that evidence is fresh against the current
HEAD, whether its level meets the pull-request policy, and the exact blocking
reasons (exit 1 when blocked). `history` aggregates pass rate, flaky tests,
and duration trends from the run manifests on disk. `last-green` prints the
newest passing evidence.

Note: `status`, `apps`, `affected`, and `ci` walk the whole app registry
and are blocked on this checkout by one invalid registered manifest — the
exact failure and the repair are in [limitations](limitations.md). The
per-app projections (`history`, `last-green`) are unaffected.

## Run only what a change affects

```bash
node agent/cli.mjs affected origin/main
node agent/cli.mjs ci origin/main
```

`affected` maps a git diff to the run targets it can touch; `ci` composes
affected → preflight-gated run → analyze and returns one consolidated
verdict. This is the same pass the [pre-push gate](gates-and-receipts.md)
invokes with `--ci`.

## Do it end-to-end, executed

Two captured walkthroughs run this whole page against a demo application on
this checkout: [register an application](walkthrough-register-app.md) and
[run → gate → signed receipt](walkthrough-gate-and-receipt.md); the
runnable scripts live in [examples](examples/README.md).
