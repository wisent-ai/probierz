import { createCipheriv, createDecipheriv, createHash, randomBytes } from "node:crypto";
import {
  closeSync,
  createReadStream,
  createWriteStream,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeSync,
  writeFileSync,
} from "node:fs";
import { finished } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { loadAppManifest } from "./apps.mjs";
import { getRun } from "./history.mjs";
import { assertNoSecrets, auditAccess } from "./security.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const RESULTS = path.join(ROOT, "test-results");
const PROTECTED = path.join(RESULTS, ".protected");
const MAGIC = Buffer.from("PROBIERZ-EVIDENCE-1\n", "utf8");
const TAG_BYTES = 16;

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function sha256File(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

function keyFromFile(file) {
  if (!file) throw new Error("artifact encryption key file is required");
  const raw = readFileSync(file);
  if (raw.length === 32) return raw;
  const text = raw.toString("utf8").trim();
  const decoded = /^[a-f0-9]{64}$/i.test(text) ? Buffer.from(text, "hex") : Buffer.from(text, "base64");
  if (decoded.length !== 32) throw new Error("artifact encryption key must contain exactly 32 bytes, hex, or base64");
  return decoded;
}

function filesBelow(root) {
  const pending = [root];
  const files = [];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      const detail = lstatSync(file);
      if (detail.isSymbolicLink()) throw new Error(`artifact source contains a symlink: ${file}`);
      if (detail.isDirectory()) pending.push(file);
      else if (detail.isFile()) files.push(file);
    }
  }
  return files.sort();
}

function retentionDays(manifest, kind) {
  const retain = manifest.artifacts?.retain || {};
  const key = {
    "pull-request": "pullRequestDays",
    nightly: "nightlyDays",
    release: "releaseDays",
    synthetic: "syntheticDays",
    adhoc: "adhocDays",
  }[kind] || "adhocDays";
  const value = Number(retain[key] ?? retain.pullRequestDays ?? 14);
  if (!Number.isFinite(value) || value <= 0) throw new Error(`invalid artifact retention for ${kind}`);
  return value;
}

function expiresAt(startedAt, days) {
  const timestamp = Date.parse(startedAt);
  if (!Number.isFinite(timestamp)) throw new Error(`invalid run timestamp: ${startedAt}`);
  return new Date(timestamp + days * 24 * 60 * 60 * 1000).toISOString();
}

function writeBuffer(fd, buffer) {
  let offset = 0;
  while (offset < buffer.length) offset += writeSync(fd, buffer, offset, buffer.length - offset);
}

function encodedHeader(header) {
  const body = Buffer.from(`${JSON.stringify(header)}\n`, "utf8");
  const size = Buffer.alloc(4);
  size.writeUInt32BE(body.length);
  return { body, prefix: Buffer.concat([MAGIC, size, body]) };
}

function readHeader(file) {
  const fd = openSync(file, "r");
  try {
    const prefix = Buffer.alloc(MAGIC.length + 4);
    if (readSync(fd, prefix, 0, prefix.length, 0) !== prefix.length || !prefix.subarray(0, MAGIC.length).equals(MAGIC)) {
      throw new Error("not a Probierz encrypted evidence bundle");
    }
    const length = prefix.readUInt32BE(MAGIC.length);
    if (length <= 0 || length > 1024 * 1024) throw new Error("invalid evidence bundle header length");
    const body = Buffer.alloc(length);
    if (readSync(fd, body, 0, length, prefix.length) !== length) throw new Error("truncated evidence bundle header");
    const header = JSON.parse(body.toString("utf8"));
    return { header, offset: prefix.length + length, aad: Buffer.concat([prefix, body]) };
  } finally {
    closeSync(fd);
  }
}

function removePlaintextSource({ source, manifestPath, retentionKind, protectedArtifact }) {
  for (const entry of readdirSync(source)) {
    if (entry !== "run-manifest.json") rmSync(path.join(source, entry), { recursive: true, force: true });
  }
  const current = JSON.parse(readFileSync(manifestPath, "utf8"));
  const replacement = `${manifestPath}.tmp-${process.pid}-${Date.now()}`;
  writeFileSync(replacement, `${JSON.stringify({
    ...current,
    kind: current.kind || retentionKind,
    protection: { ...protectedArtifact, plaintextRemoved: true },
    plaintextArtifactsRemovedAt: new Date().toISOString(),
  }, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  renameSync(replacement, manifestPath);
}

async function protectRunImpl({ appId, runId, kind, keyFile, removePlaintext = false } = {}) {
  const run = getRun(appId, runId);
  const source = path.dirname(run.manifestPath);
  const sourceRoot = path.resolve(RESULTS, appId);
  const relativeSource = path.relative(sourceRoot, source);
  if (relativeSource.startsWith("..") || path.isAbsolute(relativeSource)) throw new Error("run is outside its product artifact root");
  const manifest = loadAppManifest(appId);
  const retentionKind = kind || run.kind || "adhoc";
  const days = retentionDays(manifest, retentionKind);
  const key = keyFromFile(keyFile || process.env.PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE);
  const currentManifest = JSON.parse(readFileSync(run.manifestPath, "utf8"));
  const primaryRepository = (run.source?.repositories || []).find((repository) => repository.index === 0)
    || run.source?.repositories?.[0]
    || null;
  const journeyIdentities = run.journeys.flatMap((name) => {
    const journey = manifest.journeys[name];
    return journey?.journeyId ? [{
      name,
      journeyId: journey.journeyId,
      journeyVersion: journey.journeyVersion,
      journeyVersionId: journey.journeyVersionId,
      firstSuccessFact: journey.firstSuccessFact,
      screenId: journey.publication?.screenId || null,
    }] : [];
  });
  const evidenceLevel = run.status !== "passed"
    ? "E0"
    : run.conditions?.record && run.evidence?.report && run.evidence?.analysis && run.evidence?.capturePresent
      ? "E3"
      : "E2";
  if (currentManifest.protection?.plaintextRemoved) {
    const protectedArtifact = currentManifest.protection;
    if (!existsSync(protectedArtifact.file)) throw new Error("plaintext was removed but the encrypted evidence bundle is missing");
    if (protectedArtifact.keyFingerprintSha256 !== sha256(key)) throw new Error("artifact encryption key fingerprint mismatch");
    return { ...protectedArtifact, reused: true };
  }
  const destination = path.join(PROTECTED, appId, retentionKind, `${runId}.pev`);
  const existingBundle = existsSync(destination);
  let secretScan = null;
  if (!existingBundle) secretScan = await assertNoSecrets(source);
  const sourceFiles = filesBelow(source);
  const entries = [];
  for (const file of sourceFiles) {
    const detail = statSync(file);
    entries.push({
      file: path.relative(source, file).split(path.sep).join("/"),
      bytes: detail.size,
      mode: detail.mode & 0o777,
      sha256: await sha256File(file),
    });
  }
  const index = Buffer.from(`${JSON.stringify({ schemaVersion: 1, runId, files: entries })}\n`, "utf8");
  const contentIndexSha256 = sha256(index);
  if (existingBundle) {
    const { header: existingHeader } = readHeader(destination);
    if (existingHeader.runId !== runId || existingHeader.appId !== appId) throw new Error("encrypted bundle identity mismatch");
    if (existingHeader.keyFingerprintSha256 !== sha256(key)) throw new Error("artifact encryption key fingerprint mismatch");
    if (!existingHeader.contentIndexSha256) throw new Error("existing encrypted bundle predates source-integrity metadata");
    if (existingHeader.contentIndexSha256 !== contentIndexSha256) throw new Error("plaintext artifacts changed after the encrypted bundle was created");
    const protectedArtifact = {
      file: destination,
      bytes: statSync(destination).size,
      sha256: await sha256File(destination),
      contentIndexSha256: existingHeader.contentIndexSha256,
      keyFingerprintSha256: existingHeader.keyFingerprintSha256,
      expiresAt: existingHeader.expiresAt,
      retentionDays: existingHeader.retentionDays,
      files: existingHeader.files,
      secretScan: existingHeader.secretScan || null,
      plaintextRemoved: Boolean(removePlaintext),
      reused: true,
    };
    if (removePlaintext) removePlaintextSource({ source, manifestPath: run.manifestPath, retentionKind, protectedArtifact });
    return protectedArtifact;
  }
  const indexSize = Buffer.alloc(4);
  indexSize.writeUInt32BE(index.length);
  const nonce = randomBytes(12);
  // Destination non-existence was checked before hashing to prevent accidental
  // replacement of the only complete encrypted copy.
  mkdirSync(path.dirname(destination), { recursive: true, mode: 0o700 });
  const temporary = `${destination}.tmp-${process.pid}-${Date.now()}`;
  const header = {
    schemaVersion: 2,
    kind: "probierz-encrypted-evidence",
    algorithm: "AES-256-GCM",
    appId,
    runId,
    attemptId: runId,
    productId: manifest.productId || appId,
    releaseVersion: currentManifest.appVersion || currentManifest.conditions?.PROBIERZ_RELEASE || null,
    sourceRevision: primaryRepository?.gitSha || null,
    sourceSha256: run.source?.sha256 || null,
    buildSha256: run.build?.sha256 || null,
    evidenceLevel,
    journeys: journeyIdentities,
    runKind: retentionKind,
    createdAt: new Date().toISOString(),
    expiresAt: expiresAt(run.completedAt || run.startedAt, days),
    retentionDays: days,
    pii: manifest.artifacts?.pii || "unknown",
    nonce: nonce.toString("base64"),
    keyFingerprintSha256: sha256(key),
    contentIndexSha256,
    secretScan: {
      passed: secretScan.passed,
      scannedFiles: secretScan.scannedFiles,
      skippedBinary: secretScan.skippedBinary,
    },
    files: entries.length,
    plaintextBytes: entries.reduce((sum, entry) => sum + entry.bytes, 0),
  };
  const { prefix } = encodedHeader(header);
  const cipher = createCipheriv("aes-256-gcm", key, nonce);
  cipher.setAAD(prefix);
  const fd = openSync(temporary, "wx", 0o600);
  try {
    writeBuffer(fd, prefix);
    writeBuffer(fd, cipher.update(indexSize));
    writeBuffer(fd, cipher.update(index));
    for (let position = 0; position < sourceFiles.length; position += 1) {
      for await (const chunk of createReadStream(sourceFiles[position])) writeBuffer(fd, cipher.update(chunk));
    }
    writeBuffer(fd, cipher.final());
    writeBuffer(fd, cipher.getAuthTag());
  } catch (error) {
    closeSync(fd);
    rmSync(temporary, { force: true });
    throw error;
  }
  closeSync(fd);
  renameSync(temporary, destination);
  const protectedArtifact = {
    file: destination,
    bytes: statSync(destination).size,
    sha256: await sha256File(destination),
    contentIndexSha256: header.contentIndexSha256,
    keyFingerprintSha256: header.keyFingerprintSha256,
    expiresAt: header.expiresAt,
    retentionDays: days,
    files: entries.length,
    secretScan: header.secretScan,
    plaintextRemoved: Boolean(removePlaintext),
  };
  if (removePlaintext) {
    removePlaintextSource({ source, manifestPath: run.manifestPath, retentionKind, protectedArtifact });
  }
  return protectedArtifact;
}

export async function protectRun(options = {}) {
  try {
    const result = await protectRunImpl(options);
    auditAccess({
      action: "artifact.protect",
      outcome: "allowed",
      appId: options.appId,
      runId: options.runId,
      resource: result.file,
      details: { removePlaintext: Boolean(options.removePlaintext), bundleSha256: result.sha256 },
    });
    return result;
  } catch (error) {
    auditAccess({
      action: "artifact.protect",
      outcome: "denied",
      appId: options.appId || null,
      runId: options.runId || null,
      details: { error: error instanceof Error ? error.message : String(error) },
    });
    throw error;
  }
  return protectedArtifact;
}

function safeMember(destination, member) {
  if (!member || path.isAbsolute(member) || member.split("/").includes("..")) throw new Error(`unsafe evidence member: ${member}`);
  const resolved = path.resolve(destination, member);
  const root = path.resolve(destination);
  if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) throw new Error(`unsafe evidence member: ${member}`);
  return resolved;
}

async function copyRange(source, start, bytes, destination, mode) {
  mkdirSync(path.dirname(destination), { recursive: true, mode: 0o700 });
  const output = createWriteStream(destination, { flags: "wx", mode: mode || 0o600 });
  if (bytes > 0) createReadStream(source, { start, end: start + bytes - 1 }).pipe(output);
  else output.end();
  await finished(output);
}

async function restoreBundleImpl({ file, destination, keyFile } = {}) {
  if (!file || !destination) throw new Error("encrypted bundle file and destination are required");
  if (existsSync(destination) && readdirSync(destination).length) throw new Error("restore destination must be empty");
  mkdirSync(destination, { recursive: true, mode: 0o700 });
  const key = keyFromFile(keyFile || process.env.PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE);
  const { header, offset, aad } = readHeader(file);
  if (header.algorithm !== "AES-256-GCM") throw new Error(`unsupported evidence algorithm: ${header.algorithm}`);
  if (sha256(key) !== header.keyFingerprintSha256) throw new Error("artifact encryption key fingerprint mismatch");
  const size = statSync(file).size;
  if (size < offset + TAG_BYTES) throw new Error("truncated encrypted evidence bundle");
  const tag = Buffer.alloc(TAG_BYTES);
  const tagFd = openSync(file, "r");
  try { readSync(tagFd, tag, 0, TAG_BYTES, size - TAG_BYTES); }
  finally { closeSync(tagFd); }
  const payload = path.join(path.dirname(destination), `.probierz-restore-${process.pid}-${Date.now()}`);
  const payloadFd = openSync(payload, "wx", 0o600);
  const decipher = createDecipheriv("aes-256-gcm", key, Buffer.from(header.nonce, "base64"));
  decipher.setAAD(aad);
  decipher.setAuthTag(tag);
  try {
    for await (const chunk of createReadStream(file, { start: offset, end: size - TAG_BYTES - 1 })) {
      writeBuffer(payloadFd, decipher.update(chunk));
    }
    writeBuffer(payloadFd, decipher.final());
  } catch (error) {
    closeSync(payloadFd);
    rmSync(payload, { force: true });
    throw new Error(`encrypted evidence authentication failed: ${error instanceof Error ? error.message : String(error)}`);
  }
  closeSync(payloadFd);
  try {
    const indexSize = Buffer.alloc(4);
    const payloadFdRead = openSync(payload, "r");
    try {
      if (readSync(payloadFdRead, indexSize, 0, 4, 0) !== 4) throw new Error("truncated evidence index");
      const length = indexSize.readUInt32BE(0);
      if (length <= 0 || length > 64 * 1024 * 1024) throw new Error("invalid evidence index length");
      const indexBuffer = Buffer.alloc(length);
      if (readSync(payloadFdRead, indexBuffer, 0, length, 4) !== length) throw new Error("truncated evidence index");
      const index = JSON.parse(indexBuffer.toString("utf8"));
      let cursor = 4 + length;
      for (const entry of index.files || []) {
        const output = safeMember(destination, entry.file);
        await copyRange(payload, cursor, Number(entry.bytes), output, Number(entry.mode));
        if (await sha256File(output) !== entry.sha256) throw new Error(`restored evidence hash mismatch: ${entry.file}`);
        cursor += Number(entry.bytes);
      }
      if (cursor !== statSync(payload).size) throw new Error("encrypted evidence payload has trailing or missing bytes");
      return { appId: header.appId, runId: header.runId, destination, files: index.files.length, authenticated: true };
    } finally {
      closeSync(payloadFdRead);
    }
  } finally {
    rmSync(payload, { force: true });
  }
}

export async function restoreBundle(options = {}) {
  try {
    const result = await restoreBundleImpl(options);
    auditAccess({
      action: "artifact.restore",
      outcome: "allowed",
      appId: result.appId,
      runId: result.runId,
      resource: options.file || null,
      details: { destination: result.destination, files: result.files },
    });
    return result;
  } catch (error) {
    auditAccess({
      action: "artifact.restore",
      outcome: "denied",
      resource: options.file || null,
      details: { destination: options.destination || null, error: error instanceof Error ? error.message : String(error) },
    });
    throw error;
  }
}

function manifestsBelow(root) {
  if (!existsSync(root)) return [];
  const pending = [root];
  const manifests = [];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) pending.push(file);
      else if (entry.isFile() && entry.name === "run-manifest.json") manifests.push(file);
    }
  }
  return manifests;
}

export function retentionPlan({ appId, now = new Date() } = {}) {
  const manifest = loadAppManifest(appId);
  const at = now instanceof Date ? now : new Date(now);
  if (!Number.isFinite(at.getTime())) throw new Error("invalid retention time");
  const items = manifestsBelow(path.join(RESULTS, appId)).map((file) => {
    const run = JSON.parse(readFileSync(file, "utf8"));
    const kind = run.kind || "adhoc";
    const days = retentionDays(manifest, kind);
    const expiry = expiresAt(run.completedAt || run.startedAt, days);
    return { type: "run", appId, runId: run.runId, kind, path: path.dirname(file), expiresAt: expiry, expired: Date.parse(expiry) <= at.getTime() };
  });
  const protectedRoot = path.join(PROTECTED, appId);
  if (existsSync(protectedRoot)) {
    for (const file of filesBelow(protectedRoot).filter((candidate) => candidate.endsWith(".pev"))) {
      const { header } = readHeader(file);
      items.push({ type: "protected", appId, runId: header.runId, kind: header.runKind, path: file, expiresAt: header.expiresAt, expired: Date.parse(header.expiresAt) <= at.getTime() });
    }
  }
  items.sort((left, right) => left.expiresAt.localeCompare(right.expiresAt) || left.path.localeCompare(right.path));
  return { schemaVersion: 1, appId, at: at.toISOString(), expired: items.filter((item) => item.expired).length, items };
}

export function enforceRetention({ appId, now = new Date(), apply = false } = {}) {
  const plan = retentionPlan({ appId, now });
  const removed = [];
  if (apply) {
    for (const item of plan.items.filter((candidate) => candidate.expired)) {
      rmSync(item.path, { recursive: item.type === "run", force: true });
      removed.push(item.path);
    }
  }
  auditAccess({
    action: apply ? "retention.apply" : "retention.plan",
    outcome: "allowed",
    appId,
    details: { expired: plan.expired, removed: removed.length, at: plan.at },
  });
  return { ...plan, applied: Boolean(apply), removed };
}
