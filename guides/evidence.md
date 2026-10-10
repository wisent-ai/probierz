# Evidence: the first run, its publication, and its GIF

Producing evidence for the first time, registering verified first-use
assets against a signed receipt, and exporting one recorded journey as
an animated GIF with its provenance sidecar.

## First evidence-producing run

Choose an application and target returned by discovery, then check the exact
host before running it:

```bash
probierz check TARGET
probierz run TARGET --app APP_ID --record
```

`check` either reports readiness or names the missing prerequisite and its owner.
A successful `run` returns the run result and analysis and writes target-specific
artifacts under `test-results/`. It may drive a real application and therefore is
not a read-only continuation of the discovery path. Additional command and
failure guidance is in the published [agent interface](https://probierz.wisent.com/docs/mcp).
The integrated Tama → Probierz → Stado workflow is documented in
the published [pipeline documentation](https://probierz.wisent.com/docs/PIPELINE).

Platform diagnostics reject an incomplete capture even when its child exits
successfully. A failed collector retains its capture cause, redacted stderr
and observed exit status together; stderr does not replace the capture cause.

## Failure ownership

`probierz run TARGET --app APP_ID` records the same `failureOrigin` in its
returned run and persisted run manifest. `probierz history APP_ID --target
TARGET`, evidence and dashboard consumers project that recorded ownership as
`failureClass`: `product`, `infrastructure`, or `unclassified`.

A failed product check or application crash tied to this run remains a product
failure even if cleanup also fails. Seed, runner-start and cleanup failures are
observed harness failures when there is no stronger product-failure evidence.
The underlying operation errors, cleanup result and evidence errors remain in
the run; classification does not suppress them.

When suite execution itself fails, seeded-data cleanup still runs. The failed
run manifest retains the original failure point, code and detail together with
the cleanup result. If recording that failure also fails, the command reports
both the runner cause and the persistence cause; neither is replaced with a
successful or empty run result.

Seed and cleanup commands fail when their captured output cannot be retained,
even if the child exits successfully. The result names the affected stdout or
stderr path and filesystem cause alongside any capture or child-exit failure.
A malformed JSON result names the command and decoder cause.

A missing or invalid report, absent capture, or unsuccessful analysis alone
does not establish which component failed. Such a run is still failed, but its
ownership remains `unclassified`. Historical runs without a recorded origin
are also unclassified unless they record a setup failure. Report presence alone
is not evidence that a product assertion failed. Automatic product-code repair
refuses unclassified and infrastructure runs instead of inventing blame.

Platform log diagnostics use the recorded run start rather than an invented
look-back window. A malformed start is an explicit collection refusal.
`platformDiagnostics.ok` requires both a successful collector and retained
output: failure to create the diagnostics directory or write its file names
the operation, destination and operating-system cause. A failed write reports
`file: null` and `attemptedFile`; the nested collector result preserves its
command, arguments, exit status and error, so a storage failure does not erase
a simultaneous platform-tool failure.
Probierz Desktop shows the same failure in Runs → selected run → Platform
diagnostics unavailable, including the attempted destination and retained
collector details. This warning is separate from the suite verdict: missing
platform evidence does not rewrite a passing product assertion as a failure.

The retained `diagnostics.json` keeps complete redacted crash and console-error
messages, without a fixed-length preview. A log or console error without a
string message records that missing field explicitly instead of becoming an
empty message or disappearing from the diagnostic summary. File-backed crash
entries identify the source path.

Desktop first use records completion when a real protected evidence bundle
has been inspected. If that fact does not satisfy the current journey, the
screen reports the refusal instead of silently returning. A storage or
completion error is distinct from event delivery: once completion is recorded,
an event-flush failure says that the events remain unsent, not that inspection
was lost.
The inspector also observes a selected bundle when the journey finishes
loading or that bundle becomes available locally. Selecting the same row again
or leaving and reopening Artifacts is not required to record that observation.

## Register verified first-use evidence

An `onboarding-first-use` journey carries an immutable `journeyId`,
`journeyVersion`, UUID `journeyVersionId`, and `firstSuccessFact`. Its
`publication` policy names the stable `screenId`, allowed artifact kinds
(`screenshot`, `recording`, or `trace`), minimum evidence level, and whether
verified redaction is mandatory. The application manifest also supplies the
central `productId`.

The same manifest makes retention and redaction explicit with positive
`artifacts.retain.pullRequestDays`, `nightlyDays`, and `adhocDays`, plus a
non-empty `artifacts.redact` key list.

After a release receipt is signed, provide an asset-registration JSON array:

```json
[
  {
    "file": "media/first-use.webm",
    "kind": "recording",
    "storageUrl": "<immutable HTTPS object URL without credentials, query, or fragment>",
    "contentSha256": "64-lowercase-hex-characters",
    "redactionStatus": "verified_redacted",
    "verifiedAt": "2026-08-04T12:00:00.000Z"
  }
]
```

```bash
probierz publication RECEIPT_JSON ATTEMPT_ID JOURNEY_ID \
  --assets ASSET_REGISTRATIONS_JSON \
  --public-key TRUSTED_PROBIERZ_PUBLIC_KEY
```

Probierz emits one immutable
`probierz-first-use-publication` JSON manifest under
`test-results/publications/`. The manifest is release-, source-, journey-,
attempt-, screen-, content-, and signed-receipt-bound. It contains only
`publishable: true` records: an invalid or untrusted receipt, stale source,
missing provenance, mismatched content hash, plaintext-secret finding,
unverified redaction, unsupported recording claim, or credential-bearing
storage URL rejects publication instead of producing a downgraded manifest.
Canonical machine consumers should use `manifestId`, `artifactId`, and the
embedded receipt verification identity rather than deriving identity from file
names.

`artifactId` is the SHA-256 of the recursively key-sorted canonical asset
without `artifactId`; `manifestId` uses the same rule over the full manifest
without `manifestId`. `receiptId` is the first 24 hexadecimal characters of
SHA-256 over the canonical signed payload, a newline, and the base64 Ed25519
signature.

## Publish a README journey GIF

Probierz owns animated product evidence. Select one recorded journey video from
`test-results/`, trim it to the shortest complete outcome, and export it:

```bash
probierz gif test-results/APP_ID/RUN_ID/path/to/video.webm \
  --out /path/to/product/assets/demo.gif \
  --start 0 \
  --duration 12 \
  --fps 12 \
  --width 960
```

The command writes the silent, looping GIF and a sibling
`demo.gif.probierz.json` provenance file containing source/output SHA-256 and
the exact render settings. Duration, frame rate and width are yours: an
omitted `--duration` keeps the rest of the clip from `--start`, and an omitted
`--fps` or `--width` keeps the source's own (recorded as `null` in the
sidecar); a stated value must be positive, and fps and width whole. The sidecar deliberately marks the GIF as
`reviewRequired`: conversion does not prove that the clip is free of
credentials, personal data, production identifiers, or sensitive URLs.
`PROBIERZ_FFMPEG_BIN` may select an explicit `ffmpeg` executable. Probierz does
not create static product banners; those belong to `wisent-asset-generator`.

