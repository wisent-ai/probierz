# Limitations on this revision

Honest absences and sharp edges of the current `main`, each observed on this
checkout. None of these are documented promises; they are the present state.

## One invalid registered manifest blocks every registry-wide command

`apps/game-asset-creator/probierz.yaml` on this revision declares a surface
`eval` without a `spec`. Because `listApps()` validates every registered
manifest and fails closed, `probierz apps`, `probierz status <appId>`
(any app), `probierz overview`, `probierz affected`, `probierz ci`, and the
pre-push gate's app resolution all fail with:

```
probierz-failure {"failure_point":"cli.unknown","error_code":"config","service":"cli","impact":"cli","severity":"critical","retryable":false,"outage":true,"detail":"invalid app manifest: <checkout>/apps/game-asset-creator/probierz.yaml surface eval spec is required"}
probierz apps: probierz is missing configuration. See the detail on the line above; retrying will not help.
```

Single-app commands that load only one manifest still work: `app <appId>`,
`run --app <appId>`, `history`, `last-green`, `gate-*`, `receipt`. The repair
is to fix or remove the offending manifest; there is no flag to skip it.

## No persistent run intake on main

The only asynchronous execution path on this revision is the in-process job
queue in `agent/control.mjs`, reachable through the MCP
`probierz_start_run` family ([mcp](mcp.md#the-asynchronous-run-queue)). The
job table is a process-local `Map`: a server restart forgets every job
(the run directories on disk remain). An `agent/intake.mjs` daemon exists
only on a feature branch and is not part of main; nothing documented here
depends on it.

## `cmd` covers six targets, `run` covers nine

`RUN_COMMANDS` in `agent/lib.mjs` has entries only for `web`, `electron`,
`mobile:ios`, `mobile:android`, `desktop:mac`, `desktop:win`. Captured:

```
probierz cmd tui
→ detail: "unknown target: tui (one of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win)"
```

`probierz run tui`, `run desktop:cua`, and `run mobile:ios:byk-auth` work;
only the command-printing helper lacks them. The MCP descriptions for
`probierz_run`/`probierz_check`/`probierz_setup` understate the same list.

## `describe` sees titles, not imperative specs

`probierz describe` statically extracts `describe`/`it`/`test` titles. An
imperative spec (like the TUI demo spec in the
[walkthrough](walkthrough-register-app.md)) yields `"count": 0, "outline": []`
even though it runs and reports one test.

## Evidence level E1 has no producer

The ladder in [concepts/evidence](concepts/evidence.md) reserves `E1`;
nothing on this revision emits it.

## Identity is content-bound, and that bites quickly

Harness identity hashes every tracked and untracked-but-not-ignored file.
Editing anything in the checkout — including these docs — changes
`worktreeSha256`, so a receipt signed earlier can no longer be re-issued for
new runs without recomputing identities, and `createReceipt` refuses a stale
expectation with `expected harness source is stale relative to the current
Probierz checkout`. This is by design (evidence binds to exact content), but
it means gate and receipt commands must be given the identity captured by
`probierz source-identity` at evidence time, not an old one.

## Fleet-facing commands assume external tooling

`probierz overview` composes journey status with repository-hygiene and
fleet-health reads that shell out to external tools; on a machine without
them those sections degrade to recorded `error` fields (and on this checkout
`overview` is blocked earlier by the invalid manifest above). `probierz
stado *` requires an installed, configured `stado` CLI; without one the
bridge fails with a classified `stado.*` failure and the sentence `Local
runs are unaffected — 'probierz run <target>' still works without the stado
queue.`

## Source distribution only

There is no stable public binary or hosted service. `npm install` links the
`probierz` and `probierz-mcp` bins from this checkout; every documented
command is `node agent/cli.mjs …` under an alias.
