# Gates and receipts

A gate is Probierz's answer to "may this source merge or release?". It is a
deterministic evaluation over recorded [run evidence](evidence-model.md)
that returns `passed` plus an exhaustive error list. A receipt is the signed,
immutable form of that evidence for a release. Neither is inferred from
prose, screenshots, or a green process exit.

## What a gate checks

`probierz gate-evaluate <appId> <pull-request|release> <harnessSha256>
--source-sha SHA256 --runs ids` loads the application's policy
(`pullRequestPolicy` or `releasePolicy` from the manifest) and verifies, for
every named run:

- status is `passed` and the run's `kind` matches the gate mode;
- the recorded harness identity equals the expected harness SHA-256, with a
  complete git SHA and worktree hash;
- the recorded app source identity equals the expected source SHA-256, and
  all runs together identify exactly one app source;
- an exact build hash exists, and all runs per target identify one build;
- the evidence level meets the policy minimum (default `E3`);
- every artifact still exists inside its run directory and re-hashes to its
  recorded SHA-256 (for protected runs, the encrypted bundle re-hashes to
  its manifest value);
- policy extras hold: `requiredTargets`, `requiredJourneys`, a complete
  `requiredMatrixProfile` cell coverage with no extra runs,
  `requireProtectedArtifacts`, and `requireSecretScan`.

Release mode additionally requires a release ID, a matching
`PROBIERZ_RELEASE` run condition, and a signed receipt whose app, release,
source identities, build identities, run set, verdict, and per-run signed
evidence agree with the local evaluation byte-for-byte (canonical JSON
comparison). Every evaluation appends an audit record.

## Gate lifecycle: pending-green, activate, enforce

Gate state lives in `gates.json` beside the application manifest and is read
by `probierz gate-status <appId>`. Both modes default to
`enforcement: "pending-green"`.

- `probierz gate-activate` runs a full evaluation and refuses activation
  unless it is completely green (`PROBIERZ_GATE_NOT_GREEN`); on success it
  atomically writes `enforcement: "required"` together with the activation
  evidence (source identities, run IDs, builds, receipt fingerprint).
- `probierz gate-enforce` fails closed: while a mode is still pending-green
  it returns a failed verdict with the reason `gate is pending green
  activation`; once required, it re-evaluates current evidence.

## The pre-push gate

`probierz gate-install <appId> [--repo path]` writes a managed `pre-push`
hook into the repository (an existing hook is backed up as
`pre-push.before-probierz-gate` and chained first). On every push to
`main`/`master` the hook:

1. resolves the app from the manifest's declared repositories;
2. diffs the push range and selects the affected journeys through the
   manifest's file mappings — no affected journeys means the push passes;
3. by default runs `probierz ci <base>` to produce fresh evidence
   (`PROBIERZ_GATE_NO_CI=1` switches to evaluate-only);
4. picks the newest passing run per affected journey and evaluates the
   pull-request gate against the exact current harness and app source
   identity.

A stale, missing, failing, or identity-mismatched journey blocks the push
with its exact reason (exit 1). `probierz gate-prepush` runs the same gate
manually; `probierz status <appId> --text` shows the same eligibility and
blocking reasons without pushing.

## Evidence receipts

`probierz receipt <appId> <release> <harnessSha256> --source-sha SHA256
--runs ids` produces a `probierz-evidence-receipt` (schema version 3,
`urn:probierz:schema:evidence-receipt:v3`). Before signing, it re-verifies
the evidence: the expected identities against the *current* checkouts (a
stale expectation is an error), every artifact or protected bundle re-hashed,
a passing secret scan per run (recorded in the receipt), per-target build
uniqueness, minimum evidence level, and required journey coverage. The
payload binds:

- `appId`, `productId`, `release`, `expectedHarnessSha`, `expectedSourceSha`;
- per-target `builds` hashes and the artifact retention/redaction policy;
- `secretScans`, the `policy` (minimum evidence, required journeys), and the
  `verdict` with covered/missing journeys and all errors;
- per-run records: run ID, target, journeys and immutable journey
  identities, status, evidence level, harness and source identities, the
  40-hex `sourceRevision` (the primary repository's git commit), build,
  timestamps, artifact hashes, and report-typed media.

The payload is signed with Ed25519 (`PROBIERZ_RECEIPT_PRIVATE_KEY_FILE`,
PKCS#8 PEM). The signing block records the algorithm, public key PEM and
SHA-256 fingerprint, canonical payload hash, and base64 signature. The
`receiptId` is the first 24 hex characters of SHA-256 over the canonical
payload, a newline, and the signature. Receipts are written once
(`0600`, no overwrite) under
`test-results/receipts/<appId>/<release>/<receiptId>.json`.

## Verifying a receipt

`probierz verify-receipt <file>` recomputes the canonical payload hash and
checks the Ed25519 signature. Trust is explicit: either pass a trusted
public key file, or the embedded key's fingerprint must equal
`--fingerprint` / `PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT`. A receipt with
a valid signature but no trust anchor is reported as untrusted, and
`valid` is true only when signature, trust, and payload hash all hold.

## Publication manifests

`probierz publication` and `probierz publish-onboarding` turn a signed
release receipt plus registered, immutable, hash-verified assets into a
first-use publication manifest under `test-results/publications/`. The
manifest contains only `publishable: true` records: an invalid or untrusted
receipt, stale source, mismatched content hash, plaintext-secret finding,
unverified redaction, unsupported recording claim, or credential-bearing
storage URL rejects publication instead of producing a downgraded manifest.
Journey identity requirements come from the
[application manifest](applications.md).
