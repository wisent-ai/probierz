# Evaluators

Beyond journey execution, Probierz ships two rubric-bound evaluators —
scientific figures and SEO — plus a bounded README GIF publisher. All three
follow the same rule as the rest of the product: deterministic facts first,
models only through the authenticated Stado model router, and no model
verdict may reinterpret a deterministic blocker.

## Figure evaluation

```bash
STADO_MODEL_ROUTER_URL=<router-url> \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_FIGURE_VISION_MODEL='<vision-model-id>' \
probierz figure-evaluate \
  --reference /absolute/path/intermediate.svg \
  --candidate /absolute/path/final.tex \
  --out test-results/figure-evaluations/paper-figure.json
```

`--reference` and `--candidate` accept SVG, TeX, PDF, PNG, JPEG, or WebP.
Probierz renders both with ImageMagick (`magick`; TeX additionally needs
`pdflatex`), records dimensions, content bounds, edge margins, and
aspect-ratio drift, then obtains a structured rubric verdict from a
vision-capable model through the router. The JSON report records input and
render SHA-256 identities, model usage, dimension evidence, the weighted
score, and the complete blocker list; two immutable PNG renders are written
beside it, and existing evidence files are never overwritten.

- Exit status is 0 only with no deterministic, model, dimension-threshold,
  or overall-threshold blockers.
- A candidate that does not render is a blocking verdict
  (`candidate_render_failed`, with the renderer's own error as evidence),
  not a tool failure; a reference that does not render is an input error.
- `--rubric <json>` replaces the built-in rubric; positive weights must
  total 1 and every threshold must be between 0 and 1. `--model` overrides
  `PROBIERZ_FIGURE_VISION_MODEL`. `--tex-preamble <file>` adds the
  manuscript's own macros to the standalone TeX wrapper (preamble lines
  only).
- Text inside either figure is untrusted evidence: the model cannot
  redefine the rubric, suppress deterministic blockers, or contact a
  provider directly.
- Process integrations pass `--router-url` and send the bearer over stdin
  with `--router-token-stdin` (first line the bearer, optional second line
  the agent secret), keeping credentials out of `argv` and child
  environments.

## SEO evaluation

`probierz seo-evaluate` is a release evaluator, not a Lighthouse score
wrapper. It reads the manifest-declared brief and SEO policy
([applications](applications.md#optional-blocks)), crawls every declared and
sitemap-discovered URL as ordinary Chrome and as Googlebot Smartphone, and
evaluates robots directives, redirects, canonicals, indexability, metadata,
hreflang, internal-link reachability, duplicate content, JSON-LD, social
image responses, and a throttled mobile lab profile (LCP, CLS, TBT, failed
resources, runtime errors).

Content quality is scored by two independent pinned graders at temperature
zero (`PROBIERZ_SEO_PRIMARY_MODEL`, `PROBIERZ_SEO_SECONDARY_MODEL`),
authenticated to the router with `PROBIERZ_MODEL_AGENT_ID` and
`PROBIERZ_MODEL_AGENT_SECRET`. Probierz takes the stricter score when they
agree closely and invokes the pinned adjudicator
(`PROBIERZ_SEO_ADJUDICATOR_MODEL`; all three IDs must differ) only when a
dimension diverges beyond the policy threshold or the blocker sets differ.
Models may score search intent, factuality, information gain, and snippet
quality; they cannot override crawl, indexability, structured-data, or
performance facts.

The report separates `searchEligibility`, weighted `searchQuality`, and
`productionOutcome`. A release passes only with no hard or model-confirmed
blockers, every dimension at or above its minimum, overall quality at or
above `0.85`, and an Ed25519 signature
(`PROBIERZ_RECEIPT_PRIVATE_KEY_FILE` or `PROBIERZ_SEO_RECEIPT_PRIVATE_KEY`).
The `--mode` profiles (`pull-request`, `release`, `nightly`, `production`)
come from the application manifest; each declares whether signed evidence
and production observations are mandatory. `production` consumes a versioned
Search Console + CrUX evidence document (`--production-evidence` or
`PROBIERZ_SEO_PRODUCTION_EVIDENCE`) that must identify the
`google-search-console+crux` source, be fresh, and observe every declared
indexable URL — production adds CrUX p75 INP to the facts.

Evidence lands under `test-results/seo/`: the report, source and rendered
HTML, robots and sitemap bodies, screenshots, performance facts, exact model
identities, request and rubric hashes, source hashes, the blocker list, and
a receipt-compatible signature that `probierz verify-receipt` checks with
the same canonical Ed25519 contract as
[evidence receipts](gates-and-receipts.md#evidence-receipts). To run it on a
dedicated host, use `probierz stado seo`
([remote-stado](remote-stado.md#remote-authoring-and-seo)).

## README journey GIFs

```bash
probierz readme-gif test-results/APP_ID/RUN_ID/path/to/video.webm \
  --out /path/to/product/assets/demo.gif --start 0 --duration 12 --fps 12 --width 960
```

Converts one recorded journey video into a silent, looping GIF plus a
sibling `demo.gif.probierz.json` provenance file (source and output SHA-256
and the exact render settings). Duration, frame rate, and width are bounded
to keep repository media reviewable, and the sidecar marks the GIF as
`reviewRequired`: conversion does not prove the clip is free of credentials
or personal data. Requires ffmpeg (`PROBIERZ_FFMPEG_BIN` selects an explicit
binary). Probierz does not create static product banners.
