# Walkthrough: run → gate → signed receipt

Executed end-to-end on this checkout on 2026-08-24, continuing from the
registered `docs-demo` application of
[walkthrough-register-app](walkthrough-register-app.md). Every output is
pasted from the session (paths shortened to `<checkout>`, UUIDs to `…`).
This is as far as `main` goes without external hosts: local runs, the full
gate lifecycle, a signed and verified receipt, and the in-process async
queue. Remote Stado execution needs a fleet and is not exercised here
([remote-stado](remote-stado.md)).

Runnable version: [examples/run-gate-receipt.sh](examples/run-gate-receipt.sh).

## 1. An adhoc run

```
$ node agent/cli.mjs run tui --app docs-demo
{
  "runId": "2026-08-24T22-35-08-980Z-9bc420dc-…",
  "appId": "docs-demo",
  "kind": "adhoc",
  "target": "tui",
  "command": "npm run test:tui (PROBIERZ_SPEC=docs-demo-hello.spec.mjs)",
  "spec": "docs-demo-hello.spec.mjs",
  "exitCode": 0,
  "passed": true,
  "durationMs": 665,
  "reportValidation": { "ok": true, "runId": "2026-08-24T22-35-08-980Z-9bc420dc-…",
                        "mtime": "2026-08-24T22:35:09.616Z" },
  "evidence": { "report": true, "analysis": true, "captureRequired": false,
                "capturePresent": true, "captureErrors": [], "missingMedia": [],
                "crashes": [], "errors": [] },
  "analysis": { "tool": "playwright", "total": 1, "passed": 1, "failed": 0, … },
  …
}
```

Exit 0. The manifest surface selected the spec; the report came back
stamped with this run's own ID; the analysis found one executed, passing
check — so the evidence block is clean and the run is `E2`
([concepts/run](concepts/run.md), [concepts/evidence](concepts/evidence.md)).

`status docs-demo` would now answer merge eligibility, but it walks the
whole registry and this checkout ships one broken manifest
([limitations](limitations.md)); the per-app projections work:

```
$ node agent/cli.mjs last-green docs-demo
{ …, "run": { "runId": "2026-08-24T22-35-08-980Z-9bc420dc-…", "status": "passed",
  "harness": { "gitSha": "f8807e5…", "dirty": true,
               "sha256": "74ab23def1e65b14ce964c1f7ccb3261f9ec4b43e2f081327115c113a3014331" },
  "source":  { "sha256": "bf76b35155387978d44936fb30019110c5791c32b26648d03c814e20935be6f2", … }, … } }
```

## 2. The gate starts closed

```
$ node agent/cli.mjs gate-status docs-demo
{ "schemaVersion": 2, "appId": "docs-demo",
  "modes": { "pull-request": { "enforcement": "pending-green" },
             "release":      { "enforcement": "pending-green" } },
  "file": "<checkout>/apps/docs-demo/gates.json", "exists": false }
```

No `gates.json` exists yet; both modes report the fail-closed default.

Export the identities from step 1 (they are also printed by
`source-identity docs-demo`):

```bash
H=74ab23def1e65b14ce964c1f7ccb3261f9ec4b43e2f081327115c113a3014331
S=bf76b35155387978d44936fb30019110c5791c32b26648d03c814e20935be6f2
ADHOC=2026-08-24T22-35-08-980Z-9bc420dc-ed03-4412-b96f-a1f5c26ecaa9
```

## 3. The adhoc run cannot satisfy the gate

```
$ node agent/cli.mjs gate-evaluate docs-demo pull-request $H --source-sha $S --runs $ADHOC
… "verdict": {
  "passed": false,
  "errors": [ "2026-08-24T22-35-08-980Z-9bc420dc-…: run kind adhoc is not pull-request" ] }
```

Exit 1. Same evidence, wrong `kind` — gates only accept runs whose recorded
kind matches the mode. Produce a pull-request run by passing the kind as a
run condition:

```
$ node agent/cli.mjs run tui --app docs-demo PROBIERZ_RUN_KIND=pull-request
{ "runId": "2026-08-24T22-35-50-658Z-bd1e5394-…", "kind": "pull-request", "passed": true, … }
$ PR=2026-08-24T22-35-50-658Z-bd1e5394-e741-4c56-8f28-54ed53921968
```

## 4. Enforce fails closed until activation

```
$ node agent/cli.mjs gate-enforce docs-demo pull-request $H --source-sha $S --runs $PR
{ …, "mode": "pull-request",
  "verdict": { "passed": false, "errors": [ "gate is pending green activation" ] }, … }
```

Exit 1 — with a fully green run in hand. Enforcement refuses to pass
anything until the gate has been activated by one fully green evaluation.

## 5. Evaluate green, then activate

```
$ node agent/cli.mjs gate-evaluate docs-demo pull-request $H --source-sha $S --runs $PR
… "verdict": { "passed": true, "errors": [] },
  "evidence": { "runIds": ["2026-08-24T22-35-50-658Z-bd1e5394-…"],
                "builds": { "tui": "ab7a82f9562fd57da01c011640b0e756dd45314f03c777a7210e2378c1eb6c7c" },
                "levels": { "2026-08-24T22-35-50-658Z-bd1e5394-…": "E2" } }

$ node agent/cli.mjs gate-activate docs-demo pull-request $H --source-sha $S --runs $PR
```

Activation wrote the managed state — `apps/docs-demo/gates.json` now holds:

```json
"pull-request": {
  "enforcement": "required",
  "activatedAt": "2026-08-24T22:36:18.019Z",
  "activationEvidence": {
    "expectedHarnessSha": "74ab23de…",
    "expectedSourceSha": "bf76b351…",
    "runIds": ["2026-08-24T22-35-50-658Z-bd1e5394-…"],
    "builds": { "tui": "ab7a82f9…" },
    "receiptFingerprint": null
  }
}
```

and the same `gate-enforce` command now re-evaluates and passes:

```
$ node agent/cli.mjs gate-enforce docs-demo pull-request $H --source-sha $S --runs $PR
… "verdict": { "passed": true, "errors": [] }
```

## 6. Sign a receipt — first attempt refused, and signed anyway

```
$ openssl genpkey -algorithm ed25519 -out /tmp/receipt-key.pem
$ export PROBIERZ_RECEIPT_PRIVATE_KEY_FILE=/tmp/receipt-key.pem
$ node agent/cli.mjs receipt docs-demo v0.2.0-docs $H --source-sha $S --runs $PR
{ "receiptId": "3a335abe2b864beb3ff0223f",
  "file": "<checkout>/test-results/receipts/docs-demo/v0.2.0-docs/3a335abe2b864beb3ff0223f.json",
  "receipt": { "verdict": {
    "passed": false,
    "errors": [ "2026-08-24T22-35-50-658Z-bd1e5394-…: E2 is below E3" ],
    "coveredJourneys": ["greet"], "missingJourneys": [] }, … } }
```

Exit 1 — the receipt default minimum is `E3` and this run has no recording.
Note what happened anyway: the failing receipt **was written and signed**,
so the failure list itself is portable evidence
([concepts/receipt](concepts/receipt.md)). Sign at the level the evidence
actually has:

```
$ node agent/cli.mjs receipt docs-demo v0.2.0-docs $H --source-sha $S --runs $PR --minimum E2
{ "receiptId": "8f082f1da9ffe054043e7306",
  "file": "…/test-results/receipts/docs-demo/v0.2.0-docs/8f082f1da9ffe054043e7306.json",
  "receipt": { "verdict": { "passed": true, "errors": [], "coveredJourneys": ["greet"] },
    "secretScans": { "2026-08-24T22-35-50-658Z-bd1e5394-…": {
      "passed": true, "scannedFiles": 8, "findings": [] } },
    "runs": [ { "runId": "2026-08-24T22-35-50-658Z-bd1e5394-…", "evidenceLevel": "E2",
      "kind": "pull-request", "sourceRevision": "5ec63d9f585b20f03e803da3d9e13fbe0cb73237",
      "artifacts": [ { "file": "analysis.json", "sha256": "b0fb4263…", "bytes": 2939 }, …7 more ] } ],
    "signing": { "algorithm": "Ed25519",
      "publicKeyFingerprintSha256": "4e194878e1dec583911fa17adc02837a84ff3b52bc5dcb0fce3a9c8c6ee220fe", … } } }
```

Exit 0. Signing re-ran the secret scan and re-hashed every artifact of the
run before the signature was produced.

## 7. Verify: validity is signature AND trust AND hash

```
$ node agent/cli.mjs verify-receipt <file>
{ "valid": false, "signatureValid": true, "trusted": false,
  "fingerprint": "4e194878…", "payloadSha256": "4c98c600…", … }        # exit 1

$ node agent/cli.mjs verify-receipt <file> --fingerprint 4e194878e1dec583911fa17adc02837a84ff3b52bc5dcb0fce3a9c8c6ee220fe
{ "valid": true, "signatureValid": true, "trusted": true, … }          # exit 0
```

A receipt verifying against its own embedded key proves integrity, not
authorship; trust must be pinned explicitly.

## 8. The same lifecycle, asynchronously

The MCP server runs the identical run path through the in-process queue —
started, polled, canceled, and read back over JSON-RPC. The captured
session (start → `queued` → `running` → `passed` in 0.6 s, artifact reads
audited, cancel-in-flight settling as `canceled`, traversal refused) is in
[mcp](mcp.md#the-asynchronous-run-queue). The queue is not durable across a
server restart; the run directories it leaves behind are ordinary runs:

```
$ node agent/cli.mjs history docs-demo
{ "summary": { "runs": 7, "passed": 5, "failed": 1, "blocked": 0, "canceled": 1,
    "productFailures": 1, "infrastructureFailures": 0, "passRate": 0.833…,
    "flakyTests": 1,
    "lastGreenRunId": "2026-08-24T22-38-06-656Z-104d5be2-…" },
  "journeys": [ { "journey": "greet", "runs": 7, "passed": 5, "failed": 1,
    "canceled": 1, "passRate": 0.833… } ] }
```

Seven runs of the demo journey — including one deliberate failure from an
earlier session and the canceled queue run — all first-class records.

## What this proved

- A run passes only through report identity plus analysis, and carries its
  evidence grade explicitly (`E2` here, `E3` with `--record` on a
  recording-capable target).
- The gate lifecycle is fail-closed at every step: wrong kind refused,
  enforcement refused before activation, activation recorded as managed
  state with the exact evidence.
- A receipt refuses to overstate (`E2 is below E3`), signs its own failure
  when asked, and separates validity from trust at verification.
- The async queue runs the same lifecycle and forgets nothing on disk.
