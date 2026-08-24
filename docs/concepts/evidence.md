# Evidence item

"The suite passed" is not evidence. In Probierz an evidence item is a run
record whose strength is explicitly graded, whose files are hash-bound, and
whose weaknesses are enumerated rather than averaged away.

## What it is

The `evidence` block `completeRun` writes into every completed run manifest
(`agent/runner.mjs`), plus the graded level derived from it everywhere
evidence is consumed:

```json
"evidence": {
  "report": true,            // report exists, postdates start, carries this run's ID
  "analysis": true,          // valid analysis: >0 checks, 0 failures, 0 capture errors,
                             // 0 missing report-typed media, 0 crash diagnostics
  "captureRequired": false,  // was --record requested
  "capturePresent": true,    // report-typed video/trace/screenshot exists when required
  "captureErrors": [], "missingMedia": [], "crashes": [],
  "errors": []               // every reason this run is weaker than it claims
}
```

`errors` entries are exact and additive: `"analysis run ID mismatch"`,
`"zero executed checks"`, `"<n> failed checks"`,
`"missing report-typed artifact: <file>"`,
`"recording requested but no report-typed capture was produced"`,
`"crash evidence: <message>"`.

## The level ladder

Computed identically in `gate.mjs`, `receipt.mjs`, `status.mjs`,
`matrix.mjs`:

| Level | Meaning | Producer |
|---|---|---|
| `E0` | run did not pass | any non-passed run |
| `E1` | reserved; no current producer | — |
| `E2` | run passed with a valid report and analysis | default passing run |
| `E3` | `E2` + `--record` requested + report-typed capture present | recording-capable targets |
| `E4` | complete matrix passed, every cell at its minimum level | `probierz matrix` verdict (`evidenceLevel: "E4"` only when passed) |
| `E5` | label of the aggregated stability history projection | `probierz history` |

Policies (`pullRequestPolicy`, `releasePolicy`, matrix
`minimumCellEvidence`, receipt `--minimum`) accept `E2` or `E3`; a gate
refuses anything else with `unsupported gate evidence level: <value>`.
Capture support is driver-specific and a missing capability never upgrades a
failed run — the level is computed from what was actually produced.

## Hash binding

At completion, every file in the run directory is inventoried into
`artifacts[]` with SHA-256 and byte size. Consumers re-hash rather than
trust:

- a gate re-hashes every artifact of every submitted run
  (`<runId>: artifact hash mismatch: <file>`, `<runId>: artifact is
  missing: <file>`, `<runId>: artifact path escapes its run: <file>`);
- a receipt re-hashes again at signing time and refuses stale expectations
  (`expected harness source is stale relative to the current Probierz
  checkout`);
- protection stores a content index hash inside the encrypted bundle and
  refuses to reuse a bundle whose plaintext changed
  (`plaintext artifacts changed after the encrypted bundle was created`).

## Protection, retention, scanning

- `probierz protect <appId> <runId> [kind] --key-file <f>` encrypts the run
  directory into one AES-256-GCM bundle
  `test-results/.protected/<appId>/<kind>/<runId>.pev` (file magic
  `PROBIERZ-EVIDENCE-1`), records `sha256`, `contentIndexSha256`, and
  `keyFingerprintSha256` in the run manifest, and runs a secret scan first.
  `--remove-source` deletes plaintext after the scan passes. `probierz
  restore <bundle> <dir> --key-file <f>` authenticates and unpacks into an
  empty directory only (`restore destination must be empty`); a wrong key is
  `artifact encryption key fingerprint mismatch`.
- `probierz retention <appId> [--at ISO] [--apply]` plans expiry from
  `artifacts.retain.*` (default 14 days when undeclared) and deletes expired
  plaintext runs and bundles only with `--apply`.
- `probierz secret-scan <dir>` applies the high-confidence rules in
  `agent/security.mjs` (private keys, AWS/GitHub/Slack tokens, JWTs,
  assigned secrets) without printing values — findings carry only rule, file,
  line, column, and a SHA-256 fingerprint of the match.

## Reading evidence back

`history`, `status`, `dashboard`, `compare`, `last-green` are projections
over run manifests on disk — never alternate sources of truth. `history`
splits failures into `product` vs `infrastructure` by matching failure text
against `executable doesn't exist|driver.*not installed|toolchain|connection
refused|econnrefused`.

## Not to be confused with

- **A [run](run.md)** — the record; the evidence item is its graded,
  hash-bound content.
- **A [verdict](verdict.md)** — a decision computed over evidence items.
- **A [receipt](receipt.md)** — a signed, portable statement about evidence
  items.
