# CLI reference — gates, receipts, publication

Part of the [CLI reference](cli.md).

These are the decision commands: they judge recorded evidence, sign it, and
turn it into portable release input. None of them runs a test. Output,
failure format, and exit codes follow the [conventions](cli.md#conventions);
the gate policy semantics and the full error vocabulary live in
[concepts/gate](concepts/gate.md), [concepts/receipt](concepts/receipt.md),
[concepts/verdict](concepts/verdict.md), and the narrative
[gates-and-receipts](gates-and-receipts.md) — this page only documents the
argv contracts.

## Gates

### `probierz gate-status <appId>`

Prints the gate configuration for one app: the contents of `gates.json`
beside the manifest — or the schema-2 default with both modes
`pending-green` when the file does not exist — plus `file` and `exists`.
Read-only, no verdict, always exit 0 on a valid app. Error: `gate-status
needs an app ID` (exit 2). Captured on this checkout after activation:

```json
{ "schemaVersion": 2, "appId": "docs-demo",
  "modes": {
    "pull-request": {
      "enforcement": "required",
      "activatedAt": "2026-08-24T22:36:18.019Z",
      "activationEvidence": {
        "expectedHarnessSha": "74ab23def1e65b14ce964c1f7ccb3261f9ec4b43e2f081327115c113a3014331",
        "expectedSourceSha": "bf76b35155387978d44936fb30019110c5791c32b26648d03c814e20935be6f2",
        "release": null,
        "runIds": ["2026-08-24T22-35-50-658Z-bd1e5394-e741-4c56-8f28-54ed53921968"],
        "builds": { "tui": "ab7a82f9562fd57da01c011640b0e756dd45314f03c777a7210e2378c1eb6c7c" },
        "harnessSha256": "74ab23def1e65b14ce964c1f7ccb3261f9ec4b43e2f081327115c113a3014331",
        "sourceSha256": "bf76b35155387978d44936fb30019110c5791c32b26648d03c814e20935be6f2",
        "receiptFingerprint": null } },
    "release": { "enforcement": "pending-green" } },
  "file": ".../apps/docs-demo/gates.json", "exists": true }
```

### `probierz gate-evaluate | gate-enforce | gate-activate <appId> <mode> <harnessSha256> [flags]`

All three share one argv contract (`parseGateArgs` in `agent/cli.mjs`).
Three positionals — app ID, mode (`pull-request` or `release`), expected
harness SHA-256 — are required: `gate needs an app ID, mode, and harness
SHA-256` (exit 2). An unrecognized mode fails in `agent/gate.mjs` with
`gate needs an app ID and pull-request or release mode`. Flags (each
value-carrying; a missing value is `<flag> needs a value`, exit 2):

| Flag | Effect |
|---|---|
| `--source-sha <sha256>` | expected app source identity (required by the evaluation itself: `expected app source SHA-256 is required`) |
| `--runs id1,id2` | comma-separated run IDs to judge (`at least one run ID is required`) |
| `--release <id>` | release identifier — release mode only |
| `--receipt <file>` | signed receipt — required by release mode |
| `--public-key <pem>` | trust anchor for the receipt |
| `--fingerprint <sha256>` | trusted fingerprint of the receipt's embedded key |

- **`gate-evaluate`** prints the full evaluation (`policy`, `verdict`,
  `evidence` with per-run levels, builds, matrix and receipt results) and
  exits 1 on a failing verdict. What it checks, and every error sentence it
  can emit, is in [concepts/verdict](concepts/verdict.md). Captured (an
  `adhoc` run offered to a pull-request gate):

  ```json
  "verdict": {
    "passed": false,
    "errors": [
      "2026-08-24T22-35-08-980Z-9bc420dc-ed03-4412-b96f-a1f5c26ecaa9: run kind adhoc is not pull-request"
    ] }
  ```

- **`gate-enforce`** consults the persisted enforcement state first. On a
  `pending-green` mode it fails closed **without evaluating anything**
  (exit 1); once `required`, it re-evaluates like `gate-evaluate` and adds
  the `status` block. Captured fail-closed output, complete:

  ```json
  { "schemaVersion": 2, "appId": "docs-demo", "mode": "pull-request",
    "verdict": { "passed": false,
                 "errors": ["gate is pending green activation"] },
    "status": { "schemaVersion": 2, "appId": "docs-demo",
      "modes": { "pull-request": { "enforcement": "pending-green" },
                 "release":      { "enforcement": "pending-green" } },
      "file": ".../apps/docs-demo/gates.json", "exists": false } }
  ```

- **`gate-activate`** runs a full evaluation and refuses on **any** error:
  it throws `gate activation refused: <errors joined by "; ">` with error
  code `PROBIERZ_GATE_NOT_GREEN` (exit 1 through the
  [failure boundary](cli.md#conventions)). On success it atomically writes
  `gates.json` (temp file + rename, mode `0600`) setting the mode to
  `enforcement: "required"` with the `activationEvidence` shown above, and
  prints `{ file, config, evaluation }`. Exit 0; activation never uses the
  failing-verdict exit path.

Every evaluate/enforce/activate appends an audit record — see `probierz
audit` in [cli-evidence.md](cli-evidence.md).

### `probierz gate-prepush [--repo path] [--app id] [--base ref] [--head ref] [--ci]`

Runs the pre-push merge gate manually (`agent/prepush-gate.mjs`): push diff
→ affected journeys → newest passing run per journey → pull-request
`gate-evaluate` against the **current** harness and app identity. Exit 1
when `ok` is false. Flags:

| Flag | Effect | Default |
|---|---|---|
| `--repo <path>` | product repository to gate | current directory |
| `--app <id>` | app ID | inferred from the repo's manifest `repositories` |
| `--base <ref>` | diff base | `merge-base HEAD origin/main` |
| `--head <ref>` | diff head | `HEAD` |
| `--ci` | run `probierz ci <base> --app <id>` first to produce evidence | evaluate runs already on disk |

Resolution failures are returned as `{ ok: false, reason }` (verbatim from
`agent/prepush-gate.mjs`):

- `no probierz app manifest matches <repo>` — no `--app` and no manifest
  lists the repo;
- `cannot resolve a merge base with origin/main; fetch first or pass
  --base`;
- `probierz ci failed (exit <n>)` — with `--ci`;
- ``no passing runs recorded for the affected journeys; run `probierz ci
  <base>` (or re-run with --ci) before pushing``.

A diff touching no mapped journey short-circuits to `{ "ok": true, "note":
"no affected journeys" }`. App inference and journey mapping walk the whole
app registry, so one invalid registered manifest blocks the command — the
case on this checkout, see [limitations](limitations.md).

Invoked directly, `node agent/prepush-gate.mjs` additionally accepts
`--hook` (read git pre-push refs on stdin; a push not targeting
`main`/`master` prints `prepush-gate: push does not target main; allowed`),
`--json`, and repeatable `--ci-arg <value>`; without `--json` it prints
`prepush-gate <appId>: ALLOWED|BLOCKED` plus one `  - <reason>` line per
verdict error, and its own crashes exit 2 with `prepush-gate failed:
<detail>`.

### `probierz gate-install <appId> [--repo path]`

Installs the managed pre-push hook into `<repo>/.git/hooks/pre-push`
(mode `0755`). Errors: `gate-install needs an app ID` (exit 2), any invalid
manifest error for the app, `--repo needs a path`, and `not a git working
tree: <repo>` when `.git/hooks` does not exist. The script, verbatim:

```sh
#!/bin/sh
# managed-by: probierz-prepush-gate
HOOK_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -f "$HOOK_DIR/pre-push.before-probierz-gate" ]; then
  "$HOOK_DIR/pre-push.before-probierz-gate" "$@" || exit $?
fi
GATE_CI="--ci"
if [ "${PROBIERZ_GATE_NO_CI:-}" = "1" ]; then GATE_CI=""; fi
exec node <probierz>/agent/prepush-gate.mjs --hook --app <appId> $GATE_CI
```

Backup and chaining semantics: a pre-existing hook that does **not**
contain the marker `# managed-by: probierz-prepush-gate` (or the managed
command) is renamed to `pre-push.before-probierz-gate` and chained first,
its failures propagated; a leftover *managed* backup is deleted;
re-installing over the managed hook never creates a backup, so the command
is idempotent. `PROBIERZ_GATE_NO_CI=1` at push time makes the hook
evaluate-only. Output: `{ installed, chained, appId }`.

## Receipts

### `probierz receipt <appId> <release> <harnessSha256> [flags]`

Signs an evidence receipt (`agent/receipt.mjs createReceipt`). Positionals:
app ID, release ID, expected harness SHA-256. Flags:

| Flag | Effect | Default |
|---|---|---|
| `--source-sha <sha256>` | expected app source identity | — (required) |
| `--runs id1,id2` | run IDs to sign | — (required) |
| `--journeys a,b` | journeys the receipt must cover | none |
| `--minimum <level>` | minimum evidence level per run | `E3` |

Requires `PROBIERZ_RECEIPT_PRIVATE_KEY_FILE` pointing at an Ed25519 private
key. Input refusals (thrown, exit 1): `appId, release, expectedHarnessSha,
and expectedSourceSha are required`; `expectedHarnessSha and
expectedSourceSha must be lowercase SHA-256 values`; `at least one runId is
required`; `unknown evidence level: <level>`;
`PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required`.

Everything signing re-verifies — freshness against the current checkouts,
artifact re-hashing, per-run secret scans, identity/build/level checks,
journey coverage — is in [concepts/receipt](concepts/receipt.md). Output:
`{ file, receiptId, receipt }`; the file lands at
`test-results/receipts/<appId>/<release>/<receiptId>.json`, written once
(`wx`, mode `0600`, never overwritten). **A failing verdict still writes
and returns the receipt** — `createReceipt` writes unconditionally and the
CLI only then sets exit 1 on `!result.receipt.verdict.passed` — so the
failure list itself is signed and portable. Captured: this checkout's
`receipt … --minimum E3` against an E2 run returned exit 1 *and*
`"file": ".../receipts/docs-demo/v0.2.0-docs/3a335abe2b864beb3ff0223f.json"`
with `"verdict": { "passed": false, "errors":
["2026-08-24T22-35-50-658Z-…: E2 is below E3"] }`.

### `probierz verify-receipt <file> [--public-key pem] [--fingerprint sha256]`

Recomputes the canonical payload hash and checks the Ed25519 signature.
Error: `verify-receipt needs a file` (exit 2); a receipt without an
`Ed25519` signing block fails with `unsupported or missing receipt
signature`, a non-Ed25519 key with `receipt public key must be Ed25519`.

Trust is separate from signature validity:

- `--public-key <pem>` makes the supplied key the trust anchor (`trusted`
  is unconditionally true; the signature is checked against *that* key);
- otherwise the receipt's **embedded** key verifies the signature and is
  trusted only when its fingerprint equals `--fingerprint` or, absent the
  flag, `PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT`.

`valid` requires signature, trust, **and** payload hash together; exit 1
unless `valid` is true. Captured on the demo receipt, no trust anchor
supplied — signature fine, still refused:

```json
{ "valid": false,
  "signatureValid": true,
  "trusted": false,
  "fingerprint": "4e194878e1dec583911fa17adc02837a84ff3b52bc5dcb0fce3a9c8c6ee220fe",
  "payloadSha256": "4c98c6007ea33296985d6fe99ef3da9dd1bceabc990188bad811902562fd91ee",
  "receiptId": "8f082f1da9ffe054043e7306" }
```

The same invocation with `--fingerprint 4e194878…` returns `"valid": true,
"signatureValid": true, "trusted": true` and exit 0 (captured). The output
also carries the receipt's `verdict`, `policy`, `builds`, `secretScans`,
`runs`, and `runIds` for downstream tooling.

## Publication

### `probierz publication <receipt> <attemptId> <journeyId> --assets <json> [--public-key pem] [--fingerprint sha256]`

Turns one signed release receipt plus registered assets into an immutable
first-use publication manifest (`agent/publication.mjs`). All three
positionals and `--assets` are required: `publication needs receipt,
attemptId, journeyId, and --assets <json>` (exit 2). `--assets` is a JSON
file — an array of `{ file, contentSha256, storageUrl, redactionStatus,
verifiedAt, kind? }` registrations; `--public-key` / `--fingerprint` set
the receipt trust anchor as for `verify-receipt`.

Every refusal is `publication rejected: <reason>` (exit 1). The receipt
must be valid, trusted, and passing (`receipt signature is not valid and
trusted`, `receipt verdict did not pass`, `receipt predates publication
provenance` for schema < 3); the attempt must be signed by it (`attempt
<id> is not signed by the receipt`, `attempt <id> did not pass`, `attempt
<id> has no signed evidence artifacts`); the journey identity and
publication policy must be unchanged since issuance (`journey <id> is not
signed for attempt <id>`, `journey version is no longer present in the app
manifest`, `journey identity changed after receipt issuance`, `journey has
no publication policy`, `publication policy changed after receipt
issuance`); the source must still be current (`receipt source is stale
relative to the current product source`, `primary source revision is
stale`). Per asset: it must match signed report-typed evidence byte-for-
byte (`assets.<i>.file is not signed report-typed evidence`,
`assets.<i>.contentSha256 does not match the signed receipt`), carry an
allowed kind (`assets.<i> has unsupported artifact kind <kind>` outside
`screenshot`/`recording`/`trace`, `assets.<i> kind <kind> is not allowed by
the journey policy`, `driver <target> does not support <kind>`), an
immutable URL
(`assets.<i>.storageUrl must be immutable HTTPS without credentials, query,
or fragment`), a supported `redactionStatus` — `verified_redacted` /
`not_applicable` (`assets.<i>.redactionStatus is unsupported`, or
`assets.<i> lacks required redaction verification` when the journey policy
demands redaction) — and `verifiedAt` not before capture
(`assets.<i>.verifiedAt predates capture`).

Output: `{ file, manifestId, publication, reused }`. The manifest is
written once (`wx`, `0600`) to
`test-results/publications/<productId>/<release>/<journeyVersionId>/<attemptId>/<manifestId>.json`;
re-running with identical content returns `reused: true`, different content
at the same path is refused (`immutable publication manifest path contains
different content`).

### `probierz publish-onboarding <receipt> --run <id> --journey <id> --journey-version <v> --journey-version-id <uuid> --first-success-fact <id> --screen <id> --assets <json> --output <file> [--public-key pem] [--fingerprint sha256]`

The onboarding variant (`agent/onboarding-publication.mjs`): the journey
identity is supplied on the command line instead of resolved from the app
manifest, so it works from a bare receipt. Every listed flag is required —
a missing flag or value fails with `<flag> needs a value` (exit 2);
`--public-key` and `--fingerprint` are optional and set receipt trust as
above. `--assets` is a non-empty JSON array of `{ file, kind,
redactionStatus, storageUrl, capturedAt? }` entries.

Refusals (exit 1, verbatim): `receipt is not valid, trusted, and passing`;
`passing receipt run not found: <runId>`; `receipt run does not cover
journey: <journeyId>`; `onboarding publication requires E2 or E3 evidence`;
`receipt run has no successful protected-artifact secret scan`; `receipt
run source identity does not match the signed receipt`; `asset catalog must
be a non-empty JSON array`; `assets[<i>] is not bound by the signed
receipt`; `assets[<i>].kind is unsupported`; `assets[<i>].redactionStatus
is incomplete`; `assets[<i>].storageUrl must be a credential-free immutable
HTTPS URL`; `asset catalog contains duplicate signed artifacts`; malformed
identifiers fail as `<name> is invalid` (e.g. `journey version id is
invalid` for a non-UUID). The manifest is written to `--output` (`wx`,
`0600`) and stdout prints only `{ file, manifestId }`.
