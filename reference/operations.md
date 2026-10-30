# Operational model

What an operator configures, what Probierz keeps, which credentials it
reaches for, and how it behaves when something fails.

- **Configuration:** application manifests define repositories, targets,
  journeys, paths, and ownership. Environment variables provide explicit
  target coordinates, not hidden application defaults.
- **State:** run reports, histories, receipts, protected bundles, audit records,
  and returned remote evidence live under the configured `test-results/` and
  object-store paths. An unavailable store is an error, not an empty history.
- **Credentials:** local discovery requires none. Model authoring and figure
  evaluation reach Brama through `STADO_MODEL_ROUTER_URL`, a router-scoped
  `STADO_MODEL_ROUTER_TOKEN`, and a signed Probierz identity
  (`PROBIERZ_MODEL_AGENT_ID` and `PROBIERZ_MODEL_AGENT_SECRET`). With
  `STADO_MODEL_ROUTER_URL` unset and no surface condition naming it, Probierz
  never dials Brama itself: it asks `stado capabilities resolve inference
  --consumer probierz`, uses Brama's route when Brama answers, and otherwise
  the OpenAI-compatible endpoint declared in its place
  (`capabilities.inference.endpoint.url`), asked for that endpoint's model
  unless one is named, with the `token` of the role
  `capabilities.inference.endpoint.key_role` names and no signature. When no
  alternative answers, the refusal names each with its route and cause. Remote
  jobs are handed Brama's resolved route only: an endpoint in Brama's place is
  refused there, naming how to hand its key to the worker. Without Brama,
  point `STADO_MODEL_ROUTER_URL` at any OpenAI-compatible provider, put its key in
  `STADO_MODEL_ROUTER_TOKEN` and leave both identity variables unset: local
  authoring, figure evaluation and SEO evaluation then send unsigned requests to
  that provider. Setting only one of the identity pair is refused. Figure evaluation
  also requires `PROBIERZ_FIGURE_VISION_MODEL` or `--model`, and the render
  resolution and size as `PROBIERZ_FIGURE_RENDER_DENSITY` and
  `PROBIERZ_FIGURE_RENDER_MAX_EDGE`. Remote Stado jobs set
  the Probierz identity and materialize the token and signing secret from the
  scoped `probierz-model-router` and `probierz-agent-auth` references instead
  of embedding credentials in the job payload.
- **Setup ownership:** `probierz setup` may install npm dependencies, Playwright
  browsers, and Appium drivers owned by Probierz. Host SDKs, simulators, devices,
  permissions, and application runtimes remain operator-managed.
- **Observability:** status, overview, dashboard projection, history, audit, and
  explicit failure objects distinguish failed work from unavailable
  dependencies and blocked prerequisites.
- **Failure recovery:** every failed `probierz run` dispatches one bounded Brama
  repair worker unless `--no-repair` is present. Product fixes apply only in a
  fresh worktree, reject secret and evidence paths, cap the change at eight
  files, commit and publish a repair branch, and open a pull request when GitHub
  credentials are available. Spec fixes must pass the same real journey before
  publication. Infrastructure failures and unsafe repairs are recorded refusals,
  not model guesses.
- **Upgrades:** the repository is currently a source distribution.
  `probierz-rs/Cargo.toml` owns the Rust product version; rebuild `probierz`
  from the desired source revision. `probierz mcp` is the same binary.

