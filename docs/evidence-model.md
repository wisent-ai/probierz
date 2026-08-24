# Evidence model

Every claim Probierz makes about quality resolves to a run directory on
disk. This page defines what one run binds to, how evidence strength is
graded, and which projections read that evidence back.

## The run record

`probierz run` writes one directory per run:

```
test-results/<appId>/<target>/<YYYY-MM-DD>/<runId>/
  run-manifest.json   # schemaVersion 2 — the binding record
  report.json         # machine-readable framework report
  analysis.json       # normalized totals, media, diagnostics
  stdout.log stderr.log
  media/ frames/ diagnostics/
```

The `runId` is the run's start timestamp plus a random UUID (path-safe
characters only; `:` in a target directory name becomes `-`). The run
manifest is written before anything executes and updated atomically as the
run progresses.

## What a run binds to

`run-manifest.json` records, at start:

- **Harness source identity** — the Probierz checkout itself: `gitSha`, a
  `dirty` flag (uncommitted diff or untracked files), and `worktreeSha256`, a
  SHA-256 over every tracked and untracked-but-not-ignored file (path, kind,
  mode, size, and content), excluding `node_modules/`, `test-results/`,
  `.env*` files, and runtime `probierz-*.json` files, and including
  `package-lock.json`. A dirty worktree therefore changes the identity; the
  git SHA alone never suffices.
- **App source identity** — the same triple for every repository the
  application manifest declares, plus one combined `sha256` over the ordered
  per-repository hashes.
- **Build identity** — the SHA-256 of the exact artifact under test:
  `PROBIERZ_BUILD_PATH`, `APP_IOS`, `MAC_APP_PATH`, or `ELECTRON_APP_MAIN`
  when set, otherwise the harness `package-lock.json`.
- **Run kind** — `adhoc` by default, or `pull-request`/`release`/`nightly`
  (`PROBIERZ_RUN_KIND` or the matrix profile). Gates only accept runs whose
  kind matches the gate mode.
- **Conditions** — the requested environment, with values under sensitive
  key names (auth/cookie/credential/key/password/secret/session/token/…)
  redacted before they are persisted.
- **Context** — host (hostname, platform, release, arch, Node version),
  device name and runtime, the selected spec, the declared journeys, the
  app manifest file and owner, and `PROBIERZ_APP_VERSION` when provided.

At completion the manifest gains `status` (`passed`, `failed`, `blocked`, or
`canceled`), `completedAt`, `exitCode`, `timedOut`, `reportValidation`,
`evidence`, `analysisPath`, and `artifacts[]` — the relative path, SHA-256,
and byte size of every file the run left behind. A blocked preflight and a
canceled run are first-class recorded outcomes, not missing history.

## Report identity, not process exit

A run passes only when all of these hold:

- the child process passed **and** the report file exists, postdates the
  run's start, and carries this run's own ID (`report.probierz.runId`);
- the analysis is valid: it matches the run ID, executed more than zero
  checks, has zero failures, zero capture errors, zero missing report-typed
  media, and zero crash diagnostics;
- when recording was requested, at least one report-typed video, trace, or
  screenshot was actually produced.

A green exit with a stale, foreign, or empty report is a failed run with an
explicit error list. Recording is forced with `--record`
(`PROBIERZ_RECORD=1`); capture support is driver-specific and a missing
capability never upgrades a failed run.

## Evidence levels

| Level | Meaning |
|---|---|
| `E0` | run did not pass |
| `E1` | reserved in the level ladder; no current producer |
| `E2` | run passed with a valid report and analysis |
| `E3` | `E2` plus requested recording with report-typed capture present |
| `E4` | a complete matrix passed with every cell at its minimum level |
| `E5` | label of the aggregated stability history projection |

Policies (`pullRequestPolicy`, `releasePolicy`, matrix
`minimumCellEvidence`, receipt `minimumEvidence`) accept `E2` or `E3`.

## Matrix evidence (E4)

`probierz matrix <appId> <nightly|release>` expands the manifest's declared
targets × condition dimensions into cells (each a 16-hex `cellId` over target
plus environment; `maxCells` defaults to 128), schedules them by resource
with bounded parallelism (`maximumParallel`, default 4), and executes each
cell as a normal run. A release matrix requires a release ID and injects it
as the `PROBIERZ_RELEASE` condition; declared axes cannot be overridden from
the command line. The verdict is `E4` only when every cell passed at
`minimumCellEvidence`. When the profile sets `artifactEncryption: required`,
`PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE` must name a key and every cell is
protected after it completes.

## Protection, retention, audit

- `probierz protect` encrypts a complete run into one authenticated
  AES-256-GCM bundle under `test-results/.protected/<appId>/` (file magic
  `PROBIERZ-EVIDENCE-1`), recording bundle hash, content index hash, and key
  fingerprint in the run manifest; `--remove-source` deletes the plaintext
  after a passing secret scan. `probierz restore` authenticates and unpacks
  a bundle into an empty directory.
- `probierz retention` plans expiry from the manifest's
  `artifacts.retain.{pullRequestDays,nightlyDays,adhocDays}` and deletes
  expired plaintext runs and bundles only with `--apply`.
- `probierz secret-scan` scans plaintext artifacts for high-confidence
  secrets without printing secret values; results land in
  `diagnostics/secret-scan.json` and are re-checked at receipt time.
- Security-relevant operations (gate evaluate/activate/enforce, protection,
  restore) append integrity-checked records under `test-results/.audit/`,
  attributed to `PROBIERZ_ACTOR`, `GITHUB_ACTOR`, or `USER`.

## Reading evidence back

- `probierz history [appId] [target]` — aggregates run manifests into pass
  rate, product vs infrastructure failures, flaky tests, journey stability,
  duration trend, latest run, and last green.
- `probierz status <appId>` — per-journey latest evidence, freshness against
  the current HEAD of every declared repository (the recorded git SHA must
  equal today's HEAD), and pull-request merge eligibility with exact
  blocking reasons.
- `probierz compare <left> <right>` — deterministic diff of status,
  duration, tests, evidence, build identity, and artifact hashes.
- `probierz last-green` — the newest passing run for a product, target, or
  journey.
- `probierz dashboard <appId>` — the product → version → journey → surface →
  device → result → artifact projection.

These are projections over the run manifests; they are not alternate sources
of truth. How gates consume this evidence is in
[gates-and-receipts](gates-and-receipts.md).
