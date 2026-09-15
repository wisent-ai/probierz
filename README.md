<!-- wisent-banner:start -->
<p align="center">
  <img src="assets/readme-banner.webp" alt="probierz by Wisent" width="100%">
</p>
<!-- wisent-banner:end -->

<!-- wisent-readme-signals:start -->
[![Source](https://img.shields.io/badge/GitHub-Source-181717?logo=github)](https://github.com/wisent-ai/probierz) [![Issues](https://img.shields.io/badge/GitHub-Issues-181717?logo=github)](https://github.com/wisent-ai/probierz/issues) [![Wisent](https://img.shields.io/badge/Wisent-Website-0B0B0B)](https://wisent.com) [![Discord](https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white)](https://discord.gg/qRjpkthq54) [![LinkedIn](https://img.shields.io/badge/LinkedIn-Follow-0A66C2?logo=linkedin&logoColor=white)](https://www.linkedin.com/company/wisent-ai/) [![X](https://img.shields.io/badge/X-Follow-000000?logo=x&logoColor=white)](https://x.com/wisentai) [![Enterprise](https://img.shields.io/badge/Enterprise-Book%20a%20call-0B0B0B?logo=calendly)](https://calendly.com/lbartoszcze)
<!-- wisent-readme-signals:end -->

# Probierz: AI QA That Makes Sure You Never Ship Anything Broken

The Best Way to Improve Your AI-Generated Code Is to Have an AI Test It.

Probierz gives you the proof your software works as your AI intended. On every
commit it autonomously creates the journeys of your users and tests them directly
where your product lives. Be it the terminal, the browser, a desktop or mobile
app — Probierz tests it all. Every run gives you the evidence you need — reports,
screenshots and videos so that you can see exactly what is broken in the pipeline.

AI Agent That Tests All of Your Releases. Because the missing piece of vibe
coding is Vibe QA, Vibe Testing and Vibe Assurance.

[Quick start](#quick-start) · [Pipeline](https://probierz.wisent.com/docs/PIPELINE) ·
[Agent interface](https://probierz.wisent.com/docs/mcp) ·
[Source and issues](https://github.com/wisent-ai/probierz)

Current proof boundary: source version `0.1.0` provides the Rust CLI and MCP
binaries, local execution, evidence, receipts, and gate evaluation. No hosted
service or prebuilt binary release is currently promised.

## Problem and intended users

A release decision usually depends on test definitions, target-specific tooling,
screenshots, traces, video, source identity, run history, and policy. When those
pieces live in unrelated scripts and CI logs, teams cannot tell which user
journeys were exercised, whether evidence belongs to the current source, or why
a release was allowed.

Probierz serves three audiences:

- **Product and test engineers** define application journeys once and run them
  across the supported browser, mobile, Electron, and native desktop surfaces.
- **Release owners** inspect freshness, receipts, regressions, and explicit gate
  reasons instead of treating a green process exit as sufficient evidence.
- **Automation and AI agents** discover coverage through stable CLI and MCP
  contracts without receiving implicit permission to install tools, execute a
  target, author a specification, or mutate a repository.

Probierz is preferable to disconnected test scripts when the required outcome is
a source-bound chain from intended journey, through execution and artifacts, to
one explainable release decision.

## Product boundaries

### Included

- deterministic discovery of supported surfaces, registered applications,
  specifications, journey outlines, and exact run commands;
- preflight checks that distinguish missing Probierz-owned tooling from
  host-level prerequisites;
- Playwright execution for web and Electron applications;
- WebdriverIO and Appium execution for iOS, Android, native macOS through Mac2,
  and native Windows applications;
- `cua-driver` execution for native macOS applications when Accessibility-based
  automation and screenshot evidence are sufficient and full Xcode is absent;
- optional video, trace, screenshot, report, and frame metadata capture where
  the selected driver supports it, bounded README GIF publication from one
  selected journey recording, and rubric-scored scientific figure comparison;
- application manifests, journey coverage, source identity, run history,
  comparisons, last-green selection, and evidence dashboards;
- signed evidence receipts, receipt verification, retention, protected bundles,
  secret scanning, audit history, and pull-request or release gates;
- affected-target selection and change-driven orchestration;
- remote execution and authoring through explicitly selected Stado capacity;
- specification and manifest authoring through the authenticated Stado model
  router, followed by deterministic validation and an accepted real run;
- scientific figure evaluation from SVG, TeX, PDF, or raster inputs, combining
  deterministic render geometry with an evidence-grounded vision verdict routed
  through the authenticated Stado model router;
- a human CLI and a stdio MCP server backed by the same Rust product core.

### Explicit non-goals

- Probierz is not a unit-test framework and does not replace application-level
  assertions, fixtures, or accessibility identifiers.
- Discovery never installs dependencies, starts a driver, executes a suite, or
  changes an application repository.
- A generated specification is not trusted merely because a model produced it;
  acceptance requires the configured validation and execution path.
- Probierz does not infer application release approval from screenshots, prose,
  figure verdicts, or an unverified process exit. Application gate inputs must
  satisfy the run-evidence contract.
- Probierz does not provide provider credentials or call model vendors directly.
  Authoring and figure evaluation use only the authenticated Stado model router.
- Probierz does not install Xcode, Android SDKs, simulators, physical-device
  support, WinAppDriver, operating-system permissions, or application runtimes.
- Probierz does not make Playwright video available for Electron or promise
  screen recording from drivers that do not expose it.
- Probierz is not currently a hosted testing service or a supported prebuilt
  binary distribution.

### Supported environments and current capability

| Surface | Execution tool | Required environment | Current state |
|---|---|---|---|
| Web | Playwright: Chromium, Firefox, WebKit, emulated mobile | Node.js 22 or newer; installed browser | Implemented |
| Electron | Playwright `_electron` | Node.js 22 or newer; application entry point | Implemented |
| Mobile iOS | WebdriverIO, Appium, XCUITest | macOS, Xcode, simulator or authorized device | Implemented when host prerequisites are available |
| Mobile Android | WebdriverIO, Appium, UiAutomator2 | Android SDK, emulator or authorized device | Implemented when host prerequisites are available |
| Native macOS (Mac2) | WebdriverIO, Appium Mac2 | macOS, full Xcode, target, and required Accessibility permission | Implemented when host prerequisites are available |
| Native macOS (CUA) | `cua-driver` | macOS target and CuaDriver Accessibility permission | Implemented |
| Native Windows | WebdriverIO, WinAppDriver | Windows target, Developer Mode, WinAppDriver | Implemented when host prerequisites are available |
| Remote execution | Stado bridge | admitted host, capacity, object store, target toolchain | Implemented; availability depends on the selected host |
| Prebuilt public binary or hosted service | — | — | Not published; build the Rust binaries from source |
| Scientific figures | ImageMagick, optional pdfLaTeX, vision model through the Stado router | `magick`; `pdflatex` for TeX; router URL, scoped token, model ID | Implemented |

`probierz check <target>` is authoritative for toolchain readiness on the current
host. Readiness is not evidence that a journey passed; only a completed run can
produce that evidence.

## How Probierz works

```text
application manifest + required journeys + exact source identity
                              │
                              ▼
                 deterministic discovery / affected
                              │
                              ▼
                  target-specific preflight check
                              │
                    ┌─────────┴─────────┐
                    │                   │
               local runner        Stado runner
                    │                   │
                    └─────────┬─────────┘
                              ▼
            report + screenshots + traces + video metadata
                              │
                              ▼
        analysis + history + comparison + signed evidence receipt
                              │
                              ▼
               status projection and explicit gate verdict
```

Application manifests define intended journeys and target coordinates. Runner
modules own execution, analyzers own report and media interpretation, and the
evidence store owns durable run records and receipts. The dashboard and MCP
surface are projections over those contracts; they are not alternate sources of
truth.

Authoring is one layer above deterministic execution: the authenticated model
router may propose a manifest or specification, but Probierz validates and
exercises the accepted artifact before it can contribute evidence. Stado owns
remote capacity and secret materialization; Probierz owns the quality contract
and returned evidence.

## Core use cases

Probierz serves six outcomes: discovering what is covered without
running anything, running one journey and preserving its evidence,
deciding whether the current source may merge or release, authoring a
missing manifest or specification, executing on admitted remote
capacity, and evaluating a scientific figure against its reference.
Each one's actor, starting state, result and boundary is in
[`reference/use-cases.md`](reference/use-cases.md).

## Quick start

Build the product binaries from source and look around — no browser, no
drivers, no evidence:

```bash
git clone https://github.com/wisent-ai/probierz.git
cd probierz/probierz-rs
cargo build --release
export PATH="$PWD/target/release:$PATH"
probierz list
probierz apps
```

`list` returns the web, Electron, mobile and native-desktop surfaces
with their targets and environment requirements; `apps` returns the
validated application manifests in the checkout. Neither executes a
test target.

From there:

| guide | what it covers |
|---|---|
| [`guides/getting-started.md`](guides/getting-started.md) | prerequisites, discovery, adopting journey definitions from another repository |
| [`guides/evidence.md`](guides/evidence.md) | the first evidence-producing run, publishing verified first-use assets, exporting a README GIF |
| [`guides/evaluators.md`](guides/evaluators.md) | the scientific-figure and SEO evaluators |
| [`guides/remote-stado.md`](guides/remote-stado.md) | submitting, resuming and cancelling work on a Stado host |

Command and failure guidance for agents is in
the published [agent interface](https://probierz.wisent.com/docs/mcp); the integrated
Tama → Probierz → Stado workflow is in
the published [pipeline documentation](https://probierz.wisent.com/docs/PIPELINE).

## Primary interfaces

The `probierz` CLI is canonical, and every command that produces
machine output says so in structured form rather than prose. The
`probierz-mcp` binary exposes the same discovery and the same
explicitly named side-effecting operations over stdio JSON-RPC, and
`probierz gate-install` installs the pre-push integration. The Stado
bridge submits exact remote contracts and returns their evidence.
Each interface, with what it is allowed to do, is in
[`reference/interfaces.md`](reference/interfaces.md).

The complete command surface is printed by `probierz --help` and
summarized in the published [agent interface](https://probierz.wisent.com/docs/mcp).

## Operational model

Application manifests define repositories, targets, journeys, paths and
ownership; run reports, receipts and returned remote evidence live
under the configured `test-results/` and object-store paths; local
discovery needs no credential, and model work reaches Brama only
through the authenticated Stado router. The whole model — including
what `probierz setup` owns, how failures are distinguished from
unavailable dependencies, and the bounded repair worker a failed run
dispatches — is in
[`reference/operations.md`](reference/operations.md).

## Project status and support

- **Maturity:** public development source, version `0.1.0`.
- **Current support:** local execution, evidence contracts, receipts, and gate
  evaluation are available from source. Host and remote target availability
  remains environment-specific.
- **Public distribution:** source-built Rust binaries are supported; no stable
  hosted service or prebuilt public binary release is currently promised.
- **Source and defects:** [`wisent-ai/probierz`](https://github.com/wisent-ai/probierz).
- **Security reports:** use the private
  [GitHub Security Advisory](https://github.com/wisent-ai/probierz/security/advisories/new);
  never include credentials or private artifacts in a public issue.
- **License:** Apache License 2.0; see [`LICENSE`](LICENSE).

This README owns the product promise, boundaries, use cases, interface roles, and
support status. Executable behavior remains authoritative in `probierz --help`
and each subcommand's `--help`; downstream documentation must not advertise a
broader capability than the installed binary exposes.

