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

[Quick start](#quick-start) · [Pipeline](docs/PIPELINE.md) ·
[Agent interface](skills/probierz/SKILL.md) ·
[Source and issues](https://github.com/wisent-ai/probierz)

Current proof boundary: source version `0.1.0` provides local execution, evidence,
receipts, and gate evaluation. No stable public binary or hosted service is
currently promised.

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
- a human CLI and a stdio MCP server backed by the same agent modules.

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
- Probierz is not currently a hosted testing service or a supported public
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
| Stable hosted service or public binary | — | — | Not published |
| Scientific figures | ImageMagick, optional pdfLaTeX, vision model through the Stado router | `magick`; `pdflatex` for TeX; router URL, scoped token, model ID | Implemented |

`probierz check <target>` is authoritative for toolchain readiness on the current
host. Readiness is not evidence that a journey passed; only a completed run can
produce that evidence.

## Core use cases

### Discover existing journey coverage without executing anything

- **Actor:** a product engineer or automation agent.
- **Initial state:** a Probierz source checkout and, for product-level coverage,
  a registered application manifest.
- **Outcome:** the actor can list surfaces, applications, specifications,
  journeys, source identity, and exact run commands.
- **Boundary:** discovery and `check` are read-only; they do not install tooling
  or run an application.

### Run one journey and preserve its evidence

- **Actor:** a test engineer with an authorized target.
- **Initial state:** the target-specific preflight passes and the application
  path, URL, bundle, or package identity is explicit.
- **Outcome:** Probierz executes the selected specification, records the report
  and supported media, analyzes the result, and associates it with source and
  application identity.
- **Boundary:** execution may drive a real browser, simulator, device, or desktop
  application; recording support is driver-specific and never upgrades a failed
  run to success.

### Decide whether current source is eligible to merge or release

- **Actor:** a release owner or repository pre-push gate.
- **Initial state:** the application manifest defines required journeys and the
  evidence store contains source-bound runs and receipts.
- **Outcome:** Probierz reports eligibility and exact blocking reasons such as
  missing, stale, failing, or identity-mismatched evidence.
- **Boundary:** evaluate-only inspection is separate from activating or enforcing
  a repository gate.

### Author a missing manifest or specification

- **Actor:** an explicitly authorized engineer or automation workflow.
- **Initial state:** the product and journey are described, target coordinates
  are explicit, and the authenticated Stado model router is configured.
- **Outcome:** Probierz drafts the artifact, validates its structure, exercises
  the accepted specification through the real target path, and keeps only the
  result that satisfies the configured contract.
- **Boundary:** authoring is side-effecting. The router receives a dedicated
  router-scoped bearer; Probierz never receives provider credentials.

### Execute on admitted remote capacity

- **Actor:** a release workflow that cannot use the local host.
- **Initial state:** a Stado host has compatible capacity, toolchain, source, and
  scoped secret references.
- **Outcome:** the remote job executes the same target contract and returns its
  evidence to the configured Probierz object-store path.
- **Boundary:** selecting a host does not grant broader machine or cloud
  authority; an unavailable fleet is reported as unavailable, not as empty or
  successful.
### Evaluate a scientific figure against its intended reference

- **Actor:** a paper author or release workflow reviewing a generated figure.
- **Initial state:** reference and candidate files exist as SVG, TeX, PDF, or a
  supported raster image; ImageMagick is installed; TeX inputs additionally
  require `pdflatex`; the Stado model router URL, scoped token, and a
  vision-capable model ID are configured.
- **Outcome:** Probierz renders both artifacts, records dimensions, content
  bounds, edge margins, aspect-ratio drift, rubric evidence, fidelity losses,
  recommendations, and one pass/block verdict. It writes immutable reference
  and candidate PNGs beside the JSON report.
- **Boundary:** text inside either figure is untrusted evidence. The model cannot
  redefine the rubric, suppress deterministic blockers, or contact a provider
  directly. Existing evidence files are never overwritten.

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

## Quick start

No stable public binary exists. The safe source path below performs discovery
only: it does not start a browser, install Appium drivers, drive an application,
or create run evidence.

### Prerequisites

- Git;
- Node.js 22 or newer;
- npm;
- a source checkout of the public repository.

```bash
git clone https://github.com/wisent-ai/probierz.git
cd probierz
npm install
node agent/cli.mjs list
node agent/cli.mjs apps
```

Expected result: `list` returns the web, Electron, mobile, and native-desktop
surfaces with their targets and environment requirements. `apps` returns the
validated application manifests currently registered in the checkout. Neither
command executes a test target.

### Evaluate a figure

```bash
STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_FIGURE_VISION_MODEL='<vision-model-id>' \
node agent/cli.mjs figure-evaluate \
  --reference /absolute/path/intermediate.svg \
  --candidate /absolute/path/final.tex \
  --out test-results/figure-evaluations/paper-figure.json
```

`--reference` and `--candidate` accept SVG, TeX, PDF, PNG, JPEG, or WebP.
`--rubric <json>` replaces the built-in scientific-figure rubric; its positive
weights must total 1 and every score threshold must be between 0 and 1.
`--model` overrides `PROBIERZ_FIGURE_VISION_MODEL`. Exit status is 0 only when
there are no deterministic, model, dimension-threshold, or overall-threshold
blockers. The JSON report records both input and render SHA-256 identities,
model usage, dimension evidence, the weighted score, and the complete blocker
list; two PNG renders are written beside it.

Process integrations may provide the router base with `--router-url` and send
the scoped bearer over standard input with `--router-token-stdin`. This avoids
placing a short-lived credential in `argv` or a child-process environment;
interactive use may continue to use the documented environment variables.

A candidate that does not render is a blocking verdict, not a tool failure: the
report carries a `candidate_render_failed` blocker whose evidence is the
renderer's own error, so a caller can correct the artifact and re-submit. A
reference that does not render is an input error and fails the command.
`--tex-preamble <file>` adds the manuscript's own libraries, colours, and macros
to the standalone wrapper used for TeX input; the file must contain preamble
lines only, with no document class or document body.


### Evaluate SEO

`seo-evaluate` is a release evaluator, not a Lighthouse score wrapper. It reads
the manifest-declared brief and SEO policy, crawls every declared and
sitemap-discovered URL as ordinary Chrome and Googlebot Smartphone, evaluates
robots directives, redirects, canonicals, indexability, metadata, hreflang,
internal-link reachability, duplicate content, JSON-LD, social image responses,
and a throttled mobile lab profile for LCP, CLS, TBT, failed resources, and
runtime errors. Production evidence adds CrUX p75 INP.

```bash
STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_MODEL_AGENT_ID=probierz \
PROBIERZ_MODEL_AGENT_SECRET='<agent-secret>' \
PROBIERZ_SEO_PRIMARY_MODEL='<pinned-model-a>' \
PROBIERZ_SEO_SECONDARY_MODEL='<pinned-model-b>' \
PROBIERZ_SEO_ADJUDICATOR_MODEL='<pinned-model-c>' \
PROBIERZ_RECEIPT_PRIVATE_KEY_FILE=/absolute/path/seo-ed25519.pem \
node agent/cli.mjs seo-evaluate \
  --app landing-page \
  --base-url https://product.example.com \
  --mode release
```

The two graders run independently at temperature zero. Probierz takes the
stricter score when they agree closely and invokes the pinned adjudicator only
when a dimension differs by more than the policy threshold or their blocker
sets differ. Models may score search intent, factuality, information gain, and
snippet quality; they cannot override crawl, indexability, structured-data, or
performance facts.

The report separates `searchEligibility`, weighted `searchQuality`, and
`productionOutcome`. A release passes only with no hard or model-confirmed
blockers, every dimension at or above its minimum, overall quality at or above
`0.85`, and an Ed25519 signature. The `pull-request`, `release`, `nightly`, and
`production` profiles live in `apps/landing-page/probierz.yaml`; each profile
declares whether signed evidence and production observations are mandatory.
`production` consumes the versioned Search Console and CrUX shape shown in
`apps/landing-page/production-evidence.example.json`; its evidence must identify
the `google-search-console+crux` source, be fresh, and observe every declared
indexable URL.

Run the same evaluator on a dedicated Stado-selected host without putting any
secret in `argv`:

```bash
node agent/cli.mjs stado seo landing-page \
  --base-url https://product.example.com \
  --mode release \
  --primary-model '<pinned-model-a>' \
  --secondary-model '<pinned-model-b>' \
  --adjudicator-model '<pinned-model-c>' \
  --host stado:mini
```

Stado materializes only the manifest-declared Brama bearer, agent-auth secret,
and, when the profile requires it, SEO receipt key. The private checkout and
resulting evidence bundle move through `stado://probierz/inputs` and
`stado://probierz/results`; the report, source and rendered HTML, robots and
sitemap bodies, screenshots, mobile performance facts, exact model identities,
request and rubric hashes, source hashes, blocker list, and receipt-compatible
signature land under `test-results/seo/`. `probierz verify-receipt <report>`
checks the same canonical Ed25519 signing contract used by other Probierz
receipts.

### First evidence-producing run

Choose an application and target returned by discovery, then check the exact
host before running it:

```bash
node agent/cli.mjs check TARGET
node agent/cli.mjs run TARGET --app APP_ID --record
```

`check` either reports readiness or names the missing prerequisite and its owner.
A successful `run` returns the run result and analysis and writes target-specific
artifacts under `test-results/`. It may drive a real application and therefore is
not a read-only continuation of the discovery path. Additional command and
failure guidance is in the [Probierz agent interface](skills/probierz/SKILL.md).
The integrated Tama → Probierz → Stado workflow is documented in
[`docs/PIPELINE.md`](docs/PIPELINE.md).

### Register verified first-use evidence

An `onboarding-first-use` journey carries an immutable `journeyId`,
`journeyVersion`, UUID `journeyVersionId`, and `firstSuccessFact`. Its
`publication` policy names the stable `screenId`, allowed artifact kinds
(`screenshot`, `recording`, or `trace`), minimum evidence level, and whether
verified redaction is mandatory. The application manifest also supplies the
central `productId`.

The same manifest makes retention and redaction explicit with positive
`artifacts.retain.pullRequestDays`, `nightlyDays`, and `adhocDays`, plus a
non-empty `artifacts.redact` key list.


After a release receipt is signed, provide an asset-registration JSON array:

```json
[
  {
    "file": "media/first-use.webm",
    "kind": "recording",
    "storageUrl": "<immutable HTTPS object URL without credentials, query, or fragment>",
    "contentSha256": "64-lowercase-hex-characters",
    "redactionStatus": "verified_redacted",
    "verifiedAt": "2026-08-04T12:00:00.000Z"
  }
]
```

```bash
node agent/cli.mjs publication RECEIPT_JSON ATTEMPT_ID JOURNEY_ID \
  --assets ASSET_REGISTRATIONS_JSON \
  --public-key TRUSTED_PROBIERZ_PUBLIC_KEY
```

Probierz emits one immutable
`probierz-first-use-publication` JSON manifest under
`test-results/publications/`. The manifest is release-, source-, journey-,
attempt-, screen-, content-, and signed-receipt-bound. It contains only
`publishable: true` records: an invalid or untrusted receipt, stale source,
missing provenance, mismatched content hash, plaintext-secret finding,
unverified redaction, unsupported recording claim, or credential-bearing
storage URL rejects publication instead of producing a downgraded manifest.
Canonical machine consumers should use `manifestId`, `artifactId`, and the
embedded receipt verification identity rather than deriving identity from file
names.

`artifactId` is the SHA-256 of the recursively key-sorted canonical asset
without `artifactId`; `manifestId` uses the same rule over the full manifest
without `manifestId`. `receiptId` is the first 24 hexadecimal characters of
SHA-256 over the canonical signed payload, a newline, and the base64 Ed25519
signature.


### Publish a README journey GIF

Probierz owns animated product evidence. Select one recorded journey video from
`test-results/`, trim it to the shortest complete outcome, and export it:

```bash
node agent/cli.mjs readme-gif test-results/APP_ID/RUN_ID/path/to/video.webm \
  --out /path/to/product/assets/demo.gif \
  --start 0 \
  --duration 12 \
  --fps 12 \
  --width 960
```

The command writes the silent, looping GIF and a sibling
`demo.gif.probierz.json` provenance file containing source/output SHA-256 and
the exact render settings. Duration, frame rate, and width are bounded to keep
repository media reviewable. The sidecar deliberately marks the GIF as
`reviewRequired`: conversion does not prove that the clip is free of
credentials, personal data, production identifiers, or sensitive URLs.
`PROBIERZ_FFMPEG_BIN` may select an explicit `ffmpeg` executable. Probierz does
not create static product banners; those belong to `wisent-asset-generator`.

## Primary interfaces

- **Human CLI:** `probierz` is canonical for discovery, setup, execution,
  analysis, figure and SEO evaluation, authoring, evidence, gate, retention,
  security, and Stado workflows.
- **Machine CLI output:** status, overview, run, analysis, figure evaluation,
  SEO evaluation, and gate commands expose structured data; automation must not
  infer state from prose.
- **MCP:** `probierz-mcp` exposes the same discovery and explicitly named
  side-effecting operations over stdio JSON-RPC. Tool descriptions preserve the
  read-only versus mutation boundary; `probierz_evaluate_figure` and
  `probierz_evaluate_seo` use the same evaluators and evidence contracts as the
  CLI.
- **Repository gate:** `probierz gate-install` installs the pre-push integration;
  gate evaluation and enforcement remain distinct commands.
- **Stado bridge:** `probierz stado run`, `probierz stado author`, and
  `probierz stado seo` submit exact remote contracts and return evidence through
  the configured object store.

The complete command surface is printed by `probierz --help` and summarized in
[`skills/probierz/SKILL.md`](skills/probierz/SKILL.md).

## Operational model

- **Configuration:** application manifests define repositories, targets,
  journeys, paths, and ownership. Environment variables provide explicit
  target coordinates, not hidden application defaults.
- **State:** run reports, histories, receipts, protected bundles, audit records,
  and returned remote evidence live under the configured `test-results/` and
  object-store paths. An unavailable store is an error, not an empty history.
- **Credentials:** local discovery requires none. Model authoring and figure
  evaluation require a distinct `STADO_MODEL_ROUTER_URL` and router-scoped
  token; figure evaluation also requires `PROBIERZ_FIGURE_VISION_MODEL` or
  `--model`. Remote Stado jobs materialize the token from the scoped
  `probierz-model-router` secret reference instead of embedding it in the job
  payload.
- **Setup ownership:** `probierz setup` may install npm dependencies, Playwright
  browsers, and Appium drivers owned by Probierz. Host SDKs, simulators, devices,
  permissions, and application runtimes remain operator-managed.
- **Observability:** status, overview, dashboard projection, history, audit, and
  explicit failure objects distinguish failed work from unavailable
  dependencies and blocked prerequisites.
- **Failure recovery:** preflight prevents known-unready runs; retention and
  protected bundles preserve selected evidence; receipts can be verified before
  use; remote failures retain their classified failure point and retryability.
- **Upgrades:** the repository is currently a source distribution. `package.json`
  owns the source version and Node engine contract; no mutable installation is
  presented as a stable release channel.

## Project status and support

- **Maturity:** public development source, version `0.1.0`.
- **Current support:** local execution, evidence contracts, receipts, and gate
  evaluation are available from source. Host and remote target availability
  remains environment-specific.
- **Public distribution:** no stable hosted service or supported public binary
  release is currently promised.
- **Source and defects:** [`wisent-ai/probierz`](https://github.com/wisent-ai/probierz).
- **Security reports:** use the private
  [GitHub Security Advisory](https://github.com/wisent-ai/probierz/security/advisories/new);
  never include credentials or private artifacts in a public issue.
- **License:** Apache License 2.0; see [`LICENSE`](LICENSE).

This README owns the product promise, boundaries, use cases, interface roles, and
support status. Executable behavior remains authoritative in the CLI and agent
modules; downstream documentation must not advertise a broader capability than
the installed source exposes.

