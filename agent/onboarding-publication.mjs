import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { verifyReceipt } from "./receipt.mjs";

const IDENTIFIER = /^[A-Za-z][A-Za-z0-9._-]{0,127}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const SHA256 = /^[0-9a-f]{64}$/;
const GIT_SHA = /^[0-9a-f]{40}$/;
const KINDS = Object.freeze({ screenshot: true, recording: true, trace: true });
const REDACTION = Object.freeze({ verified_redacted: true, not_applicable: true });

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

function required(value, name, pattern = null) {
  if (typeof value !== "string" || !value || (pattern && !pattern.test(value))) {
    throw new Error(`${name} is invalid`);
  }
  return value;
}

function timestamp(value, name) {
  required(value, name);
  const milliseconds = Date.parse(value);
  if (!Number.isFinite(milliseconds)) throw new Error(`${name} is invalid`);
  return new Date(milliseconds).toISOString();
}

function immutableUrl(value, name) {
  const parsed = new URL(required(value, name));
  if (parsed.protocol !== "https:" || !parsed.hostname || parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new Error(`${name} must be a credential-free immutable HTTPS URL`);
  }
  return parsed.toString();
}

function sourceRevisionFor(run, expectedSourceSha) {
  if (run.source?.sha256 !== expectedSourceSha || !Array.isArray(run.source?.repositories)) {
    throw new Error("receipt run source identity does not match the signed receipt");
  }
  const repository = run.source.repositories.find((entry) => entry?.index === 0);
  return required(repository?.gitSha, "source revision", GIT_SHA);
}

function receiptId(receipt) {
  const { signing, ...payload } = receipt;
  return sha256(`${canonical(payload)}\n${required(signing?.signature, "receipt signature")}`).slice(0, 24);
}

export function createOnboardingPublication({
  receiptFile,
  runId,
  journeyId,
  journeyVersion,
  journeyVersionId,
  firstSuccessFact,
  screenId,
  assetCatalogFile,
  trustedPublicKeyFile,
  expectedFingerprint,
  outputFile,
} = {}) {
  const verification = verifyReceipt(required(receiptFile, "receipt file"), {
    trustedPublicKeyFile,
    expectedFingerprint,
  });
  if (!verification.valid || !verification.signatureValid || !verification.trusted || verification.verdict?.passed !== true) {
    throw new Error("receipt is not valid, trusted, and passing");
  }
  const receipt = JSON.parse(readFileSync(receiptFile, "utf8"));
  const run = receipt.runs?.find((entry) => entry?.runId === runId);
  if (!run || run.status !== "passed") throw new Error(`passing receipt run not found: ${runId}`);
  if (!run.journeys?.includes(journeyId)) throw new Error(`receipt run does not cover journey: ${journeyId}`);
  if (run.evidenceLevel !== "E2" && run.evidenceLevel !== "E3") throw new Error("onboarding publication requires E2 or E3 evidence");
  if (!run.protection?.secretScan?.passed) throw new Error("receipt run has no successful protected-artifact secret scan");
  const buildSha256 = required(run.build?.sha256, "build sha256", SHA256);
  const sourceRevision = sourceRevisionFor(run, receipt.expectedSourceSha);
  const catalog = JSON.parse(readFileSync(required(assetCatalogFile, "asset catalog file"), "utf8"));
  if (!Array.isArray(catalog) || !catalog.length) throw new Error("asset catalog must be a non-empty JSON array");
  const verifiedAt = new Date().toISOString();
  const defaultCapturedAt = timestamp(run.completedAt, "run completedAt");
  const signedArtifacts = new Map(run.artifacts.map((artifact) => [artifact.file, artifact]));
  const assets = catalog.map((entry, index) => {
    const signed = signedArtifacts.get(required(entry?.file, `assets[${index}].file`));
    if (!signed || !SHA256.test(signed.sha256) || !Number.isSafeInteger(signed.bytes) || signed.bytes <= 0) {
      throw new Error(`assets[${index}] is not bound by the signed receipt`);
    }
    const kind = required(entry.kind, `assets[${index}].kind`);
    const redactionStatus = required(entry.redactionStatus, `assets[${index}].redactionStatus`);
    if (!KINDS[kind]) throw new Error(`assets[${index}].kind is unsupported`);
    if (!REDACTION[redactionStatus]) throw new Error(`assets[${index}].redactionStatus is incomplete`);
    const identity = {
      attemptId: run.runId,
      screenId: required(screenId, "screen id", IDENTIFIER),
      kind,
      storageUrl: immutableUrl(entry.storageUrl, `assets[${index}].storageUrl`),
      contentSha256: signed.sha256,
      bytes: signed.bytes,
      evidenceLevel: run.evidenceLevel,
      redactionStatus,
      capturedAt: entry.capturedAt ? timestamp(entry.capturedAt, `assets[${index}].capturedAt`) : defaultCapturedAt,
      verifiedAt,
    };
    return { artifactId: sha256(canonical(identity)), ...identity };
  });
  if (new Set(assets.map((asset) => asset.contentSha256)).size !== assets.length) {
    throw new Error("asset catalog contains duplicate signed artifacts");
  }

  const identity = {
    schemaVersion: 1,
    kind: "probierz-first-use-publication",
    publishable: true,
    productId: required(receipt.appId, "product id", IDENTIFIER),
    journey: {
      journeyId: required(journeyId, "journey id", IDENTIFIER),
      journeyVersion: required(journeyVersion, "journey version"),
      journeyVersionId: required(journeyVersionId, "journey version id", UUID),
      firstSuccessFact: required(firstSuccessFact, "first success fact", IDENTIFIER),
      screenId: required(screenId, "screen id", IDENTIFIER),
    },
    release: {
      version: required(receipt.release, "release version"),
      sourceRevision,
      sourceSha256: required(receipt.expectedSourceSha, "source sha256", SHA256),
      buildSha256,
    },
    attempt: {
      attemptId: run.runId,
      evidenceLevel: run.evidenceLevel,
      capturedAt: defaultCapturedAt,
      verifiedAt,
    },
    receipt: {
      receiptId: receiptId(receipt),
      signed: receipt,
      verification: {
        valid: true,
        signatureValid: true,
        trusted: true,
        fingerprint: verification.fingerprint,
        payloadSha256: verification.payloadSha256,
        verifiedAt,
      },
    },
    assets,
  };
  const publication = { manifestId: sha256(canonical(identity)), ...identity };
  const target = path.resolve(outputFile || `onboarding-publication-${publication.manifestId}.json`);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, `${JSON.stringify(publication, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  return { file: target, manifestId: publication.manifestId, publication };
}
