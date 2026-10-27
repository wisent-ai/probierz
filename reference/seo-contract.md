# SEO contract

What `probierz evaluate seo` reads before it fetches a page, and what it refuses.

- **Modes:** `--mode` and the profile names under a manifest's `seo.profiles`
  are the same four modes: `pull-request`, `release`, `nightly` and
  `production`. `production` additionally requires Search Console and CrUX
  evidence. Any other name is refused, and the refusal lists the modes.
- **Schema version:** the SEO policy and the approved brief each declare
  `schemaVersion: 1`, the one version the evaluator reads. A document at any
  other version is refused before a page is fetched.
- **Profiles:** each profile under `seo.profiles` is an object stating
  `requireSignature` and `requireProductionEvidence` as booleans; a profile that
  states either as anything else is refused, naming the profile.

The evaluator itself is described in
[the evaluators guide](../guides/evaluators.md).
