import { createReadStream, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { loadAppManifest } from "./apps.mjs";
import { getRun } from "./history.mjs";
import { planMatrix } from "./matrix.mjs";
import { signedReceiptRun, verifyReceipt } from "./receipt.mjs";
import { auditAccess } from "./security.mjs";

const LEVEL = new Map(["E0", "E1", "E2", "E3"].map((value, index) => [value, index]));

function evidenceLevel(run) {
  if (run.status !== "passed") return "E0";
  if (run.conditions?.record && run.evidence?.report && run.evidence?.analysis && run.evidence?.capturePresent) return "E3";
  return "E2";
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).filter((key) => value[key] !== undefined).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}


function sha256File(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const input = createReadStream(file);
    input.on("error", reject);
    input.on("data", (chunk) => hash.update(chunk));
    input.on("end", () => resolve(hash.digest("hex")));
  });
}

function configFile(manifest) {
  return path.join(path.dirname(manifest.file), "gates.json");
}

function defaultConfig(appId) {
  return {
    schemaVersion: 2,
    appId,
    modes: {
      "pull-request": { enforcement: "pending-green" },
      release: { enforcement: "pending-green" },
    },
  };
}

export function gateStatus(appId) {
  const manifest = loadAppManifest(appId);
  const file = configFile(manifest);
  const config = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : defaultConfig(appId);
  return { ...config, file, exists: existsSync(file) };
}

function sameSet(left, right) {
  const a = [...new Set(left)].sort();
  const b = [...new Set(right)].sort();
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

function matrixCoverage({ appId, profile, runs }) {
  const plan = planMatrix({ appId, profile });
  const remaining = [...runs];
  const missing = [];
  for (const cell of plan.cells) {
    const index = remaining.findIndex((run) => run.target === cell.target
      && Object.entries(cell.axes).every(([name, value]) => String(run.conditions?.[name]) === String(value)));
    if (index < 0) missing.push({ cellId: cell.cellId, target: cell.target, axes: cell.axes });
    else remaining.splice(index, 1);
  }
  return { profile, expected: plan.cells.length, matched: plan.cells.length - missing.length, missing, extraRunIds: remaining.map((run) => run.runId) };
}

export async function evaluateGate({
  appId,
  mode,
  expectedHarnessSha,
  expectedSourceSha,
  runIds = [],
  release,
  receiptFile,
  trustedPublicKeyFile,
  expectedFingerprint,
} = {}) {
  if (!appId || !["pull-request", "release"].includes(mode)) throw new Error("gate needs an app ID and pull-request or release mode");
  const manifest = loadAppManifest(appId);
  const policy = mode === "release" ? manifest.releasePolicy : (manifest.pullRequestPolicy || {});
  const minimumEvidence = policy.minimumEvidence || "E3";
  const requiredRank = LEVEL.get(minimumEvidence);
  if (!Number.isInteger(requiredRank)) throw new Error(`unsupported gate evidence level: ${minimumEvidence}`);
  const errors = [];
  if (!expectedHarnessSha) errors.push("expected harness source SHA-256 is required");
  if (!expectedSourceSha) errors.push("expected app source SHA-256 is required");
  if (!Array.isArray(runIds) || runIds.length === 0) errors.push("at least one run ID is required");
  if (Array.isArray(runIds) && new Set(runIds).size !== runIds.length) errors.push("run IDs must be unique");
  const runs = [];
  for (const runId of [...new Set(runIds)]) {
    try { runs.push(getRun(appId, runId)); }
    catch (error) { errors.push(error instanceof Error ? error.message : String(error)); }
  }
  const bundleHashes = {};
  for (const run of runs) {
    if (run.status !== "passed") errors.push(`${run.runId}: status is ${run.status}`);
    if (!run.harness?.sha256 || !run.harness.gitSha || !run.harness.worktreeSha256) {
      errors.push(`${run.runId}: complete harness source identity is missing`);
    } else if (expectedHarnessSha && run.harness.sha256 !== expectedHarnessSha) {
      errors.push(`${run.runId}: harness source ${run.harness.sha256} does not match ${expectedHarnessSha}`);
    }
    if (!run.build?.sha256) errors.push(`${run.runId}: exact build hash is missing`);
    if ((LEVEL.get(evidenceLevel(run)) ?? -1) < requiredRank) errors.push(`${run.runId}: ${evidenceLevel(run)} is below ${minimumEvidence}`);
    if (!run.source?.sha256 || !Array.isArray(run.source.repositories)
      || run.source.repositories.some((repository) => !repository.gitSha || !repository.worktreeSha256)) {
      errors.push(`${run.runId}: complete app source identity is missing`);
    }
    if (expectedSourceSha && run.source?.sha256 !== expectedSourceSha) {
      errors.push(`${run.runId}: app source ${run.source?.sha256 || "missing"} does not match ${expectedSourceSha}`);
    }
    if (requiredRank >= LEVEL.get("E3") && (!run.artifacts.length || run.artifacts.some((artifact) => !artifact.sha256))) {
      errors.push(`${run.runId}: E3 artifact hashes are incomplete`);
    }
    if (!run.protection?.plaintextRemoved) {
      const artifactRoot = path.dirname(run.manifestPath);
      for (const artifact of run.artifacts) {
        const file = path.resolve(artifactRoot, artifact.file);
        if (file !== artifactRoot && !file.startsWith(`${artifactRoot}${path.sep}`)) {
          errors.push(`${run.runId}: artifact path escapes its run: ${artifact.file}`);
        } else if (!existsSync(file)) {
          errors.push(`${run.runId}: artifact is missing: ${artifact.file}`);
        } else {
          try {
            if (await sha256File(file) !== artifact.sha256) errors.push(`${run.runId}: artifact hash mismatch: ${artifact.file}`);
          } catch (error) {
            errors.push(`${run.runId}: artifact cannot be hashed: ${artifact.file} (${error instanceof Error ? error.message : String(error)})`);
          }
        }
      }
    }
    if (run.kind !== mode) errors.push(`${run.runId}: run kind ${run.kind} is not ${mode}`);
    if (mode === "release" && release && run.conditions?.PROBIERZ_RELEASE !== release) errors.push(`${run.runId}: release condition does not match ${release}`);
    if (policy.requireProtectedArtifacts || run.protection?.plaintextRemoved) {
      if (!run.protection?.plaintextRemoved || !run.protection?.file || !existsSync(run.protection.file)) {
        errors.push(`${run.runId}: encrypted-at-rest artifact bundle is missing`);
      } else {
        try {
          bundleHashes[run.runId] = await sha256File(run.protection.file);
          if (bundleHashes[run.runId] !== run.protection.sha256) errors.push(`${run.runId}: encrypted bundle hash does not match its manifest`);
        } catch (error) {
          errors.push(`${run.runId}: encrypted bundle cannot be hashed: ${error instanceof Error ? error.message : String(error)}`);
        }
      }
    }
    if (policy.requireSecretScan && !run.protection?.secretScan?.passed) errors.push(`${run.runId}: passing pre-upload secret scan is missing`);
  }
  const sourceHashes = [...new Set(runs.map((run) => run.source?.sha256).filter(Boolean))];
  if (runs.length && sourceHashes.length !== 1) errors.push(`runs do not identify one exact app source (${sourceHashes.length} source hashes)`);
  const builds = {};
  for (const run of runs) {
    const hash = run.build?.sha256;
    if (!hash) continue;
    if (builds[run.target] && builds[run.target] !== hash) {
      errors.push(`${run.target}: runs do not identify one exact build`);
    } else {
      builds[run.target] = hash;
    }
  }
  const requiredTargets = policy.requiredTargets || [];
  for (const target of requiredTargets) {
    if (!runs.some((run) => run.target === target)) errors.push(`required target is missing: ${target}`);
  }
  const requiredJourneys = policy.requiredJourneys || [];
  for (const journey of requiredJourneys) {
    if (!runs.some((run) => run.journeys.includes(journey))) errors.push(`required journey is missing: ${journey}`);
  }
  let matrix = null;
  if (policy.requiredMatrixProfile) {
    matrix = matrixCoverage({ appId, profile: policy.requiredMatrixProfile, runs });
    if (matrix.missing.length) errors.push(`${matrix.missing.length} required matrix cell(s) are missing`);
    if (matrix.extraRunIds.length) errors.push(`${matrix.extraRunIds.length} run(s) are outside the required matrix`);
  }
  let receipt = null;
  if (mode === "release") {
    if (!release) errors.push("release ID is required");
    if (!receiptFile) errors.push("signed receipt is required");
    else {
      try {
        receipt = verifyReceipt(receiptFile, { trustedPublicKeyFile, expectedFingerprint });
        if (!receipt.valid) errors.push("receipt signature, trust, or payload hash is invalid");
        if (receipt.appId !== appId) errors.push(`receipt app ID ${receipt.appId} does not match ${appId}`);
        if (receipt.release !== release) errors.push(`receipt release ${receipt.release} does not match ${release}`);
        if (receipt.expectedHarnessSha !== expectedHarnessSha) errors.push("receipt harness source SHA-256 does not match");
        if (receipt.expectedSourceSha !== expectedSourceSha) errors.push("receipt app source SHA-256 does not match");
        if (canonical(receipt.builds) !== canonical(builds)) errors.push("receipt build identities do not match");
        if (!sameSet(receipt.runIds, runIds)) errors.push("receipt run IDs do not match gate run IDs");
        if (!receipt.verdict?.passed) errors.push("receipt verdict is not passed");
        for (const run of runs) {
          const signed = receipt.runs.find((candidate) => candidate.runId === run.runId);
          if (!signed || canonical(signed) !== canonical(signedReceiptRun(run, manifest))) {
            errors.push(`${run.runId}: local policy evidence differs from the signed receipt`);
            continue;
          }
          if (policy.requireProtectedArtifacts && bundleHashes[run.runId] !== signed.protection?.sha256) {
            errors.push(`${run.runId}: encrypted bundle does not match the signed receipt`);
          }
        }
      } catch (error) {
        errors.push(`receipt verification failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  }
  const result = {
    schemaVersion: 2,
    appId,
    mode,
    release: release || null,
    expectedHarnessSha: expectedHarnessSha || null,
    expectedSourceSha: expectedSourceSha || null,
    policy: {
      minimumEvidence,
      requiredTargets,
      requiredJourneys,
      requiredMatrixProfile: policy.requiredMatrixProfile || null,
      requireProtectedArtifacts: Boolean(policy.requireProtectedArtifacts),
      requireSecretScan: Boolean(policy.requireSecretScan),
    },
    verdict: { passed: errors.length === 0, errors },
    evidence: {
      runIds: runs.map((run) => run.runId),
      builds,
      harnessSha256: runs.length > 0 && runs.every((run) => run.harness?.sha256 === expectedHarnessSha)
        ? expectedHarnessSha
        : null,
      sourceSha256: sourceHashes.length === 1 ? sourceHashes[0] : null,
      levels: Object.fromEntries(runs.map((run) => [run.runId, evidenceLevel(run)])),
      matrix,
      receipt,
    },
  };
  auditAccess({
    action: "gate.evaluate",
    outcome: result.verdict.passed ? "allowed" : "denied",
    appId,
    resource: mode,
    details: {
      release: release || null,
      expectedHarnessSha: expectedHarnessSha || null,
      expectedSourceSha: expectedSourceSha || null,
      runs: runs.length,
      errors: errors.length,
    },
  });
  return result;
}

export async function activateGate(options = {}) {
  const evaluation = await evaluateGate(options);
  if (!evaluation.verdict.passed) {
    const error = new Error(`gate activation refused: ${evaluation.verdict.errors.join("; ")}`);
    error.code = "PROBIERZ_GATE_NOT_GREEN";
    error.evaluation = evaluation;
    throw error;
  }
  const manifest = loadAppManifest(options.appId);
  const file = configFile(manifest);
  const current = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : defaultConfig(options.appId);
  const activatedAt = new Date().toISOString();
  const next = {
    ...current,
    schemaVersion: 2,
    modes: {
      ...current.modes,
      [options.mode]: {
        enforcement: "required",
        activatedAt,
        activationEvidence: {
          expectedHarnessSha: evaluation.expectedHarnessSha,
          expectedSourceSha: evaluation.expectedSourceSha,
          release: evaluation.release,
          runIds: evaluation.evidence.runIds,
          builds: evaluation.evidence.builds,
          harnessSha256: evaluation.evidence.harnessSha256,
          sourceSha256: evaluation.evidence.sourceSha256,
          receiptFingerprint: evaluation.evidence.receipt?.fingerprint || null,
        },
      },
    },
  };
  mkdirSync(path.dirname(file), { recursive: true });
  const temporary = `${file}.tmp-${process.pid}-${Date.now()}`;
  writeFileSync(temporary, `${JSON.stringify(next, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  renameSync(temporary, file);
  auditAccess({
    action: "gate.activate",
    outcome: "allowed",
    appId: options.appId,
    resource: options.mode,
    details: {
      expectedHarnessSha: options.expectedHarnessSha,
      expectedSourceSha: options.expectedSourceSha,
      release: options.release || null,
    },
  });
  return { file, config: next, evaluation };
}

export async function enforceGate(options = {}) {
  const status = gateStatus(options.appId);
  const mode = status.modes?.[options.mode];
  if (mode?.enforcement !== "required") {
    const result = {
      schemaVersion: 2,
      appId: options.appId,
      mode: options.mode,
      verdict: { passed: false, errors: ["gate is pending green activation"] },
      status,
    };
    auditAccess({ action: "gate.enforce", outcome: "denied", appId: options.appId, resource: options.mode, details: { reason: "pending-green" } });
    return result;
  }
  const evaluation = await evaluateGate(options);
  auditAccess({ action: "gate.enforce", outcome: evaluation.verdict.passed ? "allowed" : "denied", appId: options.appId, resource: options.mode, details: { errors: evaluation.verdict.errors.length } });
  return { ...evaluation, status };
}
