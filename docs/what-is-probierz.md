# What is Probierz

Probierz is the Wisent quality-evidence toolkit: it runs user journeys against
real product surfaces — web, Electron, mobile, native desktop, a terminal UI —
and turns each run into durable, source-bound, hash-verified evidence that a
release gate can evaluate and explain. The whole product is three moving
parts: application manifests that declare intent, runs that produce bound
evidence, and gates plus signed receipts that decide.

## Manifests declare

Every registered application carries one validated `probierz.yaml` manifest
under `apps/<appId>/`. It names the application's repositories, the surfaces
it is tested on, the journeys each surface covers, file-to-journey mappings
for change selection, artifact retention and redaction policy, optional
nightly/release matrices, and the pull-request and release policies a gate
enforces. The manifest is the only place intended coverage lives; commands
like `probierz status` and the pre-push gate read it, never a hidden default.
The schema is documented in [applications](applications.md).

## Runs produce bound evidence

`probierz run <target>` spawns a real suite (Playwright, WebdriverIO+Appium,
or `cua-driver`) under caller-chosen conditions and writes one run directory
under `test-results/` containing a `run-manifest.json`, the machine-readable
report, captured media, logs, and an analysis. Before anything executes, the
run manifest records the exact harness and application source identity (git
SHA, dirty flag, and a content hash over every tracked file), the exact build
hash, host and device facts, and the redacted run conditions. After the run,
every artifact is SHA-256 hashed into the manifest, and the report itself must
carry the run's own ID to count. Evidence strength is an explicit level
(E0–E4), never an inferred process exit. The full contract is in
[evidence-model](evidence-model.md) and [execution](execution.md).

## Gates and receipts decide

A gate is a deterministic evaluation over recorded runs: exact source and
build identity, minimum evidence level, required targets, journeys, and
matrix cells, artifact hash verification, secret scans, and — for releases —
a signed Ed25519 evidence receipt that must agree with the local evidence
byte-for-byte. The verdict is `passed` plus an exhaustive error list, not a
score. Gates start `pending-green` and fail closed until they are activated
by a fully green evaluation; a pre-push hook applies the same policy to every
push. See [gates-and-receipts](gates-and-receipts.md).

## What Probierz is not

Probierz is not a unit-test framework and does not replace application-level
assertions. Discovery never installs dependencies, starts a driver, executes
a suite, or changes an application repository. Probierz does not call model
vendors directly: authoring and the figure and SEO evaluators use only the
authenticated Stado model router, and no deterministic blocker can be
overridden by a model. It does not install host-level dependencies (Xcode,
Android SDKs, simulators, WinAppDriver), and it is currently a source
distribution — no stable public binary or hosted service is promised.

## The first three commands

```bash
node agent/cli.mjs list      # every test surface + its run script
node agent/cli.mjs apps      # registered products, targets, and journeys
node agent/cli.mjs check web # is the toolchain ready + how to fix what is missing
```

All three are read-only. The end-to-end path is [quick-start](quick-start.md);
the full command surface is [cli](cli.md).
