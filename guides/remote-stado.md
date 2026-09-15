# Running on a Stado host

Submitting a journey to admitted remote capacity, resuming a watch that
lost connectivity, running an already signed native executable against
its exact committed source, and cancelling a job without submitting
replacement work.

## Verify Stado public command documentation

```bash
node agent/cli.mjs stado run tui --app stado-docs \
  --host stado:ubuntu --node-source \
  --app-repo /absolute/path/stado-landing
```

This runs the existing website contract from `stado-landing/tests/docs/` on
the dedicated Ubuntu host. It checks every generated command route and the
complete public command index without opening a browser. Probierz copies the
selected worktrees with portable Git metadata, keeps staging under
`~/.stado/work/probierz`, and records the actual source revision and file hashes
on the worker without rewriting the application manifest.

If the watcher loses connectivity, resume the existing job instead of submitting
the run again. If Probierz's own watch budget expires while Stado still answers,
the result is `watch-expired` and recommends the same resume command; it is not
reported as an infrastructure outage and no local bypass is recommended:

```bash
node agent/cli.mjs stado resume <jobId> --host stado:ubuntu
```

This waits for the original job and imports its retained report without
changing the recorded source identities or executing another workload.
The MCP equivalent is `probierz_stado_resume`.

Remote Cargo provisioning builds the selected binary from its source directory
with the locked dependency graph, so the repository's Rust toolchain is honored.
An existing Rust installation is not upgraded by provisioning. Unless explicitly
overridden with `--timeout`, the runner uses the sum of the selected journeys'
declared time budgets, with the default budget for journeys that omit one.
The staged Cargo output belongs only to that job and is removed on exit,
including failed runs; retained reports and source identities are preserved.

To run or author with an already signed native Stado executable against its
exact, clean, committed product source without rebuilding or consulting a
mutable installation:

```bash
node agent/cli.mjs stado run tui --app stado \
  --host stado:mini \
  --app-binary-path /absolute/path/to/signed/stado \
  --app-repo /absolute/path/to/the/matching/stado/source

STADO_MODEL_ROUTER_URL=https://brama.wisent.com \
node agent/cli.mjs stado author stado <journey> \
  --target tui --desc "<journey goal>" \
  --host stado:mini \
  --app-binary-path /absolute/path/to/signed/stado \
  --app-repo /absolute/path/to/the/matching/stado/source
```

Both commands use the same provisioning path. Probierz uploads the executable
and selected committed source as separate immutable job inputs, copies the
executable into job-owned storage, and uses the ordinary TUI runner and
authoring loop. Native provisioning does not invoke Cargo or rebuild the
application; a selected journey can still run its own declared commands. The
submission receipt records `binary-identity.json` with the executable SHA-256 and
`source-identity.json` with the existing source identity and exact primary
revision. Nested authoring and run evidence hashes the staged executable as its
`build` identity. Probierz does not invent or assert build provenance: the
caller supplies the signed release binary and its source binding.

Terminal authoring also accepts finite CLI commands. It records their output
and actual exit status instead of requiring the process to remain open.
The observation is not a passing test: only the subsequently executed journey
can produce that verdict. An empty initial screen is still refused.

Cancel an existing job without submitting replacement work:

```bash
node agent/cli.mjs stado cancel <jobId> \
  --host stado:mini \
  --reason "operator-requested cancellation"
```

Stado's machine cancellation API accepts the job ID only. Probierz retains the
required reason locally with the original job, exact cancellation receipt,
canonical log pages, and available worker evidence. Each request uses a distinct
`test-results/.remote/cancellations/<jobId>/<attemptId>/` directory; downloaded
worker artifacts remain under `test-results/.remote/<jobId>/`. A successful
cancellation command exits zero when cancellation receipts, logs, and any
required worker artifacts were retained. The cancelled evaluation itself still
has `state: "cancelled"`, `passed: false`, and its failure details. A failed
cancellation or missing required evidence exits nonzero.

