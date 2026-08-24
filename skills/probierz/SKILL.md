---
name: probierz
description: Use Probierz to discover, execute, and analyze Wisent quality evidence across web, Electron, mobile, native desktop, scientific figures, and SEO releases. Its CLI and stdio MCP server expose read-only discovery, preflight, target execution with report/media capture, figure comparison, and a complete SEO evaluator that enforces crawl/index contracts, scores content through independent Brama graders, ingests production observations, and signs immutable evidence. Use it to inspect existing journeys, run an authorized target, evaluate a candidate scientific figure, or produce a release SEO verdict.
---

# probierz

Probierz is the Wisent cross-platform quality-evidence toolkit. Its monorepo drives
web, Electron, mobile, and native-desktop end-to-end tests. Each surface is
env-var driven, with no hardcoded targets.

## Two layers

- **Discovery (read-only):** list surfaces, discover spec files on disk, outline
  a spec's describe/it titles statically, and print the exact run command. No
  side effects.
- **Execution + analysis:** actually run a target under chosen conditions,
  record video/trace/screenshots, and analyze the result. A live run needs
  Chromium, Appium, or an iOS/Android/desktop target and is a heavy
  side-effecting action - only the `run` tool reaches it; discovery never does.

## Surfaces

| Surface | Tool | Package |
| --- | --- | --- |
| `web` | Playwright (Chromium / Firefox / WebKit + emulated mobile) | `packages/web` |
| `electron` | Playwright (`_electron`) | `packages/electron` |
| `mobile` | WebdriverIO + Appium (XCUITest / UiAutomator2) | `packages/mobile` |
| `desktop-native` | WebdriverIO + Appium (Mac2 / WinAppDriver) | `packages/desktop-native` |

Single sources of truth, imported by both the CLI and the MCP server:
`agent/lib.mjs` (discovery - surfaces, specs, run-command strings),
`agent/runner.mjs` (execution - spawns a suite), `agent/analyze.mjs` (analysis -
parses the report + inventories media), `agent/figure-evaluate.mjs` (scientific
figure rendering, deterministic geometry, rubric scoring, and immutable
evidence), `agent/seo-evaluate.mjs` plus `agent/seo-{policy,crawl,model,verdict}.mjs`
(SEO contract, evidence, grading, and signed verdict), `agent/preflight.mjs`
(toolchain readiness + self-provisioning), `agent/affected.mjs` (change ->
affected-target selection), and `agent/orchestrate.mjs` (the change-driven `ci`
composition).

`ci` is the composition the others build up to: `affected` picks the targets a
change touches, each runs preflight-gated (`check`/`run`), `analyze` reads what
ran, and it returns one verdict. Deciding WHICH targets is structural and lives
here; deciding whether a failure is real or what to change is an LLM's job
(Brama), one layer up. Probierz stays deterministic except for its explicit,
bounded model surfaces: automatic repair plus the rubric-bound figure and SEO
evaluators. None may reinterpret a deterministic blocker.

## CLI

```bash
# discovery (read-only)
probierz list                 # the four surfaces + tool, npm script, targets, env
probierz specs [surface]      # e2e/spec files discovered on disk (optional filter)
probierz describe <spec>      # static outline (describe/it titles) of a spec file
probierz cmd <target>         # the exact shell command to run a target yourself

# toolchain
probierz check <target>       # is the toolchain ready? what is missing + how to fix
probierz setup <target>       # install the parts probierz owns (browsers / appium drivers)

# selection + execution + analysis
probierz affected [ref]       # which targets a change touched (git diff vs ref, or --files a b c)
probierz run <target> [opts]  # execute, capture, analyze, auto-repair failures through Brama
probierz repair <appId> [--run id] [--rounds N] [--dry-run]  # repair recorded failure
probierz analyze <report> [dir] [--tool playwright|wdio] [--frames N]
probierz ci [ref] [opts]      # change-driven: affected -> run the ready ones -> analyze -> verdict
probierz figure-evaluate --reference <file> --candidate <file> [--rubric json] [--model id] [--out report.json]
probierz seo-evaluate --app <id> --base-url <url> --mode <profile> [--policy json] [--brief json] [--production-evidence json]
probierz stado seo <appId> --base-url <url> --primary-model <id> --secondary-model <id> --adjudicator-model <id> [--host stado:mini]
```

Targets: `web`, `electron`, `mobile:ios`, `mobile:android`, `desktop:mac`,
`desktop:win`.

`run` options: `--record` (force video+trace+screenshot on), `--force` (skip the
preflight gate and spawn anyway), `--no-repair` (record the failure without
dispatching Brama), `--spec <path>` (run only one spec, e.g. a single app's
suite instead of every spec in the package), `--frames N` (extract N frames per
recorded video, needs ffmpeg), `--timeout MS`, `--no-analyze`, and any
`KEY=VALUE` condition env (e.g. `BASE_URL=...`, `APP_IOS=...`,
`PROBIERZ_LOCALE=...`, `PROBIERZ_COLOR_SCHEME=dark`).

## MCP

Run the stdio server with:

```bash
probierz-mcp
# or
node agent/mcp.mjs
```

It speaks the same protocol every Wisent surface speaks - `initialize`, `ping`,
`tools/list`, `tools/call` - as newline-delimited JSON-RPC 2.0 on stdio, one
response per request, diagnostics on stderr. Tools:

- `probierz_list_surfaces` - the four surfaces with tool, script, targets, env.
- `probierz_list_specs` - spec files on disk; optional `surface` filter.
- `probierz_describe_spec` - static describe/it/test outline of a spec by path.
- `probierz_run_command` - the exact command string for a target (never run).
- `probierz_check` - preflight a target's toolchain without running anything:
  ready? what is missing + exactly how to fix each piece. Read-only.
- `probierz_setup` - install the parts probierz owns for a target (npm deps +
  Playwright browsers or the Appium driver). Side-effecting.
- `probierz_run` - EXECUTE a target end-to-end under chosen conditions, record
  when `record=true`, and return the run result plus an analysis. Preflight-
  gated: if the toolchain is not ready it returns `{skipped:true, preflight}`
  and does not spawn (pass `force:true` to override). Args: `target` (required),
  `record`, `env` (condition vars), `timeoutMs`, `frames`, `analyze`, `force`, `spec`.
- `probierz_analyze` - parse a finished run's report + inventory its media. Args:
  `reportPath` (required), `artifactsDir`, `tool`, `frames`.
- `probierz_evaluate_figure` - render SVG, TeX, PDF, or raster reference and
  candidate artifacts; record dimensions, content bounds, edge margins, input
  and render identities; obtain a structured rubric verdict through the
  authenticated model router; write immutable PNG and JSON evidence. Requires
  `magick`, plus `pdflatex` for TeX input, `STADO_MODEL_ROUTER_URL`,
  `STADO_MODEL_ROUTER_TOKEN`, and a vision model via `model` or
  `PROBIERZ_FIGURE_VISION_MODEL`.
- `probierz_affected` - which targets a change could affect, so you re-run only
  what is relevant. Deterministic + structural (file -> target by package
  containment; agent/ or repo-root files are cross-cutting -> all). Args: `files`
  (explicit paths) or `ref` (git diff the working tree against it, default HEAD).
- `probierz_ci` - change-driven pass: select affected targets, run the ready
  ones (blocked ones reported with their fix, not spawned), analyze, and return
  `{summary:{passed,failed,blocked,ran}, results}`. Composes affected + run +
  analyze. Args: `files` or `ref`, plus `record`, `force`, `frames`, `timeoutMs`.

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

Artifacts land in each package's `test-results/`. `analyze` parses the report,
classifies media (video / screenshot / trace) with sizes, pulls recording
metadata (duration/dimensions via ffprobe), and can extract a frame montage
(ffmpeg). ffprobe/ffmpeg are optional - missing binaries just omit that detail.

## Toolchain

`run` is preflight-gated: before spawning it checks the target's toolchain and,
if something is missing, returns exactly what and how to fix it instead of a
failure buried in npm/Playwright/Appium. `check` runs that preflight on its own.

What probierz **owns and installs itself** (`setup`): npm deps, Playwright
browsers, Appium drivers (`xcuitest` / `uiautomator2` / `mac2`). The Appium
server the WDIO configs auto-start.

What is **host-level and probierz only detects + tells you how to get** (never
installs): Xcode + command-line tools, the Android SDK / `ANDROID_HOME`, iOS
simulators, WinAppDriver, physical devices. Driver detection is a deterministic
filesystem check against `$APPIUM_HOME/node_modules/appium-<name>-driver`.

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
- `agent/lib.mjs` (discovery), `agent/runner.mjs` (execution),
  `agent/repair.mjs` (bounded Brama diagnosis, worktree patches, and spec
  verification), `agent/figure-evaluate.mjs` (figure evaluation),
  `agent/seo-evaluate.mjs` (SEO orchestration), and `agent/preflight.mjs`
  (toolchain) are the single sources of truth for their contracts; add one
  there, not scattered across the CLI/server.
- probierz installs the parts it owns (browsers, drivers) but never host-level
  dependencies (Xcode, Android SDK, simulators, WinAppDriver) - `check` reports
  those with a one-line install hint.
- Authoring or editing a spec under `test/` is gated by the harness
  device-level-test consent (DEVICE_LEVEL_TESTS_APPROVED set outside the
  session). Running an existing suite via `run` is not spec authoring; it does
  need the real toolchain present (which `check`/`setup` help you reach).
