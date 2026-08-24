# CLI reference — conventions, discovery, execution

`probierz` (source entry point `agent/cli.mjs`; `npm install` links the
`probierz` bin) is the canonical interface. This page covers output and
failure conventions, discovery, the toolchain, and execution. The rest of
the surface is split across:

- [cli-evidence.md](cli-evidence.md) — status, history, comparison,
  protection, retention, audit;
- [cli-gates.md](cli-gates.md) — gates, receipts, publication;
- [cli-authoring-remote.md](cli-authoring-remote.md) — authoring,
  evaluators, remote Stado execution.

`probierz --help` (also bare `probierz`, `help`, `-h`) prints the
authoritative usage on stderr and exits 0.

## Conventions

- **Output** is pretty-printed JSON on stdout, one document per invocation.
  Human renderings exist only where flagged (`status --text`,
  `overview --text`).
- **Failures** produce exactly two stderr lines at the process boundary
  (`agent/failure.mjs`): one greppable structured line and one sentence:

  ```
  probierz-failure {"failure_point":"cli.unknown","error_code":"unknown","service":"cli","impact":"cli","severity":"error","retryable":false,"outage":false,"detail":"unknown command: frobnicate"}
  probierz frobnicate: probierz failed in a way probierz does not recognise. See the detail on the line above.
  ```

  `error_code` is one of `config`, `auth`, `not_found`, `rate_limit`,
  `timeout`, `infra_down`, `unknown`, classified from the failure text;
  `failure_point` names the dependency axis (`stado.upload`,
  `stado.download`, `stado.submit`, `stado.watch`, `stado.worker`,
  `stado.pack`, `objects.config`, `objects.list`, `objects.read`,
  `model.route`, `run.spawn`, `run.report`, `cli.unknown`). `detail` is
  bounded to 300 characters. When a retryable remote failure would strand
  an operator, the sentence appends: `Local runs are unaffected — 'probierz
  run <target>' still works without the stado queue.`

- **Exit codes** (captured on this checkout):

  | Code | Meaning |
  |---|---|
  | `0` | success; for decision commands, a passing verdict |
  | `1` | failing verdict / blocked status / findings / not-found and other classified non-retryable failures |
  | `2` | usage or configuration mistake (`configError`): unknown command/option/target, missing required flag value |
  | `3` | `run` blocked by preflight (never spawned); `ci` with any blocked target and no failures |
  | `69` | retryable infrastructure failure (`EXIT_RETRY`, sysexits `EX_UNAVAILABLE`) — back off and retry |

- **Vocabulary.** Surfaces: `web`, `electron`, `mobile`, `desktop-native`,
  `desktop-cua`, `tui`. Targets: `web`, `electron`, `mobile:ios`,
  `mobile:android`, `desktop:mac`, `desktop:cua`, `desktop:win`, `tui`
  (plus the special `mobile:ios:byk-auth` runner target).

## Discovery (read-only)

### `probierz list`

No arguments. Prints the `SURFACES` table from `agent/lib.mjs`: per surface
`name`, `pkg`, `tool`, `script`, `targets` description, and the env vars the
suite reads. Never fails, exit 0.

### `probierz apps`

No arguments. Loads and validates **every** `apps/*/probierz.yaml`; output
per app: `appId`, `owner`, `file`, sorted `targets`, sorted `journeys`.
Fail-closed: one invalid manifest anywhere fails the whole command with
`invalid app manifest: <file> <reason>` (exit 1). See
[limitations](limitations.md) for the current state of this checkout.

### `probierz app <appId>`

Prints the single validated manifest plus its `file` path. Errors:
`app needs an app ID` (exit 2), `invalid app ID: <id>` (exit 1),
`app manifest not found: <path>` (exit 1, classified `not_found`),
`app manifest ID mismatch: expected <a>, got <b>` (exit 1), any
`invalid app manifest: …` sentence from
[concepts/application](concepts/application.md).

### `probierz source-identity <appId>`

Prints `{ schemaVersion: 1, harness, app }` — the exact identity a gate or
receipt expects: per repository `gitSha`, `dirty`, `worktreeSha256` (SHA-256
over every tracked and untracked-but-not-ignored file's path, kind, mode,
size, and content; harness identity excludes `node_modules/`,
`test-results/`, `.env*`, runtime `probierz-*.json`, and includes
`package-lock.json`), and a combined `sha256`. Error: `source-identity
needs an app ID` (exit 2); a non-git repository root fails with
`source inventory failed for <root>`.

### `probierz specs [surface]`

Spec files discovered on disk per surface (`test/specs`, `tests`, `specs`
dirs; `.e2e.ts`, `.spec.ts`, `.spec.mjs` suffixes). Error:
`unknown surface: <name>`.

### `probierz describe <spec>`

Static outline — `describe` / `it` / `test` titles in file order; nothing is
executed. Errors: `describe needs a spec path`, `path escapes the probierz
root`, `spec not found: <path>`.

### `probierz cmd <target>`

Prints the exact shell command string for a target; never executes it.
**Only** `web`, `electron`, `mobile:ios`, `mobile:android`, `desktop:mac`,
`desktop:win` have entries; `cmd tui` and `cmd desktop:cua` fail with
`unknown target: <t> (one of web, electron, mobile:ios, mobile:android,
desktop:mac, desktop:win)` — captured, see [limitations](limitations.md).

### `probierz accessibility <appId>`

Validates stable IDs and native selectors between app sources and specs;
output `{ ok, sourceFiles, specFiles, identifiers: { defined, explicit,
referenced, unused }, errors }`; exit 1 when `ok` is false.

### `probierz affected [ref] [--files a b c]`

Maps a change to run targets. With `--files`, classifies the given paths;
otherwise diffs the working tree against `ref` (default `HEAD`). Rules
(`agent/affected.mjs`): a file matching an app manifest mapping affects that
app's journey targets; a file inside `packages/<x>` affects that package's
targets; a file under `agent/` or at the repo root is cross-cutting and
affects **all** targets; anything else affects nothing. Output
`{ ref?, targets, crossCutting, files: [{file, affects, apps?}], apps }`.
Errors: `--files needs at least one path` (exit 2); `git diff --name-only
<ref> failed: <stderr>`; walks the app registry, so an invalid registered
manifest fails it.

### `probierz hosts`

Prints the run-host table: `local` plus the `stado:*` selectors with their
capacity request JSON. Read-only; queries nothing.

## Toolchain

### `probierz check <target>`

Runs preflight detection standalone — deterministic binary probes and
filesystem checks, never a network call or browser launch. Output:

```json
{ "target": "tui", "ready": true,
  "checks": [{ "name": "...", "ok": true, "own": false, "hint": "..." }],
  "missing": [], "remediation": [] }
```

`own: true` means `probierz setup <target>` can install it (npm deps,
Playwright browsers, Appium drivers); otherwise `hint` is a one-line host
instruction (Xcode, Android SDK, WinAppDriver, Accessibility grants —
Probierz never installs these). Exit 1 when not ready. Errors:
`check needs a target (one of …)`, `unknown target: <t>` (exit 2).

Captured example of a not-ready target on this machine:

```json
{ "ready": false,
  "missing": ["adb", "ANDROID_HOME set", "appium driver: uiautomator2"],
  "remediation": [
    "install Android SDK platform-tools and add them to PATH",
    "export ANDROID_HOME to your Android SDK location",
    "probierz setup mobile:android" ] }
```

### `probierz setup <target> [--timeout MS]`

Executes the ordered provisioning steps Probierz owns, stopping at the first
failure, then re-runs preflight and prints both. Steps per target
(`agent/preflight.mjs setupSteps`): workspace `npm install` always; then
`playwright install` (`--with-deps` for web), or `appium driver install
xcuitest|uiautomator2|mac2@2.2.2|windows`, or the ScreenCaptureKit recorder
build (`desktop:mac`), or the cua-driver daemon (`desktop:cua`). `tui` needs
only the npm install. Exit 1 on a failed step.

## Execution

### `probierz run <target> [opts] [KEY=VALUE...]`

Runs one target end-to-end: preflight gate → resource leases → manifest
`data.seed` hook → spawn `npm run <script>` → report identity check →
`data.cleanup` → auto-analysis → completion verdict. The full lifecycle and
manifest contract are in [concepts/run](concepts/run.md).

Options (unknown flags are rejected — `unknown option: <flag>`, exit 2):

| Flag | Effect | Default |
|---|---|---|
| `--app <appId>` | resolve the app surface: manifest conditions, `secretRefs` forwarding, `env` renames, declared spec and journeys | `probierz` (no app) |
| `--record` | force capture on (`PROBIERZ_RECORD=1`); makes E3 reachable | off |
| `--force` | skip the preflight gate | off |
| `--spec <path>` | run only this spec (path or substring) | app surface spec |
| `--frames N` | extract N evenly spaced frames per video (needs ffmpeg) | 0 |
| `--timeout MS` | kill the run after MS | 1 200 000 (20 min) |
| `--resource-wait MS` | poll for busy device locks instead of failing | fail immediately |
| `--no-analyze` | skip auto-analysis (no completion verdict) | analyze |
| `KEY=VALUE` | recorded run condition (redacted if sensitive) | — |

Exit codes: `0` passed; `1` failed (including report/analysis integrity
failures); `3` blocked by preflight — the JSON carries
`preflight.missing` and `preflight.remediation` and a `blocked` run manifest
is still written. Numeric flag validation: `--frames needs a non-negative
number`, `--timeout needs a non-negative number`, `--resource-wait needs a
non-negative number` (exit 2). A spawn that cannot start is classified
`run.spawn` / `config`: `Starting the <target> runner failed: …`.

### `probierz analyze <report> [artifactsDir] [--tool playwright|wdio] [--frames N]`

Parses a report (canonical Probierz shape, Playwright, or WDIO — inferred
when `--tool` is omitted), types media from the report itself (never from
file names), enriches with on-disk size and video metadata, writes
`timeline.json`, summarizes diagnostics, and inventories all other files.
Error: `report not found: <path> (did the run produce one?)`; a report
carrying another run's ID fails with `report run ID mismatch: expected <id>,
got <id|missing>` when `runId` is enforced.

### `probierz ci [ref] [--files a b c] [run opts]`

Change-driven pass: affected targets → accessibility checks for affected
apps → preflight-gated runs (device resources serialized in-process and
cross-process) → analysis → one consolidated result
`{ ref?, affected, results, summary: { total, passed, failed, blocked,
ran } }`. Exit `1` when anything failed, else `3` when anything is blocked,
else `0`. This is the same pass the pre-push hook invokes with `--ci`.

### `probierz matrix <appId> <profile> [--plan] [--release id] [KEY=VALUE...]`

Plans or executes a declared condition matrix (profile is typically
`nightly` or `release`). `--plan` prints the expansion without running:
cells are target × sorted scalar dimensions, each with a 16-hex `cellId`;
refusals: `app <id> has no <profile> matrix`, `matrix <profile> expands to
<n> cells (max <maxCells>)`. Execution schedules cells by resource with
bounded parallelism (`maximumParallel`, default 4; `maxCells` default 128),
injects `PROBIERZ_RELEASE` for release matrices (`release matrix execution
needs --release <id>`, exit 2 from the CLI; `release matrix execution needs
a release ID` from the module), refuses axis overrides (`matrix axis <name>
cannot be overridden`), and — when the profile requires encryption —
demands `PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE` (`matrix requires
PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE`) and protects every cell after it
completes. Verdict: `passed` only when every cell passed at
`minimumCellEvidence`; then `evidenceLevel` is `"E4"`. Exit 1 on a failing
executed verdict.

### `probierz readme-gif <video> --out <file.gif> [--start s] [--duration s] [--fps N] [--width px] [--force]`

Renders one bounded, silent, looping journey GIF plus a
`<out>.probierz.json` provenance sidecar (source and output SHA-256, exact
render settings, `reviewRequired: true`). Bounds and refusals:
`readme-gif needs an input video`, `readme-gif needs --out <file.gif>`,
`--out must end in .gif`, `input video and --out must be different files`,
`output already exists: <path>; pass force=true to replace it`,
`durationSeconds must be between 1 and 30`, `framesPerSecond must be between
1 and 15`, `width must be between 1 and 1280`, `README GIF export failed:
<detail>`. Requires ffmpeg (`PROBIERZ_FFMPEG_BIN` selects the binary).
