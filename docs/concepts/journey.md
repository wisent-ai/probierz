# Journey

What unit does a gate actually protect? Not a test title — a journey: one
named user behavior an [application](application.md) declares, owns, and must
keep proven.

## What it is

An entry in the manifest's `journeys` map:

```yaml
journeys:
  checkout:
    owner: team-or-person     # required
    timeoutMs: 60000          # required, positive
```

Surfaces claim journeys (`surfaces.<target>.journeys: [checkout]`), and file
mappings translate changed paths into affected journeys
(`repositories[].mappings[]`). A run covers the journeys of the surface it
executed — recorded in the run manifest as `appManifest.journeys` — so
journey coverage is derived from run records, never asserted separately.

## Journey identity and publication

A journey that publishes first-use evidence (and `onboarding-first-use`
always) must carry an immutable identity:

| Field | Rule (from `agent/apps.mjs`) |
|---|---|
| `journeyId` | non-empty string |
| `journeyVersion` | non-empty string |
| `journeyVersionId` | UUID, refused otherwise: `journey <name> journeyVersionId must be a UUID` |
| `firstSuccessFact` | non-empty string |
| `publication.screenId` | non-empty string |
| `publication.artifactKinds` | unique subset of `screenshot`, `recording`, `trace` |
| `publication.minimumEvidence` | `E2` or `E3` |
| `publication.redactionRequired` | boolean |

A journey may claim `recording` only if at least one of its surfaces runs on
a recording-capable target (`web`, `mobile:ios`, `mobile:android`,
`desktop:mac`, `desktop:cua`, `desktop:win`); otherwise:
`journey <name> claims recording but none of its drivers support recording`.
`onboarding-first-use` additionally requires a `productId`, all three
retention windows, and a redaction list including `TOKEN`, `SECRET`,
`PASSWORD`, `KEY`, `COOKIE`, and `AUTH`. These identities are copied
verbatim into signed receipts (`journeyIdentities` per run) and enforced at
publication time — see [receipt](receipt.md).

## Journey overrides

A surface may swap its journey list per run environment:

```yaml
journeyOverrides:
  - when: { PROBIERZ_LOCALE: pl }
    journeys: [checkout-pl]
```

The first override whose `when` values all match the run's conditions wins
(`surfaceJourneys` in `agent/apps.mjs`); `when` keys must be non-sensitive
scalars.

## Lifecycle

1. **Declared** in the manifest with an owner and timeout.
2. **Covered** when a run of a surface claiming it completes; the newest run
   per journey is what [status](../cli-evidence.md) reports.
3. **Affected** when a changed file matches a mapping naming it; only
   affected journeys are evaluated for merge eligibility.
4. **Stale** when its latest evidence's recorded `gitSha` no longer equals
   the current HEAD of every declared repository — reported as
   `"<journey>: evidence is older than HEAD"`.
5. **Untested** when no run covers it at all: `"<journey>: no runs recorded"`.

## Invariants

- Every journey named by a surface, override, policy, or matrix must exist in
  `journeys`; unknown names are load-time refusals
  (`surface <t> journey <j> is unknown`, `<policy> journey <j> is unknown`).
- Journey stability is a projection: `probierz history` aggregates per-journey
  pass rate and latest run from run manifests on disk; there is no separate
  journey database.
- A gate's `requiredJourneys` fails with `required journey is missing: <j>`
  when no submitted run covers it; a receipt fails with
  `missing journey: <j>`.

## Commands

```bash
probierz status <appId> --text        # per-journey coverage + freshness (currently blocked on this checkout, see limitations)
probierz history <appId>              # journeys[] stability block
probierz last-green <appId> [target] [journey]
```

## Not to be confused with

- **A spec** — the file that drives a surface; several journeys can live in
  one spec, and one journey can be proven on several surfaces.
- **A test title** — a row inside a report; journeys are declared in the
  manifest, not discovered from titles.
- **A [run](run.md)** — one execution instance; the journey is what the run
  is evidence *of*.
