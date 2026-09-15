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
  (`PROBIERZ_MODEL_AGENT_ID` and `PROBIERZ_MODEL_AGENT_SECRET`). Figure evaluation
  also requires `PROBIERZ_FIGURE_VISION_MODEL` or `--model`. Remote Stado jobs set
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
  `probierz-rs/Cargo.toml` owns the Rust product version; rebuild both
  `probierz` and `probierz-mcp` from the desired source revision.

