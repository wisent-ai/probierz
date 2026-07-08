---
name: probierz
description: Use probierz to see and drive the Wisent cross-platform test toolkit - web (Playwright), Electron, mobile (iOS/Android via WebdriverIO+Appium), and native desktop (macOS/Windows). It ships a read-only agent surface (a CLI and a stdio MCP server) that discovers the test surfaces and their spec files, outlines a spec's describe/it titles statically, and emits the exact command to run a target. Use it to find what E2E coverage exists for an app (e.g. Byk iOS), to see which specs live where, or to get the precise run invocation. probierz never executes a suite - running one needs Chromium/Appium/a simulator and stays a manual step.
---

# probierz

probierz (Polish for the assayer's touchstone - the tool that tests purity) is
the Wisent cross-platform test toolkit: one npm-workspaces monorepo that drives
web, Electron, mobile, and native-desktop end-to-end tests. Each surface is
env-var driven, with no hardcoded targets.

Its agent surface is **read-only**: it discovers and reports, and it prints the
command to run a suite, but it never runs one - a live run needs Chromium,
Appium, or an iOS/Android/desktop target and is a heavy side-effecting action
kept out of this surface (mirroring how echo / skarbiec / stado expose only
reads).

## Surfaces

| Surface | Tool | Package |
| --- | --- | --- |
| `web` | Playwright (Chromium / Firefox / WebKit + emulated mobile) | `packages/web` |
| `electron` | Playwright (`_electron`) | `packages/electron` |
| `mobile` | WebdriverIO + Appium (XCUITest / UiAutomator2) | `packages/mobile` |
| `desktop-native` | WebdriverIO + Appium (Mac2 / WinAppDriver) | `packages/desktop-native` |

The registry of surfaces and run commands is the single source of truth:
`agent/lib.mjs`. Both the CLI and the MCP server import it.

## CLI

```bash
probierz list                 # the four surfaces + tool, npm script, targets, env
probierz specs [surface]      # e2e/spec files discovered on disk (optional filter)
probierz describe <spec>      # static outline (describe/it titles) of a spec file
probierz cmd <target>         # the exact shell command to run a target yourself
```

Targets for `cmd`: `web`, `electron`, `mobile:ios`, `mobile:android`,
`desktop:mac`, `desktop:win`.

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

## Operational rules

- Read-only: probierz discovers and reports; it never executes a suite or
  mutates anything. `cmd` / `probierz_run_command` return a string to run
  yourself.
- Keep MCP and CLI stdout clean: only JSON-RPC frames and command output on
  stdout; diagnostics on stderr.
- `agent/lib.mjs` is the single source of truth for surfaces and run commands -
  add a surface there, not scattered across the CLI and server.
- Authoring or editing a spec under `test/` is gated by the harness
  device-level-test consent (DEVICE_LEVEL_TESTS_APPROVED set outside the
  session); probierz's agent surface only reads specs, so it is unaffected.
