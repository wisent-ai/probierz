# Runbook

Something failed — which line do you read first, and what does it mean? Each
entry starts from the symptom, quotes the exact sentence Probierz prints,
and names the repair. Flag-by-flag detail lives in [cli](cli.md); every
example below was captured on this checkout unless marked source-grounded.

## Any command failed: read the two stderr lines first

Every failure crossing the process boundary produces exactly two stderr
lines (`agent/failure.mjs`). Captured:

```
probierz-failure {"failure_point":"cli.unknown","error_code":"config","service":"cli","impact":"cli","severity":"critical","retryable":false,"outage":true,"detail":"invalid app manifest: <checkout>/apps/game-asset-creator/probierz.yaml surface eval spec is required"}
probierz apps: probierz is missing configuration. See the detail on the line above; retrying will not help.
```

Read them in this order:

1. **`error_code`** answers "whose problem": `config` (yours — fix and
   rerun), `auth` (`… rejected our credentials. Refresh them; retrying will
   not help.`), `not_found` (`… has no such object. Check the identifier;
   retrying will not help.`), `timeout` / `infra_down` / `rate_limit`
   (theirs — `retry later`), `unknown` (`… failed in a way probierz does not
   recognise. See the detail on the line above.`). The code is classified
   from the failure text by fixed regexes — network wording maps to
   `infra_down`, "unauthorized/forbidden" to `auth`, "rate limit/throttl"
   to `rate_limit`.
2. **`retryable`** decides the exit code: retryable failures exit `69`
   (sysexits `EX_UNAVAILABLE`) so wrappers can back off; everything else
   exits with the command's own code (`1` classified, `2` usage).
3. **`failure_point`** names the dependency axis: `stado.*` (remote queue),
   `objects.*`, `model.route`, `run.spawn`, `run.report`, `cli.unknown`.
   When a retryable failure is remote-only, the sentence appends:
   `Local runs are unaffected — 'probierz run <target>' still works without
   the stado queue.`

## `apps`, `status`, `overview`, `affected`, or `ci` refuse with `invalid app manifest`

The registry fails closed: one invalid `apps/*/probierz.yaml` fails every
command that walks all manifests. On this checkout that is the state — see
the captured lines above and [limitations](limitations.md). The `detail`
names the file and the exact violated rule (the full rule vocabulary is in
[concepts/application](concepts/application.md)). Repair the named manifest;
there is no skip flag. Single-manifest commands (`app`, `run --app`,
`history`, `last-green`, `gate-*`, `receipt`) keep working meanwhile.

## `run` exits 3 and nothing spawned

The preflight gate blocked the run. Captured:

```json
{ "runId": "2026-08-24T22-38-58-561Z-…", 
  "preflight": {
    "missing": ["adb", "ANDROID_HOME set", "appium driver: uiautomator2"],
    "remediation": [
      "install Android SDK platform-tools and add them to PATH",
      "export ANDROID_HOME to your Android SDK location",
      "probierz setup mobile:android" ] } }
```

A `blocked` run manifest is still written — blocked is evidence, not
absence. Run the listed remediation: `probierz setup <target>` installs only
the parts Probierz owns; host hints (Xcode, Android SDK, WinAppDriver,
Accessibility grants) you install yourself. `--force` skips detection — use
it only when detection is wrong, not when the toolchain is missing.

## The suite exited 0 but the run is `failed`

Report identity failed. The run's `reportValidation.error` is one of
(`agent/runner.mjs`):

```
report missing
report predates run start
report run ID mismatch: expected <id>, got <id|missing>
report unreadable: <reason>
```

The suite must write its report to the `PROBIERZ_REPORT_PATH` it was given
and stamp it with `report.probierz.runId = PROBIERZ_RUN_ID`. A green child
with a stale or foreign report is a failed run by design. The same applies
after analysis: `completeRun` flips a passing process to `failed` with
explicit `evidence.errors` such as `zero executed checks`, `<n> failed
checks`, `analysis run ID mismatch`, or `recording requested but no
report-typed capture was produced` — see
[concepts/evidence](concepts/evidence.md).

## A run fails immediately with `seed failed: <detail>`

The manifest's `data.seed` hook failed before the suite spawned. The run is
recorded `failed` with that string in `reportValidation.error`, and
`data.cleanup` is attempted as rollback. Fix the seed command; the suite was
never the problem.

## `resource locked: <resource> by run <id> (pid <pid>)`

Mobile and native-desktop targets serialize real devices through directory
locks under `test-results/.locks/` (`agent/locks.mjs`, error code
`PROBIERZ_RESOURCE_LOCKED`). Either wait — `--resource-wait MS` polls
instead of failing — or find the owning run. A lock whose owner pid is dead
(30-second grace) is reclaimed automatically, so a crashed run does not
wedge the device; a live foreign pid means another run genuinely owns it.

## A run ends `timedOut: true`

The default budget is 20 minutes (`--timeout MS` overrides). On expiry the
whole process group gets SIGTERM, then SIGKILL after a 5-second grace; the
run is recorded `failed` with `timedOut: true` and the redacted output
tails. Raise the budget only after reading `stdout.log` — a hung driver
retries forever without one.

## A gate refuses

First command:

```bash
probierz gate-status <appId>
```

- `enforcement: "pending-green"` plus a `gate-enforce` verdict of
  `gate is pending green activation` (captured) is fail-closed by design:
  the gate has never seen a fully green evaluation. Produce one and run
  `gate-activate`; see the [walkthrough](walkthrough-gate-and-receipt.md).
- `<runId>: run kind adhoc is not pull-request` (captured): the gate only
  accepts runs whose recorded `kind` matches the mode. Re-run with
  `PROBIERZ_RUN_KIND=pull-request` as a run condition — an adhoc rerun can
  never satisfy a pull-request gate.
- `<runId>: harness source <sha> does not match <sha>` or `… app source …`:
  the checkout changed since the run. Recompute with `probierz
  source-identity <appId>` and either re-run the evidence or pass the
  identity the runs actually recorded. Identity hashes every
  tracked and untracked-but-not-ignored file — a stray editor file changes
  it ([limitations](limitations.md#identity-is-content-bound-and-that-bites-quickly)).
- `<runId>: E2 is below E3` (captured, receipt default): record the run
  (`--record`) so report-typed capture exists, or lower the policy floor
  explicitly (`--minimum E2`) where the policy allows it.

The complete gate error vocabulary is in
[concepts/verdict](concepts/verdict.md); each sentence names exactly one
failing condition, and the only way through is different evidence.

## `verify-receipt` says the signature is valid but `valid` is false

Captured:

```json
{ "valid": false, "signatureValid": true, "trusted": false, … }
```

Validity requires signature **and** trust **and** payload hash. An embedded
key is trusted only when its fingerprint is pinned — pass `--fingerprint
<sha256>` (or set `PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT`), or pass the
trusted key itself with `--public-key`. A receipt that verifies against its
own embedded key proves integrity, not authorship.

## `receipt` refuses with a stale expectation

`expected harness source is stale relative to the current Probierz
checkout` (or the app-source variant): receipts re-verify the expected
identities against the checkouts at signing time. Recompute `probierz
source-identity <appId>` and sign in the same content state as the runs;
any edit between run and signing changes the harness identity.

## `stado` commands fail while local runs work

Source-grounded (remote execution needs a Stado queue and was not exercised
for these docs): the bridge shells out to the installed `stado` CLI, and
every failure is classified onto a `stado.*` failure point — `stado.submit`
(host selector unknown, submit refused), `stado.upload`/`stado.download`
(tarball or storage transfer), `stado.watch` (queue unreachable, job failed,
one-hour watch timeout), `stado.worker` (the job itself failed remotely;
its retained `test-results/` archive is still downloaded, including blocked
preflights). Retryable cases exit `69` and end with the sentence
`Local runs are unaffected — 'probierz run <target>' still works without
the stado queue.` — believe it: the local path shares no state with the
queue.

## An MCP async run vanished

`unknown runId: <id>` (captured) from `probierz_run_status` after a server
restart is expected: the job table is an in-process `Map`
([mcp](mcp.md#the-asynchronous-run-queue)). The run directory under
`test-results/<appId>/<target>/<date>/<runId>/` survives — read it with
`probierz analyze` and `probierz history`, which never consult the queue.

## Something read evidence it should not have

```bash
probierz audit [appId] [--run runId] [--action name]
```

Every gate evaluation, protection, restore, and artifact read appends an
integrity-checked record under `test-results/.audit/<date>/`; each record
carries its own `sha256` and reads back with `valid: true` unless tampered.
Denied accesses are recorded too (`outcome: "denied"`), including MCP
artifact reads that attempted path traversal.
