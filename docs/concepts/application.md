# Application

How does Probierz know what a product is supposed to prove? Not from its test
files — from one declared manifest per product. An application is a
registration: `apps/<appId>/probierz.yaml`, validated in full on every load.

## What it is

A YAML document (schema version 1) naming the product's repositories, the
surfaces it is tested on, the journeys each surface covers, the file→journey
mappings that drive change selection, and the policies gates enforce. The
manifest is the only place intended coverage lives; `status`, `affected`,
`ci`, the pre-push gate, and receipts all read it, never a hidden default.

## Fields

Required (`agent/apps.mjs` `validateManifest`):

| Field | Rule |
|---|---|
| `schemaVersion` | must be `1` |
| `appId` | must match the directory name; `loadAppManifest` refuses `app manifest ID mismatch: expected <a>, got <b>` |
| `owner` | non-empty string |
| `repositories[]` | at least one; each `root` must be an absolute path, each entry must carry `mappings` |
| `surfaces` | one entry per run target; each needs `spec` and a non-empty `journeys` list naming declared journeys |
| `journeys` | every journey needs an `owner` and positive `timeoutMs` |

Optional blocks: `productId`, `secretRefs` (every value must start
`vault://`), `data.seed`/`data.cleanup` hooks, `artifacts.retain.*` (positive
day counts), `artifacts.redact`, `artifacts.pii`, `matrix.<profile>`,
`pullRequestPolicy` / `releasePolicy`, `seo`. Per-surface options:
`conditions` (no sensitive key names — those must go through `secretRefs`),
`env` renames, `journeyOverrides` (`when` must be non-empty, scalar,
non-sensitive). The complete schema walk-through with an example manifest is
in [applications](../applications.md).

## Lifecycle

1. **Registered** the moment a valid `apps/<appId>/probierz.yaml` exists.
   There is no separate register command; `probierz author-manifest` can
   draft one, but the file is the registration.
2. **Loaded and validated on every read.** Validation failures are hard
   errors, not partial registrations. `probierz app <appId>` prints the
   validated document plus its `file` path.
3. **Extended** by `gates.json` written beside it by `gate-activate` —
   managed state, not hand-edited (see [gate](gate.md)).
4. **Deregistered** by deleting the directory. Runs already recorded under
   `test-results/<appId>/` remain readable by ID.

## Invariants

- One invalid registered manifest fails every registry-wide command:
  `listApps()` validates all manifests, so `apps`, `status`, `overview`,
  `affected`, `ci`, and `gate-prepush` refuse until it is fixed. Observed on
  this checkout — see [limitations](../limitations.md).
- App IDs match `/^[a-z0-9][a-z0-9._-]*$/i`; anything else is
  `invalid app ID: <value>`.
- A journey that publishes (or is named `onboarding-first-use`) must carry
  immutable identity: `journeyId`, `journeyVersion`, UUID
  `journeyVersionId`, `firstSuccessFact` — see [journey](journey.md).
- Secrets never enter a manifest: sensitive-named `conditions` keys are
  refused with `secret condition <key> must use secretRefs`.

## Exact refusal sentences

All prefixed `invalid app manifest: <file> …`:

```
… is not an object
… schemaVersion must be 1
… appId is required
… owner is required
… repositories are required
… repository root must be absolute
… repository mappings are required
… surfaces are required
… surface <t> spec is required
… surface <t> journeys are required
… surface <t> journey <j> is unknown
… secret condition <key> must use secretRefs
… journeys are required
… journey <j> owner is required
… journey <j> timeoutMs must be positive
… secretRefs.<key> must be a vault:// reference
… matrix.<p> target <t> has no surface
… <policy>.minimumEvidence must be E2 or E3
… <policy> target <t> has no surface
… <policy> journey <j> is unknown
```

Plus `app manifest not found: <path>` when the file is absent.

## Commands

```bash
probierz apps                # all registered applications (fails closed on any invalid manifest)
probierz app <appId>         # one validated manifest
probierz source-identity <appId>
probierz accessibility <appId>
```

## Not to be confused with

- **A [journey](journey.md)** — one behavior the application declares;
  the application is the container and policy holder.
- **A surface** — one target the application is tested on (`web`,
  `tui`, `mobile:ios`, …); an application usually has several.
- **The harness** — Probierz itself. Both identities are recorded per
  [run](run.md), separately.
