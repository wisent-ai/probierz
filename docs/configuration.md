# Configuration

Probierz has no global config file. Intended per-application state lives in
the [application manifest](applications.md); everything invocation-specific
is an explicit environment variable or flag. `.env.example` at the
repository root documents the local starting set. Local discovery requires
no credentials at all.

## Model router (authoring, figure, SEO)

| Variable | Purpose |
|---|---|
| `STADO_MODEL_ROUTER_URL` | authenticated Stado model router base URL (HTTPS, or HTTP loopback) |
| `STADO_MODEL_ROUTER_TOKEN` | router-scoped bearer; never a provider credential |
| `PROBIERZ_MODEL_AGENT_ID` / `PROBIERZ_MODEL_AGENT_SECRET` | agent identity headers for router requests |
| `PROBIERZ_FIGURE_VISION_MODEL` | vision model ID for `figure-evaluate` (or `--model`) |
| `PROBIERZ_SEO_PRIMARY_MODEL` / `PROBIERZ_SEO_SECONDARY_MODEL` | the two independent pinned SEO graders (must differ) |
| `PROBIERZ_SEO_ADJUDICATOR_MODEL` | pinned adjudicator, used only on material divergence |

Probierz never receives provider credentials and never calls a model vendor
directly. Commands also accept `--router-url` and `--router-token-stdin` so
the bearer never enters `argv` or a child environment. Remote Stado jobs
materialize the token from the `probierz-model-router` / `token` secret
reference instead of shipping it in the payload.

## Signing and trust

| Variable | Purpose |
|---|---|
| `PROBIERZ_RECEIPT_PRIVATE_KEY_FILE` | Ed25519 PKCS#8 PEM used to sign evidence and SEO receipts |
| `PROBIERZ_SEO_RECEIPT_PRIVATE_KEY` | inline PEM alternative for the SEO evaluator |
| `PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT` | trusted fingerprint for `verify-receipt` without an explicit key file |
| `PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE` | key for `protect`/`restore` and encryption-required matrices |

## Run conditions

Any `KEY=VALUE` passed to `run`/`ci`/`matrix` becomes a recorded run
condition (values under sensitive key names are redacted before
persistence). Variables with defined meaning:

| Variable | Purpose |
|---|---|
| `BASE_URL` | web target under test |
| `APP_IOS`, `MAC_APP_PATH`, `ELECTRON_APP_MAIN`, `PROBIERZ_BUILD_PATH` | the exact artifact under test; first present value defines the build hash |
| `IOS_DEVICE`, `IOS_VERSION`, `ANDROID_DEVICE`, `ANDROID_VERSION` | device and runtime selection (also recorded and lock-scoped) |
| `PROBIERZ_RUN_KIND` | `adhoc` (default), `pull-request`, `release`, `nightly` |
| `PROBIERZ_RELEASE` | release identity condition; required by release gates and injected by release matrices |
| `PROBIERZ_APP_VERSION` | recorded application version |
| `APPIUM_HOME` | Appium driver install root used by preflight detection |

## Variables Probierz sets for suites

`run` exports `PROBIERZ_APP_ID`, `PROBIERZ_RUN_ID`, `PROBIERZ_ARTIFACTS`,
`PROBIERZ_REPORT_PATH`, `PROBIERZ_JOURNEYS`, `PROBIERZ_SPEC` (when a spec is
selected), and `PROBIERZ_RECORD=1` (when recording). Framework configs must
write the report to `PROBIERZ_REPORT_PATH` and stamp it with the run ID.

## Everything else

| Variable | Purpose |
|---|---|
| `STADO_API_URL` | Stado control origin used by the remote bridge's `stado` CLI calls (host selectors may override it per job) |
| `PROBIERZ_SEO_PRODUCTION_EVIDENCE` | path to the Search Console + CrUX evidence document |
| `PROBIERZ_LANDING_BRIEF` | override path for the SEO landing brief |
| `PROBIERZ_GATE_NO_CI` | `1` makes the installed pre-push hook evaluate-only |
| `PROBIERZ_ACTOR` | audit-record actor (falls back to `GITHUB_ACTOR`, then `USER`) |
| `PROBIERZ_FFMPEG_BIN` | explicit ffmpeg binary for `readme-gif` |

Secrets never belong in a manifest: `secretRefs` hold `vault://` references,
and the values reach a suite only from the invoking process environment
([applications](applications.md#optional-blocks)).
