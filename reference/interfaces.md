# Primary interfaces

Every way into Probierz, and what each one is allowed to do.

- **Human CLI:** `probierz` is canonical for discovery, setup, execution,
  analysis, figure and SEO evaluation, authoring, automatic repair, evidence,
  gate, retention, security, and Stado workflows.
- **Machine CLI output:** status, overview, run, analysis, figure evaluation,
  SEO evaluation, and gate commands expose structured data; automation must not
  infer state from prose.
- **MCP:** the `probierz-mcp` binary exposes the same discovery and explicitly
  named side-effecting operations over stdio JSON-RPC. Tool descriptions
  preserve the read-only versus mutation boundary;
  `probierz_evaluate_figure` and `probierz_evaluate_seo` use the same evaluators
  and evidence contracts as the CLI.
- **Repository gate:** `probierz gate-install` installs the pre-push integration;
  gate evaluation and enforcement remain distinct commands.
- **Stado bridge:** `probierz stado run`, `probierz stado author`, and
  `probierz stado seo` submit exact remote contracts and return evidence through
  the configured object store; `probierz stado resume`, `collect`, and `cancel`
  operate on the original job without submitting replacement work.
  Authoring applies the surface's matching single-journey override before
  executing its candidate, including on a remote worker. Native `desktop:cua`
  authoring returns the accepted spec alongside the manifest and evidence.
  Every run exports `PROBIERZ_TOOLKIT_ROOT` for product-owned specs that use the
  toolkit's real drivers. Remote source provisioning exports `PROBIERZ_APP_SOURCE`
  as the staged product checkout; native application bundles keep their source
  beside the bundle, under the `-src` directory.
  The submitter measures the source identity of the harness and every manifest
  repository and ships `inputs/source-identity.json`; the worker records it
  with `sourceIdentityOrigin: "submitter"` rather than hashing absent checkouts.
  `--app-repo` selects the product tree that is packed and measured.
  For native TUI releases, `--app-binary-path FILE --app-repo REPO` gives
  `stado run tui` and `stado author ... --target tui` the same immutable
  executable and exact source inputs without a provisioning-time Cargo build,
  and records the source revision and executable SHA-256 in submission metadata
  and nested run evidence. A selected journey can still invoke its own declared
  commands.
  `stado run --env NAME=VALUE` supplies non-secret execution conditions for
  remote jobs; `--env=NAME=VALUE` is equivalent.
  Values are passed literally, including embedded `=` characters. Credentials
  continue to use the manifest's scoped `secretRefs`, not command arguments.
  Submission requests and responses remain under `test-results/.remote/`;
  stderr prints the request receipt and accepted job ID before watching.
  `probierz stado collect <job-id> --app <id> --host stado:mini` (also
  `probierz_stado_collect` over MCP) returns the current state immediately and
  retrieves a terminal job's retained evidence without submitting or running it again.
  A job cancelled before its worker starts remains `cancelled`; the result keeps
  the exact Stado job and source-input metadata and marks run evidence as not
  required, without asking the artifact store for output the worker never made.
  A structured, non-retryable `NO_ARTIFACTS` response for evidence that was
  required is reported as missing evidence, not as an object-store outage.
  `probierz stado cancel <job-id> --host <host> --reason <reason>` retains the
  original job identity, actual machine cancellation receipt, canonical logs,
  and available evidence under `test-results/.remote/cancellations/<job-id>/`
  and `test-results/.remote/<job-id>/`. The command exits 0 when cancellation
  and required evidence retention succeed; the evaluation remains cancelled,
  non-passing, and keeps its failure details.
  GUI readiness has its own 30-minute audit deadline; an expired audit means
  readiness is unknown and no GUI job was submitted, not that the host is down.
- **Worktree selection:** `probierz source-identity APP --app-repo /path/to/worktree`
  and `probierz run TARGET --app APP --app-repo /path/to/worktree --spec /path/to/spec`
  bind their evidence to the selected primary checkout without changing the
  application manifest. Other declared repositories keep their own identities.
  Select a binary and product-owned spec built from that same checkout.

The complete command surface is printed by `probierz --help` and summarized in
the published [agent interface](https://probierz.wisent.com/docs/mcp).

