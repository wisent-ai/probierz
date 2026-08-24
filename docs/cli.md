# CLI reference

`probierz` (source entry point `agent/cli.mjs`) is the canonical interface
for discovery, setup, execution, analysis, figure and SEO evaluation,
authoring, evidence, gate, retention, security, and Stado workflows.
`probierz --help` prints the authoritative usage; this page groups the same
commands by intent. Commands marked read-only never execute a target,
install anything, or mutate a repository.

Surfaces: `web`, `electron`, `mobile`, `desktop-native`, `desktop-cua`,
`tui`. Targets: `web`, `electron`, `mobile:ios`, `mobile:android`,
`desktop:mac`, `desktop:cua`, `desktop:win`, `tui`.

## Discovery (read-only)

| Command | What it answers |
|---|---|
| `probierz list` | every test surface + run script |
| `probierz apps` | registered products, targets, and journeys |
| `probierz app <appId>` | the validated product manifest |
| `probierz source-identity <appId>` | exact harness and app source SHA-256 |
| `probierz specs [surface]` | spec files on disk (optional surface filter) |
| `probierz describe <spec>` | static outline (describe/it titles) of a spec file |
| `probierz cmd <target>` | the exact command to run a target (prints only) |
| `probierz accessibility <appId>` | validate stable IDs and native selectors |
| `probierz hosts` | run hosts: local and stado providers |
| `probierz affected [ref]` | which targets a change affects (git diff vs ref, or `--files a b c`) |

## Toolchain

- `probierz check <target>` — is the toolchain ready + how to fix what is
  missing. Read-only.
- `probierz setup <target>` — install the parts Probierz owns (browsers,
  Appium drivers). Side-effecting.

## Execution and analysis

- `probierz run <target> [opts]` — execute a target (preflight-gated),
  capture the result, auto-analyze.
- `probierz analyze <report> [dir]` — parse a report + inventory media.
- `probierz ci [ref] [opts]` — change-driven: select affected targets, run
  the ready ones, analyze.
- `probierz matrix <appId> <nightly|release> [--plan] [--release id]
  [KEY=VALUE...]` — plan or execute a declared condition matrix
  ([evidence-model](evidence-model.md#matrix-evidence-e4)).

### Run options

`--app <appId>`, `--record`, `--force` (skip preflight), `--spec <path>`,
`--frames N`, `--timeout MS`, `--resource-wait MS`, `--no-analyze`, plus any
`KEY=VALUE` condition variables (for example `BASE_URL=...`,
`APP_IOS=/abs/App.app`).

## Evidence, history, and comparison

| Command | What it answers |
|---|---|
| `probierz status <appId> [--base ref] [--text]` | journey coverage, freshness vs HEAD, merge eligibility (exit 1 when blocked) |
| `probierz overview [appId...] [--text]` | unified status: journeys + merge eligibility + violations + stado fleet health |
| `probierz history [appId] [target] [--limit N]` | stability by run, journey, and test |
| `probierz dashboard <appId> [limit]` | product/version/journey evidence projection |
| `probierz compare <leftRunId> <rightRunId> [appId]` | deterministic run diff |
| `probierz last-green [appId] [target] [journey]` | newest passing evidence |
| `probierz audit [appId] [--run runId] [--action name] [--limit N]` | access audit records |

## Gates, receipts, and publication

- `probierz gate-status <appId>` — pull-request and release gate activation
  state.
- `probierz gate-evaluate|gate-enforce|gate-activate <appId>
  <pull-request|release> <harnessSha256> --source-sha SHA256 --runs ids
  [release opts]` — evaluate, enforce, or activate a gate.
- `probierz gate-prepush [--repo path] [--app id] [--base ref] [--head sha]
  [--ci]` — pre-push merge gate (exit 1 when blocked).
- `probierz gate-install <appId> [--repo path]` — install the pre-push hook
  into a repository (chains an existing hook).
- `probierz receipt <appId> <release> <harnessSha256> --source-sha SHA256
  --runs ids` — sign an evidence receipt.
- `probierz verify-receipt <file>` — verify signature, trust, and payload
  hash.
- `probierz publication <receipt> <attemptId> <journeyId> --assets <json>
  [--public-key file | --fingerprint sha256]` — emit an immutable verified
  first-use publication manifest.
- `probierz publish-onboarding <receipt> --run id --journey id
  --journey-version v --journey-version-id uuid --first-success-fact fact
  --screen id --assets catalog.json --output publication.json` — emit an
  Echo-ingestible first-use proof manifest.

Details: [gates-and-receipts](gates-and-receipts.md).

## Artifact protection, retention, and scanning

- `probierz protect <appId> <runId> [kind] --key-file <path>
  [--remove-source]` — encrypt a run into an authenticated evidence bundle.
- `probierz restore <bundle> <destination> --key-file <path>` — restore a
  bundle into an empty directory.
- `probierz retention <appId> [--at ISO] [--apply]` — plan or apply
  retention expiry.
- `probierz secret-scan <directory>` — scan plaintext artifacts for
  high-confidence secrets.

## Authoring (model-routed, side-effecting)

- `probierz author-spec <appId> <journey> --target <t> --desc <goal>
  [--base-url u | --app-path p] [--paths glob] [--rounds N] [--dry-run]` —
  draft one journey spec through the authenticated Stado model router,
  verify it with a real run, keep it green.
- `probierz author-manifest <appId> --desc <what> --repo <path> --target <t>
  [--base-url u | --app-path p] [--owner s] [--specs] [--dry-run]` — draft
  the application manifest, then optionally cover every journey.

## Evaluators

- `probierz figure-evaluate --reference <svg|tex|pdf|image> --candidate
  <svg|tex|pdf|image> [--rubric json] [--model id] [--out report.json]
  [--tex-preamble file] [--router-url url] [--agent-id id]
  [--router-token-stdin]` — deterministic render checks plus rubric-scored
  vision evaluation.
- `probierz seo-evaluate --app <id> --base-url <url> [--policy json]
  [--brief json] [--mode pull-request|release|nightly|production]
  [--out report.json] [--production-evidence json] [--primary-model id]
  [--secondary-model id] [--adjudicator-model id] [--router-url url]
  [--agent-id id] [--private-key-file pem] [--router-token-stdin]` — full
  crawl, indexability, structured-data, dual-model content, performance,
  production, and signed SEO verdict.
- `probierz readme-gif <video> --out <file.gif> [--start seconds]
  [--duration seconds] [--fps N] [--width pixels] [--force]` — render a
  bounded, silent journey demo GIF plus a provenance sidecar.

Details: [evaluators](evaluators.md).

## Remote execution on Stado

- `probierz stado run <target> --app <id> [--spec f] [--record] [--host ...]
  [--no-watch]` — run a target on a chosen Stado host; evidence lands back
  in `test-results/`.
- `probierz stado author <appId> <journey> --target <t> --desc <d>
  [--host h] [--no-watch]` — author on a Stado host with scoped model
  credentials.
- `probierz stado seo <appId> --base-url <url> --primary-model <id>
  --secondary-model <id> --adjudicator-model <id> [--mode ...] [--host ...]
  [--no-watch]` — execute the complete SEO evaluator on a Stado-selected
  dedicated host.

Host selectors and provisioning flags: [remote-stado](remote-stado.md).

## Machine output

Status, overview, run, analysis, figure evaluation, SEO evaluation, and gate
commands expose structured data; automation must not infer state from prose.
The same modules also back the stdio MCP server, [mcp](mcp.md).
