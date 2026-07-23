// Change-driven test orchestration for the probierz toolkit. Composes the
// deterministic pieces into one CI-style pass: given a change, select the
// affected targets (affected.mjs), run each one preflight-gated (runner.mjs
// skips + reports the not-ready ones instead of failing opaquely), analyze what
// ran (analyze.mjs), and return a consolidated verdict. No LLM, no watching, no
// autonomy: it decides WHICH targets by structure and reports facts. Whether a
// failure is "real" and what to change about the code is brama's job, upstream.
import { affectedTargets, affectedFromGit } from "./affected.mjs";
import { completeRun, runSurface } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";
import { validateAccessibility } from "./accessibility.mjs";
import { resourcesFor } from "./locks.mjs";

// Parallelize independent work while preserving FIFO order on every shared
// resource. Callers still need cross-process leases at the execution boundary.
export async function scheduleByResources(items, keysFor, operation) {
  const locks = new Map();
  const scheduled = items.map((item) => {
    const keys = [...new Set(keysFor(item))];
    const previous = [...new Set(keys.map((key) => locks.get(key)).filter(Boolean))];
    const current = Promise.all(previous.map((promise) => promise.catch(() => undefined)))
      .then(() => operation(item));
    const settled = current.catch(() => undefined);
    for (const key of keys) locks.set(key, settled);
    return current;
  });
  return Promise.all(scheduled);
}


// Run every target a change affects, end-to-end, and consolidate the outcome.
// input: { ref } to diff the working tree via git, or { files: [...] } explicit.
// opts:  { record, timeoutMs, force, frames } forwarded to each run/analysis.
// Each target lands in exactly one bucket:
//   blocked  toolchain not ready -> not spawned; carries the fix (remediation)
//   passed   ran and the suite passed
//   failed   ran and the suite failed (or the run errored / timed out)
// Returns { ref?, affected, results: [...], summary: { total, passed, failed,
// blocked, ran } }. Never throws for a failing suite; rejects only if selection
// itself cannot be computed (e.g. a bad git ref).
export async function orchestrate(input = {}, opts = {}) {
  const selection = Array.isArray(input.files)
    ? affectedTargets(input.files)
    : affectedFromGit(input.ref);
  const appIds = new Set((selection.apps || []).map((app) => app.appId));
  if (opts.appId) appIds.add(opts.appId);
  const checks = [...appIds].sort().map((appId) => {
    try {
      const result = validateAccessibility(appId);
      return { name: `accessibility:${appId}`, status: result.ok ? "passed" : "failed", result };
    } catch (error) {
      return {
        name: `accessibility:${appId}`,
        status: "failed",
        result: { ok: false, error: error instanceof Error ? error.message : String(error) },
      };
    }
  });

  // Hardware and fixed service ports are serialized in-process here; runner.mjs
  // also takes cross-process atomic leases, so parallel CI workers cannot race.

  const execute = async (target) => {
    const run = await runSurface(target, {
      appId: opts.appId,
      kind: opts.kind || "pull-request",
      env: opts.env || {},
      record: Boolean(opts.record),
      timeoutMs: Number(opts.timeoutMs) || Number("0"),
      force: Boolean(opts.force),
      spec: opts.spec,
      resourceWaitMs: opts.resourceWaitMs === undefined ? 10 * 60 * 1000 : Number(opts.resourceWaitMs),
    });

    if (run.skipped) {
      return {
        target,
        runId: run.runId,
        status: "blocked",
        artifactsDir: run.artifactsDir,
        manifestPath: run.manifestPath,
        missing: run.preflight?.missing || [],
        remediation: run.preflight?.remediation || (run.resourceLock ? [run.resourceLock.error] : []),
        resourceLock: run.resourceLock || null,
      };
    }

    let analysis = null;
    let analysisError = null;
    try {
      analysis = analyzeRun({
        reportPath: run.reportPath,
        artifactsDir: run.artifactsDir,
        tool: run.tool,
        frames: Number(opts.frames) || Number("0"),
        runId: run.runId,
      });
    } catch (error) {
      analysisError = error;
      analysis = { error: error instanceof Error ? error.message : String(error) };
    }
    const completed = completeRun(run, analysisError ? null : analysis, analysisError);

    return {
      target,
      runId: completed.runId,
      status: completed.passed ? "passed" : "failed",
      exitCode: completed.exitCode,
      timedOut: completed.timedOut,
      durationMs: completed.durationMs,
      reportPath: completed.reportPath,
      artifactsDir: completed.artifactsDir,
      manifestPath: completed.manifestPath,
      analysisPath: completed.analysisPath,
      evidence: completed.evidence,
      analysis,
    };
  };

  const results = await scheduleByResources(
    selection.targets,
    (target) => resourcesFor(target, opts.env || {}),
    execute,
  );
  const count = (status) => results.filter((result) => result.status === status).length;
  const summary = {
    total: results.length + checks.length,
    passed: count("passed") + checks.filter((check) => check.status === "passed").length,
    failed: count("failed") + checks.filter((check) => check.status === "failed").length,
    blocked: count("blocked"),
    ran: count("passed") + count("failed"),
    checks: checks.length,
  };

  return {
    ...(input.ref !== undefined || !Array.isArray(input.files) ? { ref: selection.ref || input.ref || "HEAD" } : {}),
    affected: {
      targets: selection.targets,
      crossCutting: selection.crossCutting,
      files: selection.files,
      apps: selection.apps || [],
    },
    results,
    checks,
    summary,
  };
}
