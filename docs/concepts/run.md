# Run

Every claim Probierz makes ultimately points at one run: a single execution of
one target's suite, under recorded conditions, that leaves one directory of
bound evidence behind. A run is not "the tests passed" — it is a durable
record whose status is derived from a report that must prove it belongs to
this run.

## What it is

One invocation of `probierz run <target>` (or the same path through `ci`,
`matrix`, the MCP `probierz_run` / `probierz_start_run` tools, or a remote
Stado job). It owns one directory:

```
test-results/<appId>/<target>/<YYYY-MM-DD>/<runId>/
  run-manifest.json   # schemaVersion 2 — the binding record
  report.json         # the framework report, stamped with the run's own ID
  analysis.json       # normalized totals, media, diagnostics
  timeline.json       # merged event timeline
  performance.json    # process sampler output
  stdout.log stderr.log
  media/ frames/ diagnostics/
```

The `runId` is the start timestamp plus a random UUID
(`2026-08-24T22-06-02-464Z-a5bf2f0c-…`); `:` in a target name becomes `-` in
the directory path (`mobile:ios` → `mobile-ios`). The manifest is written
*before* anything spawns and updated atomically (`write to .tmp`, `rename`)
as the run progresses, so a crash mid-run still leaves a readable record.

## Fields

`run-manifest.json` records at start (all from `agent/runner.mjs`):

| Field | Content |
|---|---|
| `runId`, `appId`, `kind`, `target`, `spec` | identity; `kind` is `adhoc` unless `PROBIERZ_RUN_KIND` or a matrix profile says `pull-request` / `release` / `nightly` |
| `harness` | Probierz's own source identity: `gitSha`, `dirty`, `worktreeSha256`, combined `sha256` |
| `source` | the same identity per manifest-declared app repository, plus one combined `sha256` |
| `build` | SHA-256 of the artifact under test: first of `PROBIERZ_BUILD_PATH`, `APP_IOS`, `MAC_APP_PATH`, `ELECTRON_APP_MAIN`, else the harness `package-lock.json` |
| `appManifest` | manifest file, owner, and the journeys this run covers |
| `host`, `device` | hostname, platform, release, arch, Node version; `IOS_DEVICE`/`ANDROID_DEVICE` and runtime when set |
| `conditions` | the requested environment, values under sensitive key names replaced with `[REDACTED:<key>]` |
| `paths`, `resources` | artifact locations and held resource locks |

At completion it gains `status`, `completedAt`, `exitCode`, `signal`,
`timedOut`, `canceled`, `durationMs`, `reportValidation`, `cleanup`,
`performance`, `platformDiagnostics`, `evidence`, `analysisPath`, and
`artifacts[]` — relative path, SHA-256, and byte size of every file the run
left behind (the manifest itself excluded).

## Lifecycle

`status` in the manifest moves through exactly these states:

```
preflight ──▶ blocked            (toolchain not ready, or resource lock refused)
    │
    ├───────▶ canceled           (AbortSignal before or during the spawn)
    │
    ▼
running ────▶ failed             (non-zero exit, timeout, seed failure,
    │                             report missing/foreign, spawn failure)
    ▼
executed ───▶ passed | failed    (completeRun applies the analysis verdict)
```

- **Blocked** runs record the exact failing preflight checks (or the
  conflicting resource lock owner) and never spawn. The CLI exits `3`.
- **Seed hooks**: a manifest `data.seed` command runs first; if it fails the
  run is recorded `failed` with `setupError: "seed failed: <detail>"` and
  `data.cleanup` is attempted as rollback. A seed may return
  `{"env": {...}}` on its last stdout line to inject conditions.
- **Spawn**: `npm run <script>` for the target, in its own process group,
  20-minute default timeout (`--timeout MS`), SIGTERM then SIGKILL after a
  5-second grace. Output streams are secret-redacted, timestamped, and
  persisted with mode `0600`.
- **Pass condition** (`agent/runner.mjs`): exit code 0 AND not timed out AND
  not canceled AND `reportValidation.ok` AND cleanup ok. Then `completeRun`
  additionally requires a valid analysis (see [evidence](evidence.md));
  either gate failing flips the final status to `failed` with explicit
  `evidence.errors`.

## Report identity, not process exit

`reportValidation` refuses a report that:

- does not exist — `"report missing"`;
- is older than the run start — `"report predates run start"`;
- carries another run's stamp — `"report run ID mismatch: expected <id>, got
  <id|missing>"`;
- cannot be parsed — `"report unreadable: <reason>"`.

Suites receive `PROBIERZ_REPORT_PATH` and must stamp
`report.probierz.runId` with the `PROBIERZ_RUN_ID` they were given. A green
child process with a stale or foreign report is a **failed** run.

## Resource locks

Targets that own real devices serialize through directory locks under
`test-results/.locks/` (`agent/locks.mjs`): `mobile:ios` and
`mobile:android` lock `device:...` plus `port:4723`, `desktop:mac`/`win`
lock the host app. A held lock blocks the run with
`resource locked: <resource> by run <id> (pid <pid>)`; `--resource-wait MS`
polls every 250 ms instead of failing immediately. Stale locks (dead pid,
30 s grace) are reclaimed automatically. `web`, `electron`, and `tui` need
no locks.

## Asynchronous runs

The MCP server also runs this lifecycle detached: `probierz_start_run`
returns a `runId` immediately, `probierz_run_status` reports
queued/running/blocked/passed/failed/canceled, `probierz_cancel_run` aborts
the whole process tree. The job table is an in-process `Map` in
`agent/control.mjs` — it does not survive a server restart, but the run
directories on disk do. There is no persistent run queue or intake daemon on
this branch; see [limitations](../limitations.md).

## Invariants

- A run directory is never reused; every attempt gets a fresh `runId`.
- The manifest exists from before the spawn; there is no such thing as a run
  that "left nothing" — blocked and canceled runs are first-class records.
- Secret-looking condition values (`auth|cookie|credential|email|gmail|key|
  otp|password|secret|session|token`) never reach the persisted manifest or
  logs unredacted.
- Artifact hashes are computed after completion; anything later mutated in
  the directory is caught by gates and receipts re-hashing.

## Commands

```bash
probierz run tui --app <appId> [--record] [--spec f] [--timeout MS] [KEY=VALUE...]
probierz analyze <report.json> [artifactsDir]
probierz history <appId>            # projections over run manifests
probierz compare <left> <right> <appId>
```

## Not to be confused with

- **A [journey](journey.md)** — the declared behavior a run covers; one run
  may cover several journeys.
- **An [evidence item](evidence.md)** — the graded strength of what a run
  proved; the run is the record, the level is the grade.
- **A [verdict](verdict.md)** — a gate's decision over one or more runs.
