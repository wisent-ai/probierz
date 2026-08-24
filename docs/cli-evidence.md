# CLI reference — status, history, protection

Part of the [CLI reference](cli.md).

These commands read evidence back and manage its lifecycle: coverage and
merge eligibility, run history, comparison, encrypted protection, retention,
and the access audit trail. Every one of them is a projection over the run
manifests under `test-results/` — never an alternate source of truth (see
[evidence-model](evidence-model.md) and [concepts/evidence](concepts/evidence.md)).
Output, failure, and exit-code conventions are in [cli.md](cli.md) and are
not repeated here. Gates, receipts, and publication are in
[cli-gates.md](cli-gates.md).

## Status and projections

### `probierz status <appId> [--base <ref>] [--text]`

Coverage and merge eligibility for one app: which manifest journeys have
evidence, how fresh it is against the current HEAD of every declared
repository, how strong it is (the level ladder from
[concepts/evidence](concepts/evidence.md)), which journeys the
`<base>..HEAD` diff affects, and whether the pull-request policy would let
HEAD merge right now.

| Flag | Effect | Default |
|---|---|---|
| `--base <ref>` | base ref resolved in every declared repository | `origin/main` |
| `--text` | human rendering instead of JSON | JSON |

Output (`agent/status.mjs`, `schemaVersion` 1): `appId`, `generatedAt`,
`baseRef`, `repositories` (per repo `root`, `headSha`, `baseSha`),
`journeys` — sorted, each `{ journey, lastRun: { runId, target, status,
startedAt, evidenceLevel } | null, fresh, affected }` — `untested`,
`affectedJourneys`, and `mergeEligibility`:

- `mode` is always `"pull-request"`; `minimumEvidence` comes from the
  manifest `pullRequestPolicy.minimumEvidence` (default `E2`).
- A journey is `fresh` only when the recorded git SHA of **every** declared
  repository equals that repository's current HEAD.
- Only `affected` journeys are evaluated (`evaluatedJourneys`). The exact
  blocking-reason sentences, one condition each:

  ```
  <journey>: no runs recorded
  <journey>: last run is <status>
  <journey>: evidence is older than HEAD
  <journey>: <level> is below <minimum>
  ```

- `eligible` is true only when `blockingReasons` is empty; `gate` embeds the
  app's pull-request gate block (see [cli-gates.md](cli-gates.md)).

Exit `1` whenever `mergeEligibility.eligible` is false — with `--text` too.
This mirrors the [verdict](concepts/verdict.md) discipline: reasons, not a
score. Errors: `status needs an app ID`, `--base needs a ref` (exit 2).

`--text` renders one line per journey (date, run ID, level,
`fresh`/`stale`, or `— no runs —  E0  untested`), the affected list, and
`merge-eligibility(pull-request, min <level>): ELIGIBLE|BLOCKED` with the
reasons indented below.

`status` maps the diff through **every** registered app manifest
(`affectedAppJourneys`), so one invalid manifest anywhere fails it. On this
checkout it fails with `invalid app manifest:
<checkout>/apps/game-asset-creator/probierz.yaml surface eval spec is
required` (exit 1) — see [limitations](limitations.md).

### `probierz history [appId] [target] [--limit N]`

Aggregates run manifests into stability history: pass rate, product vs
infrastructure failures, flaky tests, per-journey stability, duration trend,
latest run, and last green. `appId` defaults to `probierz`; `target`
narrows to one run target (`test-results/<appId>/<target with : as ->`);
`--limit` keeps the newest N runs by `startedAt` (default 50). Errors
(exit 2, captured): `unknown history option: <flag>`, `--limit needs a
positive number`, `history accepts [appId] [target]`. Exit 0 otherwise,
even with zero runs.

Output (`agent/history.mjs`, `schemaVersion` 2) — captured summary block
from the demo app on this checkout:

```json
"summary": {
  "runs": 5,
  "passed": 3,
  "failed": 1,
  "blocked": 0,
  "canceled": 1,
  "productFailures": 1,
  "infrastructureFailures": 0,
  "passRate": 0.75,
  "flakyTests": 1,
  "latestRunId": "2026-08-24T22-38-17-072Z-6a87c8d9-8e9f-4758-a78d-b7c658699949",
  "lastGreenRunId": "2026-08-24T22-38-06-656Z-104d5be2-98c4-4dcb-8333-f418a995d992",
  "performanceRegression": false
}
```

- **Failure split.** A failed run is classified `infrastructure` when its
  failure text matches `executable doesn't exist|driver.*not
  installed|toolchain|connection refused|econnrefused` (case-insensitive),
  otherwise `product`. `passRate` is `passed / (passed + productFailures)` —
  infrastructure failures never dilute the product signal, and
  infrastructure-failed runs are excluded from per-test history.
- **`tests`** — per test title across runs: observations, pass/fail counts,
  status transitions, `flaky` (transitions with mixed outcomes), latest
  observation, and passing-duration `p50Ms`/`p95Ms`/`maxMs`.
- **`journeys`** — per [journey](concepts/journey.md): runs, passed, failed,
  product vs infrastructure failures, blocked, canceled, passRate,
  `latestRunId`.
- **`performance`** — latest passing duration vs the median of the previous
  up-to-ten passing runs; `regression` only when the baseline is ≥ 500 ms
  and the ratio ≥ 1.2.
- **`runs`** — the full normalized records, newest first: identity
  (`harness`, `source`, `build`), `kind`, `spec`, `journeys`,
  `failureClass`, `device`, `conditions`, `evidence`, `artifacts[]` (file,
  SHA-256, bytes), `protection`, `manifestPath`, `analysisPath`, `tests`.
  Manifest status `executed` normalizes to `passed`; a manifest with no
  recognized status is `failed` when completed, `incomplete` otherwise.

### `probierz last-green [appId] [target] [journey]`

The newest passing [run](concepts/run.md) for a product, optionally narrowed
to one target and/or one journey (searched over the newest 1000 runs).
Output: `{ schemaVersion: 2, appId, target, journey, run }` — `run` is the
full history record, or `null` when nothing green exists. Exit 0 either way.

### `probierz compare <left> <right> [appId]`

Deterministic diff of two runs (`appId` defaults to `probierz`): status,
duration, per-test changes, artifact hashes, and identity blocks. Errors:
`compare needs left and right run IDs` (exit 2); an unknown ID is `run not
found for <appId>: <runId>` (classified `not_found`, exit 1 — captured).

Output (`schemaVersion` 2): `left`/`right` (each `runId`, `status`,
`harness`, `source`, `build`, `durationMs`, `evidence`), then — captured
from the two demo runs on this checkout:

```json
"verdict": {
  "statusChanged": false,
  "regression": false,
  "newlyFailing": [],
  "durationRegression": false
},
"duration": { "deltaMs": -15, "ratio": 0.9774436090225563 }
```

- `tests.changes[]` — `added`, `removed`, `status`, or `duration` per title,
  with `before`/`after` and `durationDeltaMs`; `verdict.newlyFailing` lists
  titles failing on the right but not on the left.
- `artifacts.changes[]` — `added`, `removed`, or `content` when SHA-256 or
  byte size differ.
- `verdict.durationRegression` requires left ≥ 500 ms and ratio ≥ 1.2.

Exit 0 whenever both runs resolve — the verdict lives in the JSON, not the
exit code.

### `probierz dashboard <appId> [limit]`

The product → version → journey → surface → device → result → artifact
projection (`agent/dashboard.mjs`, `schemaVersion` 2). `limit` is a bare
second positional (default 500 newest runs). Error: `dashboard needs an app
ID` (exit 2).

Runs are grouped into `versions` by build identity (`build.sha256`, falling
back to `harness.sha256`, then `"unknown"`). Per version and journey, every
declared surface reports its latest result and one entry per device
(`<name>:<runtime>`, default `host:default`), each result carrying `runId`,
`status`, timestamps, `durationMs`, `evidence`, and `artifacts` (file,
SHA-256, bytes). Rollups are strict: a journey or version is `passed` only
when everything under it passed, `failed` when anything failed, otherwise
`incomplete`; a surface with no runs for that version is `missing`.
`requirements` lists the declared journey × surface matrix, and `summary`
carries `versions`, `runs`, `latestRunId`, `lastGreenRunId`.

### `probierz overview [appId...] [--text]`

One view over every registered app (or just the ones named): journey counts,
untested and affected journeys, merge eligibility with its blocking reasons,
plus two layers Probierz does **not** own, each read by shelling out
(`agent/overview.mjs`):

- **Repository hygiene** — runs the sibling checkout's scanner
  `node <checkout>/../hooks-rotator/src/cli.mjs find-violations --repo
  <root> --json` against each app's first repository root; reports
  `{ violations, skipped, errors }` counts.
- **Stado fleet health** — runs `gcloud storage ls -l
  gs://stado/capacity/**` and classifies each agent heartbeat as `live`
  (updated within 900 seconds) or `stale`.

Both layers degrade to an `error` field instead of failing the command: a
failing scanner becomes `violations: { "error": "<first stderr line>" }`
(or `"scanner output not parseable"`, or `"no repository root"`), and an
unavailable `gcloud` becomes `fleet: { "error": "<first stderr line>" }`
(or `"gcloud storage ls failed"`). `--text` renders one line per app with
at most the first three blocking reasons, then the fleet line. Because it
walks the app registry and contacts GCS, it is registry-wide and
network-dependent — it fails on this checkout like `status` does
([limitations](limitations.md)).

## Protection and lifecycle

### `probierz secret-scan <dir>`

Scans a directory of plaintext artifacts for high-confidence secrets without
ever printing a secret value. Errors: `secret-scan needs a directory`
(exit 2, captured); `secret scan root is not a directory: <root>`. Exit 1
when there are findings (captured).

Rules (`agent/security.mjs`):

| Rule | Matches |
|---|---|
| `private-key` | `-----BEGIN … PRIVATE KEY-----` blocks (RSA/EC/OPENSSH/DSA) |
| `aws-access-key` | `AKIA`/`ASIA` + 16 uppercase alphanumerics |
| `github-token` | `ghp_`/`gho_`/`ghu_`/`ghs_`/`ghr_` + 30+ chars |
| `slack-token` | `xoxb/a/p/r/s-` + 20+ chars |
| `jwt` | `eyJ…` plus two more dot-separated base64url segments |
| `assigned-secret` | `token/secret/password/api_key/authorization/cookie` `:`/`=` a value of 8+ chars |

Obvious placeholders are never findings: empty values, `[REDACTED]`,
`vault:…`, `${…}`, `env.X`/`source.X`/`process.env.X` references,
`ALL_CAPS_NAMES`, and `<angle-bracket>` placeholders. Binary files (NUL byte
in the first 8 KiB) and generated `html-report/trace/assets/` files are
skipped and counted; findings are capped at 1000. Each finding carries only
`rule`, `file`, `line`, `column`, and `fingerprintSha256` — the SHA-256 of
the matched value, so two findings of the same secret correlate without
disclosure. Output: `{ schemaVersion: 1, kind: "probierz-secret-scan",
root, scannedAt, scannedFiles, skippedBinary, skippedGenerated, passed,
findings }`.

The same scan runs automatically before a bundle is first created by
`protect`, writing `diagnostics/secret-scan.json` into the run and failing
with `secret scan failed with <n> finding(s)`; receipts re-check it
([cli-gates.md](cli-gates.md)).

### `probierz protect <appId> <runId> [kind] [--key-file <f>] [--remove-source]`

Encrypts a complete run directory into one authenticated bundle
`test-results/.protected/<appId>/<kind>/<runId>.pev` (`agent/artifacts.mjs`).
`kind` defaults to the run's recorded kind, then `adhoc`. Errors: `protect
needs an app ID and run ID`, `--key-file needs a path` (exit 2); an unknown
run is `run not found for <appId>: <runId>`.

**Key handling.** The key comes from `--key-file` or
`PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE` and must be exactly 32 raw bytes,
64 hex characters, or base64 of 32 bytes — otherwise `artifact encryption
key file is required` / `artifact encryption key must contain exactly 32
bytes, hex, or base64`. The key itself is never stored; only its SHA-256
(`keyFingerprintSha256`) is recorded, and every later operation checks it:
`artifact encryption key fingerprint mismatch`.

**Bundle format** (`PROBIERZ-EVIDENCE-1`): the magic line, a 4-byte
big-endian header length (bounded at 1 MiB — `invalid evidence bundle
header length`), a plaintext JSON header readable without the key
(`schemaVersion` 2, `kind: "probierz-encrypted-evidence"`, `algorithm:
"AES-256-GCM"`, run and app identity, `evidenceLevel`, journey identities,
`runKind`, `createdAt`/`expiresAt`/`retentionDays`, `pii`, nonce, key
fingerprint, `contentIndexSha256`, secret-scan summary, file count,
plaintext bytes), then the AES-256-GCM ciphertext of a content index plus
every file's bytes, and a 16-byte auth tag. Magic and header are bound as
AAD, so header tampering breaks authentication. A file that is not a bundle
is `not a Probierz encrypted evidence bundle`.

Refusals, all verbatim:

- `run is outside its product artifact root` — path-safety check;
- `artifact source contains a symlink: <file>` — symlinks are never bundled;
- re-protecting an existing bundle re-derives the content index and refuses
  divergence: `encrypted bundle identity mismatch`, `existing encrypted
  bundle predates source-integrity metadata`, `plaintext artifacts changed
  after the encrypted bundle was created` (an unchanged bundle is returned
  with `reused: true`);
- `plaintext was removed but the encrypted evidence bundle is missing`;
- `invalid artifact retention for <kind>` when the manifest declares a
  non-positive retention.

`--remove-source` deletes every plaintext file except `run-manifest.json`
after the bundle exists, recording `protection: { …, plaintextRemoved:
true }` and `plaintextArtifactsRemovedAt` in the manifest. Result: `{ file,
bytes, sha256, contentIndexSha256, keyFingerprintSha256, expiresAt,
retentionDays, files, secretScan, plaintextRemoved }`. Every attempt —
allowed or denied — is audited as `artifact.protect`.

### `probierz restore <bundle> <destination> [--key-file <f>]`

Authenticates and unpacks a bundle. Errors: `restore needs a bundle and
destination`, `--key-file needs a path` (exit 2). The destination must not
contain anything: `restore destination must be empty`. The key rules and
fingerprint check are the same as `protect`; further refusals: `unsupported
evidence algorithm: <algorithm>`, `truncated encrypted evidence bundle`,
`truncated evidence bundle header`, `encrypted evidence authentication
failed: <reason>` (wrong tag, corrupt payload, or tampered header),
`truncated evidence index`, `invalid evidence index length`, `unsafe
evidence member: <member>` (absolute or `..` paths in the index), `restored
evidence hash mismatch: <file>` (every restored file is re-hashed against
the content index), `encrypted evidence payload has trailing or missing
bytes`. Result: `{ appId, runId, destination, files, authenticated: true }`.
Audited as `artifact.restore`.

### `probierz retention <appId> [--at <ISO>] [--apply]`

Plans — and only with `--apply` enforces — artifact expiry. Errors:
`retention needs an app ID`, `--at needs an ISO timestamp` (exit 2);
`invalid retention time` when `--at` does not parse; `invalid run
timestamp: <startedAt>` for a corrupt manifest.

Expiry is `completedAt` (or `startedAt`) plus the manifest's
`artifacts.retain.{pullRequestDays,nightlyDays,releaseDays,syntheticDays,adhocDays}`
for the run's kind, falling back to `pullRequestDays`, then **14 days**.
The plan covers both plaintext run directories (`type: "run"`) and
encrypted bundles (`type: "protected"`, expiry read from the bundle
header), sorted by `expiresAt`: `{ schemaVersion: 1, appId, at, expired,
items, applied, removed }`, each item `{ type, appId, runId, kind, path,
expiresAt, expired }`. Without `--apply` nothing is deleted and `applied`
is `false` (captured on this checkout: 7 items, `"expired": 0`). With
`--apply`, expired paths are removed and listed in `removed`. Both modes
are audited (`retention.plan` / `retention.apply`).

### `probierz audit [appId] [--run <id>] [--action <name>] [--limit N]`

Reads the integrity-checked access audit trail under
`test-results/.audit/<YYYY-MM-DD>/`. `--limit` defaults to 200 (`--limit
needs a positive number`, exit 2); the positional and flags filter by app,
run, and action. Records are returned newest first as `{ schemaVersion: 1,
filters, total, returned, valid, invalid, records }`.

Every security-relevant operation appends one record: `gate.evaluate`,
`gate.activate`, `gate.enforce` ([cli-gates.md](cli-gates.md)),
`artifact.protect`, `artifact.restore`, `retention.plan`,
`retention.apply`, and `artifact.list` / `artifact.read` from the job
control surface the [MCP server](mcp.md) uses. Records are written once
(`wx`, mode 0600 in 0700 date directories) and attributed to
`PROBIERZ_ACTOR`, `GITHUB_ACTOR`, or `USER` (else `"unknown"`); `details`
values under sensitive key names are stored as `[REDACTED]`. Each record
embeds its own SHA-256 over the key-sorted payload; `audit` recomputes it
and marks every record `valid`/`invalid` — an unreadable file still appears
as `{ valid: false, file, error }`. A captured record from this checkout
(actor value replaced by `"…"` for the docs; everything else verbatim):

```json
{
  "schemaVersion": 1,
  "kind": "probierz-access-audit",
  "eventId": "ddeb1587-c1d8-44b9-88e9-b0e4a7e330d6",
  "at": "2026-08-24T22:38:17.071Z",
  "actor": "…",
  "action": "artifact.read",
  "outcome": "denied",
  "appId": "docs-demo",
  "runId": "2026-08-24T22-38-06-656Z-104d5be2-98c4-4dcb-8333-f418a995d992",
  "resource": "../../secrets",
  "context": { "ci": false, "workflow": null, "job": null },
  "details": { "error": "artifact path escapes the run directory" },
  "sha256": "5ff62897dc8db49e5898a2207b0dc806c2a835b3a9f2dd32628ba095ccedac1b",
  "valid": true
}
```
