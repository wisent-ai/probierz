# Receipt

How does evidence travel — to a release pipeline, another machine, an
auditor — without becoming hearsay? As a receipt: a canonical, Ed25519-signed
statement of exactly which runs, hashes, and policies stood behind one
release, re-verified against the working tree at the moment of signing.

## What it is

A JSON document, `kind: "probierz-evidence-receipt"`, schema version 3
(`urn:probierz:schema:evidence-receipt:v3` in `agent/receipt.mjs`), written
once — `0600`, flag `wx`, no overwrite — to:

```
test-results/receipts/<appId>/<release>/<receiptId>.json
```

`receiptId` is the first 24 hex chars of SHA-256 over
`canonical(payload) + "\n" + signature`. Canonicalization sorts object keys
recursively and drops `undefined`, so the same facts always hash the same.

## Fields

Payload (everything signed):

| Field | Content |
|---|---|
| `appId`, `productId`, `release` | identity; `productId` falls back to `appId` |
| `expectedHarnessSha`, `expectedSourceSha` | the identities the caller claims (64-hex, lowercase) |
| `builds` | per-target build hash, refused if runs disagree per target |
| `artifactPolicy` | manifest retention windows, sorted redaction keys, PII stance |
| `secretScans` | per-run scan result recorded at signing time |
| `issuedAt`, `policy` | timestamp; `minimumEvidence` + sorted `requiredJourneys` |
| `verdict` | `passed`, `errors`, `coveredJourneys`, `missingJourneys` |
| `runs[]` | per-run signed record: `runId`, `target`, `spec`, `journeys`, `journeyIdentities`, `status`, `evidenceLevel`, `kind`, full `harness`/`source`/`build` identities, 40-hex `sourceRevision`, `device`, timestamps, policy-relevant `conditions`, `protection`, `artifacts[]` hashes, report-typed `media` |

Signing block (`signing`): `algorithm: "Ed25519"`,
`publicKeyFingerprintSha256` (SHA-256 of the SPKI DER),
`publicKeyPem`, `payloadSha256`, base64 `signature`.

## What signing re-verifies

`createReceipt` refuses to sign fiction. Before the signature is produced it
checks — and records any failure in the receipt's own verdict:

- required inputs: `appId, release, expectedHarnessSha, and
  expectedSourceSha are required`; both SHAs must be
  `lowercase SHA-256 values`; `at least one runId is required`;
  `PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required`; the key must be Ed25519
  (`evidence private key must be Ed25519`);
- freshness: the expected identities are recomputed from the **current**
  checkouts — `expected harness source is stale relative to the current
  Probierz checkout`, `expected app source is stale relative to the current
  product checkout`;
- every artifact of every run re-hashed (`<runId>: artifact hash mismatch:
  <file>`), or for protected runs the bundle re-hashed inside
  `test-results/.protected/<appId>` (`<runId>: protected artifact is missing
  or escapes its product root`);
- a passing secret scan per run, executed now for plaintext runs
  (`<runId>: plaintext secret scan is missing or has findings`);
- per-run status/identity/build/level checks and per-target build
  uniqueness, and journey coverage (`missing journey: <j>`).

A receipt whose verdict fails is still written and returned — with exit 1 —
so the failure list itself is signed and portable.

## Verification and trust

`probierz verify-receipt <file> [--public-key pem | --fingerprint sha256]`
recomputes the canonical payload hash and checks the signature. Trust is
explicit and separate from validity:

- with `--public-key`, the supplied key is the trust anchor;
- otherwise the **embedded** key is used, and it is trusted only when its
  fingerprint equals `--fingerprint` or
  `PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT`.

Captured behavior (see the [walkthrough](../walkthrough-gate-and-receipt.md)):
a self-signed receipt with no trust anchor returns
`"valid": false, "signatureValid": true, "trusted": false` and exit 1;
`valid` is true only when signature, trust, and payload hash all hold.

## Who consumes it

- The **release gate**: `gate-evaluate <app> release … --receipt <file>`
  demands a valid, trusted receipt whose app, release, identities, builds,
  run IDs, verdict, and per-run canonical records agree with local evidence
  (see [verdict](verdict.md) for the disagreement sentences).
- **Publication**: `probierz publication` and `publish-onboarding` accept
  only a valid, trusted, passing receipt
  (`receipt is not valid, trusted, and passing`) and bind published assets
  to its signed artifact hashes (`assets[<i>] is not bound by the signed
  receipt`).
- The **SEO evaluator** signs its own report with the same canonical
  Ed25519 contract, so `verify-receipt` checks both.

## Commands

```bash
export PROBIERZ_RECEIPT_PRIVATE_KEY_FILE=/path/ed25519.pem
probierz receipt <appId> <release> <harnessSha> --source-sha <sha> \
  --runs id1,id2 [--journeys a,b] [--minimum E2|E3]
probierz verify-receipt <file> [--public-key pub.pem | --fingerprint <sha256>]
```

## Not to be confused with

- **A [gate](gate.md)** — the decision machine; the receipt is portable
  input to its release mode.
- **A run manifest** — mutable-by-completion local state; the receipt is a
  signed snapshot that re-verified it.
- **An encrypted bundle** (`.pev`) — confidentiality at rest; a receipt is
  integrity plus attribution, and they compose (receipts record bundle
  hashes).
