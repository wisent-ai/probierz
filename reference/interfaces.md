# Primary interfaces

Every way into Probierz, and what each one is allowed to do.

- **Human CLI:** `probierz` is canonical for discovery, setup, execution,
  analysis, figure and SEO evaluation, authoring, automatic repair, evidence,
  gate, retention, security, and Stado workflows.
- **Machine CLI output:** status, overview, errors and failures print text for a
  person and the same report as JSON with `--json`; run, analysis, figure
  evaluation, SEO evaluation, and gate commands expose structured data;
  automation must not infer state from prose.
- **Receipt refusals:** `probierz receipt create` still signs and writes a
  receipt whose verdict failed and prints it, then refuses with the receipt id,
  its file and every verdict error. `probierz receipt verify` prints the result,
  including `payloadSha256Matches` beside `signatureValid` and `trusted`, and
  refuses an invalid receipt naming which of the three failed. Both return the
  refusal instead of ending the process, so the in-process MCP tools
  `probierz_create_receipt` and `probierz_verify_receipt` answer with an error
  and the server keeps serving later requests.
  Creating a signed negative receipt is classified as `refused`: its file has
  already been written, so the failure never claims that nothing ran.
  `probierz secrets scan` likewise prints its report and then refuses with the
  directory and number of findings instead of ending the process.
  Real signed verification is exercised by
  `node tests/mcp/receipt-verification.mjs`. Supply `PROBIERZ`,
  `PROBIERZ_TEST_SOURCE_REVISION`, `PROBIERZ_TEST_RELEASE_EVIDENCE`,
  `PROBIERZ_TEST_SIGNED_RECEIPT` from a real product run, and
  `PROBIERZ_TEST_RECEIPT_PUBLIC_KEY` as its trusted public key file.
  The journey verifies the genuine receipt, refuses a modified test-owned
  copy with signature and payload-hash causes, then requires ping and valid
  verification from the same server. Original receipt and key bytes remain
  unchanged. Release provenance, requests, replies and process results are
  retained under `probierz-rs/target/real-tests/mcp-receipt/`.
  This journey does not qualify receipt creation or publication.
- **Gate refusals:** failed `probierz gate evaluate`, `probierz gate enforce`
  and `probierz gate prepush` return the complete verdict, including every error,
  blocking reason and pending-green activation state, without terminating the
  in-process MCP server. A refused gate is classified `refused` — the verdict
  was evaluated — is not a successful tool result and does not prevent later
  requests.
  Unreadable or malformed run manifests refuse evidence collection with their
  path and actual cause. Evidence directory and entry-read failures also name
  the affected path instead of silently excluding runs; only a missing
  evidence directory or manifest counts as absent.
- **Matrix refusals:** a completed matrix that fails its verdict returns a
  `refused` failure with its complete cell results, evidence requirement and
  summary. `probierz_run_matrix` does not terminate the MCP server when a
  matrix fails; the CLI still exits unsuccessfully and retains the same report.
- **Completed mobile and WebDriver desktop refusals:** when these surface
  runners return a failed report, the CLI and MCP refusal retain that report
  together with its analysis and repair results. The failure is classified as
  `refused`, not as a request that never ran, and does not terminate MCP.
- **Merge-status and authoring refusals:** `probierz status <app>` prints the
  status report and, when the app is not eligible to merge, refuses with the
  complete `mergeEligibility` object, every blocking reason included.
  `probierz accessibility`, `probierz repair`, `probierz evaluate figure`,
  `probierz evaluate seo`, `probierz author spec` and `probierz author manifest`
  print their result and, when it did not pass, refuse with the whole result.
  All of them are classified as `refused` and return the refusal instead of
  ending the process, so the in-process MCP tools that run them answer with an
  error and the server keeps serving later requests; the CLI still exits
  unsuccessfully. `probierz project adopt` and `probierz onboarding --source`
  likewise refuse a conflicting adoption (`project.adopt` with the whole
  adoption result, `onboarding.adoption` after printing the conflicts) without
  ending the process; no files change. A wrong option to either command, or
  to `probierz project adoptions`, is an `invalid` failure at
  `adoption.arguments` naming the option, not a clap exit.
  Searching for a recorded specification distinguishes absence from failed
  directory listing, entry inspection and metadata access. A failed search
  retains the affected path and operating-system cause in the repair brief
  and refuses a specification repair rather than claiming no file exists.
  Failure to read prior successful runs is likewise included in the brief
  with its cause, not reported as the absence of a successful run.
- **Run and CI refusals:** a `tui`, `desktop:cua` or `web` journey whose runner
  ends unsuccessfully returns a `refused` failure with its report, and the CLI
  exits with the runner's own status. A skipped `probierz run` refuses with the
  whole result: a held resource lock is `unavailable` (retryable, exit 75), a
  failed preflight is `prerequisite` (exit 1, its remediation in the result).
  `probierz ci` refuses with the whole result when any journey or check failed
  (`refused`) or, otherwise, when any journey was blocked (`prerequisite`).
  None of them ends the process, so the in-process MCP server keeps serving.
  Run-record updates preserve an existing manifest when it cannot be read,
  decoded as JSON, or interpreted as an object. The refusal names
  `run.record.update`, the manifest path and the actual read, decoding or
  object-shape cause. Only a missing manifest starts an empty object; an
  invalid patch is refused before writing rather than silently ignored.
  `tests/records/update.rs` exercises the actual Rust update boundary against
  an isolated filesystem and a supplied genuine run manifest. It covers
  creation, preservation of unrelated fields during merge, malformed JSON,
  non-object records and patches, an actual read failure, and recovery.
  Supply `PROBIERZ_TEST_RUN_MANIFEST` and `PROBIERZ_TEST_SOURCE_REVISION`;
  the qualification runner must bind that revision to the test executable.
  Run the `manifest_updates_preserve_failed_inputs_and_recover` test in the
  Probierz binary test target and retain its output and the evidence under
  `probierz-rs/target/real-tests/manifest-update/`. This filesystem journey
  does not replace full CLI run lifecycle or native graphical qualification.
- **Setup refusals:** `probierz setup <target>` and `probierz_setup` preserve
  the failed step, command, full stderr and preflight report. A subprocess
  launch failure also exposes its operating-system cause in `error`;
  `exitCode` is null when no numeric exit status was observed, not a made-up
  failure code. The command returns a refusal instead of terminating the
  in-process MCP server, so later requests can still be served: a step whose
  program could not be started is `prerequisite`, one that ran and failed is
  `refused`. `probierz check <target>` and `probierz_check` likewise return
  the complete missing-prerequisite and remediation report, classified
  `prerequisite`, without ending the server.
  The real `tests/mcp/admission.mjs` journey uses an empty executable search
  path to exercise missing npm during setup and missing script during
  preflight, then requires a reply to ping from that same server and verifies
  that no dependencies or applications were installed in its isolated harness.
  To run it, provide `PROBIERZ` as the actual candidate executable,
  `PROBIERZ_TEST_SOURCE_REVISION` as the full clean-checkout revision, and
  `PROBIERZ_TEST_RELEASE_EVIDENCE` as the release output's `evidence` directory.
  Invoke `node tests/mcp/admission.mjs` from the repository root. The release
  producer writes `SOURCE_REVISION` beside `DIGESTS` after signing the binary;
  admission refuses absent evidence, a different revision or different binary
  bytes before starting MCP. Reports under
  `probierz-rs/target/real-tests/mcp-admission/` retain that release evidence,
  requests, executable search paths, process results and binary/test hashes.
  Use the evidence from the candidate's actual release, not a manually authored
  sidecar: checkout identity alone does not establish executable provenance.
- **Overview scanner refusals:** `probierz overview` (not `probierz errors`) runs
  `tama violations find --repo <root> --json`. A failed Tama launch names the
  command and operating-system cause. A non-JSON scanner failure retains the
  target repository, actual exit or signal, and complete stdout and stderr.
  Malformed JSON also names the decoding cause. A report without a `problems`
  array, with any scan problem, without a repository entry, or without that
  repository's `violations`, `skippedFiles` or `errors` array is refused with
  the missing part, target repository, actual exit or signal, and complete
  stdout and stderr; it is never reported as a clean scan.
  The real `overview_scan` journey compares every returned count with the
  canonical Tama scan of the application's declared repository. Its
  missing-scanner case verifies Tama is absent from the restricted executable
  search path before requiring the launch refusal.
- **Native driver refusals:** an existing CuaDriver socket is probed once.
  Executable, socket-inspection, child-exit, JSON decoding and missing permission
  state failures retain their own cause; only an explicit false Accessibility
  state is reported as denied permission. A failed probe does not stop the
  existing daemon or delete its socket. When the socket is absent, normal
  application launch waits for its socket and then verifies permissions.
  If the launched daemon exits before creating its socket, the refusal retains
  its log; an unreadable log is reported with its path and filesystem cause,
  not presented as an empty daemon log.
  Process discovery treats only pgrep's documented no-match exit as absence.
  A failed lookup or malformed process identifier retains its status and cause
  rather than claiming that the daemon exited.
- **OTP apphook refusals:** the `--after` timestamp is parsed as RFC 3339.
  An invalid value names the parsing operation, supplied timestamp and decoder
  cause before a broker request is sent.
- **Resource lock refusals:** a failure to create the lock directory or open a
  resource lock names that filesystem operation, the affected path and the
  operating-system cause. Opening a lock also identifies the resource whose
  acquisition failed; it is not reported as contention or a time limit.
  If retaining the refusal or hashing its evidence also fails, the command
  reports both the original lock failure and the evidence failure.
  Failure to truncate, seek or write the holder record names the resource,
  lock path and underlying filesystem cause. The failed acquisition releases
  its open lock rather than returning an unrecorded holder.
  Lock filenames use the complete SHA-256 of the resource identifier; the
  holder record retains the readable identifier. There is no clipped label
  or shortened digest. Finish active runs before upgrading from the old
  filename scheme: old and new executables must not share concurrent leases.
  Resource selection reads the effective suite environment, including inherited
  device settings and explicit overrides, rather than only the override map.
  A seed result that changes the selected resources is refused before the suite
  starts. The failed run names the held and requested resources and invokes its
  declared cleanup with the seeded environment; it never silently runs the suite
  against a device whose lease it does not hold.
- **MCP:** `probierz mcp` exposes the same discovery and explicitly named
  side-effecting operations over stdio JSON-RPC; every tool is the CLI command
  of the same name run inside the server's process, and only an asynchronous
  run is a child process so its process tree can be cancelled. Tool
  descriptions preserve the read-only versus mutation boundary;
  `probierz_evaluate_figure` and `probierz_evaluate_seo` use the same evaluators
  and evidence contracts as the CLI.
  An asynchronous job cannot pass when its child exits unsuccessfully, even
  if the child wrote a JSON report claiming success. The job retains that
  report alongside the failed exit or signal and stderr. Invalid JSON retains
  the decoder cause, observed process outcome and stderr together.
  Artifact requests distinguish a missing report location, failed path
  resolution, failed metadata lookup and a location that is not a directory;
  filesystem refusals name the path, run and operating-system cause.
  Calls must supply an object for `arguments` and every field marked required
  by the same catalogue returned by `tools/list`. Missing coordinates are
  refused by tool and field name before CLI routing or an asynchronous job
  starts; later positional values cannot slide into an omitted coordinate.
  Declared argument types and array element types are checked against that
  catalogue before dispatch: `env` must be an object, `watch` a boolean, and
  each declared string-array member must be text. A type refusal names the
  tool and argument path; malformed options are never silently dropped.
  The real stdio admission journey is
  `PROBIERZ=/path/to/candidate PROBIERZ_TEST_SOURCE_REVISION=<full-sha> PROBIERZ_TEST_RELEASE_EVIDENCE=/path/to/release/evidence node tests/mcp/admission.mjs`.
  Use the `SOURCE_REVISION` and `DIGESTS` emitted and staged by the candidate's
  actual release build, not hand-authored sidecars. The journey refuses a
  source-revision or executable-digest mismatch before starting the server.
  It retains source/binary-bound commands and responses, requires a failure
  response identifying the invalid tool argument and expected type rather than
  a successful result, checks publication refusals leave the isolated harness
  untouched, and checks the server still answers afterward.
  Reports are retained under
  `probierz-rs/target/real-tests/mcp-admission/run-*`.
- **Repository gate:** `probierz gate install` installs the pre-push integration
  and `probierz gate uninstall` removes it, putting back the hook it chained and
  refusing a `pre-push` it did not write; gate evaluation and enforcement remain
  distinct commands. `probierz gate prepush` is the only pre-push command;
  `gate-prepush` and the other retired hyphenated spellings are refused.
  Run `probierz gate install APP_ID --repo REPO` to migrate an older managed
  hook to the grouped command. Installation replaces the managed hook rather
  than chaining its obsolete invocation; unrelated hooks remain chained.
- **Remote fleet bridge:** `probierz remote run`, `probierz remote author`, and
  `probierz remote seo` submit exact remote contracts to a Stado-placed host and
  return evidence through the configured object store; `probierz remote resume`,
  `collect`, and `cancel`
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
  `remote run tui` and `remote author ... --target tui` the same immutable
  executable and exact source inputs without a provisioning-time Cargo build,
  and records the source revision and executable SHA-256 in submission metadata
  and nested run evidence. A selected journey can still invoke its own declared
  commands.
  `remote run --env NAME=VALUE` supplies non-secret execution conditions for
  remote jobs; `--env=NAME=VALUE` is equivalent.
  Values are passed literally, including embedded `=` characters. Credentials
  continue to use the manifest's scoped `secretRefs`, not command arguments.
  Submission requests and responses remain under `test-results/.remote/`;
  stderr prints the request receipt and accepted job ID before watching.
  `probierz remote collect <job-id> --app <id> --host stado:<target>` returns the
  current state immediately and retrieves a terminal job's retained evidence
  without submitting or running it again; `--host` is required. MCP exposes the
  same operation as `probierz_stado_collect` (`jobId`, `appId`, `host`).
  A job cancelled before its worker starts remains `cancelled`; the result keeps
  the exact Stado job and source-input metadata and marks run evidence as not
  required, without asking the artifact store for output the worker never made.
  A structured, non-retryable `NO_ARTIFACTS` response for evidence that was
  required is reported as missing evidence, not as an object-store outage.
  `probierz remote cancel <job-id> --host <host> --reason <reason>` retains the
  original job identity, actual machine cancellation receipt, canonical logs,
  and available evidence under `test-results/.remote/cancellations/<job-id>/`
  and `test-results/.remote/<job-id>/`. The command exits 0 when cancellation
  and required evidence retention succeed; the evaluation remains cancelled,
  non-passing, and keeps its failure details.
  GUI readiness has its own 30-minute audit deadline; an expired audit means
  readiness is unknown and no GUI job was submitted, not that the host is down.
- **Worktree selection:** `probierz identity APP --app-repo /path/to/worktree`
  and `probierz run TARGET --app APP --app-repo /path/to/worktree --spec /path/to/spec`
  bind their evidence to the selected primary checkout without changing the
  application manifest. Other declared repositories keep their own identities.
  Select a binary and product-owned spec built from that same checkout.

The complete command surface is printed by `probierz --help` and summarized in
the published [agent interface](https://probierz.wisent.com/docs/mcp).

