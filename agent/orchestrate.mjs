// Change-driven test orchestration for the probierz toolkit. Composes the
// deterministic pieces into one CI-style pass: given a change, select the
// affected targets (affected.mjs), run each one preflight-gated (runner.mjs
// skips + reports the not-ready ones instead of failing opaquely), analyze what
// ran (analyze.mjs), and return a consolidated verdict. No LLM, no watching, no
// autonomy: it decides WHICH targets by structure and reports facts. Whether a
// failure is "real" and what to change about the code is brama's job, upstream.
import { affectedTargets, affectedFromGit } from "./affected.mjs";
import { runSurface } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";

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

  const results = [];
  for (const target of selection.targets) {
    const run = await runSurface(target, {
      record: Boolean(opts.record),
      timeoutMs: Number(opts.timeoutMs) || Number("0"),
      force: Boolean(opts.force),
    });

    if (run.skipped) {
      results.push({
        target,
        status: "blocked",
        missing: run.preflight.missing,
        remediation: run.preflight.remediation,
      });
      continue;
    }

    let analysis = null;
    try {
      analysis = analyzeRun({
        reportPath: run.reportPath,
        artifactsDir: run.artifactsDir,
        tool: run.tool,
        frames: Number(opts.frames) || Number("0"),
      });
    } catch (e) {
      analysis = { error: e instanceof Error ? e.message : String(e) };
    }

    results.push({
      target,
      status: run.passed ? "passed" : "failed",
      exitCode: run.exitCode,
      timedOut: run.timedOut,
      durationMs: run.durationMs,
      reportPath: run.reportPath,
      artifactsDir: run.artifactsDir,
      analysis,
    });
  }

  const count = (s) => results.filter((r) => r.status === s).length;
  const summary = {
    total: results.length,
    passed: count("passed"),
    failed: count("failed"),
    blocked: count("blocked"),
    ran: count("passed") + count("failed"),
  };

  return {
    ...(input.ref !== undefined || !Array.isArray(input.files) ? { ref: selection.ref || input.ref || "HEAD" } : {}),
    affected: { targets: selection.targets, crossCutting: selection.crossCutting },
    results,
    summary,
  };
}
