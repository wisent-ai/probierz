---
name: probierz
description: Use Probierz to discover, execute, and analyze Wisent quality evidence across web, Electron, mobile, native desktop, terminal applications, scientific figures, and SEO releases. Its CLI and stdio MCP server expose read-only discovery, preflight, target execution with report/media capture, figure comparison, and a complete SEO evaluator that enforces crawl/index contracts, scores content through independent Brama graders, ingests production observations, and signs immutable evidence. Use it to inspect existing journeys, run an authorized target, evaluate a candidate scientific figure, or produce a release SEO verdict.
---

# probierz

Probierz is the Wisent cross-platform quality-evidence toolkit. It is one Rust
crate, `probierz-rs`, shipping two binaries: `probierz`, the command an operator
runs, and `probierz-mcp`, the stdio MCP server an agent talks to. Each surface
is env-var driven, with no hardcoded targets.

## Two layers

- **Discovery (read-only):** list surfaces, discover spec files on disk, outline
  a spec's titles statically, and print the exact run command. No side effects.
- **Execution + analysis:** actually run a target under chosen conditions,
  record video/trace/screenshots, and analyze the result. A live run needs
  Chromium, Appium, a cua-driver session, a terminal, or an iOS/Android/desktop
  target and is a heavy side-effecting action — only `run` reaches it;
  discovery never does.

## Surfaces

|Surface|Tool|Package|
|---|---|---|
|`web`|Playwright (Chromium / Firefox / WebKit + emulated mobile)|`packages/web`|
|`electron`|Playwright (`_electron`)|`packages/electron`|
|`mobile`|WebdriverIO + Appium (XCUITest / UiAutomator2)|`packages/mobile`|
|`desktop-native`|WebdriverIO + Appium (Mac2 / WinAppDriver)|`packages/desktop-native`|
|`desktop-cua`|cua-driver (macOS accessibility)|`packages/desktop-cua`|
|`tui`|a real PTY|`packages/tui`|

Targets: `web`, `electron`, `mobile:ios`, `mobile:ios:byk-auth`,
`mobile:android`, `desktop:mac`, `desktop:win`, `desktop:cua`, `tui`.

The `tui` and `desktop:cua` journeys are functions in this crate rather than
spec files on disk: `probierz specs tui` and `probierz specs desktop:cua` list
what is registered, and `probierz run tui` executes them in-process, writing
the same canonical report every other surface writes.

`ci` is the composition the others build up to: `affected` picks the targets a
change touches, each runs preflight-gated (`check`/`run`), `analyze` reads what
ran, and it returns one verdict. Deciding WHICH targets is structural and lives
here; deciding whether a failure is real or what to change is an LLM's job
(Brama), one layer up. Probierz stays deterministic except for its explicit,
bounded model surfaces: automatic repair plus the rubric-bound figure and SEO
evaluators. None may reinterpret a deterministic blocker.

## CLI

Install it by building the crate: `cargo build --release` in `probierz-rs`
produces `probierz` and `probierz-mcp` under `probierz-rs/target/release`. A
host in the fleet gets them from a published Stado release instead.

<!-- generated: cli -->

Every command of `probierz 0.1.0`:

|Command|What it does|
|---|---|
|`onboarding`|Show the first-run journey and optionally adopt existing definitions|
|`project`|Durable project-adoption operations|
|`serve`|Run the loopback API used by Probierz Desktop|
|`list`|Every test surface, its tool, its npm script and its target coordinates|
|`apps`|Registered products, their targets and their journeys|
|`app`|One validated product manifest|
|`apphook`|Run one built-in application setup, broker, or evaluation capability|
|`specs`|The spec files on disk, optionally for one surface|
|`describe`|The static outline of one spec file: its describe and it titles|
|`cmd`|The exact shell command that runs a target, printed and not run|
|`hosts`|The run hosts this harness can use: local and the Stado providers|
|`source-identity`|Exact path-independent harness and application source identity|
|`accessibility`|Validate stable identifiers and native selectors|
|`author-spec`|Draft, execute, and accept one real journey specification|
|`author-manifest`|Draft and validate a complete application manifest|
|`repair`|Dispatch a bounded repair worker for a recorded failed run|
|`figure-evaluate`|Render and rubric-score a scientific figure pair|
|`seo-evaluate`|Crawl and evaluate a declared SEO contract|
|`readme-gif`|Render a bounded, silent journey video as a looping README GIF|
|`history`|Stability by run, journey, and test|
|`dashboard`|Product/version/journey evidence projection|
|`status`|Journey coverage, freshness against HEAD, and merge eligibility|
|`overview`|Unified app status, repository violations, and Stado fleet health|
|`errors`|Fast all-app failure view without repository violation scans|
|`intake`|Receive failure envelopes from desktop applications|
|`failures`|Counts and newest envelopes in the failure intake store|
|`gate-status`|Gate configuration and activation status for an application|
|`gate-prepush`|Judge the changes being pushed to a repository|
|`gate-install`|Install the repository pre-push gate, preserving an existing hook|
|`gate-evaluate`|Evaluate evidence against a merge or release policy|
|`gate-enforce`|Enforce an activated merge or release policy|
|`gate-activate`|Require a green evaluation and activate its gate|
|`check`|Is the target toolchain ready?|
|`setup`|Install the browser or driver layers Probierz owns|
|`run`|Execute a target, capture its artifacts, and analyze its report|
|`analyze`|Normalize a Playwright, WDIO, or canonical Probierz report|
|`affected`|Select the targets and application journeys affected by changed files|
|`ci`|Run every affected and ready target|
|`matrix`|Plan or execute a declared application matrix|
|`protect`|Encrypt and authenticate one run's evidence artifacts|
|`restore`|Authenticate and restore an encrypted evidence bundle|
|`retention`|Plan or apply application evidence retention|
|`secret-scan`|Find credentials and tokens in an evidence directory|
|`audit`|Query the tamper-evident access audit|
|`compare`|Compare two recorded runs|
|`last-green`|Find the newest passing run|
|`receipt`|Sign exact runs and policy into an evidence receipt|
|`verify-receipt`|Verify a receipt signature, payload hash, and trust anchor|
|`publication`|Emit a verified immutable first-use publication manifest|
|`publish-onboarding`|Emit an Echo-ingestible onboarding proof manifest|
|`stado`|Submit, recover, resume, cancel, or author work on the Stado fleet|

<!-- /generated: cli -->

Run flags are shared by `run`, `setup`, `analyze`, `affected`, `ci` and
`matrix`, and every one of them is printed by that command's `--help`. Two
belong to a single target: `--local` runs `mobile:ios:byk-auth` on this machine
instead of the dedicated host, and `--seed-resend` seeds that journey's login
mailbox and stops. Conditions are given as `KEY=VALUE` arguments (for example
`BASE_URL=...`, `APP_IOS=...`, `PROBIERZ_COLOR_SCHEME=dark`).

## MCP

```bash
probierz-mcp
```

It speaks the same protocol every Wisent surface speaks — `initialize`, `ping`,
`tools/list`, `tools/call` — as newline-delimited JSON-RPC 2.0 on stdio, one
response per request, diagnostics on stderr.

<!-- generated: mcp -->

Every tool `probierz-mcp` advertises over `tools/list`:

|Tool|What it does|
|---|---|
|`probierz_list_surfaces`|List the cross-platform test surfaces (web, electron, mobile, desktop-native): tool, npm script, targets, and relevant env vars.|
|`probierz_list_specs`|Discover e2e/spec files on disk; optional surface narrows to one (web|electron|mobile|desktop-native).|
|`probierz_describe_spec`|Static outline of a spec (describe/it/test titles in file order) by its path under the probierz root. Does not execute anything.|
|`probierz_run_command`|Return the exact shell command to run a target yourself (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win). Read-only: probierz never runs it.|
|`probierz_check`|Preflight a target's toolchain WITHOUT running anything: reports whether it is ready and, for each missing piece, exactly how to fix it -- `probierz setup <target>` for parts probierz owns (Playwright browsers, Appium drivers) or a host install command for the rest (Xcode, Android SDK, simulators, WinAppDriver). Read-only.|
|`probierz_setup`|Install the toolchain parts probierz owns for a target (npm deps + Playwright browsers, or npm deps + the Appium driver). Does NOT install host-level dependencies (Xcode, Android SDK, simulators, WinAppDriver) -- probierz_check reports those. Side-effecting: runs npm / appium driver install.|
|`probierz_run`|EXECUTE a target end-to-end, capture evidence, analyze it, and dispatch a bounded Brama repair worker when it fails. Heavy + side-effecting: needs the target toolchain; noRepair=true records without repair.|
|`probierz_analyze`|Parse a finished run's report (Playwright report.json or the WDIO probierz-<kind>-results.json) and inventory its media: totals, per-test status, failure reasons, and recording metadata (duration/dimensions via ffprobe, optional frame montage via ffmpeg).|
|`probierz_evaluate_figure`|SIDE-EFFECTING: render a scientific reference/candidate pair, run deterministic geometry checks, score the declared visual rubric through the authenticated model router, and write immutable PNG evidence plus a JSON verdict.|
|`probierz_evaluate_seo`|SIDE-EFFECTING: crawl a declared site as ordinary Chrome and Googlebot Smartphone, enforce indexability and structured-data contracts, collect mobile performance evidence, run two independent Brama content graders with conditional adjudication, ingest optional Search Console/CrUX evidence, and write an immutable signed SEO verdict.|
|`probierz_create_readme_gif`|SIDE-EFFECTING: convert one recorded journey video into a bounded, silent, looping README GIF and write a provenance sidecar with source/output SHA-256 and mandatory publication checks. Requires ffmpeg.|
|`probierz_affected`|Given a change, report which run targets it could affect, so you re-run only what is relevant. Deterministic + structural (maps files to targets by package containment; agent/ or repo-root files are cross-cutting -> all targets). Provide `files` explicitly, or omit to diff the working tree against `ref` (default HEAD) via git. Read-only.|
|`probierz_ci`|Change-driven pass: select affected targets, run and analyze them, then dispatch a bounded Brama repair worker for each failure unless noRepair=true. Selection and blockers stay deterministic; only the explicit repair step asks a model what to change.|
|`probierz_history`|Read deterministic E5 stability history: pass rate, infrastructure failures, duration trend, flaky tests, journeys, latest run, and last green.|
|`probierz_dashboard`|Project evidence for product → version → journey → surface → device → result → artifact dashboard navigation.|
|`probierz_matrix_plan`|Read the deterministic nightly or release matrix without executing it.|
|`probierz_run_matrix`|HEAVY + SIDE-EFFECTING: execute every cell of a declared nightly or release matrix and return an E4 verdict.|
|`probierz_protect_run`|SIDE-EFFECTING: encrypt a complete run into an authenticated AES-256-GCM evidence bundle; optionally remove plaintext artifacts.|
|`probierz_restore_bundle`|SIDE-EFFECTING: authenticate and restore an encrypted evidence bundle into an empty directory.|
|`probierz_retention`|Plan retention expiry; with apply=true, delete expired plaintext runs and encrypted bundles.|
|`probierz_secret_scan`|Scan a plaintext artifact directory for high-confidence secrets without returning secret values.|
|`probierz_audit`|Read and integrity-check access audit records, optionally filtered by app, run, or action.|
|`probierz_source_identity`|Compute exact path-independent harness and app source SHA-256 identities.|
|`probierz_gate_status`|Read pull-request and release gate activation state.|
|`probierz_status`|Journey coverage, evidence freshness vs HEAD, untested surfaces, and pull-request merge eligibility for an app.|
|`probierz_gate_prepush`|Pre-push merge gate: select affected journeys from the push diff and evaluate the newest passing runs against the exact current HEAD identity (pull-request policy).|
|`probierz_author_spec`|SIDE-EFFECTING: use the authenticated Stado model router to draft one journey spec from a probe of the real app, verify it with an actual run, and keep it on green (registers the journey in the app manifest).|
|`probierz_repair`|SIDE-EFFECTING: dispatch one bounded Brama worker at a recorded failed run. Product fixes land on a fresh published branch; spec fixes must pass the real journey before publication.|
|`probierz_author_manifest`|SIDE-EFFECTING: use the authenticated Stado model router to draft the whole app journey manifest from a probe and repository layout, validate it, and optionally cover every journey with author-spec.|
|`probierz_stado_run`|SIDE-EFFECTING: run a target on a chosen stado host (provider/pin/spot/GPU); evidence lands back in test-results.|
|`probierz_stado_evaluate_seo`|SIDE-EFFECTING: submit the complete SEO evaluator to a Stado-selected dedicated host, materialize only the declared Brama and signing secrets, and fetch the immutable evidence bundle.|
|`probierz_gate_evaluate`|Evaluate exact build, E3 evidence, coverage, matrix, encryption, secret scan, and signed receipt eligibility; appends an audit record.|
|`probierz_gate_enforce`|Enforce an activated gate against current evidence; pending-green gates fail closed.|
|`probierz_gate_activate`|SIDE-EFFECTING: atomically activate a gate only after all green evidence requirements pass.|
|`probierz_compare_runs`|Deterministically compare status, duration, tests, evidence, build identity, and artifact hashes between two run IDs.|
|`probierz_last_green`|Return the newest passing run for a product, optional target, and optional journey.|
|`probierz_create_receipt`|SIDE-EFFECTING: secret-scan evidence, verify exact source/build/artifact provenance, and sign a release receipt with immutable journey identities and report-typed publication media.|
|`probierz_verify_receipt`|Verify receipt payload hash and Ed25519 signature against an explicit trusted public key or fingerprint.|
|`probierz_create_publication_manifest`|SIDE-EFFECTING: verify a signed receipt, current source, secret scan, evidence hashes, driver capability, redaction review, and immutable storage registrations before emitting a deterministic first-use publication manifest.|
|`probierz_start_run`|HEAVY + SIDE-EFFECTING: start a real run asynchronously and return its runId immediately. Poll with probierz_run_status; cancel with probierz_cancel_run.|
|`probierz_run_status`|Return queued/running/blocked/passed/failed/canceled state for an asynchronous run.|
|`probierz_cancel_run`|Cancel an asynchronous run and terminate its complete spawned process tree.|
|`probierz_get_result`|Return the completed normalized result and evidence for an asynchronous run.|
|`probierz_list_artifacts`|List run-scoped evidence artifacts for a completed asynchronous run.|
|`probierz_get_artifact`|Read one run-scoped artifact up to 5 MiB as base64; path traversal is rejected.|

<!-- /generated: mcp -->

## Recording

`run --record` (or `record=true`) sets `PROBIERZ_RECORD=1`, which the configs
read:

- **Playwright (web):** video + trace + screenshot forced on; a JSON reporter
  writes `report.json`. Conditions via `PROBIERZ_LOCALE` /
  `PROBIERZ_COLOR_SCHEME` and the browser/device projects.
- **Playwright (electron):** trace + screenshot (Playwright video is a
  browser-context feature and does not attach to Electron windows).
- **WDIO (mobile / desktop-native):** per-test Appium screen recording written
  as `<slug>.mp4` under the artifacts dir, plus a `probierz-<kind>-results.json`
  summary. Best-effort: drivers without screen recording (often Mac2 /
  WinAppDriver) degrade silently and never fail the run.
- **Terminal and cua journeys:** a journey declares each screenshot, trace, or
  video it produced, and the runner refuses a path outside the artifacts
  directory or a file that is not there — a report never points at evidence
  that does not exist.

Artifacts land in each package's `test-results/`. `analyze` parses the report,
classifies media (video / screenshot / trace) with sizes, pulls recording
metadata (duration/dimensions via ffprobe), and can extract a frame montage
(ffmpeg). ffprobe/ffmpeg are optional — missing binaries just omit that detail.

## Toolchain

`run` is preflight-gated: before spawning it checks the target's toolchain and,
if something is missing, returns exactly what and how to fix it instead of a
failure buried in npm/Playwright/Appium. `check` runs that preflight on its own.

What probierz **owns and installs itself** (`setup`): npm deps, Playwright
browsers, Appium drivers (`xcuitest` / `uiautomator2` / `mac2`). The Appium
server the WDIO configs auto-start.

What is **host-level and probierz only detects + tells you how to get** (never
installs): Xcode + command-line tools, the Android SDK / `ANDROID_HOME`, iOS
simulators, WinAppDriver, cua-driver and its accessibility grant, physical
devices, and the login mailbox broker named by `BYK_MAILBOX_BROKER`. Driver
detection is a deterministic filesystem check against
`$APPIUM_HOME/node_modules/appium-<name>-driver`.

Typical flow: `probierz check mobile:ios` -> if it names a missing driver, run
`probierz setup mobile:ios`; if it names Xcode/a simulator, install those, then
`probierz run mobile:ios --record APP_IOS=/abs/Byk.app`.

## Operational rules

- Discovery and `check` are read-only. `setup`, `run`, `repair`,
  `figure-evaluate`, `seo-evaluate`, authoring, artifact, and gate operations
  are explicitly side-effecting. `cmd` / `probierz_run_command` still return a
  string to run yourself.
- Keep MCP and CLI stdout clean: only JSON-RPC frames and command output on
  stdout; diagnostics on stderr.
- One module per contract in `probierz-rs/src`, and a command lives in the
  module that owns its contract — not spread across the CLI and the server.
  The failure contract, timestamps, JSON printing and owner-only files are in
  `failure.rs` and are never reimplemented anywhere else.
- The two generated tables above come from the binaries themselves;
  `cargo test --test skill_doc` fails when this document drifts from them, and
  `SKILL_DOC_WRITE=1 cargo test --test skill_doc` regenerates them. Never edit
  between the generated markers by hand.
- probierz installs the parts it owns (browsers, drivers) but never host-level
  dependencies (Xcode, Android SDK, simulators, WinAppDriver, cua-driver) —
  `check` reports those with a one-line install hint.
- Authoring or editing a spec under `test/` is gated by the harness
  device-level-test consent (DEVICE_LEVEL_TESTS_APPROVED set outside the
  session). Running an existing suite via `run` is not spec authoring; it does
  need the real toolchain present (which `check`/`setup` help you reach).
