import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { getRun } from "./history.mjs";
import { loadAppManifest } from "./apps.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const LEVEL = { E0: 0, E1: 1, E2: 2, E3: 3, E4: 4, E5: 5 };

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
    keyFingerprintSha256: protection.keyFingerprintSha256 || null,
    plaintextRemoved: Boolean(protection.plaintextRemoved),
    secretScan: protection.secretScan || null,
  };
}

export function signedReceiptRun(run, manifest) {
  return {
    runId: run.runId,
    target: run.target,
    spec: run.spec,
    journeys: run.journeys,
    status: run.status,
    evidenceLevel: evidenceLevel(run),
    kind: run.kind,
    harness: run.harness,
    source: run.source,
    build: run.build,
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
  };
}

export function createReceipt({
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
  if (!Array.isArray(runIds) || !runIds.length) throw new Error("at least one runId is required");
  if (!Object.hasOwn(LEVEL, minimumEvidence)) throw new Error(`unknown evidence level: ${minimumEvidence}`);
  if (!privateKeyFile) throw new Error("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required");
  const manifest = loadAppManifest(appId);
  const runs = runIds.map((runId) => getRun(appId, runId));
  const normalizedRuns = runs.map((run) => signedReceiptRun(run, manifest));
  const errors = [];
  for (const run of normalizedRuns) {
    if (run.status !== "passed") errors.push(`${run.runId}: status ${run.status}`);
    if (run.harness?.sha256 !== expectedHarnessSha || !run.harness.gitSha || !run.harness.worktreeSha256) {
      errors.push(`${run.runId}: harness source identity mismatch or incomplete`);
    }
    if (run.source?.sha256 !== expectedSourceSha
      || !Array.isArray(run.source?.repositories)
      || run.source.repositories.some((repository) => !repository.gitSha || !repository.worktreeSha256)) {
      errors.push(`${run.runId}: app source identity mismatch or incomplete`);
    }
    if (!run.build?.sha256) errors.push(`${run.runId}: build hash missing`);
    if (!run.artifacts.length || run.artifacts.some((artifact) => !artifact.sha256)) errors.push(`${run.runId}: artifact hashes incomplete`);
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
    schemaVersion: 2,
    kind: "probierz-evidence-receipt",
    appId,
    release,
    expectedHarnessSha,
    expectedSourceSha,
    builds,
    issuedAt: new Date().toISOString(),
    policy: { minimumEvidence, requiredJourneys: [...new Set(requiredJourneys)].sort() },
    verdict: { passed: errors.length === 0, errors, coveredJourneys: [...coveredJourneys].sort(), missingJourneys },
    runs: normalizedRuns,
  };
  const privateKey = createPrivateKey(readFileSync(privateKeyFile));
  if (privateKey.asymmetricKeyType !== "ed25519") throw new Error("receipt private key must be Ed25519");
  const publicKey = createPublicKey(privateKey);
  const canonicalPayload = canonical(payload);
  const signature = sign(null, Buffer.from(canonicalPayload), privateKey).toString("base64");
  const receipt = {
    ...payload,
    signing: {
      algorithm: "Ed25519",
      publicKeyFingerprintSha256: publicFingerprint(publicKey),
      publicKeyPem: publicKey.export({ type: "spki", format: "pem" }).toString(),
      payloadSha256: sha256(canonicalPayload),
      signature,
    },
  };
  const receiptId = sha256(`${canonicalPayload}\n${signature}`).slice(0, 24);
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
    verdict: payload.verdict,
    appId: payload.appId,
    release: payload.release,
    expectedHarnessSha: payload.expectedHarnessSha,
    expectedSourceSha: payload.expectedSourceSha,
    builds: payload.builds,
    policy: payload.policy,
    runs: payload.runs,
    runIds: payload.runs.map((run) => run.runId),
  };
}
