import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { analyzeRun } from "./analyze.mjs";
import { completeRun, runSurface } from "./runner.mjs";
import { auditAccess } from "./security.mjs";
import { repairFailedRun } from "./repair.mjs";

const jobs = new Map();
const MAX_ARTIFACT_BYTES = 5 * 1024 * 1024;

function newRunId() {
  return `${new Date().toISOString().replace(/[:.]/g, "-")}-${randomUUID()}`;
}

function publicJob(job) {
  return {
    runId: job.runId,
    status: job.status,
    target: job.target,
    appId: job.appId,
    spec: job.spec,
    record: job.record,
    createdAt: job.createdAt,
    startedAt: job.startedAt,
    completedAt: job.completedAt,
    error: job.error,
    artifactsDir: job.result?.artifactsDir || null,
  };
}

async function execute(job, options) {
  job.status = "running";
  job.startedAt = new Date().toISOString();
  try {
    const result = await runSurface(job.target, {
      ...options,
      runId: job.runId,
      signal: job.controller.signal,
    });
    if (result.skipped) {
      job.result = result;
      job.status = "blocked";
      return;
    }
    let analysis = null;
    let analysisError = null;
    if (options.analyze !== false && !result.canceled) {
      try {
        analysis = analyzeRun({
          reportPath: result.reportPath,
          artifactsDir: result.artifactsDir,
          tool: result.tool,
          frames: Number(options.frames) || 0,
          runId: result.runId,
        });
      } catch (error) {
        analysisError = error;
        analysis = { error: error instanceof Error ? error.message : String(error) };
      }
    }
    const completed = options.analyze === false || result.canceled
      ? result
      : completeRun(result, analysisError ? null : analysis, analysisError);
    job.result = { ...completed, analysis };
    job.status = result.canceled
      ? "canceled"
      : (completed.passed ? "passed" : "failed");
    if (job.status === "failed" && options.noRepair !== true && !process.env.PROBIERZ_REPAIR_SUPPRESS) {
      job.result.repair = await repairFailedRun({
        appId: completed.appId,
        runId: completed.runId,
        rounds: Number("1"),
      });
    }
  } catch (error) {
    job.error = error instanceof Error ? error.message : String(error);
    job.status = job.controller.signal.aborted ? "canceled" : "failed";
  } finally {
    job.completedAt = new Date().toISOString();
  }
}

export function startRun(options = {}) {
  if (typeof options.target !== "string" || !options.target) throw new Error("target must be a non-empty string");
  const runId = newRunId();
  const job = {
    runId,
    status: "queued",
    target: options.target,
    appId: typeof options.appId === "string" ? options.appId : "probierz",
    spec: typeof options.spec === "string" ? options.spec : null,
    record: Boolean(options.record),
    createdAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    error: null,
    result: null,
    controller: new AbortController(),
    promise: null,
  };
  jobs.set(runId, job);
  job.promise = Promise.resolve().then(() => execute(job, {
    appId: options.appId,
    env: options.env && typeof options.env === "object" ? options.env : {},
    record: Boolean(options.record),
    timeoutMs: Number(options.timeoutMs) || 0,
    resourceWaitMs: options.resourceWaitMs === undefined ? undefined : Number(options.resourceWaitMs),
    force: Boolean(options.force),
    spec: typeof options.spec === "string" ? options.spec : undefined,
    frames: Number(options.frames) || 0,
    analyze: options.analyze !== false,
    noRepair: options.noRepair === true,
  }));
  return publicJob(job);
}

function requireJob(runId) {
  const job = jobs.get(runId);
  if (!job) throw new Error(`unknown runId: ${runId}`);
  return job;
}

export function runStatus(runId) {
  return publicJob(requireJob(runId));
}

export function cancelRun(runId) {
  const job = requireJob(runId);
  if (["passed", "failed", "blocked", "canceled"].includes(job.status)) {
    return { ...publicJob(job), cancelRequested: false };
  }
  job.controller.abort();
  return { ...publicJob(job), cancelRequested: true };
}

export function getResult(runId) {
  const job = requireJob(runId);
  return { ...publicJob(job), result: job.result };
}

function artifactRoot(job) {
  const root = job.result?.artifactsDir;
  if (!root || !existsSync(root)) throw new Error(`artifacts unavailable for runId: ${job.runId}`);
  return path.resolve(root);
}

export function listArtifacts(runId) {
  const job = requireJob(runId);
  try {
    const root = artifactRoot(job);
    const pending = [root];
    const artifacts = [];
    while (pending.length) {
      const directory = pending.pop();
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) pending.push(file);
        else if (entry.isFile()) {
          const stat = lstatSync(file);
          artifacts.push({ file: path.relative(root, file), bytes: stat.size });
        }
      }
    }
    const result = { runId, artifacts: artifacts.sort((left, right) => left.file.localeCompare(right.file)) };
    auditAccess({ action: "artifact.list", appId: job.appId, runId, resource: root, details: { files: artifacts.length } });
    return result;
  } catch (error) {
    auditAccess({ action: "artifact.list", outcome: "denied", appId: job.appId, runId, details: { error: error instanceof Error ? error.message : String(error) } });
    throw error;
  }
}

export function getArtifact(runId, relativeFile) {
  const job = requireJob(runId);
  try {
    if (typeof relativeFile !== "string" || !relativeFile) throw new Error("file must be a non-empty relative path");
    const root = artifactRoot(job);
    const file = path.resolve(root, relativeFile);
    if (file !== root && !file.startsWith(`${root}${path.sep}`)) throw new Error("artifact path escapes the run directory");
    const stat = lstatSync(file);
    if (!stat.isFile()) throw new Error(`artifact is not a file: ${relativeFile}`);
    if (stat.size > MAX_ARTIFACT_BYTES) throw new Error(`artifact exceeds ${MAX_ARTIFACT_BYTES} byte inline limit`);
    const result = { runId, file: path.relative(root, file), bytes: stat.size, encoding: "base64", content: readFileSync(file).toString("base64") };
    auditAccess({ action: "artifact.read", appId: job.appId, runId, resource: result.file, details: { bytes: stat.size } });
    return result;
  } catch (error) {
    auditAccess({ action: "artifact.read", outcome: "denied", appId: job.appId, runId, resource: relativeFile || null, details: { error: error instanceof Error ? error.message : String(error) } });
    throw error;
  }
}
