# Application manifests

An application is registered by one YAML manifest at
`apps/<appId>/probierz.yaml`. The manifest is validated on every load; an
invalid manifest is an error, not a partial registration. `probierz apps`
lists all registered applications, `probierz app <appId>` prints one
validated manifest.

## Required shape

```yaml
schemaVersion: 1
appId: my-app            # must match the directory name
owner: team-or-person
repositories:
  - root: /absolute/path/to/repo
    mappings:
      - paths: ["src/**", "package.json"]   # glob patterns, repo-relative
        journeys: [checkout, onboarding]    # journeys those files affect
surfaces:
  web:
    spec: my-app.e2e.ts        # the spec file the target runs
    journeys: [checkout]       # must name declared journeys
journeys:
  checkout:
    owner: team-or-person
    timeoutMs: 60000
```

- `repositories[]` — every source repository, with absolute `root` paths and
  `mappings` that translate changed files into affected journeys. These
  mappings drive `probierz affected`, `probierz status`, `probierz ci`, and
  the pre-push gate.
- `surfaces` — one entry per run target (`web`, `electron`, `mobile:ios`,
  `mobile:android`, `desktop:mac`, `desktop:cua`, `desktop:win`, `tui`).
  Each declares its `spec` and its `journeys`. Optional keys:
  - `conditions` — default environment for the target; keys with sensitive
    names (token/secret/password/…) are rejected — secrets go through
    `secretRefs`.
  - `env` — a `{TARGET_NAME: SOURCE_NAME}` map that forwards an existing
    variable under the name the suite expects.
  - `journeyOverrides` — `[{when: {VAR: value}, journeys: [...]}]`; the
    first override whose `when` conditions all match the run environment
    replaces the journey list for that run.
- `journeys` — every journey with an `owner` and a positive `timeoutMs`.

## Optional blocks

- `productId` — the stable central product identifier; required when any
  journey carries publication identity.
- `secretRefs` — named secret references; every value must be a `vault://`
  reference. At run time a matching process environment variable is
  forwarded to the suite; the manifest never holds a secret value.
- `data.seed` / `data.cleanup` — `{command, args[]}` hooks executed around a
  run to prepare and clean test data.
- `artifacts.retain.{pullRequestDays,nightlyDays,adhocDays}` — positive
  retention windows per run kind; `artifacts.redact` — the key names whose
  values must be redacted from persisted evidence; `artifacts.pii` — a
  declared PII stance recorded into receipts.
- `matrix.<profile>` — declared condition matrices (typically `nightly` and
  `release`): `targets` (must have surfaces), scalar `dimensions`,
  `minimumCellEvidence` (`E2`/`E3`, default `E3`), `artifactEncryption`
  (`optional`/`required`), `removePlaintextAfterProtection`, `maxCells`
  (default 128), `maximumParallel` (default 4). See
  [evidence-model](evidence-model.md#matrix-evidence-e4).
- `pullRequestPolicy` / `releasePolicy` — what a gate enforces:
  `minimumEvidence` (`E2`/`E3`), `requiredTargets`, `requiredJourneys`,
  `requiredMatrixProfile`, `requireProtectedArtifacts`,
  `requireSecretScan`. See [gates-and-receipts](gates-and-receipts.md).
- `seo` — `policy` and `brief` file paths plus per-profile settings
  (`pull-request`, `release`, `nightly`, `production`), each declaring
  `requireSignature` and `requireProductionEvidence`. See
  [evaluators](evaluators.md#seo-evaluation).

## Journey identity and publication

A journey that publishes first-use evidence (and the `onboarding-first-use`
journey always) must carry an immutable identity: `journeyId`,
`journeyVersion`, a UUID `journeyVersionId`, and `firstSuccessFact`. Its
`publication` block names the stable `screenId`, the allowed
`artifactKinds` (`screenshot`, `recording`, `trace`), `minimumEvidence`
(`E2`/`E3`), and whether `redactionRequired` is true. A journey may claim a
`recording` kind only if at least one of its surfaces runs on a
recording-capable target (`web`, `mobile:ios`, `mobile:android`,
`desktop:mac`, `desktop:cua`, `desktop:win`). `onboarding-first-use`
additionally requires `productId`, all three retention windows, and a
redaction key list including `TOKEN`, `SECRET`, `PASSWORD`, `KEY`, `COOKIE`,
and `AUTH`.

## Gate state

`gates.json`, written beside `probierz.yaml` by `probierz gate-activate`,
holds the per-mode enforcement state
([gates-and-receipts](gates-and-receipts.md#gate-lifecycle-pending-green-activate-enforce)).
It is managed by Probierz, not hand-edited.

## Authoring

`probierz author-manifest` drafts a manifest through the authenticated Stado
model router; the draft must pass this same validation, and journeys are
only accepted with real verifying runs (`probierz author-spec`). A generated
artifact is never trusted merely because a model produced it.
