import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { getRun } from "./history.mjs";
import { loadAppManifest } from "./apps.mjs";
import { scanSecrets } from "./security.mjs";
import { appSourceIdentity } from "./runner.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const LEVEL = { E0: 0, E1: 1, E2: 2, E3: 3, E4: 4, E5: 5 };
export const EVIDENCE_RECEIPT_SCHEMA = Object.freeze({
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "urn:probierz:schema:evidence-receipt:v3",
  required: [
    "schemaVersion", "kind", "appId", "productId", "release",
    "expectedHarnessSha", "expectedSourceSha", "builds", "artifactPolicy",
    "secretScans", "issuedAt", "policy", "verdict", "runs", "signing",
  ],
  properties: {
    schemaVersion: { const: 3 },
    kind: { const: "probierz-evidence-receipt" },
    appId: { type: "string", minLength: 1 },
    productId: { type: "string", minLength: 1 },
    release: { type: "string", minLength: 1 },
    expectedHarnessSha: { type: "string", pattern: "^[0-9a-f]{64}$" },
    expectedSourceSha: { type: "string", pattern: "^[0-9a-f]{64}$" },
    artifactPolicy: { type: "object" },
    builds: { type: "object", additionalProperties: { type: "string", pattern: "^[0-9a-f]{64}$" } },
    secretScans: { type: "object" },
    issuedAt: { type: "string", format: "date-time" },
    policy: { type: "object" },
    verdict: { type: "object" },
    runs: {
      type: "array",
      minItems: 1,
      items: {
        type: "object",
        required: [
          "runId", "target", "journeys", "journeyIdentities", "status",
          "evidenceLevel", "harness", "source", "sourceRevision", "build",
          "startedAt", "completedAt", "artifacts", "media",
        ],
        properties: {
          runId: { type: "string", minLength: 1 },
          target: { type: "string", minLength: 1 },
          journeys: { type: "array", items: { type: "string" } },
          journeyIdentities: { type: "array" },
          status: { type: "string" },
          evidenceLevel: { enum: ["E0", "E1", "E2", "E3"] },
          sourceRevision: { type: ["string", "null"], pattern: "^[0-9a-f]{40}$" },
          artifacts: { type: "array" },
          media: { type: "array" },
        },
      },
    },
    signing: {
      type: "object",
      required: ["algorithm", "publicKeyFingerprintSha256", "publicKeyPem", "payloadSha256", "signature"],
      properties: {
        algorithm: { const: "Ed25519" },
        publicKeyFingerprintSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
        publicKeyPem: { type: "string" },
        payloadSha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
        signature: { type: "string", minLength: 1 },
      },
    },
  },
});


function segment(value, fallback = "unknown") {
  const clean = String(value || fallback).trim().replace(/[^a-zA-Z0-9._-]+/g, "-");
  return clean || fallback;
}

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

function evidenceLevel(run) {
  if (run.status !== "passed") return "E0";
  if (run.conditions?.record && run.evidence?.report && run.evidence?.analysis && run.evidence?.capturePresent) return "E3";
  return "E2";
}

function publicFingerprint(publicKey) {
  const der = publicKey.export({ type: "spki", format: "der" });
  return sha256(der);
}
export function signEvidencePayload(payload, privateKeyInput) {
  const text = Buffer.isBuffer(privateKeyInput) ? privateKeyInput.toString("utf8").trim() : String(privateKeyInput || "").trim();
  const privateKey = text.includes("BEGIN")
    ? createPrivateKey(text)
    : createPrivateKey({ key: Buffer.from(text, "base64"), format: "der", type: "pkcs8" });
  if (privateKey.asymmetricKeyType !== "ed25519") throw new Error("evidence private key must be Ed25519");
  const publicKey = createPublicKey(privateKey);
  const canonicalPayload = canonical(payload);
  return {
    algorithm: "Ed25519",
    publicKeyFingerprintSha256: publicFingerprint(publicKey),
    publicKeyPem: publicKey.export({ type: "spki", format: "pem" }).toString(),
    payloadSha256: sha256(canonicalPayload),
    signature: sign(null, Buffer.from(canonicalPayload), privateKey).toString("base64"),
  };
}
export function signedEvidenceId(payload, signing) {
  return sha256(`${canonical(payload)}\n${signing.signature}`).slice(0, 24);
}

function policyConditions(run, manifest) {
  const names = new Set(["PROBIERZ_RELEASE"]);
  for (const profile of Object.values(manifest.matrix || {})) {
    for (const name of Object.keys(profile.dimensions || {})) names.add(name);
  }
  return Object.fromEntries([...names].sort().flatMap((name) => (
    Object.hasOwn(run.conditions || {}, name) ? [[name, run.conditions[name]]] : []
  )));
}

function signedProtection(protection) {
  if (!protection) return null;
  return {
    bytes: Number(protection.bytes || 0),
    sha256: protection.sha256 || null,
    contentIndexSha256: protection.contentIndexSha256 || null,
    keyFingerprintSha256: protection.keyFingerprintSha256 || null,
    plaintextRemoved: Boolean(protection.plaintextRemoved),
    secretScan: protection.secretScan || null,
  };
}
function normalizedArtifactKind(kind) {
  if (kind === "video") return "recording";
  return ["screenshot", "trace"].includes(kind) ? kind : null;
}

function sourceRevision(source) {
  const repositories = Array.isArray(source?.repositories) ? source.repositories : [];
  return repositories.find((repository) => repository.index === 0)?.gitSha
    || repositories[0]?.gitSha
    || null;
}

function journeyIdentities(run, manifest) {
  return run.journeys.flatMap((name) => {
    const journey = manifest.journeys[name];
    if (!journey?.journeyId) return [];
    return [{
      name,
      journeyId: journey.journeyId,
      journeyVersion: journey.journeyVersion,
      journeyVersionId: journey.journeyVersionId,
      firstSuccessFact: journey.firstSuccessFact,
      publication: journey.publication || null,
    }];
  });
}

function signedMedia(run) {
  const root = path.dirname(run.manifestPath);
  const artifacts = new Map(run.artifacts.map((artifact) => [artifact.file.split(path.sep).join("/"), artifact]));
  let analysis = null;
  try {
    analysis = run.analysisPath ? JSON.parse(readFileSync(run.analysisPath, "utf8")) : null;
  } catch {
    analysis = null;
  }
  const typed = new Map();
  for (const media of analysis?.media || []) {
    const relative = path.relative(root, path.resolve(media.file)).split(path.sep).join("/");
    if (relative.startsWith("../") || path.isAbsolute(relative)) continue;
    const kind = normalizedArtifactKind(media.kind);
    if (kind) typed.set(relative, kind);
  }
  for (const file of ["report.json", "timeline.json"]) {
    if (artifacts.has(file)) typed.set(file, "trace");
  }
  return [...typed.entries()].flatMap(([file, artifact]) => {
    const inventory = artifacts.get(file);
    if (!inventory) return [];
    return [{
      file,
      artifactKind: artifact,
      contentSha256: inventory.sha256 || null,
      bytes: Number(inventory.bytes || 0),
      capturedAt: run.completedAt || run.startedAt,
    }];
  }).sort((left, right) => left.file.localeCompare(right.file));
}


export function signedReceiptRun(run, manifest) {
  return {
    runId: run.runId,
    target: run.target,
    spec: run.spec,
    journeys: run.journeys,
    journeyIdentities: journeyIdentities(run, manifest),
    status: run.status,
    evidenceLevel: evidenceLevel(run),
    kind: run.kind,
    harness: run.harness,
    source: run.source,
    build: run.build,
    sourceRevision: sourceRevision(run.source),
    device: run.device,
    startedAt: run.startedAt,
    completedAt: run.completedAt,
    conditions: policyConditions(run, manifest),
    protection: signedProtection(run.protection),
    artifacts: run.artifacts.map((artifact) => ({
      file: artifact.file,
      sha256: artifact.sha256 || null,
      bytes: Number(artifact.bytes || 0),
    })),
    media: signedMedia(run),
  };
}

export async function createReceipt({
  appId,
  release,
  expectedHarnessSha,
  expectedSourceSha,
  runIds,
  requiredJourneys = [],
  minimumEvidence = "E3",
  privateKeyFile = process.env.PROBIERZ_RECEIPT_PRIVATE_KEY_FILE,
  outputRoot = path.join(ROOT, "test-results", "receipts"),
} = {}) {
  if (!appId || !release || !expectedHarnessSha || !expectedSourceSha) {
    throw new Error("appId, release, expectedHarnessSha, and expectedSourceSha are required");
  }
  if (!/^[0-9a-f]{64}$/.test(expectedHarnessSha) || !/^[0-9a-f]{64}$/.test(expectedSourceSha)) {
    throw new Error("expectedHarnessSha and expectedSourceSha must be lowercase SHA-256 values");
  }
  if (!Array.isArray(runIds) || !runIds.length) throw new Error("at least one runId is required");
  if (!Object.hasOwn(LEVEL, minimumEvidence)) throw new Error(`unknown evidence level: ${minimumEvidence}`);
  if (!privateKeyFile) throw new Error("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required");
  const manifest = loadAppManifest(appId);
  const runs = runIds.map((runId) => getRun(appId, runId));
  const normalizedRuns = runs.map((run) => signedReceiptRun(run, manifest));
  const errors = [];
  const currentIdentity = appSourceIdentity(appId);
  if (currentIdentity.harness?.sha256 !== expectedHarnessSha) {
    errors.push("expected harness source is stale relative to the current Probierz checkout");
  }
  if (currentIdentity.app?.sha256 !== expectedSourceSha) {
    errors.push("expected app source is stale relative to the current product checkout");
  }
  const secretScans = {};
  for (let index = 0; index < runs.length; index += 1) {
    const sourceRun = runs[index];
    const signedRun = normalizedRuns[index];
    const artifactRoot = path.dirname(sourceRun.manifestPath);
    for (const artifact of signedRun.artifacts) {
      const file = path.resolve(artifactRoot, artifact.file);
      if ((file !== artifactRoot && !file.startsWith(`${artifactRoot}${path.sep}`)) || !existsSync(file) || !statSync(file).isFile()) {
        errors.push(`${signedRun.runId}: artifact is missing or escapes its run: ${artifact.file}`);
      } else if (sha256(readFileSync(file)) !== artifact.sha256) {
        errors.push(`${signedRun.runId}: artifact hash mismatch: ${artifact.file}`);
      }
    }
    const scan = sourceRun.protection?.plaintextRemoved
      ? sourceRun.protection.secretScan
      : await scanSecrets(artifactRoot);
    secretScans[signedRun.runId] = scan ? {
      passed: Boolean(scan.passed),
      scannedAt: scan.scannedAt || null,
      scannedFiles: Number(scan.scannedFiles || 0),
      skippedBinary: Number(scan.skippedBinary || 0),
      findings: Array.isArray(scan.findings) ? scan.findings : [],
    } : null;
    if (!scan?.passed) errors.push(`${signedRun.runId}: plaintext secret scan is missing or has findings`);
  }
  for (const run of normalizedRuns) {
    if (run.status !== "passed") errors.push(`${run.runId}: status ${run.status}`);
    if (run.harness?.sha256 !== expectedHarnessSha
      || !/^[0-9a-f]{40}$/.test(run.harness?.gitSha || "")
      || !/^[0-9a-f]{64}$/.test(run.harness?.worktreeSha256 || "")) {
      errors.push(`${run.runId}: harness source identity mismatch or incomplete`);
    }
    if (run.source?.sha256 !== expectedSourceSha
      || !Array.isArray(run.source?.repositories)
      || run.source.repositories.some((repository) =>
        !/^[0-9a-f]{40}$/.test(repository.gitSha || "") || !/^[0-9a-f]{64}$/.test(repository.worktreeSha256 || ""))) {
      errors.push(`${run.runId}: app source identity mismatch or incomplete`);
    }
    if (!/^[0-9a-f]{64}$/.test(run.build?.sha256 || "")) errors.push(`${run.runId}: build hash missing or invalid`);
    if (!run.artifacts.length || run.artifacts.some((artifact) => !/^[0-9a-f]{64}$/.test(artifact.sha256 || ""))) {
      errors.push(`${run.runId}: artifact hashes incomplete or invalid`);
    }
    if (LEVEL[run.evidenceLevel] < LEVEL[minimumEvidence]) {
      errors.push(`${run.runId}: ${run.evidenceLevel} is below ${minimumEvidence}`);
    }
  }
  const builds = {};
  for (const run of normalizedRuns) {
    const hash = run.build?.sha256;
    if (!hash) continue;
    if (builds[run.target] && builds[run.target] !== hash) {
      errors.push(`${run.target}: runs do not identify one exact build`);
    } else {
      builds[run.target] = hash;
    }
  }
  const coveredJourneys = new Set(normalizedRuns.flatMap((run) => run.status === "passed" ? run.journeys : []));
  const missingJourneys = [...new Set(requiredJourneys)].filter((journey) => !coveredJourneys.has(journey)).sort();
  for (const journey of missingJourneys) errors.push(`missing journey: ${journey}`);
  const payload = {
    schemaVersion: 3,
    kind: "probierz-evidence-receipt",
    appId,
    release,
    expectedHarnessSha,
    expectedSourceSha,
    builds,
    productId: manifest.productId || appId,
    artifactPolicy: {
      retain: manifest.artifacts?.retain || {},
      redact: [...(manifest.artifacts?.redact || [])].sort(),
      pii: manifest.artifacts?.pii || "unknown",
    },
    secretScans,
    issuedAt: new Date().toISOString(),
    policy: { minimumEvidence, requiredJourneys: [...new Set(requiredJourneys)].sort() },
    verdict: { passed: errors.length === 0, errors, coveredJourneys: [...coveredJourneys].sort(), missingJourneys },
    runs: normalizedRuns,
  };
  const signing = signEvidencePayload(payload, readFileSync(privateKeyFile));
  const receipt = { ...payload, signing };
  const receiptId = signedEvidenceId(payload, signing);
  const directory = path.join(outputRoot, segment(appId), segment(release));
  const file = path.join(directory, `${receiptId}.json`);
  mkdirSync(directory, { recursive: true });
  writeFileSync(file, `${JSON.stringify(receipt, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  return { file, receiptId, receipt };
}

export function verifyReceipt(file, { trustedPublicKeyFile, expectedFingerprint } = {}) {
  const receipt = JSON.parse(readFileSync(file, "utf8"));
  const { signing, ...payload } = receipt;
  if (!signing || signing.algorithm !== "Ed25519") throw new Error("unsupported or missing receipt signature");
  const publicKey = trustedPublicKeyFile
    ? createPublicKey(readFileSync(trustedPublicKeyFile))
    : createPublicKey(signing.publicKeyPem);
  if (publicKey.asymmetricKeyType !== "ed25519") throw new Error("receipt public key must be Ed25519");
  const fingerprint = publicFingerprint(publicKey);
  const expected = expectedFingerprint || process.env.PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT;
  const canonicalPayload = canonical(payload);
  const signatureValid = verify(null, Buffer.from(canonicalPayload), publicKey, Buffer.from(signing.signature, "base64"));
  const trusted = trustedPublicKeyFile
    ? true
    : Boolean(expected && fingerprint === expected);
  return {
    valid: signatureValid && trusted && sha256(canonicalPayload) === signing.payloadSha256,
    signatureValid,
    trusted,
    fingerprint,
    payloadSha256: sha256(canonicalPayload),
    receiptId: signedEvidenceId(payload, signing),
    issuedAt: payload.issuedAt,
    productId: payload.productId || payload.appId,
    verdict: payload.verdict,
    appId: payload.appId,
    release: payload.release,
    expectedHarnessSha: payload.expectedHarnessSha,
    expectedSourceSha: payload.expectedSourceSha,
    builds: payload.builds,
    secretScans: payload.secretScans || {},
    policy: payload.policy,
    runs: payload.runs || [],
    runIds: (payload.runs || []).map((run) => run.runId),
  };
}
