import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { appSourceIdentity } from "./runner.mjs";
import { loadAppManifest, targetSupportsArtifactKind } from "./apps.mjs";
import { verifyReceipt } from "./receipt.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const SHA256 = /^[0-9a-f]{64}$/;
const GIT_SHA = /^[0-9a-f]{40}$/;
const EVIDENCE = { E0: 0, E1: 1, E2: 2, E3: 3 };
const ARTIFACT_KINDS = new Set(["screenshot", "recording", "trace"]);
const REDACTION_STATUSES = new Set(["verified_redacted", "not_applicable"]);

export const PUBLICATION_ARTIFACT_SCHEMA = Object.freeze({
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "urn:probierz:schema:publication-artifact:v1",
  type: "object",
  additionalProperties: false,
  required: [
    "artifactId", "attemptId", "screenId", "kind", "storageUrl", "contentSha256",
    "bytes", "evidenceLevel", "redactionStatus", "capturedAt", "verifiedAt",
  ],
  properties: {
    artifactId: { type: "string", pattern: "^[0-9a-f]{64}$" },
    attemptId: { type: "string", minLength: 1 },
    screenId: { type: "string", minLength: 1 },
    kind: { enum: ["screenshot", "recording", "trace"] },
    storageUrl: { type: "string", format: "uri", pattern: "^https://" },
    contentSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
    bytes: { type: "integer", minimum: 0 },
    evidenceLevel: { enum: ["E2", "E3"] },
    redactionStatus: { enum: ["verified_redacted", "not_applicable"] },
    capturedAt: { type: "string", format: "date-time" },
    verifiedAt: { type: "string", format: "date-time" },
  },
});

export const FIRST_USE_PUBLICATION_SCHEMA = Object.freeze({
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "urn:probierz:schema:first-use-publication:v1",
  type: "object",
  additionalProperties: false,
  required: [
    "schemaVersion", "kind", "manifestId", "publishable", "productId", "journey",
    "release", "attempt", "receipt", "assets",
  ],
  properties: {
    schemaVersion: { const: 1 },
    kind: { const: "probierz-first-use-publication" },
    manifestId: { type: "string", pattern: "^[0-9a-f]{64}$" },
    publishable: { const: true },
    productId: { type: "string", minLength: 1 },
    journey: {
      type: "object",
      additionalProperties: false,
      required: ["journeyId", "journeyVersion", "journeyVersionId", "firstSuccessFact", "screenId"],
      properties: {
        journeyId: { type: "string", minLength: 1 },
        journeyVersion: { type: "string", minLength: 1 },
        journeyVersionId: { type: "string", format: "uuid" },
        firstSuccessFact: { type: "string", minLength: 1 },
        screenId: { type: "string", minLength: 1 },
      },
    },
    release: {
      type: "object",
      additionalProperties: false,
      required: ["version", "sourceRevision", "sourceSha256", "buildSha256"],
      properties: {
        version: { type: "string", minLength: 1 },
        sourceRevision: { type: "string", pattern: "^[0-9a-f]{40}$" },
        sourceSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
        buildSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
      },
    },
    attempt: {
      type: "object",
      additionalProperties: false,
      required: ["attemptId", "evidenceLevel", "capturedAt", "verifiedAt"],
      properties: {
        attemptId: { type: "string", minLength: 1 },
        evidenceLevel: { enum: ["E2", "E3"] },
        capturedAt: { type: "string", format: "date-time" },
        verifiedAt: { type: "string", format: "date-time" },
      },
    },
    receipt: {
      type: "object",
      additionalProperties: false,
      required: ["receiptId", "signed", "verification"],
      properties: {
        receiptId: { type: "string", pattern: "^[0-9a-f]{24}$" },
        signed: { type: "object" },
        verification: {
          type: "object",
          additionalProperties: false,
          required: ["valid", "signatureValid", "trusted", "fingerprint", "payloadSha256", "verifiedAt"],
          properties: {
            valid: { const: true },
            signatureValid: { const: true },
            trusted: { const: true },
            fingerprint: { type: "string", pattern: "^[0-9a-f]{64}$" },
            payloadSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
            verifiedAt: { type: "string", format: "date-time" },
          },
        },
      },
    },
    assets: { type: "array", minItems: 1, items: PUBLICATION_ARTIFACT_SCHEMA },
  },
});

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map((item) => canonical(item ?? null)).join(",")}]`;
  if (value && typeof value === "object") {
    const keys = Object.keys(value).filter((key) => value[key] !== undefined).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function requireValue(condition, message) {
  if (!condition) throw new Error(`publication rejected: ${message}`);
}

function requireIso(value, name) {
  requireValue(typeof value === "string" && Number.isFinite(Date.parse(value)), `${name} must be an ISO timestamp`);
  return value;
}

function immutableStorageUrl(value) {
  try {
    const url = new URL(value);
    return url.protocol === "https:"
      && !url.username
      && !url.password
      && !url.search
      && !url.hash
      && Boolean(url.hostname)
      && url.pathname !== "/";
  } catch {
    return false;
  }
}

function sourceRevision(source) {
  const repositories = Array.isArray(source?.repositories) ? source.repositories : [];
  return repositories.find((repository) => repository.index === 0)?.gitSha
    || repositories[0]?.gitSha
    || null;
}

function safeSegment(value, fallback = "unknown") {
  const clean = String(value || fallback).trim().replace(/[^a-zA-Z0-9._-]+/g, "-");
  return clean || fallback;
}

function artifactWithId(asset) {
  return { artifactId: sha256(canonical(asset)), ...asset };
}

function publicationWithId(publication) {
  return { ...publication, manifestId: sha256(canonical(publication)) };
}

export function createPublicationManifest({
  receiptFile,
  attemptId,
  journeyId,
  assets,
  trustedPublicKeyFile,
  expectedFingerprint,
  outputRoot = path.join(ROOT, "test-results", "publications"),
} = {}) {
  requireValue(typeof receiptFile === "string" && receiptFile.length > 0, "receiptFile is required");
  requireValue(typeof attemptId === "string" && attemptId.length > 0, "attemptId is required");
  requireValue(typeof journeyId === "string" && journeyId.length > 0, "journeyId is required");
  requireValue(Array.isArray(assets) && assets.length > 0, "at least one asset registration is required");

  const verification = verifyReceipt(receiptFile, { trustedPublicKeyFile, expectedFingerprint });
  requireValue(verification.valid && verification.signatureValid && verification.trusted, "receipt signature is not valid and trusted");
  requireValue(verification.verdict?.passed, "receipt verdict did not pass");
  requireValue(verification.productId, "receipt productId is missing");
  requireValue(verification.expectedSourceSha, "receipt source SHA-256 is missing");

  const signedReceipt = JSON.parse(readFileSync(receiptFile, "utf8"));
  requireValue(signedReceipt.schemaVersion >= 3, "receipt predates publication provenance");
  const run = verification.runs.find((candidate) => candidate.runId === attemptId);
  requireValue(run, `attempt ${attemptId} is not signed by the receipt`);
  requireValue(run.status === "passed", `attempt ${attemptId} did not pass`);
  requireValue(Array.isArray(run.media) && run.media.length > 0, `attempt ${attemptId} has no signed evidence artifacts`);

  const appManifest = loadAppManifest(verification.appId);
  requireValue(appManifest.productId === verification.productId, "app manifest and receipt productId differ");
  const identity = (run.journeyIdentities || []).find((candidate) => candidate.journeyId === journeyId);
  requireValue(identity, `journey ${journeyId} is not signed for attempt ${attemptId}`);
  const configuredJourney = Object.values(appManifest.journeys).find((candidate) => candidate.journeyVersionId === identity.journeyVersionId);
  requireValue(configuredJourney, "journey version is no longer present in the app manifest");
  requireValue(
    configuredJourney.journeyId === identity.journeyId
      && configuredJourney.journeyVersion === identity.journeyVersion
      && configuredJourney.firstSuccessFact === identity.firstSuccessFact,
    "journey identity changed after receipt issuance",
  );
  const policy = configuredJourney.publication;
  requireValue(policy, "journey has no publication policy");

  const currentSource = appSourceIdentity(verification.appId).app;
  requireValue(currentSource?.sha256 === verification.expectedSourceSha, "receipt source is stale relative to the current product source");
  requireValue(run.source?.sha256 === verification.expectedSourceSha, "attempt source does not match the receipt source");
  const revision = sourceRevision(run.source);
  requireValue(GIT_SHA.test(revision || ""), "primary source revision must be a full Git SHA-40");
  requireValue(canonical(policy) === canonical(identity.publication), "publication policy changed after receipt issuance");
  requireValue(sourceRevision(currentSource) === revision, "primary source revision is stale");
  requireValue(SHA256.test(run.build?.sha256 || ""), "attempt build SHA-256 is missing");
  requireValue(verification.release, "receipt release version is missing");

  const scan = verification.secretScans?.[attemptId];
  requireValue(scan?.passed && (!Array.isArray(scan.findings) || scan.findings.length === 0), "plaintext secret scan is missing or has findings");
  requireValue(Object.hasOwn(EVIDENCE, run.evidenceLevel), `unsupported evidence level ${run.evidenceLevel}`);
  requireValue(EVIDENCE[run.evidenceLevel] >= EVIDENCE[policy.minimumEvidence], `${run.evidenceLevel} is below ${policy.minimumEvidence}`);

  const signedMedia = new Map(run.media.map((artifact) => [artifact.file, artifact]));
  const seen = new Set();
  const publicationAssets = assets.map((registration, index) => {
    requireValue(registration && typeof registration === "object", `assets.${index} must be an object`);
    requireValue(typeof registration.file === "string" && registration.file.length > 0, `assets.${index}.file is required`);
    requireValue(!seen.has(registration.file), `assets.${index}.file is duplicated`);
    seen.add(registration.file);
    const media = signedMedia.get(registration.file);
    requireValue(media, `assets.${index}.file is not signed report-typed evidence`);
    requireValue(ARTIFACT_KINDS.has(media.artifactKind), `assets.${index} has unsupported artifact kind ${media.artifactKind}`);
    requireValue(policy.artifactKinds.includes(media.artifactKind), `assets.${index} kind ${media.artifactKind} is not allowed by the journey policy`);
    requireValue(targetSupportsArtifactKind(run.target, media.artifactKind), `driver ${run.target} does not support ${media.artifactKind}`);
    requireValue(registration.kind === undefined || registration.kind === media.artifactKind, `assets.${index}.kind conflicts with signed evidence`);
    requireValue(SHA256.test(registration.contentSha256 || ""), `assets.${index}.contentSha256 is required`);
    requireValue(registration.contentSha256 === media.contentSha256, `assets.${index}.contentSha256 does not match the signed receipt`);
    requireValue(immutableStorageUrl(registration.storageUrl), `assets.${index}.storageUrl must be immutable HTTPS without credentials, query, or fragment`);
    requireValue(REDACTION_STATUSES.has(registration.redactionStatus), `assets.${index}.redactionStatus is unsupported`);
    if (policy.redactionRequired) {
      requireValue(registration.redactionStatus === "verified_redacted", `assets.${index} lacks required redaction verification`);
    }
    const verifiedAt = requireIso(registration.verifiedAt, `assets.${index}.verifiedAt`);
    const capturedAt = requireIso(media.capturedAt, `assets.${index}.capturedAt`);
    requireValue(Date.parse(verifiedAt) >= Date.parse(capturedAt), `assets.${index}.verifiedAt predates capture`);
    return artifactWithId({
      attemptId,
      screenId: policy.screenId,
      kind: media.artifactKind,
      storageUrl: registration.storageUrl,
      contentSha256: media.contentSha256,
      bytes: Number(media.bytes || 0),
      evidenceLevel: run.evidenceLevel,
      redactionStatus: registration.redactionStatus,
      capturedAt,
      verifiedAt,
    });
  }).sort((left, right) => left.artifactId.localeCompare(right.artifactId));

  const verifiedAt = publicationAssets.map((asset) => asset.verifiedAt).sort().at(-1);
  const capturedAt = requireIso(run.completedAt || run.startedAt, "attempt.capturedAt");
  const unsigned = {
    schemaVersion: 1,
    kind: "probierz-first-use-publication",
    publishable: true,
    productId: verification.productId,
    journey: {
      journeyId: identity.journeyId,
      journeyVersion: identity.journeyVersion,
      journeyVersionId: identity.journeyVersionId,
      firstSuccessFact: identity.firstSuccessFact,
      screenId: policy.screenId,
    },
    release: {
      version: verification.release,
      sourceRevision: revision,
      sourceSha256: verification.expectedSourceSha,
      buildSha256: run.build.sha256,
    },
    attempt: {
      attemptId,
      evidenceLevel: run.evidenceLevel,
      capturedAt,
      verifiedAt,
    },
    receipt: {
      receiptId: verification.receiptId,
      signed: signedReceipt,
      verification: {
        valid: verification.valid,
        signatureValid: verification.signatureValid,
        trusted: verification.trusted,
        fingerprint: verification.fingerprint,
        payloadSha256: verification.payloadSha256,
        verifiedAt: requireIso(verification.issuedAt, "receipt.verifiedAt"),
      },
    },
    assets: publicationAssets,
  };
  const publication = publicationWithId(unsigned);
  const directory = path.join(
    outputRoot,
    safeSegment(publication.productId),
    safeSegment(publication.release.version),
    safeSegment(publication.journey.journeyVersionId),
    safeSegment(attemptId),
  );
  const file = path.join(directory, `${publication.manifestId}.json`);
  const serialized = `${JSON.stringify(publication, null, 2)}\n`;
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  if (existsSync(file)) {
    requireValue(readFileSync(file, "utf8") === serialized, "immutable publication manifest path contains different content");
    return { file, manifestId: publication.manifestId, publication, reused: true };
  }
  writeFileSync(file, serialized, { mode: 0o600, flag: "wx" });
  return { file, manifestId: publication.manifestId, publication, reused: false };
}
