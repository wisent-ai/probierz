# The evaluators

Three release evaluators that read an artifact against its declared
contract: the scientific-figure comparison, the talk given to a review panel,
and the SEO contract. None can be talked out of a deterministic blocker by a
model.

## Evaluate a figure

```bash
STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_FIGURE_VISION_MODEL='<vision-model-id>' \
PROBIERZ_FIGURE_RENDER_DENSITY='<dots-per-inch>' \
PROBIERZ_FIGURE_RENDER_MAX_EDGE='<pixels>' \
probierz evaluate figure \
  --reference /absolute/path/intermediate.svg \
  --candidate /absolute/path/final.tex \
  --out test-results/figure-evaluations/paper-figure.json
```

`--reference` and `--candidate` accept SVG, TeX, PDF, PNG, JPEG, or WebP.
`--rubric <json>` replaces the built-in scientific-figure rubric: a `name` and
`dimensions`, each with a `criterion` sentence. The vision model judges every
dimension met or not met, with evidence and the issues it found; there are no
weights, per-dimension minimums or overall score, and a rubric that still
carries `weight`, `minimum` or `overallMinimum` is refused naming the field.
`--model` overrides `PROBIERZ_FIGURE_VISION_MODEL`. `PROBIERZ_FIGURE_RENDER_DENSITY`
(the resolution a TeX or PDF page is rasterized at) and
`PROBIERZ_FIGURE_RENDER_MAX_EDGE` (the longest edge, in pixels, of the PNG the
model is shown; a smaller render is not enlarged) are required positive whole
numbers; a missing or malformed one is refused naming it. Exit status is 0 only when
there are no deterministic or model blockers and every criterion is met; each
unmet criterion is a `criterion_unmet:<dimension>` blocker carrying the issues
the model named. The one deterministic blocker is content that reaches a canvas
edge (`<artifact>_content_at_canvas_edge`); render size and the aspect-ratio
drift between the two renders are reported to the model and in the report, not
held to a chosen bar. The JSON report records both input and render SHA-256
identities, model usage, dimension evidence and the complete blocker list; two
PNG renders are written beside it.

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

## Evaluate a talk

```bash
STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
STADO_MODEL_ROUTER_TOKEN='<scoped-token>' \
PROBIERZ_TALK_MODEL='<vision-model-id>' \
probierz evaluate talk \
  --slides /absolute/path/slides \
  --script /absolute/path/script.md \
  --source /absolute/path/application.pdf \
  --out test-results/talk-evaluations/panel.json
```

`--slides` is the directory of `slide-<number>.png` pictures and `--script` the
speaker script that `lem-cli talk render --slides <dir> --slide-width <px>
--notes <script.md>` writes. The numbers order the slides and must run from the
first without a gap; a picture named any other way, or a missing slide, is
refused naming it. `--source` names a document the talk presents and is
required at least once: Markdown, text and TeX are read as written, a PDF
through `pdftotext -layout` (poppler); any other type, or one with no text, is
refused.

One vision model, `--model` or `PROBIERZ_TALK_MODEL`, is asked once with the
full text of every source, the script and every slide in order, and judges each
rubric dimension met or not with its evidence. The built-in rubric holds:

- `source_fidelity`: every figure, date, name and claim on the slides and in
  the script appears in, or follows from, the sources, and no commitment is
  stronger or weaker than they make it;
- `argument`: one argument a panel can follow, with nothing it needs missing;
- `slide_design`: every slide legible at projection size, one point each, with
  nothing clipped, overlapping or crowded;
- `human_authorship`: specific, concrete text in the presenters' register,
  without the filler, buzzword stacks, formulaic triplets, uniform bullets or
  boilerplate that mark text as machine-generated;
- `delivery`: the script fits the minutes it gives each slide and answers
  every backup slide.

`--rubric <json>` replaces it with the same shape as the figure rubric. The
talk passes only when every criterion is met and the model names no blocker;
each unmet criterion is a `criterion_unmet:<dimension>` blocker with the issues
the model named, and each blocker the model names carries its slide. The JSON
report records the SHA-256 of every slide, the script and every source, the
rubric, the model asked and the one that answered, and the whole evaluation; it
is written once and an existing `--out` is refused.

## Evaluate SEO

`evaluate seo` is a release evaluator, not a Lighthouse score wrapper. It reads
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
probierz evaluate seo \
  --app landing-page \
  --base-url https://product.example.com \
  --mode release
```

The two graders run independently with the routed model's own sampling, and
the evidence they are sent is bounded only by the policy's stated
`model.maxEvidenceCharacters` (a policy that states none sends it whole). Probierz takes the
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
probierz remote seo landing-page \
  --base-url https://product.example.com \
  --mode release \
  --primary-model '<pinned-model-a>' \
  --secondary-model '<pinned-model-b>' \
  --adjudicator-model '<pinned-model-c>' \
  --host stado:<target>
```

`--host` is required: a `stado:<target>` from `probierz hosts`.

Stado materializes only the manifest-declared Brama bearer, agent-auth secret,
and, when the profile requires it, SEO receipt key. The private checkout and
resulting evidence bundle move through `stado://probierz/inputs` and
`stado://probierz/results`; the report, source and rendered HTML, robots and
sitemap bodies, screenshots, mobile performance facts, exact model identities,
request and rubric hashes, source hashes, blocker list, and receipt-compatible
signature land under `test-results/seo/`. `probierz receipt verify <report>`
checks the same canonical Ed25519 signing contract used by other Probierz
receipts.

