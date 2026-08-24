# Verdict

Probierz never answers "mostly green" — every decision surface returns a
verdict: `passed` plus an exhaustive error list, deterministic over recorded
inputs, never a score.

## What it is

The shape `{ passed: boolean, errors: string[] }` (sometimes with extra
fields beside it), produced by:

| Producer | Verdict location | Extra fields |
|---|---|---|
| `gate-evaluate` / `gate-enforce` / `gate-activate` | `.verdict` | `policy`, `evidence` (run IDs, builds, levels, matrix, receipt) |
| `receipt` | `.receipt.verdict` | `coveredJourneys`, `missingJourneys` |
| `matrix` (executed) | `.verdict` | `evidenceLevel: "E4"` when passed, `minimumCellEvidence`, `evidenceSatisfied` |
| `gate-prepush` | `.verdict` (plus top-level `ok`) | `affectedJourneys`, `runIds`, or a single `reason` |
| `status` | `.mergeEligibility` | `eligible`, `blockingReasons`, `evaluatedJourneys` |
| run completion | `.evidence` | `errors[]` explains a failed run |

A captured example (this checkout, demo app; see
[walkthrough](../walkthrough-gate-and-receipt.md)):

```json
"verdict": {
  "passed": false,
  "errors": [
    "2026-08-24T22-35-08-980Z-…: run kind adhoc is not pull-request"
  ]
}
```

## Properties

- **Deterministic.** A verdict is a pure function of recorded runs, the
  manifest policy, and the expected identities passed in. Re-evaluating with
  the same inputs yields the same verdict; there is no flake allowance and
  no model in the loop.
- **Exhaustive.** Evaluation does not stop at the first problem: a gate
  verdict lists every failing run, every hash mismatch, every missing
  target, journey, and matrix cell at once.
- **Fail-closed.** Missing inputs are errors, not passes:
  `expected harness source SHA-256 is required`,
  `at least one run ID is required`,
  `gate is pending green activation`.
- **Exit-code mapped.** Every CLI decision command exits `1` on a failing
  verdict (`gate-evaluate`, `gate-enforce`, `gate-prepush`, `status`,
  `matrix`, `receipt`, `verify-receipt`, `figure-evaluate`,
  `seo-evaluate`, `secret-scan`), so automation needs no output parsing.

## The error vocabulary

Gate-verdict errors are sentences, one condition each (all from
`agent/gate.mjs`):

```
<runId>: status is <status>
<runId>: complete harness source identity is missing
<runId>: harness source <sha> does not match <sha>
<runId>: exact build hash is missing
<runId>: <level> is below <minimum>
<runId>: complete app source identity is missing
<runId>: app source <sha|missing> does not match <sha>
<runId>: E3 artifact hashes are incomplete
<runId>: artifact path escapes its run: <file>
<runId>: artifact is missing: <file>
<runId>: artifact hash mismatch: <file>
<runId>: artifact cannot be hashed: <file> (<reason>)
<runId>: run kind <kind> is not <mode>
<runId>: release condition does not match <release>
<runId>: encrypted-at-rest artifact bundle is missing
<runId>: encrypted bundle hash does not match its manifest
<runId>: passing pre-upload secret scan is missing
runs do not identify one exact app source (<n> source hashes)
<target>: runs do not identify one exact build
required target is missing: <target>
required journey is missing: <journey>
<n> required matrix cell(s) are missing
<n> run(s) are outside the required matrix
release ID is required
signed receipt is required
receipt signature, trust, or payload hash is invalid
receipt app ID <id> does not match <id>
receipt release <r> does not match <r>
receipt harness source SHA-256 does not match
receipt app source SHA-256 does not match
receipt build identities do not match
receipt run IDs do not match gate run IDs
receipt verdict is not passed
<runId>: local policy evidence differs from the signed receipt
<runId>: encrypted bundle does not match the signed receipt
receipt verification failed: <reason>
```

Merge-eligibility blocking reasons (`agent/status.mjs`):

```
<journey>: no runs recorded
<journey>: last run is <status>
<journey>: evidence is older than HEAD
<journey>: <level> is below <minimum>
```

## Not to be confused with

- **A [gate](gate.md)** — the policy machine that produces verdicts; the
  verdict is its output, the gate also has activation state.
- **A run's `passed` flag** — one input among many; a verdict can fail over
  runs that all passed (wrong kind, stale identity, missing coverage).
- **An analysis** — normalization of one report; verdicts consume analyses,
  they do not replace them.
