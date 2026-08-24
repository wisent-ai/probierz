# Execution

`probierz run <target>` is the only local path that executes anything.
Discovery (`list`, `apps`, `specs`, `describe`, `cmd`) and `check` are
read-only by construction; execution is a separate, explicitly
side-effecting layer.

## Targets and drivers

| Target | Driver | Workspace package |
|---|---|---|
| `web` | Playwright (Chromium, Firefox, WebKit, emulated mobile) | `packages/web` |
| `electron` | Playwright `_electron` | `packages/electron` |
| `mobile:ios` | WebdriverIO + Appium (XCUITest) | `packages/mobile` |
| `mobile:android` | WebdriverIO + Appium (UiAutomator2) | `packages/mobile` |
| `desktop:mac` | WebdriverIO + Appium (Mac2) | `packages/desktop-native` |
| `desktop:win` | WebdriverIO + Appium (WinAppDriver) | `packages/desktop-native` |
| `desktop:cua` | `cua-driver` (Accessibility-based macOS automation) | `packages/desktop-cua` |
| `tui` | Playwright-driven terminal harness | `packages/tui` |

Each target maps to one root npm script (`test:web`, `test:electron`,
`test:mobile:ios`, …); `probierz cmd <target>` prints the exact command
without running it.

## Preflight, check, setup

Every run is preflight-gated. `probierz check <target>` runs the same
detection standalone: each check reports `ok`, whether Probierz owns the fix
(`probierz setup <target>` installs npm dependencies, Playwright browsers,
and Appium drivers), or a one-line host install hint for what Probierz never
installs (Xcode and command-line tools, the Android SDK, iOS simulators,
WinAppDriver, physical devices, OS permissions). Detection is deterministic —
binary probes and filesystem checks (for example
`$APPIUM_HOME/node_modules/appium-<name>-driver`), never a network call or a
browser launch. A run whose preflight fails is recorded as `blocked` with the
exact missing checks and never spawns; `--force` overrides detection when it
is wrong. Readiness is not evidence: only a completed run produces evidence.

## What a run does

1. Resolves the application surface: manifest `conditions`, forwarded
   `secretRefs` variables, and `env` renames merge with the caller's
   `KEY=VALUE` conditions; the surface's declared `spec` scopes the suite.
2. Writes the initial `run-manifest.json` with the full source, build, host,
   and condition binding ([evidence-model](evidence-model.md)).
3. Acquires resource locks: mobile and native-desktop targets take
   exclusive locks on their device identity and the Appium port so
   concurrent runs queue instead of corrupting each other
   (`--resource-wait MS` bounds the wait); a run blocked on a busy resource
   is recorded as `blocked` with its owner.
4. Runs the manifest's `data.seed` hook if declared; a failed seed rolls
   back through `data.cleanup` and records a failed run.
5. Spawns the target's npm script with `PROBIERZ_APP_ID`, `PROBIERZ_RUN_ID`,
   `PROBIERZ_ARTIFACTS`, `PROBIERZ_REPORT_PATH`, `PROBIERZ_JOURNEYS`, and —
   with `--record` — `PROBIERZ_RECORD=1`. The default timeout is 20 minutes
   (`--timeout MS`); on timeout or cancel the whole process tree is
   terminated.
6. Runs `data.cleanup`, validates the report's identity, analyzes the
   result, hashes every artifact, and finalizes the manifest.

Full redacted stdout/stderr streams are persisted in the run directory;
command responses carry bounded tails only.

## Recording

`--record` forces capture on; what that means is driver-specific:

- **Playwright (web):** video + trace + screenshot, plus the JSON reporter's
  `report.json`.
- **Playwright (Electron):** trace + screenshot; Playwright video is a
  browser-context feature and does not attach to Electron windows.
- **WebdriverIO (mobile, native desktop):** per-test Appium screen recording
  plus a `probierz-<kind>-results.json` summary; drivers without screen
  recording (often Mac2 / WinAppDriver) degrade silently and never fail the
  run — but a run that *requested* recording and produced no report-typed
  capture cannot pass, and cannot reach `E3`.

## Analysis

`probierz analyze <report> [dir]` (run automatically unless `--no-analyze`)
normalizes the Playwright or WebdriverIO report into one summary: totals,
per-test status, failure reasons, and a media list typed from the report's
own attachment metadata — never guessed from file names — enriched with
sizes and recording metadata (duration and dimensions via ffprobe, an
optional frame montage via ffmpeg with `--frames N`; both best-effort, a
missing binary omits the detail). A run-ID mismatch between report and run
is a hard integrity failure.

## Change-driven selection

`probierz affected [ref]` maps a git diff (or explicit `--files`) to
affected run targets: structural containment for package files, manifest
mappings for application files; `agent/` and repo-root files are
cross-cutting and affect all targets. `probierz ci [ref]` composes
affected → preflight-gated run → analyze into one consolidated verdict
(`{passed, failed, blocked, ran}`); blocked targets are reported with their
fix, not spawned. This is what the pre-push gate's `--ci` invokes.

## Failure classification

A failed run distinguishes product failures from infrastructure failures
(driver, toolchain, timeout) in its recorded failure classification;
`probierz history` reports the two separately, and infrastructure failures
do not count against journey pass rates.
