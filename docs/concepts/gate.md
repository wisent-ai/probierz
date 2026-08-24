# Gate

Who decides whether evidence is good enough to merge or release — and what
stops that decision from being quietly weakened? A gate: a deterministic
evaluation bound to exact source identity, with an activation state that
fails closed until it has been green once.

## What it is

Per application, two modes — `pull-request` and `release` — each with an
enforcement state persisted in `gates.json` beside the manifest:

```json
{
  "schemaVersion": 2,
  "appId": "docs-demo",
  "modes": {
    "pull-request": { "enforcement": "pending-green" },
    "release":      { "enforcement": "pending-green" }
  }
}
```

Until the file exists, `gate-status` reports this default with
`"exists": false`. The policy a gate enforces comes from the manifest
(`pullRequestPolicy` / `releasePolicy`): `minimumEvidence` (default `E3`
when a policy exists but is silent), `requiredTargets`, `requiredJourneys`,
`requiredMatrixProfile`, `requireProtectedArtifacts`, `requireSecretScan`.

## What an evaluation checks

`gate-evaluate <appId> <mode> <harnessSha256> --source-sha SHA --runs ids`
checks, for every submitted run: passed status, complete harness and app
source identity matching the expected values, exact build hash, evidence
level at or above the minimum, artifact re-hash (or encrypted-bundle
re-hash when protected), run `kind` equal to the gate mode, and — release
mode — the `PROBIERZ_RELEASE` condition. Across runs: one exact app source,
one build per target, required targets/journeys present, required matrix
fully covered with no extra runs. Release mode additionally requires a
signed [receipt](receipt.md) that agrees with the local evidence
byte-for-byte (`canonical(signed) === canonical(local)`). The result is a
[verdict](verdict.md); the full error vocabulary is listed there.

## Lifecycle: pending-green → required

```
pending-green ──gate-activate (fully green evaluation)──▶ required
      │                                                      │
      └── gate-enforce: fails closed with                    └── gate-enforce:
          "gate is pending green activation"                     re-evaluates current evidence
```

- `gate-activate` runs a full evaluation and refuses on any error:
  `gate activation refused: <error>; <error>…` with error code
  `PROBIERZ_GATE_NOT_GREEN`. On success it atomically writes
  `enforcement: "required"` plus the activation evidence — expected
  identities, run IDs, builds, and receipt fingerprint (captured example in
  the [walkthrough](../walkthrough-gate-and-receipt.md)).
- `gate-enforce` on a `pending-green` mode returns
  `{"passed": false, "errors": ["gate is pending green activation"]}` and
  exit 1 without evaluating anything — fail closed. Once `required`, it
  re-evaluates.

Every evaluate/enforce/activate appends an integrity-checked audit record
(`gate.evaluate` / `gate.enforce` / `gate.activate`, outcome
`allowed`/`denied`) under `test-results/.audit/` — see `probierz audit`.

## The pre-push gate

`gate-install <appId> [--repo path]` writes a managed `pre-push` hook
(marker `# managed-by: probierz-prepush-gate`; an existing foreign hook is
backed up as `pre-push.before-probierz-gate` and chained first). On a push
to `main`/`master` the hook runs `agent/prepush-gate.mjs --hook --app
<appId> --ci` (evaluate-only when `PROBIERZ_GATE_NO_CI=1`), which:

1. resolves the app (explicit `--app`, else the repo must match a manifest:
   `no probierz app manifest matches <repo>`);
2. resolves the base (`cannot resolve a merge base with origin/main; fetch
   first or pass --base`);
3. maps the push diff to affected journeys — none means
   `{"ok": true, "note": "no affected journeys"}`;
4. optionally runs `probierz ci <base>` (`probierz ci failed (exit <n>)`);
5. picks the newest passing run per affected journey (none:
   `no passing runs recorded for the affected journeys; run 'probierz ci
   <base>' (or re-run with --ci) before pushing`);
6. evaluates the pull-request gate against the exact **current** harness and
   app identity and blocks with the verdict's reasons (exit 1).

`gate-prepush` runs the same pass manually; `probierz status <appId>` shows
the same eligibility without pushing. Note: steps that map the diff walk the
whole app registry, so one invalid registered manifest blocks the hook —
see [limitations](../limitations.md).

## Invariants

- No model, human note, or environment variable can override a gate error;
  the only way through is different evidence.
- Gates only accept runs whose `kind` matches the mode — an `adhoc` rerun
  can never satisfy a pull-request gate
  (`<runId>: run kind adhoc is not pull-request`, captured).
- Activation is one-way in the data model: the code path only ever writes
  `required`; loosening means editing managed state by hand, which the audit
  trail would show.

## Commands

```bash
probierz gate-status <appId>
probierz gate-evaluate <appId> pull-request <harnessSha> --source-sha <sha> --runs id1,id2
probierz gate-activate <appId> pull-request <harnessSha> --source-sha <sha> --runs ids
probierz gate-enforce  <appId> pull-request <harnessSha> --source-sha <sha> --runs ids
probierz gate-install  <appId> --repo /path/to/product
probierz gate-prepush  --repo /path/to/product --app <appId> [--base ref] [--ci]
```

## Not to be confused with

- **A [verdict](verdict.md)** — one evaluation's output; the gate also owns
  activation state across evaluations.
- **A [receipt](receipt.md)** — evidence the release gate consumes; signing
  a receipt does not open any gate.
- **Preflight** — readiness of a toolchain before a run; a gate judges
  recorded evidence after runs.
