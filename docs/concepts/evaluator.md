# Evaluator

Journeys prove behavior; some release questions — "is this figure faithful?",
"will this page survive search?" — need judgment. An evaluator is Probierz's
bounded answer: deterministic facts first, pinned models only through the
authenticated Stado router, and no model verdict that can reinterpret a
deterministic blocker.

## What it is

A command that renders or crawls real inputs, computes deterministic facts,
optionally obtains structured scores from pinned models, and emits one
report with a pass/fail verdict and a complete blocker list. Two ship today,
plus one bounded publisher:

| Evaluator | Command | Facts | Models |
|---|---|---|---|
| Figure | `probierz figure-evaluate` | ImageMagick renders, dimensions, content bounds, margins, aspect drift | one vision model, structured rubric verdict |
| SEO | `probierz seo-evaluate` | crawl (Chrome + Googlebot Smartphone UAs), robots, redirects, canonicals, indexability, metadata, hreflang, links, duplicates, JSON-LD, social images, throttled mobile lab | two independent pinned graders at temperature 0 + pinned adjudicator on divergence |
| README GIF | `probierz readme-gif` | ffmpeg render, bounded duration/fps/width, SHA-256 provenance sidecar | none |

## The router boundary

Evaluators never hold provider credentials. Model access goes through the
Stado model router only (`agent/model-router.mjs`):

- `STADO_MODEL_ROUTER_URL` must be HTTPS or loopback HTTP, with no
  credentials, query, or fragment — refusals:
  `STADO_MODEL_ROUTER_URL is required`, `… must be a valid URL`, `… must not
  contain credentials, query parameters, or a fragment`, `… must use HTTPS
  or loopback HTTP`;
- `STADO_MODEL_ROUTER_TOKEN` is a router-scoped bearer
  (`… must not contain whitespace`), optionally delivered on stdin via
  `--router-token-stdin` so it never enters `argv`;
- `PROBIERZ_MODEL_AGENT_ID` / `PROBIERZ_MODEL_AGENT_SECRET` are the agent
  identity headers;
- a router response must contain exactly one expected tool call
  (`Stado model router response must contain exactly one <tool> tool
  call`), with parseable, non-empty content — anything else is an error,
  never a silent fallback.

Manifest authoring (`author-manifest`) drafts through this same router
boundary; spec authoring (`author-spec`) drafts through a local coding-agent
binary (`codex`) instead — see
[cli-authoring-remote](../cli-authoring-remote.md). Either way a drafted
artifact must pass the same manifest validation and is accepted only with a
real verifying run.

## Verdict discipline

- Exit status is 0 only when the evaluator's own verdict passes
  (`result.verdict.pass` for figures, `result.pass` for SEO — `agent/cli.mjs`).
- Deterministic blockers are never model-overridable: a candidate figure
  that does not render is the blocking verdict `candidate_render_failed`
  with the renderer's error as evidence; crawl, indexability,
  structured-data, and performance facts stand regardless of content
  scores.
- Text inside evaluated inputs is untrusted: it cannot redefine the rubric,
  suppress blockers, or reach a provider.
- Reports record input and render hashes, exact model identities, request
  and rubric hashes, and the complete blocker list; existing evidence files
  are never overwritten. The SEO report is Ed25519-signed with the receipt
  contract and checked by `verify-receipt`.

## Where results live

- Figures: the `--out` report plus two immutable PNG renders beside it.
- SEO: `test-results/seo/` — report, source and rendered HTML, robots and
  sitemap bodies, screenshots, performance facts, signature.
- GIFs: the `--out` GIF plus `<out>.gif.probierz.json` provenance sidecar
  marked `reviewRequired` (conversion does not prove the clip is free of
  credentials or personal data).

Full flag tables, rubric rules, model-selection env vars, and the
production-evidence contract: [evaluators](../evaluators.md).

## Not to be confused with

- **A [journey](journey.md) run** — executes the product; an evaluator
  judges artifacts and pages, and produces its own signed evidence.
- **A [gate](gate.md)** — consumes recorded evidence; evaluators produce
  it.
- **Authoring** — drafts specs and manifests through the same router
  boundary, but its output is code that must then earn evidence, not
  evidence itself.
