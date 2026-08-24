# MCP server reference

`probierz-mcp` (source entry point `agent/mcp.mjs`; `npm install` links the
bin) exposes the same agent modules the CLI uses as a stdio MCP server. This
page is the complete tool reference; the underlying contracts are the CLI's
and are documented in [cli](cli.md), [evidence-model](evidence-model.md),
[gates-and-receipts](gates-and-receipts.md), and [evaluators](evaluators.md).

## Protocol

Newline-delimited JSON-RPC 2.0: UTF-8 requests on stdin, exactly one response
line per request on stdout, diagnostics on stderr. Four methods are served:
`initialize`, `ping`, `tools/list`, and `tools/call`. No resources or prompts
are registered. Captured `initialize` answer (this checkout):

```json
{ "protocolVersion": "2024-11-05",
  "capabilities": { "tools": {} },
  "serverInfo": { "name": "probierz", "version": "0.1.0" } }
```

- Every tool result is one `content` entry of type `text` holding
  pretty-printed JSON — the same document the CLI would print.
- Errors: `-32700` unparseable line, `-32601` unknown method or tool
  (captured: `unknown tool: probierz_nope`), `-32000` any handler failure
  with the thrown message (captured: `unknown runId: nope`).
- `tools/list` serves 44 tools; every schema below is taken verbatim from
  that answer on this checkout.
- The `probierz_run`/`probierz_check`/`probierz_setup` description strings
  enumerate six targets, but the handlers accept the full `TARGETS` map in
  `agent/runner.mjs`: `web`, `electron`, `mobile:ios`, `mobile:ios:byk-auth`,
  `mobile:android`, `desktop:mac`, `desktop:win`, `desktop:cua`, `tui`.

## The asynchronous run queue

Six tools drive the in-process run queue in `agent/control.mjs` — the full
lifecycle semantics are in [concepts/run](concepts/run.md#asynchronous-runs).
Captured session (demo app, this checkout):

```
probierz_start_run {"target":"tui","appId":"docs-demo"}
  → { "runId": "2026-08-24T22-38-06-656Z-104d5be2-…", "status": "queued", … }
probierz_run_status (immediately)          → "status": "running"
probierz_run_status (after 0.5 s)          → "status": "passed",
    "artifactsDir": "…/test-results/docs-demo/tui/2026-08-24/2026-08-24T22-38-06-656Z-…"
probierz_list_artifacts                    → 8 files (analysis.json, diagnostics.json,
    performance.json, report.json, run-manifest.json, stderr.log, stdout.log, timeline.json)
probierz_get_artifact {"file":"report.json"} → base64, 513 bytes; the report
    carries the run's own ID under report.probierz.runId
probierz_cancel_run (settled job)          → "cancelRequested": false
probierz_cancel_run (running job)          → "cancelRequested": true; the job
    settles as "status": "canceled" with no error
probierz_get_artifact {"file":"../../secrets"} → error -32000
    "artifact path escapes the run directory"
```

Queue facts (`agent/control.mjs`):

- `startRun` refuses a missing target with `target must be a non-empty
  string`, assigns `runId` (timestamp + UUID), and executes the same
  `runSurface` → `analyzeRun` → `completeRun` path as `probierz run`.
- Job states: `queued` → `running` → `passed` | `failed` | `blocked` |
  `canceled`. A preflight-refused run settles as `blocked`.
- The job table is an in-process `Map`; it does not survive a server restart
  (run directories on disk do), and every artifact read is audited
  (`artifact.list` / `artifact.read`, denied reads included).
- `probierz_get_artifact` is bounded to 5 MiB
  (`artifact exceeds 5242880 byte inline limit`) and refuses non-files
  (`artifact is not a file: <file>`) and traversal
  (`artifact path escapes the run directory`).

## Execution note

Read-only tools, the async queue, and `probierz_verify_receipt` were
exercised live for this page. The heavy local tools (`probierz_setup`,
`probierz_run_matrix`, `probierz_protect_run`, …) share their contracts with
the CLI commands documented in [cli](cli.md) and siblings; the authoring,
evaluator, and `probierz_stado_*` tools additionally need the Stado queue or
the authenticated model router and were not executed here — their entries
below are schema-grounded only.

## Discovery (read-only)

### `probierz_list_surfaces`

List the cross-platform test surfaces (web, electron, mobile, desktop-native): tool, npm script, targets, and relevant env vars.

No arguments.

### `probierz_list_specs`

Discover e2e/spec files on disk; optional surface narrows to one (web|electron|mobile|desktop-native).

- `surface` (string, optional) — Optional surface filter.

### `probierz_describe_spec`

Static outline of a spec (describe/it/test titles in file order) by its path under the probierz root. Does not execute anything.

- `spec` (string, required) — Spec path, e.g. packages/mobile/test/specs/byk.e2e.ts

### `probierz_run_command`

Return the exact shell command to run a target yourself (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win). Read-only: probierz never runs it.

- `target` (string, required) — One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win.

### `probierz_check`

Preflight a target's toolchain WITHOUT running anything: reports whether it is ready and, for each missing piece, exactly how to fix it -- `probierz setup <target>` for parts probierz owns (Playwright browsers, Appium drivers) or a host install command for the rest (Xcode, Android SDK, simulators, WinAppDriver). Read-only.

- `target` (string, required) — One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win.

### `probierz_affected`

Given a change, report which run targets it could affect, so you re-run only what is relevant. Deterministic + structural (maps files to targets by package containment; agent/ or repo-root files are cross-cutting -> all targets). Provide `files` explicitly, or omit to diff the working tree against `ref` (default HEAD) via git. Read-only.

- `files` (array of string, optional) — Changed file paths (repo-relative). If given, git is not consulted.
- `ref` (string, optional) — git ref to diff the working tree against when `files` is omitted (default HEAD).

### `probierz_source_identity`

Compute exact path-independent harness and app source SHA-256 identities.

- `appId` (string, required)

### `probierz_history`

Read deterministic E5 stability history: pass rate, infrastructure failures, duration trend, flaky tests, journeys, latest run, and last green.

- `appId` (string, optional) — Product identifier (default probierz).
- `target` (string, optional) — Optional target filter.
- `limit` (number, optional) — Maximum recent runs (default 50).

### `probierz_dashboard`

Project evidence for product → version → journey → surface → device → result → artifact dashboard navigation.

- `appId` (string, required)
- `limit` (number, optional) — Maximum recent runs (default 500).

### `probierz_matrix_plan`

Read the deterministic nightly or release matrix without executing it.

- `appId` (string, required)
- `profile` (string, required) — nightly or release

### `probierz_status`

Journey coverage, evidence freshness vs HEAD, untested surfaces, and pull-request merge eligibility for an app.

- `appId` (string, required)
- `baseRef` (string, optional) — Default origin/main.

### `probierz_gate_status`

Read pull-request and release gate activation state.

- `appId` (string, required)

### `probierz_audit`

Read and integrity-check access audit records, optionally filtered by app, run, or action.

- `appId` (string, optional)
- `runId` (string, optional)
- `action` (string, optional)
- `limit` (number, optional)

### `probierz_secret_scan`

Scan a plaintext artifact directory for high-confidence secrets without returning secret values.

- `directory` (string, required)

### `probierz_compare_runs`

Deterministically compare status, duration, tests, evidence, build identity, and artifact hashes between two run IDs.

- `appId` (string, optional) — Product identifier (default probierz).
- `leftRunId` (string, required)
- `rightRunId` (string, required)

### `probierz_last_green`

Return the newest passing run for a product, optional target, and optional journey.

- `appId` (string, optional) — Product identifier (default probierz).
- `target` (string, optional)
- `journey` (string, optional)

### `probierz_verify_receipt`

Verify receipt payload hash and Ed25519 signature against an explicit trusted public key or fingerprint.

- `file` (string, required)
- `trustedPublicKeyFile` (string, optional)
- `expectedFingerprint` (string, optional)

## Toolchain and execution

### `probierz_setup`

Install the toolchain parts probierz owns for a target (npm deps + Playwright browsers, or npm deps + the Appium driver). Does NOT install host-level dependencies (Xcode, Android SDK, simulators, WinAppDriver) -- probierz_check reports those. Side-effecting: runs npm / appium driver install.

- `target` (string, required) — One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win.
- `timeoutMs` (number, optional) — Kill a setup step after this many ms (default 30 min).

### `probierz_run`

EXECUTE a target end-to-end (spawns Playwright or WebdriverIO+Appium) under chosen conditions, records video/trace/screenshot when record=true, and returns the run result plus an analysis of what it produced. Heavy + side-effecting: needs Chromium / Appium / a simulator.

- `target` (string, required) — One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win.
- `record` (boolean, optional) — Force video + trace + screenshot capture on.
- `appId` (string, optional) — Product identifier used in the run-scoped artifact path and manifest.
- `env` (object, optional) — Condition env vars, e.g. { BASE_URL, APP_IOS, PROBIERZ_LOCALE, PROBIERZ_COLOR_SCHEME }.
- `timeoutMs` (number, optional) — Kill the run after this many ms (default 20 min).
- `resourceWaitMs` (number, optional) — Wait this long for a busy device/port lease; 0 fails fast.
- `frames` (number, optional) — Extract this many frames per recorded video (needs ffmpeg).
- `analyze` (boolean, optional) — Analyze the report after the run (default true).
- `force` (boolean, optional) — Skip the preflight gate and spawn even if the toolchain looks incomplete.
- `spec` (string, optional) — Run only this one spec (path/substring), e.g. packages/mobile/test/specs/byk.e2e.ts, to scope the run to a single app's suite.

### `probierz_analyze`

Parse a finished run's report (Playwright report.json or the WDIO probierz-<kind>-results.json) and inventory its media: totals, per-test status, failure reasons, and recording metadata (duration/dimensions via ffprobe, optional frame montage via ffmpeg).

- `reportPath` (string, required) — Path to the machine-readable report (from a probierz_run result).
- `artifactsDir` (string, optional) — Directory to inventory for media (from a probierz_run result).
- `tool` (string, optional) — playwright | wdio (inferred from the report if omitted).
- `frames` (number, optional) — Extract this many frames per video (needs ffmpeg).

### `probierz_ci`

Change-driven test pass: select the targets a change affects, run the ready ones (preflight-gated, blocked ones are reported with their fix, not spawned), analyze what ran, and return a consolidated verdict {summary:{passed,failed,blocked,ran}, results}. Composes probierz_affected + probierz_run + probierz_analyze. Heavy: runs real suites. Deterministic selection; no LLM reasoning about the results.

- `files` (array of string, optional) — Changed file paths (repo-relative). If given, git is not consulted.
- `ref` (string, optional) — git ref to diff the working tree against when `files` is omitted (default HEAD).
- `appId` (string, optional) — Product identifier for every selected run.
- `env` (object, optional) — Conditions forwarded to every selected run.
- `spec` (string, optional) — Optional spec filter forwarded to every selected target.
- `record` (boolean, optional) — Force video/trace/screenshot capture on for every run.
- `force` (boolean, optional) — Skip each target's preflight gate and spawn anyway.
- `frames` (number, optional) — Extract this many frames per recorded video (needs ffmpeg).
- `timeoutMs` (number, optional) — Per-run timeout in ms.
- `resourceWaitMs` (number, optional) — Wait per selected run for a busy device/port lease; default 10 min, 0 fails fast.

### `probierz_run_matrix`

HEAVY + SIDE-EFFECTING: execute every cell of a declared nightly or release matrix and return an E4 verdict.

- `appId` (string, required)
- `profile` (string, required) — nightly or release
- `release` (string, optional) — Required for a release matrix.
- `env` (object, optional) — Secrets and exact release artifact conditions; matrix axes cannot be overridden.

### `probierz_gate_prepush`

Pre-push merge gate: select affected journeys from the push diff and evaluate the newest passing runs against the exact current HEAD identity (pull-request policy).

- `repo` (string, required)
- `appId` (string, optional) — Inferred from manifest repositories when omitted.
- `base` (string, optional)
- `head` (string, optional)
- `runCi` (boolean, optional) — Run probierz ci <base> before evaluating.

## Asynchronous runs (in-process queue)

### `probierz_start_run`

HEAVY + SIDE-EFFECTING: start a real run asynchronously and return its runId immediately. Poll with probierz_run_status; cancel with probierz_cancel_run.

- `target` (string, required)
- `record` (boolean, optional)
- `appId` (string, optional)
- `env` (object, optional)
- `timeoutMs` (number, optional)
- `resourceWaitMs` (number, optional) — Wait this long for a busy device/port lease; 0 fails fast.
- `frames` (number, optional)
- `analyze` (boolean, optional)
- `force` (boolean, optional)
- `spec` (string, optional)

### `probierz_run_status`

Return queued/running/blocked/passed/failed/canceled state for an asynchronous run.

- `runId` (string, required)

### `probierz_cancel_run`

Cancel an asynchronous run and terminate its complete spawned process tree.

- `runId` (string, required)

### `probierz_get_result`

Return the completed normalized result and evidence for an asynchronous run.

- `runId` (string, required)

### `probierz_list_artifacts`

List run-scoped evidence artifacts for a completed asynchronous run.

- `runId` (string, required)

### `probierz_get_artifact`

Read one run-scoped artifact up to 5 MiB as base64; path traversal is rejected.

- `runId` (string, required)
- `file` (string, required) — Run-relative artifact path.

## Evidence handling

### `probierz_protect_run`

SIDE-EFFECTING: encrypt a complete run into an authenticated AES-256-GCM evidence bundle; optionally remove plaintext artifacts.

- `appId` (string, required)
- `runId` (string, required)
- `kind` (string, optional)
- `keyFile` (string, optional)
- `removePlaintext` (boolean, optional)

### `probierz_restore_bundle`

SIDE-EFFECTING: authenticate and restore an encrypted evidence bundle into an empty directory.

- `file` (string, required)
- `destination` (string, required)
- `keyFile` (string, optional)

### `probierz_retention`

Plan retention expiry; with apply=true, delete expired plaintext runs and encrypted bundles.

- `appId` (string, required)
- `at` (string, optional) — Optional ISO timestamp.
- `apply` (boolean, optional)

### `probierz_create_receipt`

SIDE-EFFECTING: secret-scan evidence, verify exact source/build/artifact provenance, and sign a release receipt with immutable journey identities and report-typed publication media.

- `appId` (string, required)
- `release` (string, required)
- `expectedHarnessSha` (string, required)
- `expectedSourceSha` (string, required)
- `runIds` (array of string, required)
- `requiredJourneys` (array of string, optional)
- `minimumEvidence` (string, optional) — Default E3.
- `privateKeyFile` (string, optional) — Defaults to PROBIERZ_RECEIPT_PRIVATE_KEY_FILE.

### `probierz_create_publication_manifest`

SIDE-EFFECTING: verify a signed receipt, current source, secret scan, evidence hashes, driver capability, redaction review, and immutable storage registrations before emitting a deterministic first-use publication manifest.

- `receiptFile` (string, required)
- `attemptId` (string, required)
- `journeyId` (string, required)
- `assets` (array of object, required)
- `trustedPublicKeyFile` (string, optional)
- `expectedFingerprint` (string, optional)

## Gates

### `probierz_gate_evaluate`

Evaluate exact build, E3 evidence, coverage, matrix, encryption, secret scan, and signed receipt eligibility; appends an audit record.

- `appId` (string, required)
- `mode` (string, required) — pull-request or release
- `expectedHarnessSha` (string, required)
- `expectedSourceSha` (string, required)
- `runIds` (array of string, required)
- `release` (string, optional) — Required for release mode.
- `receiptFile` (string, optional) — Required signed evidence receipt for release mode.
- `trustedPublicKeyFile` (string, optional)
- `expectedFingerprint` (string, optional)

### `probierz_gate_enforce`

Enforce an activated gate against current evidence; pending-green gates fail closed.

- `appId` (string, required)
- `mode` (string, required) — pull-request or release
- `expectedHarnessSha` (string, required)
- `expectedSourceSha` (string, required)
- `runIds` (array of string, required)
- `release` (string, optional) — Required for release mode.
- `receiptFile` (string, optional) — Required signed evidence receipt for release mode.
- `trustedPublicKeyFile` (string, optional)
- `expectedFingerprint` (string, optional)

### `probierz_gate_activate`

SIDE-EFFECTING: atomically activate a gate only after all green evidence requirements pass.

- `appId` (string, required)
- `mode` (string, required) — pull-request or release
- `expectedHarnessSha` (string, required)
- `expectedSourceSha` (string, required)
- `runIds` (array of string, required)
- `release` (string, optional) — Required for release mode.
- `receiptFile` (string, optional) — Required signed evidence receipt for release mode.
- `trustedPublicKeyFile` (string, optional)
- `expectedFingerprint` (string, optional)

## Authoring

### `probierz_author_spec`

SIDE-EFFECTING: use the authenticated Stado model router to draft one journey spec from a probe of the real app, verify it with an actual run, and keep it on green (registers the journey in the app manifest).

- `appId` (string, required)
- `journey` (string, required)
- `target` (string, required) — web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win|tui
- `desc` (string, required) — Journey goal in one or two sentences.
- `baseUrl` (string, optional)
- `appPath` (string, optional)
- `rounds` (number, optional)

### `probierz_author_manifest`

SIDE-EFFECTING: use the authenticated Stado model router to draft the whole app journey manifest from a probe and repository layout, validate it, and optionally cover every journey with author-spec.

- `appId` (string, required)
- `desc` (string, required) — What the app does, in one or two sentences.
- `repositories` (array of string, required)
- `target` (string, required)
- `baseUrl` (string, optional)
- `appPath` (string, optional)
- `withSpecs` (boolean, optional)

## Evaluators and media

### `probierz_evaluate_figure`

SIDE-EFFECTING: render a scientific reference/candidate pair, run deterministic geometry checks, score the declared visual rubric through the authenticated model router, and write immutable PNG evidence plus a JSON verdict.

- `referencePath` (string, required) — Reference or intermediate SVG, TeX, PDF, or raster image.
- `candidatePath` (string, required) — Candidate or final SVG, TeX, PDF, or raster image.
- `rubricPath` (string, optional) — Optional JSON rubric; uses the scientific-figure release rubric by default.
- `texPreamblePath` (string, optional) — Optional LaTeX preamble lines added to the standalone wrapper used for TeX input.
- `model` (string, optional) — Vision-capable model ID; defaults to PROBIERZ_FIGURE_VISION_MODEL.
- `agentId` (string, optional) — Agent identity for subscription routes; the secret comes from PROBIERZ_MODEL_AGENT_SECRET.
- `outputPath` (string, optional) — Optional destination ending in .json; existing evidence is never overwritten.

### `probierz_evaluate_seo`

SIDE-EFFECTING: crawl a declared site as ordinary Chrome and Googlebot Smartphone, enforce indexability and structured-data contracts, collect mobile performance evidence, run two independent Brama content graders with conditional adjudication, ingest optional Search Console/CrUX evidence, and write an immutable signed SEO verdict.

- `appId` (string, optional) — Manifest app ID; defaults to landing-page.
- `baseUrl` (string, required) — Credential-free HTTPS origin or loopback HTTP URL to evaluate.
- `policyPath` (string, optional) — Optional SEO policy JSON; defaults to manifest seo.policy.
- `briefPath` (string, optional) — Optional approved landing brief JSON; defaults to manifest seo.brief.
- `mode` (string, optional) — pull-request, release, nightly, or production; defaults to release.
- `outputPath` (string, optional) — Optional immutable report destination ending in .json.
- `productionEvidencePath` (string, optional) — Optional Search Console and CrUX evidence JSON.
- `primaryModel` (string, optional) — Pinned first Brama model ID.
- `secondaryModel` (string, optional) — Pinned independent second Brama model ID.
- `adjudicatorModel` (string, optional) — Pinned Brama model used only when graders disagree.
- `routerBaseUrl` (string, optional) — Brama-compatible router base; defaults to STADO_MODEL_ROUTER_URL.
- `agentId` (string, optional) — Probierz model identity.
- `privateKeyFile` (string, optional) — Ed25519 PKCS#8 PEM file for release evidence signing.

### `probierz_create_readme_gif`

SIDE-EFFECTING: convert one recorded journey video into a bounded, silent, looping README GIF and write a provenance sidecar with source/output SHA-256 and mandatory publication checks. Requires ffmpeg.

- `input` (string, required) — Recorded journey video path.
- `output` (string, required) — Destination path ending in .gif.
- `startSeconds` (number, optional) — Non-negative trim offset; default 0.
- `durationSeconds` (number, optional) — Published clip duration; default 12, maximum 30.
- `framesPerSecond` (number, optional) — GIF frame rate; default 12, maximum 20.
- `width` (number, optional) — Output width; default 960, maximum 1200.
- `force` (boolean, optional) — Replace an existing GIF and sidecar.

## Remote execution

### `probierz_stado_run`

SIDE-EFFECTING: run a target on a chosen stado host (provider/pin/spot/GPU); evidence lands back in test-results.

- `target` (string, required)
- `appId` (string, required)
- `spec` (string, optional)
- `host` (string, optional) — stado:gcp|azure|aws|any|spot|local|t4
- `cargoRelease` (boolean, optional) — Build the app binary on the worker with cargo (needs appRepo).
- `appRepo` (string, optional)
- `watch` (boolean, optional) — Default true; waits for completion and fetches results.

### `probierz_stado_evaluate_seo`

SIDE-EFFECTING: submit the complete SEO evaluator to a Stado-selected dedicated host, materialize only the declared Brama and signing secrets, and fetch the immutable evidence bundle.

- `appId` (string, optional) — Manifest app ID; defaults to landing-page.
- `baseUrl` (string, required)
- `mode` (string, optional) — pull-request, release, nightly, or production.
- `policyPath` (string, optional)
- `briefPath` (string, optional)
- `primaryModel` (string, required)
- `secondaryModel` (string, required)
- `adjudicatorModel` (string, required)
- `agentId` (string, optional)
- `productionEvidencePath` (string, optional)
- `host` (string, optional) — Dedicated Stado host; defaults to stado:mini.
- `watch` (boolean, optional) — Default true; waits for completion and fetches results.
