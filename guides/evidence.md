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
probierz readme-gif test-results/APP_ID/RUN_ID/path/to/video.webm \
  --out /path/to/product/assets/demo.gif \
  --start 0 \
  --duration 12 \
  --fps 12 \
  --width 960
```

The command writes the silent, looping GIF and a sibling
`demo.gif.probierz.json` provenance file containing source/output SHA-256 and
the exact render settings. Duration, frame rate, and width are bounded to keep
repository media reviewable. The sidecar deliberately marks the GIF as
`reviewRequired`: conversion does not prove that the clip is free of
credentials, personal data, production identifiers, or sensitive URLs.
`PROBIERZ_FFMPEG_BIN` may select an explicit `ffmpeg` executable. Probierz does
not create static product banners; those belong to `wisent-asset-generator`.

