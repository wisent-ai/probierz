import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const RESULTS = path.join(ROOT, "test-results");

function readJson(file) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch { return null; }
}

function manifestsBelow(root) {
  if (!existsSync(root)) return [];
  const files = [];
  const pending = [root];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) pending.push(file);
      else if (entry.isFile() && entry.name === "run-manifest.json") files.push(file);
    }
  }
  return files;
}

function percentile(values, fraction) {
  if (!values.length) return null;
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[Math.min(ordered.length - 1, Math.ceil(ordered.length * fraction) - 1)];
}

function normalizedStatus(manifest) {
  if (manifest.status === "passed") return "passed";
  if (manifest.status === "blocked") return "blocked";
  if (manifest.status === "canceled") return "canceled";
  if (manifest.status === "executed") return "passed";
  if (manifest.status === "failed") return "failed";
  return manifest.completedAt ? "failed" : "incomplete";
}

function runRecord(manifestPath) {
  const manifest = readJson(manifestPath);
  if (!manifest) return null;
  const directory = path.dirname(manifestPath);
  const analysis = readJson(manifest.analysisPath || path.join(directory, "analysis.json"));
  const report = readJson(manifest.paths?.reportPath || path.join(directory, "report.json"));
  const failureText = (analysis?.failures || report?.failures || [])
    .map((failure) => failure.error || failure.message || "")
    .join("\n");
  const failureClass = /executable doesn't exist|driver.*not installed|toolchain|connection refused|econnrefused/i.test(failureText)
    ? "infrastructure"
    : "product";
  const testsByTitle = new Map();
  for (const test of analysis?.tests || report?.tests || []) {
    testsByTitle.set(test.title, {
      title: test.title,
      status: test.status || (test.passed ? "passed" : "failed"),
      durationMs: Number(test.durationMs ?? test.duration ?? 0),
    });
  }
  const tests = [...testsByTitle.values()];
  return {
    runId: manifest.runId,
    appId: manifest.appId,
    kind: manifest.kind || "adhoc",
    target: manifest.target,
    spec: manifest.spec || null,
    status: normalizedStatus(manifest),
    startedAt: manifest.startedAt,
    completedAt: manifest.completedAt || null,
    durationMs: Number(manifest.durationMs || 0),
    harness: manifest.harness || null,
    source: manifest.source || null,
    build: manifest.build || null,
    journeys: manifest.appManifest?.journeys || [],
    failureClass: normalizedStatus(manifest) === "failed" ? failureClass : null,
    device: manifest.device || null,
    conditions: manifest.conditions || {},
    evidence: manifest.evidence || null,
    artifacts: manifest.artifacts || [],
    protection: manifest.protection || null,
    manifestPath,
    analysisPath: manifest.analysisPath || null,
    tests,
  };
}

function testHistory(runs) {
  const byTitle = new Map();
  for (let runIndex = runs.length - 1; runIndex >= 0; runIndex -= 1) {
    const run = runs[runIndex];
    if (run.failureClass === "infrastructure") continue;
    for (const test of run.tests) {
      const observations = byTitle.get(test.title) || [];
      observations.push({ runId: run.runId, at: run.startedAt, status: test.status, durationMs: test.durationMs });
      byTitle.set(test.title, observations);
    }
  }
  return [...byTitle.entries()].map(([title, observations]) => {
    const passed = observations.filter((item) => item.status === "passed").length;
    let transitions = 0;
    for (let index = 1; index < observations.length; index += 1) {
      if (observations[index].status !== observations[index - 1].status) transitions += 1;
    }
    const durations = observations.filter((item) => item.status === "passed").map((item) => item.durationMs);
    return {
      title,
      observations: observations.length,
      passed,
      failed: observations.length - passed,
      passRate: observations.length ? passed / observations.length : null,
      transitions,
      flaky: transitions > 0 && passed > 0 && passed < observations.length,
      latest: observations.at(-1) || null,
      duration: {
        p50Ms: percentile(durations, 0.5),
        p95Ms: percentile(durations, 0.95),
        maxMs: durations.length ? Math.max(...durations) : null,
      },
    };
  }).sort((left, right) => left.title.localeCompare(right.title));
}

function journeyHistory(runs) {
  const names = new Set(runs.flatMap((run) => run.journeys));
  return [...names].sort().map((journey) => {
    const relevant = runs.filter((run) => run.journeys.includes(journey));
    const passed = relevant.filter((run) => run.status === "passed").length;
    const productFailures = relevant.filter((run) => run.status === "failed" && run.failureClass !== "infrastructure").length;
    return {
      journey,
      runs: relevant.length,
      passed,
      failed: relevant.filter((run) => run.status === "failed").length,
      productFailures,
      infrastructureFailures: relevant.filter((run) => run.status === "failed" && run.failureClass === "infrastructure").length,
      blocked: relevant.filter((run) => run.status === "blocked").length,
      canceled: relevant.filter((run) => run.status === "canceled").length,
      passRate: passed + productFailures ? passed / (passed + productFailures) : null,
      latestRunId: relevant[0]?.runId || null,
    };
  });
}

function performanceTrend(runs) {
  const passed = runs.filter((run) => run.status === "passed" && run.durationMs > 0);
  const latest = passed[0] || null;
  const baselineValues = passed.slice(1, 11).map((run) => run.durationMs);
  const baselineMs = percentile(baselineValues, 0.5);
  const ratio = latest && baselineMs ? latest.durationMs / baselineMs : null;
  return {
    latestMs: latest?.durationMs || null,
    baselineMedianMs: baselineMs,
    ratio,
    regression: ratio !== null && baselineMs >= 500 && ratio >= 1.2,
  };
}

export function runHistory({ appId = "probierz", target, limit = 50 } = {}) {
  const root = path.join(RESULTS, appId, ...(target ? [target.replace(/:/g, "-")] : []));
  const runs = manifestsBelow(root)
    .map(runRecord)
    .filter(Boolean)
    .filter((run) => !target || run.target === target)
    .sort((left, right) => String(right.startedAt).localeCompare(String(left.startedAt)))
    .slice(0, Math.max(1, Number(limit) || 50));
  const passed = runs.filter((run) => run.status === "passed").length;
  const failed = runs.filter((run) => run.status === "failed").length;
  const infrastructureFailures = runs.filter((run) => run.status === "failed" && run.failureClass === "infrastructure").length;
  const productFailures = failed - infrastructureFailures;
  const blocked = runs.filter((run) => run.status === "blocked").length;
  const canceled = runs.filter((run) => run.status === "canceled").length;
  const tests = testHistory(runs);
  const performance = performanceTrend(runs);
  return {
    schemaVersion: 2,
    appId,
    target: target || null,
    generatedAt: new Date().toISOString(),
    summary: {
      runs: runs.length,
      passed,
      failed,
      blocked,
      canceled,
      productFailures,
      infrastructureFailures,
      passRate: passed + productFailures ? passed / (passed + productFailures) : null,
      flakyTests: tests.filter((test) => test.flaky).length,
      latestRunId: runs[0]?.runId || null,
      lastGreenRunId: runs.find((run) => run.status === "passed")?.runId || null,
      performanceRegression: performance.regression,
    },
    performance,
    journeys: journeyHistory(runs),
    tests,
    runs,
  };
}
export function getRun(appId, runId) {
  const run = manifestsBelow(path.join(RESULTS, appId))
    .map(runRecord)
    .find((candidate) => candidate?.runId === runId);
  if (!run) throw new Error(`run not found for ${appId}: ${runId}`);
  return run;
}

function artifactIndex(run) {
  return new Map((run.artifacts || []).map((artifact) => [artifact.file, artifact]));
}

export function compareRuns({ appId = "probierz", leftRunId, rightRunId } = {}) {
  const left = getRun(appId, leftRunId);
  const right = getRun(appId, rightRunId);
  const leftTests = new Map(left.tests.map((test) => [test.title, test]));
  const rightTests = new Map(right.tests.map((test) => [test.title, test]));
  const testNames = [...new Set([...leftTests.keys(), ...rightTests.keys()])].sort();
  const testChanges = testNames.flatMap((title) => {
    const before = leftTests.get(title) || null;
    const after = rightTests.get(title) || null;
    if (!before) return [{ title, change: "added", before, after }];
    if (!after) return [{ title, change: "removed", before, after }];
    if (before.status !== after.status || before.durationMs !== after.durationMs) {
      return [{
        title,
        change: before.status === after.status ? "duration" : "status",
        before,
        after,
        durationDeltaMs: after.durationMs - before.durationMs,
      }];
    }
    return [];
  });
  const leftArtifacts = artifactIndex(left);
  const rightArtifacts = artifactIndex(right);
  const artifactNames = [...new Set([...leftArtifacts.keys(), ...rightArtifacts.keys()])].sort();
  const artifactChanges = artifactNames.flatMap((file) => {
    const before = leftArtifacts.get(file) || null;
    const after = rightArtifacts.get(file) || null;
    if (!before) return [{ file, change: "added", before, after }];
    if (!after) return [{ file, change: "removed", before, after }];
    if (before.sha256 !== after.sha256 || before.bytes !== after.bytes) {
      return [{ file, change: "content", before, after }];
    }
    return [];
  });
  const durationDeltaMs = right.durationMs - left.durationMs;
  const durationRatio = left.durationMs > 0 ? right.durationMs / left.durationMs : null;
  const newlyFailing = testChanges
    .filter((change) => change.after?.status === "failed" && change.before?.status !== "failed")
    .map((change) => change.title);
  return {
    schemaVersion: 2,
    appId,
    left: {
      runId: left.runId,
      status: left.status,
      harness: left.harness,
      source: left.source,
      build: left.build,
      durationMs: left.durationMs,
      evidence: left.evidence,
    },
    right: {
      runId: right.runId,
      status: right.status,
      harness: right.harness,
      source: right.source,
      build: right.build,
      durationMs: right.durationMs,
      evidence: right.evidence,
    },
    verdict: {
      statusChanged: left.status !== right.status,
      regression: right.status === "failed" && left.status === "passed",
      newlyFailing,
      durationRegression: durationRatio !== null && left.durationMs >= 500 && durationRatio >= 1.2,
    },
    duration: { deltaMs: durationDeltaMs, ratio: durationRatio },
    tests: { changed: testChanges.length, changes: testChanges },
    artifacts: { changed: artifactChanges.length, changes: artifactChanges },
  };
}

export function lastGreen({ appId = "probierz", target, journey } = {}) {
  const history = runHistory({ appId, target, limit: 1000 });
  const run = history.runs.find((candidate) => candidate.status === "passed"
    && (!journey || candidate.journeys.includes(journey)));
  return {
    schemaVersion: 2,
    appId,
    target: target || null,
    journey: journey || null,
    run: run || null,
  };
}
