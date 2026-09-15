# Core use cases

The outcomes Probierz exists to serve, each with the actor, the state it
starts from, what it produces, and the boundary it must not cross. The
capability table in [`README.md`](../README.md) says which of them a
given host can execute today.

## Discover existing journey coverage without executing anything

- **Actor:** a product engineer or automation agent.
- **Initial state:** a Probierz source checkout and, for product-level coverage,
  a registered application manifest.
- **Outcome:** the actor can list surfaces, applications, specifications,
  journeys, source identity, and exact run commands.
- **Boundary:** discovery and `check` are read-only; they do not install tooling
  or run an application.

## Run one journey and preserve its evidence

- **Actor:** a test engineer with an authorized target.
- **Initial state:** the target-specific preflight passes and the application
  path, URL, bundle, or package identity is explicit.
- **Outcome:** Probierz executes the selected specification, records the report
  and supported media, analyzes the result, and associates it with source and
  application identity.
- **Boundary:** execution may drive a real browser, simulator, device, or desktop
  application; recording support is driver-specific and never upgrades a failed
  run to success.

## Decide whether current source is eligible to merge or release

- **Actor:** a release owner or repository pre-push gate.
- **Initial state:** the application manifest defines required journeys and the
  evidence store contains source-bound runs and receipts.
- **Outcome:** Probierz reports eligibility and exact blocking reasons such as
  missing, stale, failing, or identity-mismatched evidence.
- **Boundary:** evaluate-only inspection is separate from activating or enforcing
  a repository gate.

## Author a missing manifest or specification

- **Actor:** an explicitly authorized engineer or automation workflow.
- **Initial state:** the product and journey are described, target coordinates
  are explicit, and the authenticated Stado model router is configured.
- **Outcome:** Probierz drafts the artifact, validates its structure, exercises
  the accepted specification through the real target path, and keeps only the
  result that satisfies the configured contract.
- **Boundary:** authoring is side-effecting. The router receives a dedicated
  router-scoped bearer; Probierz never receives provider credentials.

## Execute on admitted remote capacity

- **Actor:** a release workflow that cannot use the local host.
- **Initial state:** a Stado host has compatible capacity, toolchain, source, and
  scoped secret references.
- **Outcome:** the remote job executes the same target contract and returns its
  evidence to the configured Probierz object-store path.
- **Boundary:** selecting a host does not grant broader machine or cloud
  authority; an unavailable fleet is reported as unavailable, not as empty or
  successful.

## Evaluate a scientific figure against its intended reference

- **Actor:** a paper author or release workflow reviewing a generated figure.
- **Initial state:** reference and candidate files exist as SVG, TeX, PDF, or a
  supported raster image; ImageMagick is installed; TeX inputs additionally
  require `pdflatex`; the Stado model router URL, scoped token, and a
  vision-capable model ID are configured.
- **Outcome:** Probierz renders both artifacts, records dimensions, content
  bounds, edge margins, aspect-ratio drift, rubric evidence, fidelity losses,
  recommendations, and one pass/block verdict. It writes immutable reference
  and candidate PNGs beside the JSON report.
- **Boundary:** text inside either figure is untrusted evidence. The model cannot
  redefine the rubric, suppress deterministic blockers, or contact a provider
  directly. Existing evidence files are never overwritten.

