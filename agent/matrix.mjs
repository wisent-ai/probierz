import { createHash } from "node:crypto";
import { loadAppManifest, surfaceJourneys } from "./apps.mjs";
import { protectRun } from "./artifacts.mjs";
import { analyzeRun } from "./analyze.mjs";
import { resourcesFor } from "./locks.mjs";
import { scheduleByResources } from "./orchestrate.mjs";
import { completeRun, runSurface } from "./runner.mjs";

const LEVEL = new Map(["E0", "E1", "E2", "E3"].map((value, index) => [value, index]));
const SENSITIVE_KEY = /(auth|cookie|credential|email|key|otp|password|pii|secret|session|token)/i;

function publicCell(cell) {
  return {
    ...cell,
    env: Object.fromEntries(Object.entries(cell.env).map(([name, value]) => [
      name,
      SENSITIVE_KEY.test(name) ? "[REDACTED]" : value,
    ])),
  };
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function cellId(target, env) {
  return createHash("sha256").update(JSON.stringify(stable({ target, env }))).digest("hex").slice(0, 16);
}

function expand(dimensions) {
  let cells = [{}];
  for (const [name, values] of Object.entries(dimensions || {}).sort(([left], [right]) => left.localeCompare(right))) {
    cells = cells.flatMap((cell) => values.map((value) => ({ ...cell, [name]: String(value) })));
  }
  return cells;
}

function runEvidenceLevel(run) {
  if (!run?.passed) return "E0";
  const evidence = run.evidence || {};
  if (run.conditions?.record && evidence.report && evidence.analysis && evidence.capturePresent) return "E3";
  return "E2";
}

export function planMatrix({ appId, profile = "nightly" } = {}) {
  const manifest = loadAppManifest(appId);
  const policy = manifest.matrix?.[profile];
  if (!policy) throw new Error(`app ${appId} has no ${profile} matrix`);
  const targets = policy.targets || Object.keys(manifest.surfaces);
  const cells = [];
  for (const target of [...targets].sort()) {
    const surface = manifest.surfaces[target];
    if (!surface) throw new Error(`matrix ${profile} references unknown target: ${target}`);
    const dimensions = { ...(policy.dimensions || {}), ...(policy.surfaces?.[target]?.dimensions || {}) };
    for (const env of expand(dimensions)) {
      const conditions = { ...(surface.conditions || {}), ...env };
      cells.push({
        index: cells.length,
        cellId: cellId(target, conditions),
        target,
        spec: policy.surfaces?.[target]?.spec || surface.spec,
        journeys: [...surfaceJourneys(surface, conditions)].sort(),
        axes: env,
        env: conditions,
      });
    }
  }
  const maxCells = Number(policy.maxCells || 128);
  if (cells.length > maxCells) throw new Error(`matrix ${profile} expands to ${cells.length} cells (max ${maxCells})`);
  return {
    schemaVersion: 1,
    appId,
    owner: manifest.owner,
    profile,
    record: policy.record !== false,
    frames: Number(policy.frames || 0),
    timeoutMs: Number(policy.timeoutMs || 0),
    resourceWaitMs: Number(policy.resourceWaitMs ?? 10 * 60 * 1000),
    maximumParallel: Math.max(1, Number(policy.maximumParallel || 4)),
    minimumCellEvidence: policy.minimumCellEvidence || "E3",
    artifactEncryption: policy.artifactEncryption || "optional",
    removePlaintextAfterProtection: Boolean(policy.removePlaintextAfterProtection),
    release: policy.release || null,
    cells,
  };
}

export async function runMatrix({ appId, profile = "nightly", env = {}, release } = {}) {
  const plan = planMatrix({ appId, profile });
  const effectiveRelease = release || plan.release || null;
  if (profile === "release" && !effectiveRelease) throw new Error("release matrix execution needs a release ID");
  const artifactKeyFile = env.PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE
    || process.env.PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE;
  if (plan.artifactEncryption === "required" && !artifactKeyFile) {
    throw new Error("matrix requires PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE");
  }
  const executionEnv = { ...env };
  delete executionEnv.PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE;
  for (const cell of plan.cells) {
    const conflict = Object.keys(executionEnv).find((name) => name in cell.axes && String(executionEnv[name]) !== cell.axes[name]);
    if (conflict) throw new Error(`matrix axis ${conflict} cannot be overridden`);
  }
  const cells = plan.cells.map((cell) => ({
    ...cell,
    env: { ...cell.env, ...executionEnv, ...(effectiveRelease ? { PROBIERZ_RELEASE: effectiveRelease } : {}) },
  }));
  const protectionFor = async (run) => {
    if (!artifactKeyFile) return { artifact: null, error: null };
    try {
      return {
        artifact: await protectRun({
          appId,
          runId: run.runId,
          kind: profile,
          keyFile: artifactKeyFile,
          removePlaintext: plan.removePlaintextAfterProtection,
        }),
        error: null,
      };
    } catch (error) {
      return { artifact: null, error: error instanceof Error ? error.message : String(error) };
    }
  };
  const results = await scheduleByResources(
    cells,
    (cell) => [...resourcesFor(cell.target, cell.env), `matrix-slot:${cell.index % plan.maximumParallel}`],
    async (cell) => {
      const run = await runSurface(cell.target, {
        appId,
        kind: profile,
        env: cell.env,
        record: plan.record,
        timeoutMs: plan.timeoutMs,
        resourceWaitMs: plan.resourceWaitMs,
        spec: cell.spec,
      });
      if (run.skipped || run.canceled) {
        const protection = await protectionFor(run);
        return {
          ...publicCell(cell),
          status: run.canceled ? "canceled" : "blocked",
          run,
          protectedArtifact: protection.artifact,
          protectionError: protection.error,
        };
      }
      let analysis;
      let analysisError;
      try {
        analysis = analyzeRun({
          reportPath: run.reportPath,
          artifactsDir: run.artifactsDir,
          tool: run.tool,
          frames: plan.frames,
          runId: run.runId,
        });
      } catch (error) {
        analysisError = error;
        analysis = { error: error instanceof Error ? error.message : String(error) };
      }
      const completed = completeRun(run, analysisError ? null : analysis, analysisError);
      const protection = await protectionFor(completed);
      return {
        ...publicCell(cell),
        status: completed.passed && !protection.error ? "passed" : "failed",
        evidenceLevel: runEvidenceLevel(completed),
        run: completed,
        analysis,
        protectedArtifact: protection.artifact,
        protectionError: protection.error,
      };
    },
  );
  const count = (status) => results.filter((result) => result.status === status).length;
  const requiredRank = LEVEL.get(plan.minimumCellEvidence);
  const evidenceSatisfied = Number.isInteger(requiredRank)
    && results.every((result) => (LEVEL.get(result.evidenceLevel) ?? -1) >= requiredRank);
  const passed = count("passed") === results.length && evidenceSatisfied;
  return {
    schemaVersion: 1,
    appId,
    profile,
    release: effectiveRelease,
    generatedAt: new Date().toISOString(),
    verdict: {
      passed,
      evidenceLevel: passed ? "E4" : null,
      minimumCellEvidence: plan.minimumCellEvidence,
      evidenceSatisfied,
    },
    summary: {
      total: results.length,
      passed: count("passed"),
      failed: count("failed"),
      blocked: count("blocked"),
      canceled: count("canceled"),
    },
    results,
  };
}
