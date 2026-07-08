---
name: probierz
description: Use probierz to see, drive, and analyze the Wisent cross-platform test toolkit - web (Playwright), Electron, mobile (iOS/Android via WebdriverIO+Appium), and native desktop (macOS/Windows). It ships a CLI and a stdio MCP server with two layers - a read-only discovery layer that lists the test surfaces and their spec files, outlines a spec's describe/it titles statically, and emits the exact run command; and an execution layer that actually runs a target under chosen conditions (BASE_URL, locale, color-scheme, device), records video/trace/screenshots, and analyzes what the run produced (pass/fail, failure reasons, media inventory with recording metadata). Use it to find what E2E coverage exists for an app (e.g. Byk iOS), to run a suite and capture a recording, or to inspect a finished run. Executing a suite needs Chromium/Appium/a simulator and is heavy + side-effecting.
---

# probierz

probierz (Polish for the assayer's touchstone - the tool that tests purity) is
the Wisent cross-platform test toolkit: one npm-workspaces monorepo that drives
web, Electron, mobile, and native-desktop end-to-end tests. Each surface is
env-var driven, with no hardcoded targets.

## Two layers

- **Discovery (read-only):** list surfaces, discover spec files on disk, outline
  a spec's describe/it titles statically, and print the exact run command. No
  side effects.
- **Execution + analysis:** actually run a target under chosen conditions,
  record video/trace/screenshots, and analyze the result. A live run needs
  Chromium, Appium, or an iOS/Android/desktop target and is a heavy
  side-effecting action - only the `run` tool reaches it; discovery never does.

## Surfaces

| Surface | Tool | Package |
| --- | --- | --- |
| `web` | Playwright (Chromium / Firefox / WebKit + emulated mobile) | `packages/web` |
| `electron` | Playwright (`_electron`) | `packages/electron` |
| `mobile` | WebdriverIO + Appium (XCUITest / UiAutomator2) | `packages/mobile` |
| `desktop-native` | WebdriverIO + Appium (Mac2 / WinAppDriver) | `packages/desktop-native` |

Single sources of truth, imported by both the CLI and the MCP server:
`agent/lib.mjs` (discovery - surfaces, specs, run-command strings),
`agent/runner.mjs` (execution - spawns a suite), `agent/analyze.mjs` (analysis -
parses the report + inventories media).

## CLI

```bash
# discovery (read-only)
probierz list                 # the four surfaces + tool, npm script, targets, env
probierz specs [surface]      # e2e/spec files discovered on disk (optional filter)
probierz describe <spec>      # static outline (describe/it titles) of a spec file
probierz cmd <target>         # the exact shell command to run a target yourself

# execution + analysis
probierz run <target> [opts]  # EXECUTE a target, capture the result, auto-analyze
probierz analyze <report> [dir] [--tool playwright|wdio] [--frames N]
```

Targets: `web`, `electron`, `mobile:ios`, `mobile:android`, `desktop:mac`,
`desktop:win`.

`run` options: `--record` (force video+trace+screenshot on), `--frames N`
(extract N frames per recorded video, needs ffmpeg), `--timeout MS`,
`--no-analyze`, and any `KEY=VALUE` condition env (e.g. `BASE_URL=...`,
`APP_IOS=...`, `PROBIERZ_LOCALE=...`, `PROBIERZ_COLOR_SCHEME=dark`).

## MCP

Run the stdio server with:

```bash
probierz-mcp
# or
node agent/mcp.mjs
```

It speaks the same protocol every Wisent surface speaks - `initialize`, `ping`,
`tools/list`, `tools/call` - as newline-delimited JSON-RPC 2.0 on stdio, one
response per request, diagnostics on stderr. Tools:

- `probierz_list_surfaces` - the four surfaces with tool, script, targets, env.
- `probierz_list_specs` - spec files on disk; optional `surface` filter.
- `probierz_describe_spec` - static describe/it/test outline of a spec by path.
- `probierz_run_command` - the exact command string for a target (never run).
- `probierz_run` - EXECUTE a target end-to-end under chosen conditions, record
  when `record=true`, and return the run result plus an analysis. Args:
  `target` (required), `record`, `env` (condition vars), `timeoutMs`, `frames`,
  `analyze`.
- `probierz_analyze` - parse a finished run's report + inventory its media. Args:
  `reportPath` (required), `artifactsDir`, `tool`, `frames`.

## Recording

`run --record` (or `record=true`) sets `PROBIERZ_RECORD=1`, which the configs
read:

- **Playwright (web):** video + trace + screenshot forced on; a JSON reporter
  writes `report.json`. Conditions via `PROBIERZ_LOCALE` /
  `PROBIERZ_COLOR_SCHEME` and the browser/device projects.
- **Playwright (electron):** trace + screenshot (Playwright video is a
  browser-context feature and does not attach to Electron windows).
- **WDIO (mobile / desktop-native):** per-test Appium screen recording written
  as `<slug>.mp4` under the artifacts dir, plus a `probierz-<kind>-results.json`
  summary. Best-effort: drivers without screen recording (often Mac2 /
  WinAppDriver) degrade silently and never fail the run.

Artifacts land in each package's `test-results/`. `analyze` parses the report,
classifies media (video / screenshot / trace) with sizes, pulls recording
metadata (duration/dimensions via ffprobe), and can extract a frame montage
(ffmpeg). ffprobe/ffmpeg are optional - missing binaries just omit that detail.

## Operational rules

- Discovery is read-only; `run` is the only path that executes a suite or writes
  artifacts. `cmd` / `probierz_run_command` still return a string to run
  yourself - use them when you want the invocation without running it.
- Keep MCP and CLI stdout clean: only JSON-RPC frames and command output on
  stdout; diagnostics on stderr.
- `agent/lib.mjs` (discovery) and `agent/runner.mjs` (execution) are the single
  sources of truth for surfaces/targets - add one there, not scattered across
  the CLI and server.
- Authoring or editing a spec under `test/` is gated by the harness
  device-level-test consent (DEVICE_LEVEL_TESTS_APPROVED set outside the
  session). Running an existing suite via `run` is not spec authoring; it does
  need the real toolchain (Chromium/Appium/simulator) present.
