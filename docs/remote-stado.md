# Remote execution on Stado

The Stado bridge runs a Probierz target, an authoring round, or the SEO
evaluator on explicitly selected Stado capacity and brings the evidence back
into the local `test-results/` tree, so history, status, and the gate treat
remote runs like local ones. Selecting a host grants exactly one job on that
capacity, nothing broader; an unavailable fleet is reported as unavailable,
not as empty or successful.

## Host selectors

`probierz hosts` lists the run hosts. `local` is this machine (the default
for plain `probierz run`). The `stado:*` selectors submit to the Stado
queue:

| Selector | Capacity |
|---|---|
| `stado:gcp` / `stado:azure` / `stado:aws` | provider-pinned consumers |
| `stado:any` | any consumer with capacity |
| `stado:spot` | cost-capped capacity |
| `stado:local` | local-kind consumers |
| `stado:mini` / `stado:macbook` | dedicated macOS consumers |
| `stado:t4` | `nvidia-tesla-t4` GPU capacity |

## How a remote job works

The private Probierz checkout travels as a tarball through
`stado://probierz/inputs`, together with a generated job script and any
application inputs. The worker unpacks the checkout, provisions the
application, runs the same `node agent/cli.mjs run <target>` contract (with
`PROBIERZ_RUN_KIND=pull-request`), archives `test-results/` — even for a
failing run — into `stado://probierz/results`, and exits with the run's own
status. `probierz stado run` then downloads and unpacks the evidence under
`test-results/.remote/<jobId>/` and into the normal evidence tree. A failed
job's retained artifacts are fetched too, including a blocked preflight, so
a remote failure stays diagnosable. `--no-watch` submits without waiting.

Application provisioning is explicit per invocation:

- `--cargo-release --app-repo <path> --binary <name>
  [--cargo-manifest <path>]` — pack the application repository, build it
  with a Rust release build on the worker.
- `--app-bundle-path <path> --app-repo <path>` — ship a prebuilt macOS app
  bundle plus its source for identity.
- `--node-source --app-repo <path> [--env K=V ...]
  [--script apps/<id>/remote/x.sh]` — pack a JS application's source; an
  optional repo-committed script replaces the default run command.

## Secrets

Remote jobs never embed credentials in the payload. The job declares secret
references — for example the `probierz-model-router` / `token` reference for
`STADO_MODEL_ROUTER_TOKEN` — and Stado materializes them on the worker.
Locally, the bridge shells out to the installed `stado` CLI (`stado storage
put/get`, `stado machine submit/status/artifacts`), so Stado's own
configuration applies; a host selector may override `STADO_API_URL` for its
jobs. See [configuration](configuration.md).

## Remote authoring and SEO

- `probierz stado author <appId> <journey> --target <t> --desc <d>
  [--host h]` runs the [authoring loop](applications.md#authoring) on a
  Stado host with scoped model credentials; the accepted spec and updated
  manifest come back in the results archive alongside `test-results/`.
- `probierz stado seo <appId> --base-url <url> --primary-model <id>
  --secondary-model <id> --adjudicator-model <id> [--mode ...]
  [--host stado:mini]` runs the complete
  [SEO evaluator](evaluators.md#seo-evaluation) on a dedicated host,
  materializing only the declared router bearer, agent secret, and — when
  the profile requires signing — the SEO receipt key. No secret enters
  `argv`.
