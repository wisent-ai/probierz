# CLI reference — authoring, evaluators, remote Stado execution

Part of the [CLI reference](cli.md).

These commands write specs and manifests with model help, score figures and
SEO against rubrics, and run Probierz jobs on remote Stado capacity. This
page is the argv contract: positionals, flags, defaults, verbatim refusals,
exit rules. The workflow-level story lives in
[remote-stado.md](remote-stado.md) and [evaluators.md](evaluators.md); the
two-line stderr failure format and the exit-code table are in the
[conventions](cli.md#conventions).

**Evidence statement.** This page's own evidence is source-grounded, not
executed: the authoring, evaluator, and `stado` subcommands need external
services (the Stado queue, the Stado model router, local model binaries, a
live application to probe) and were NOT executed for this documentation —
every statement below comes from `agent/cli.mjs`, `agent/author-spec.mjs`,
`agent/author-manifest.mjs`, `agent/figure-evaluate.mjs`,
`agent/seo-evaluate.mjs`, `agent/seo-model.mjs`, `agent/model-router.mjs`,
and `agent/stado.mjs`. The one exception is `probierz hosts`, which queries
nothing and was captured on this checkout.

## Shared option validation

`author-spec`, `author-manifest`, `stado author`, and `stado seo` run a
strict argv validator: an unrecognized flag is `unknown option: <flag>`, an
extra positional is `unexpected argument: <arg>`, and a value flag followed
by nothing or by another flag is `<flag> needs a value` (all exit 2).
`stado run` looks its flags up positionally and does **not** reject unknown
options. The two evaluators have their own parser with the same value rule
plus a repetition guard: `<flag> may be supplied only once`.

## Authoring (local)

### `probierz author-spec <appId> <journey> --desc <goal> --target <t> [--paths p ...] [--base-url url] [--app-path p] [--rounds N] [--dry-run]`

Autonomous journey-spec authoring against the real app — no mocks, no fake
selectors, no stubbing the app under test. Per round (default 3 rounds;
`--rounds` accepts a number, anything non-numeric or `0` falls back to 3):

1. **Probe.** The real app is inspected once up front: `web`/`electron`
   loads `--base-url` in headless Playwright Chromium and captures the
   title, body text, headings, and interactive elements; `tui` spawns
   `--app-path` in a PTY and captures the initial screen after 3 s;
   `mobile:*`/`desktop:mac` fetch the accessibility tree through Appium on
   `127.0.0.1:4723` (iOS runtime version auto-detected via
   `xcrun simctl`); `desktop:cua` snapshots the tree through the
   cua-driver.
2. **Draft.** The brief (goal, probe, per-target style guide, prior
   failures) goes to a **local headless model binary**, not the Stado
   model router: the default and only model reachable from the CLI is
   `codex`, invoked as `spawnSync("codex", ["exec",
   "--skip-git-repo-check", "--sandbox", "workspace-write", brief])`; the
   module also supports `kimi` (`spawnSync("kimi", ["-p", brief,
   "--yolo"])`) behind a `model` parameter the CLI does not expose. Each
   model call is budgeted 3 600 000 ms. The model must write exactly one
   staged file, `.author-staging-<journey><ext>` in the target package's
   spec directory (`.spec.ts` for web/electron, `.spec.mjs` for
   tui/desktop:cua, `.e2e.ts` otherwise); if it does not, the round ends
   with `ok: false`, `reason: "model did not write the staged spec"` plus
   the agent's exit code and bounded stderr.
3. **Verify with a real run.** The staged spec is executed as
   `node agent/cli.mjs run <target> --app <appId> --spec <staged>
   PROBIERZ_RUN_KIND=pull-request`, with `--base-url` exported as
   `BASE_URL` and `--app-path` exported per target (`APP_IOS` plus
   auto-detected `IOS_VERSION`, `TUI_CMD`, `CUA_BUNDLE_ID`, or
   `MAC_APP_PATH`). Up to six failure messages (400 chars each) from the
   run's `analysis.json` feed the next round's brief together with the
   previous spec.
4. **Accept or iterate.** A passed run renames the spec to
   `<appId>-<journey><ext>` in the target spec directory and registers the
   journey in the app manifest (journey entry, surface journey list, and —
   when `--paths` was given — a `{ paths, journeys: [<journey>] }` mapping
   appended to `repositories[0].mappings`). Rounds exhausted: the staged
   file is removed and the result is `ok: false` with
   `authoring did not converge in <rounds> rounds` and the last failures.

`--paths` is repeatable. `--dry-run` returns the round-1 brief and the
staged path without invoking any model. Exit 1 whenever the result is not
`ok`.

Refusals (exit 2): `author-spec needs an app ID and a journey name`,
`author-spec needs --desc <journey goal>`, `author-spec needs --target
<web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui>`.
Module refusals (exit 1): `unsupported target: <t>`, `app <appId> has no
<target> surface`, `web authoring needs --base-url`, `<target> authoring
needs --app-path` (every target except `web` and `electron`).

### `probierz author-manifest <appId> --desc <d> --target <t> --repo <path> [--repo ...] [--owner o] [--base-url url] [--app-path p] [--specs] [--dry-run]`

Autonomous [manifest authoring](applications.md#authoring): probes the app
exactly like `author-spec`, summarizes up to 40 top-level directories per
`--repo`, and drafts the manifest through the **authenticated Stado model
router** (`draftStructuredArtifact`, tool `submit_probierz_manifest`;
requires `STADO_MODEL_ROUTER_URL` and `STADO_MODEL_ROUTER_TOKEN` — see
[configuration](configuration.md)). Each of up to 3 rounds writes the draft
to `test-results/.author-manifest/<appId>.probierz.yaml`, installs it as
`apps/<appId>/probierz.yaml` (an existing manifest is backed up first and
restored when validation fails), and validates it with the same loader
every hand-written manifest passes; validation errors feed the next brief.
`--owner` defaults to `<appId> maintainers`. `--specs` continues into
`author-spec` for every journey the accepted manifest declares. `--dry-run`
returns the round-1 brief without contacting the router. Exit 1 when not
`ok`; a router failure reports `reason: "Stado model-router authoring
failed"` with the error detail, and non-convergence reports `manifest did
not validate in 3 rounds: <last error>`.

Refusals (exit 2): `author-manifest needs an app ID`, `author-manifest
needs --desc <what the app does>`, `author-manifest needs --target
<web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui>`,
`author-manifest needs at least one --repo <path>`. Module refusals
(exit 1): `<target> needs --app-path`, `<target> needs --base-url`.

## Evaluators

Rubric semantics, deterministic blockers, and the model boundary are in
[evaluators.md](evaluators.md); below is only the argv contract.

### `probierz figure-evaluate --reference <file> --candidate <file> [opts]`

| Flag | Effect | Default |
|---|---|---|
| `--reference <file>` | ground-truth figure (required) | — |
| `--candidate <file>` | figure under evaluation (required) | — |
| `--rubric <json>` | replacement rubric file | built-in rubric |
| `--model <id>` | vision model ID | `PROBIERZ_FIGURE_VISION_MODEL` |
| `--out <file>` | report path | derived from the candidate |
| `--router-url <url>` | Stado model router base URL | `STADO_MODEL_ROUTER_URL` |
| `--tex-preamble <file>` | extra TeX preamble lines | built-in preamble |
| `--agent-id <id>` | HMAC agent identity for subscription routes | none (bearer only) |
| `--router-token-stdin` | read credentials from stdin | env token |

`--router-token-stdin` reads **all of stdin**: the first line is the router
bearer, the optional second line is the agent secret paired with
`--agent-id`. Without it the bearer comes from `STADO_MODEL_ROUTER_TOKEN`
(`STADO_MODEL_ROUTER_TOKEN or an explicit router bearer is required`).
Model refusal: `--model or PROBIERZ_FIGURE_VISION_MODEL is required`. Input
refusals: `reference path is required`, `reference is not a file: <path>`,
`reference type is not supported: <ext>` (and the same three for
`candidate`); accepted extensions are `.jpeg`, `.jpg`, `.pdf`, `.png`,
`.svg`, `.tex`, `.webp`.

Parser refusals (exit 2): `unknown figure-evaluate option: <flag>`,
`<flag> needs a value`, `<flag> may be supplied only once`,
`figure-evaluate needs --reference`, `figure-evaluate needs --candidate`.
Exit rule: the report is printed either way; exit 1 when
`verdict.pass` is false — deterministic blockers and the model rubric both
gate ([figure evaluation](evaluators.md#figure-evaluation)).

### `probierz seo-evaluate --base-url <url> [opts]`

| Flag | Effect | Default |
|---|---|---|
| `--app <appId>` | application whose SEO contract applies | `landing-page` |
| `--base-url <url>` | site under evaluation (required) | — |
| `--policy <file>` | SEO policy override | manifest `seo.policy` |
| `--brief <file>` | approved content brief override | manifest `seo.brief` |
| `--mode <m>` | manifest-declared profile | `release` |
| `--out <file>` | report path | derived |
| `--production-evidence <file>` | Search Console + CrUX document | `PROBIERZ_SEO_PRODUCTION_EVIDENCE` |
| `--primary-model <id>` | first grader | `PROBIERZ_SEO_PRIMARY_MODEL` |
| `--secondary-model <id>` | second grader | `PROBIERZ_SEO_SECONDARY_MODEL` |
| `--adjudicator-model <id>` | divergence adjudicator | `PROBIERZ_SEO_ADJUDICATOR_MODEL` |
| `--router-url <url>` | Stado model router base URL | `STADO_MODEL_ROUTER_URL` |
| `--agent-id <id>` | HMAC agent identity | `PROBIERZ_MODEL_AGENT_ID` |
| `--private-key-file <file>` | Ed25519 signing key | `PROBIERZ_RECEIPT_PRIVATE_KEY_FILE` |
| `--router-token-stdin` | read credentials from stdin | env token / secret |

With `--router-token-stdin` the stdin contract extends the figure
evaluator's: line 1 is the router bearer, line 2 the agent secret, and
**everything from line 3 on** is the Ed25519 private key (an alternative to
`--private-key-file` and `PROBIERZ_SEO_RECEIPT_PRIVATE_KEY`). Parser
refusals (exit 2): `unknown seo-evaluate option: <flag>`, `<flag> needs a
value`, `<flag> may be supplied only once`, `seo-evaluate needs
--base-url`. Exit rule: exit 1 when the report's `pass` is false. Grader
identity rules (`SEO primary and secondary model IDs must differ`, distinct
adjudicator) and everything the models may and may not score are in
[SEO evaluation](evaluators.md#seo-evaluation).

## Remote execution on Stado

The bridge shells out to the installed `stado` CLI and brings evidence back
into the local `test-results/` tree — mechanics in
[remote-stado.md](remote-stado.md). All three subcommands share the usage
refusal when the subcommand is missing or unknown (exit 2):

```
usage: probierz stado run <target> --app <id> [...] | probierz stado author <appId> <journey> [...] | probierz stado seo <appId> --base-url <url> --primary-model <id> --secondary-model <id> --adjudicator-model <id> [...]
```

an unknown `--host` fails with ``No such stado host: "<host>". Run `probierz
hosts` for the list.`` (classified `not_found`), and all three finish
through the same exit rule (`remoteExit`): exit 0 when the result state is
`completed`, or `queued` with `submitted: true` and no failure (an accepted
`--no-watch` submission); anything else prints one stderr line — the
failure's message, or `Remote run ended as "<state>".` — and exits
`EXIT_RETRY` 69 when the failure is retryable, else 1.

### `probierz hosts`

The host-selector table, from `listHosts()` in `agent/stado.mjs`.
Read-only; queries nothing. Captured on this checkout:

| Selector | Platform | Capacity request | Description |
|---|---|---|---|
| `local` | — | — | this machine (default) |
| `stado:gcp` | linux | `{"provider":"gcp","pin_to_provider":true}` | stado queue, GCP consumers only |
| `stado:azure` | linux | `{"provider":"azure","pin_to_provider":true}` | stado queue, Azure consumers only |
| `stado:aws` | linux | `{"provider":"aws","pin_to_provider":true}` | stado queue, AWS consumers only |
| `stado:any` | linux | `{}` | stado queue, any consumer with capacity |
| `stado:spot` | linux | `{"max_cost_per_hour_usd":4}` | stado queue, cost-capped capacity |
| `stado:local` | linux | `{"provider":"local","pin_to_provider":true}` | stado queue, local-kind consumers only |
| `stado:mini` | darwin | pinned host `local-charless-mac-mini.local` | stado queue, dedicated Mac mini consumer |
| `stado:macbook` | darwin | pinned host `local-lukaszs-macbook-pro-5485.local`; API `http://127.0.0.1:18765` | stado queue, dedicated MacBook consumer |
| `stado:t4` | linux | `{"gpu_type":"nvidia-tesla-t4"}` | stado queue, nvidia-tesla-t4 capacity |

(`platform: darwin` is explicit in the table; selectors without a platform
entry generate linux job scripts.)

### `probierz stado run <target> --app <appId> [opts]`

Submits one run job and, unless `--no-watch`, watches it and downloads the
evidence. Refusals (exit 2): `stado run needs a target (e.g. tui)`,
`stado run needs --app <appId>`.

| Flag | Effect | Default |
|---|---|---|
| `--app <appId>` | application (required) | — |
| `--spec <path>` | run only this spec | app surface spec |
| `--host <selector>` | Stado host | `stado:gcp` |
| `--app-repo <path>` | application source repository | manifest `repositories[0].root` (app-bundle only) |
| `--no-watch` | submit and exit `queued` | watch |
| `--record` | force capture on the worker | off |
| `--env=KEY=VALUE` | worker env (recorded into node-source provisioning only; repeatable) | — |

Application provisioning is one of three mutually exclusive kinds
(checked in this order: `--cargo-release` wins, then `--app-bundle-path`,
then `--node-source`; none means the job runs from the Probierz checkout
alone):

- **`--cargo-release [--binary <name>] [--cargo-manifest <path>]`** —
  Rust release build on the worker; `--binary` defaults to the appId,
  `--cargo-manifest` to `Cargo.toml`. Needs the source: `Remote
  cargo-release provisioning needs --app-repo <path>.` An absolute or
  traversing manifest path fails with `--cargo-manifest must be a safe
  path relative to --app-repo.`
- **`--app-bundle-path <path>`** — ship a prebuilt macOS app bundle plus
  its source for identity. A missing bundle: `The --app-bundle-path you
  gave does not exist. Build the bundle first.` No resolvable source repo:
  `Remote app-bundle runs need the app source repo: pass --app-repo, or
  set repositories[0].root in apps/<appId>/probierz.yaml.`
- **`--node-source [--script <path>]`** — pack a JS application's source
  (`Remote node-source provisioning needs --app-repo <path>.`);
  `--script` switches the job to script mode, running the repo-committed
  script instead of the default run command. `--script` with any other
  kind (or none): `--script requires --node-source (custom app jobs run
  from app sources)` (exit 2).

The printed result carries `host`, `jobId`, `target`, `appId`,
`submitted`, `state`, `failure`, and — for a completed job — `resultsDir`.
A failed job's retained artifacts are fetched too; a remotely blocked
preflight is reported as a `config` failure: `Job <jobId> did not execute
because the selected host is missing: <missing>.`

### `probierz stado author <appId> <journey> --target <t> --desc <goal> [opts]`

Runs the `author-spec` loop on a Stado host with scoped model credentials
([remote authoring](remote-stado.md#remote-authoring-and-seo)). Requires
`STADO_MODEL_ROUTER_URL` locally before anything is packed
(`STADO_MODEL_ROUTER_URL is required`). Refusals (exit 2): `stado author
needs an app ID and a journey name`, `stado author needs --target <t>`,
`stado author needs --desc <journey goal>`.

| Flag | Effect | Default |
|---|---|---|
| `--target <t>` / `--desc <goal>` | journey target and goal (required) | — |
| `--host <selector>` | Stado host | `stado:gcp` |
| `--app-path <path>` | installed-TUI command on the worker | — |
| `--app-bundle-path <path>` | prebuilt bundle provisioning | — |
| `--cargo-release [--binary <name>] [--cargo-manifest <path>]` | Rust release provisioning | binary = appId, `Cargo.toml` |
| `--app-repo <path>` | application source repository | — |
| `--no-watch` | submit and exit `queued` | watch |

`--target tui` demands real provisioning: `stado author --target tui needs
--app-path <installed-command> or --cargo-release --app-repo <path>
[--binary <name>]` (exit 2); a relative `--app-path` fails with `Remote
installed-TUI authoring needs --app-path <absolute-path>.` A completed job
adds `resultsDir` and `specDir` (the worker-relative spec directory the
accepted spec landed in) to the result.

### `probierz stado seo <appId> --base-url <url> --primary-model <id> --secondary-model <id> --adjudicator-model <id> [opts]`

Runs the complete SEO evaluator on a dedicated host. Refusals: `stado seo
needs an app ID` (exit 2); a missing member of the required quartet fails
with `Remote SEO evaluation needs --base-url, --primary-model,
--secondary-model, and --adjudicator-model.` (classified `config`);
`app <appId> has no SEO profile for <mode>`; `<mode> SEO profile requires
--production-evidence`; `production SEO evidence not found: <path>`.
Requires `STADO_MODEL_ROUTER_URL` locally.

| Flag | Effect | Default |
|---|---|---|
| `--base-url <url>` | site under evaluation (required) | — |
| `--mode <m>` | manifest-declared profile | `release` |
| `--policy <file>` / `--brief <file>` | contract overrides | manifest `seo.policy` / `seo.brief` |
| `--primary-model` / `--secondary-model` / `--adjudicator-model` | pinned grader IDs (required) | — |
| `--production-evidence <file>` | uploaded as `inputs/production-evidence.json` (path resolved to absolute) | — |
| `--agent-id <id>` | router agent identity | `probierz` |
| `--host <selector>` | Stado host | `stado:mini` |
| `--no-watch` | submit and exit `queued` | watch |

The job declares `STADO_MODEL_ROUTER_TOKEN` and
`PROBIERZ_MODEL_AGENT_SECRET` as Stado secret references — plus
`PROBIERZ_SEO_RECEIPT_PRIVATE_KEY` when the profile requires a signature —
so no secret enters `argv` or the payload
([secrets](remote-stado.md#secrets)).
