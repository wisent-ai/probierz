# The evaluators

Two release evaluators that read an artifact against its declared
contract: the scientific-figure comparison, and the SEO contract.
Neither can be talked out of a deterministic blocker by a model.

## Evaluate a figure

```bash
STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_FIGURE_VISION_MODEL='<vision-model-id>' \
probierz figure-evaluate \
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

## Evaluate SEO

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
probierz seo-evaluate \
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
probierz stado seo landing-page \
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

