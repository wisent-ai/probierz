import { createHash, randomUUID } from "node:crypto";
import {
  createReadStream,
  existsSync,
  mkdirSync,
  openSync,
  closeSync,
  readFileSync,
  readSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import readline from "node:readline";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const AUDIT_ROOT = path.join(ROOT, "test-results", ".audit");
const SENSITIVE_KEY = /(auth|cookie|credential|email|key|otp|password|pii|secret|session|token)/i;
const RULES = [
  { id: "private-key", regex: /-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----/g },
  { id: "aws-access-key", regex: /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/g },
  { id: "github-token", regex: /\bgh[pousr]_[A-Za-z0-9]{30,}\b/g },
  { id: "slack-token", regex: /\bxox[baprs]-[A-Za-z0-9-]{20,}\b/g },
  { id: "jwt", regex: /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b/g },
  { id: "assigned-secret", regex: /(?:token|secret|password|api[_-]?key|authorization|cookie)["']?\s*[:=]\s*["']?([A-Za-z0-9+/_=.:-]{8,})/gi, capture: 1 },
];

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function redact(value, key = "") {
  if (SENSITIVE_KEY.test(key)) return "[REDACTED]";
  if (Array.isArray(value)) return value.map((item) => redact(item));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([name, item]) => [name, redact(item, name)]));
  }
  return value;
}

function actor() {
  return process.env.PROBIERZ_ACTOR || process.env.GITHUB_ACTOR || process.env.USER || "unknown";
}

export function auditAccess({ action, outcome = "allowed", appId = null, runId = null, resource = null, details = {} } = {}) {
  if (!action) throw new Error("audit action is required");
  const at = new Date().toISOString();
  const eventId = randomUUID();
  const payload = {
    schemaVersion: 1,
    kind: "probierz-access-audit",
    eventId,
    at,
    actor: actor(),
    action,
    outcome,
    appId,
    runId,
    resource,
    context: {
      ci: Boolean(process.env.CI),
      workflow: process.env.GITHUB_WORKFLOW || null,
      job: process.env.GITHUB_JOB || null,
    },
    details: redact(details),
  };
  const record = { ...payload, sha256: sha256(JSON.stringify(stable(payload))) };
  const directory = path.join(AUDIT_ROOT, at.slice(0, 10));
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const file = path.join(directory, `${at.replace(/[:.]/g, "-")}-${eventId}.json`);
  writeFileSync(file, `${JSON.stringify(record, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  return { eventId, file, at, sha256: record.sha256 };
}

function auditFiles(root) {
  if (!existsSync(root)) return [];
  const pending = [root];
  const files = [];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) pending.push(file);
      else if (entry.isFile() && entry.name.endsWith(".json")) files.push(file);
    }
  }
  return files.sort();
}

export function auditTrail({ appId, runId, action, limit = 200 } = {}) {
  const records = [];
  for (const file of auditFiles(AUDIT_ROOT)) {
    try {
      const record = JSON.parse(readFileSync(file, "utf8"));
      const { sha256: expected, ...payload } = record;
      const valid = expected === sha256(JSON.stringify(stable(payload)));
      if (appId && record.appId !== appId) continue;
      if (runId && record.runId !== runId) continue;
      if (action && record.action !== action) continue;
      records.push({ ...record, valid, file });
    } catch (error) {
      records.push({ valid: false, file, error: error instanceof Error ? error.message : String(error) });
    }
  }
  records.sort((left, right) => String(right.at || "").localeCompare(String(left.at || "")));
  const selected = records.slice(0, Math.max(1, Number(limit) || 200));
  return {
    schemaVersion: 1,
    filters: { appId: appId || null, runId: runId || null, action: action || null },
    total: records.length,
    returned: selected.length,
    valid: selected.filter((record) => record.valid).length,
    invalid: selected.filter((record) => !record.valid).length,
    records: selected,
  };
}

function filesBelow(root) {
  const pending = [root];
  const files = [];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) pending.push(file);
      else if (entry.isFile()) files.push(file);
    }
  }
  return files.sort();
}

function binary(file) {
  const fd = openSync(file, "r");
  try {
    const sample = Buffer.alloc(Math.min(8192, statSync(file).size));
    const count = readSync(fd, sample, 0, sample.length, 0);
    return sample.subarray(0, count).includes(0);
  } finally {
    closeSync(fd);
  }
}

function acceptable(value) {
  return !value
    || value === "[REDACTED]"
    || value.startsWith("vault:")
    || value.startsWith("${")
    || /^(?:env|source|process\.env)\.[A-Za-z_][A-Za-z0-9_]*$/.test(value)
    || /^[A-Z][A-Z0-9_]+$/.test(value)
    || /^<[^>]+>$/.test(value);
}

function relativePath(root, file) {
  return path.relative(root, file).split(path.sep).join("/");
}

function generatedReportAsset(file) {
  return file.startsWith("html-report/trace/assets/");
}

export async function scanSecrets(root) {
  const absolute = path.resolve(root);
  if (!existsSync(absolute) || !statSync(absolute).isDirectory()) throw new Error(`secret scan root is not a directory: ${root}`);
  const findings = [];
  let scannedFiles = 0;
  let skippedBinary = 0;
  let skippedGenerated = 0;
  for (const file of filesBelow(absolute)) {
    const relative = relativePath(absolute, file);
    if (generatedReportAsset(relative)) {
      skippedGenerated += 1;
      continue;
    }
    if (binary(file)) {
      skippedBinary += 1;
      continue;
    }
    scannedFiles += 1;
    const input = readline.createInterface({ input: createReadStream(file, { encoding: "utf8" }), crlfDelay: Infinity });
    let lineNumber = 0;
    for await (const line of input) {
      lineNumber += 1;
      for (const rule of RULES) {
        rule.regex.lastIndex = 0;
        for (const match of line.matchAll(rule.regex)) {
          const value = String(match[rule.capture || 0] || "");
          if (acceptable(value)) continue;
          findings.push({
            rule: rule.id,
            file: relative,
            line: lineNumber,
            column: match.index + 1,
            fingerprintSha256: sha256(value),
          });
          if (findings.length >= 1000) break;
        }
        if (findings.length >= 1000) break;
      }
      if (findings.length >= 1000) break;
    }
    if (findings.length >= 1000) break;
  }
  return {
    schemaVersion: 1,
    kind: "probierz-secret-scan",
    root: absolute,
    scannedAt: new Date().toISOString(),
    scannedFiles,
    skippedBinary,
    skippedGenerated,
    passed: findings.length === 0,
    findings,
  };
}

export async function assertNoSecrets(root, reportFile = path.join(root, "diagnostics", "secret-scan.json")) {
  const result = await scanSecrets(root);
  mkdirSync(path.dirname(reportFile), { recursive: true, mode: 0o700 });
  writeFileSync(reportFile, `${JSON.stringify(result, null, 2)}\n`, { mode: 0o600 });
  if (!result.passed) {
    const error = new Error(`secret scan failed with ${result.findings.length} finding(s)`);
    error.code = "PROBIERZ_SECRET_SCAN_FAILED";
    error.result = result;
    throw error;
  }
  return result;
}
