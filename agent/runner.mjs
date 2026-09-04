// Execution + recording layer for the probierz test toolkit. Unlike the
// read-only agent/lib.mjs discovery surface, this module SPAWNS a real test run
// (an npm script -> Playwright or WebdriverIO+Appium) under caller-chosen
// conditions, forces recording on when asked, and reports where the artifacts
// and the machine-readable report landed. Heavy and side-effecting, so only the
// CLI/MCP `run` tool reaches it -- never the read-only discovery path.
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  appendFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  statSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import { finished } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { preflight } from "./preflight.mjs";
import { appSurface, loadAppManifest, surfaceJourneys } from "./apps.mjs";
import { collectPlatformDiagnostics, startPerformanceSampler } from "./collect.mjs";
import { acquireResourcesWait, resourcesFor } from "./locks.mjs";
import { repositoryIdentity } from "./source-identity.mjs";
import { CODE, failureFrom, failureSummary } from "./failure.mjs";
import { recordPassingQualityEvidenceWritten } from "./onboarding.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// probierz/agent -> probierz project root.
const ROOT = path.resolve(HERE, "..");

// Runnable targets -> the workspace package + root npm script that drives them,
// and whether Playwright or WebdriverIO produces the report.
export const TARGETS = {
  web: { pkg: "packages/web", script: "test:web", tool: "playwright" },
  electron: { pkg: "packages/electron", script: "test:electron", tool: "playwright" },
  "mobile:ios": { pkg: "packages/mobile", script: "test:mobile:ios", tool: "wdio", kind: "mobile" },
  "mobile:ios:byk-auth": { pkg: "packages/mobile", script: "test:mobile:ios:byk-auth", tool: "wdio", kind: "mobile" },
  "mobile:android": { pkg: "packages/mobile", script: "test:mobile:android", tool: "wdio", kind: "mobile" },
  "desktop:mac": { pkg: "packages/desktop-native", script: "test:desktop:mac", tool: "wdio", kind: "native" },
  "desktop:win": { pkg: "packages/desktop-native", script: "test:desktop:win", tool: "wdio", kind: "native" },
  "desktop:cua": { pkg: "packages/desktop-cua", script: "test:desktop:cua", tool: "cua-driver" },
  tui: { pkg: "packages/tui", script: "test:tui", tool: "playwright" },
};

const BYK_AUTH_CONDITION_NAMES = new Set([
  "APP_IOS", "BUNDLE_ID", "IOS_DEVICE", "IOS_VERSION", "APPIUM_HOME", "DEVELOPER_DIR",
]);

export function targetList() {
  return Object.keys(TARGETS);
}

// Every run gets one canonical report path regardless of the underlying
// framework. The framework configs receive it through PROBIERZ_REPORT_PATH.
function reportFor(artifactsDir) {
  return path.join(artifactsDir, "report.json");
}

// API responses contain bounded tails. Full, redacted, timestamped streams are
// persisted in the run directory.
const TAIL = 4000;
const DEFAULT_TIMEOUT_MS = 20 * 60 * 1000;
const KILL_GRACE_MS = 5000;
const SENSITIVE_KEY = /(auth|cookie|credential|email|gmail|key|otp|password|secret|session|token)/i;

function segment(value, fallback) {
  const clean = String(value || fallback).trim().replace(/[^a-zA-Z0-9._-]+/g, "-");
  return clean || fallback;
}


function isoDay(date) {
  return date.toISOString().slice(0, 10);
}

function runIdentifier(date) {
  return `${date.toISOString().replace(/[:.]/g, "-")}-${randomUUID()}`;
}

function redactConditions(values = {}) {
  return Object.fromEntries(
    Object.entries(values).map(([key, value]) => [
      key,
      SENSITIVE_KEY.test(key) ? `[REDACTED:${key}]` : String(value),
    ]),
  );
}

function secretValues(values = {}) {
  return Object.entries(values)
    .filter(([key, value]) => SENSITIVE_KEY.test(key) && String(value).length >= Number("4"))
    .map(([key, value]) => ({ key, value: String(value) }));
}

function redactText(text, secrets) {
  let value = String(text);
  for (const secret of secrets) {
    value = value.split(secret.value).join(`[REDACTED:${secret.key}]`);
  }
  return value
    .replace(
      /((?:AUTH|COOKIE|CREDENTIAL|EMAIL|GMAIL|KEY|OTP|PASSWORD|SECRET|SESSION|TOKEN)[A-Z0-9_]*\s*[=:]\s*)[^\s,;]+/gi,
      "$1[REDACTED]",
    )
    .replace(
      /("(?:auth|cookie|credential|email|gmail|key|otp|password|secret|session|token)[^"]*"\s*:\s*")[^"]*"/gi,
      "$1[REDACTED]\"",
    );
}

function stamped(text) {
  const stamp = new Date().toISOString();
  return String(text)
    .split(/\r?\n/)
    .map((line) => line ? `${stamp} ${line}` : "")
    .join("\n");
}

function runDataCommand(config, env, secrets, stdoutPath, stderrPath) {
  if (!config) return { ok: true, result: null };
  if (!config || typeof config.command !== "string" || !Array.isArray(config.args)) {
    return { ok: false, error: "invalid data command configuration" };
  }
  const cwd = config.cwd ? path.resolve(ROOT, config.cwd) : ROOT;
  const execution = spawnSync(config.command, config.args.map(String), {
    cwd,
    env,
    encoding: "utf8",
    timeout: Number(config.timeoutMs) > 0 ? Number(config.timeoutMs) : 120000,
    maxBuffer: 8 * 1024 * 1024,
  });
  const safeOut = redactText(execution.stdout || "", secrets);
  const safeErr = redactText(execution.stderr || "", secrets);
  if (safeOut) appendFileSync(stdoutPath, stamped(safeOut), { encoding: "utf8", mode: 0o600 });
  if (safeErr) appendFileSync(stderrPath, stamped(safeErr), { encoding: "utf8", mode: 0o600 });
  if (execution.error || execution.status !== 0) {
    const detail = execution.error?.message || safeErr.trim() || `exit ${execution.status}`;
    return { ok: false, error: detail.slice(-TAIL), exitCode: execution.status };
  }
  const lines = String(execution.stdout || "").trim().split("\n").filter(Boolean);
  if (!lines.length) return { ok: true, result: null };
  try {
    return { ok: true, result: JSON.parse(lines.at(-1)) };
  } catch {
    return { ok: false, error: "data command did not return JSON" };
  }
}

function sha256File(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function walkFiles(root) {
  if (!existsSync(root)) return [];
  const stat = statSync(root);
  if (stat.isFile()) return [root];
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(full));
    else if (entry.isFile()) files.push(full);
  }
  return files.sort();
}

function sha256Path(value) {
  if (!value || !existsSync(value)) return null;
  const stat = statSync(value);
  if (stat.isFile()) return sha256File(value);
  const hash = createHash("sha256");
  for (const file of walkFiles(value)) {
    hash.update(path.relative(value, file));
    hash.update(readFileSync(file));
  }
  return hash.digest("hex");
}


function sourceIdentity(app, primaryRoot = null) {
  if (!app) return null;
  const repositories = app.manifest.repositories.map((repository, index) =>
    repositoryIdentity(
      index === Number("0") && primaryRoot ? path.resolve(primaryRoot) : repository.root,
      path.basename(repository.root),
      index,
    ));
  return {
    sha256: createHash("sha256").update(JSON.stringify(repositories.map(({ index, sha256 }) => ({ index, sha256 })))).digest("hex"),
    repositories,
  };
}

export function appSourceIdentity(appId, { primaryRoot = null } = {}) {
  return {
    schemaVersion: 1,
    harness: repositoryIdentity(ROOT, "probierz", null, {
      excludeRuntimeSecrets: true,
      includePackageLock: true,
    }),
    app: sourceIdentity({ manifest: loadAppManifest(appId) }, primaryRoot),
  };
}

function buildIdentity(env) {
  const candidate = env.PROBIERZ_BUILD_PATH
    || env.APP_IOS
    || env.MAC_APP_PATH
    || env.ELECTRON_APP_MAIN
    || path.join(ROOT, "package-lock.json");
  const resolved = candidate ? path.resolve(candidate) : null;
  return {
    path: resolved,
    sha256: sha256Path(resolved),
  };
}

function writeJsonAtomic(file, value) {
  const temporary = `${file}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(value, null, Number("2"))}\n`, { mode: 0o600 });
  renameSync(temporary, file);
}

function artifactHashes(artifactsDir, manifestPath) {
  return walkFiles(artifactsDir)
    .filter((file) => file !== manifestPath)
    .map((file) => ({
      file: path.relative(artifactsDir, file),
      sha256: sha256File(file),
      bytes: statSync(file).size,
    }));
}

function updateManifest(run, patch) {
  const current = existsSync(run.manifestPath)
    ? JSON.parse(readFileSync(run.manifestPath, "utf8"))
    : {};
  writeJsonAtomic(run.manifestPath, { ...current, ...patch });
}

function reportIdentity(reportPath, runId, startedMs) {
  if (!existsSync(reportPath)) {
    return { ok: false, error: "report missing" };
  }
  const before = statSync(reportPath);
  if (before.mtimeMs < startedMs) {
    return { ok: false, error: "report predates run start" };
  }
  try {
    const report = JSON.parse(readFileSync(reportPath, "utf8"));
    const actualRunId = report?.probierz?.runId || null;
    if (actualRunId !== runId) {
      return {
        ok: false,
        error: `report run ID mismatch: expected ${runId}, got ${actualRunId || "missing"}`,
      };
    }
    return { ok: true, runId, mtime: before.mtime.toISOString() };
  } catch (error) {
    return { ok: false, error: `report unreadable: ${error instanceof Error ? error.message : String(error)}` };
  }
}

function terminateTree(child, signal) {
  if (!child.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", signal === "SIGKILL" ? "/F" : ""].filter(Boolean), {
      stdio: "ignore",
    });
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch {
    try { child.kill(signal); } catch { /* process already exited */ }
  }
}

export function completeRun(run, analysis, analysisError = null) {
  if (run.canceled) return run;
  const analysisPath = path.join(run.artifactsDir, "analysis.json");
  const normalizedError = analysisError
    ? (analysisError instanceof Error ? analysisError.message : String(analysisError))
    : null;
  const payload = normalizedError
    ? { runId: run.runId, error: normalizedError }
    : { ...analysis, runId: run.runId };
  writeJsonAtomic(analysisPath, payload);

  const captureErrors = Array.isArray(analysis?.captureErrors) ? analysis.captureErrors : [];
  const missingMedia = (analysis?.media || []).filter((item) => item.missing);
  const crashes = Array.isArray(analysis?.diagnostics?.crashes) ? analysis.diagnostics.crashes : [];
  const analysisValid = !normalizedError
    && analysis
    && analysis.runId === run.runId
    && Number(analysis.total) > Number("0")
    && Number(analysis.failed) === Number("0")
    && captureErrors.length === Number("0")
    && missingMedia.length === Number("0")
    && crashes.length === Number("0");
  const capturedKinds = new Set((analysis?.media || []).map((item) => item.kind));
  const captureRequired = Boolean(run.conditions.record);
  const capturePresent = !captureRequired
    || capturedKinds.has("video")
    || capturedKinds.has("trace")
    || capturedKinds.has("screenshot");
  const evidence = {
    report: Boolean(run.reportValidation?.ok),
    analysis: Boolean(analysisValid),
    captureRequired,
    capturePresent,
    captureErrors,
    missingMedia: missingMedia.map((item) => item.file),
    crashes,
    errors: [
      ...(normalizedError ? [normalizedError] : []),
      ...(!normalizedError && analysis?.runId !== run.runId ? ["analysis run ID mismatch"] : []),
      ...(!normalizedError && Number(analysis?.total) <= Number("0") ? ["zero executed checks"] : []),
      ...(!normalizedError && Number(analysis?.failed) > Number("0") ? [`${analysis.failed} failed checks`] : []),
      ...captureErrors,
      ...missingMedia.map((item) => `missing report-typed artifact: ${item.file}`),
      ...(!capturePresent ? ["recording requested but no report-typed capture was produced"] : []),
      ...crashes.map((item) => `crash evidence: ${item.message || item.source || "unknown crash"}`),
    ],
  };
  const passed = Boolean(run.passed) && evidence.report && evidence.analysis && evidence.capturePresent;
  updateManifest(run, {
    status: passed ? "passed" : "failed",
    completedAt: new Date().toISOString(),
    exitCode: run.exitCode,
    timedOut: run.timedOut,
    reportValidation: run.reportValidation,
    evidence,
    analysisPath,
    artifacts: artifactHashes(run.artifactsDir, run.manifestPath),
  });
  if (passed) recordPassingQualityEvidenceWritten();
  return { ...run, passed, analysisPath, evidence };
}

// Spawn a real run. opts:
//   env        extra condition vars (BASE_URL, APP_IOS, PROBIERZ_LOCALE, ...)
//   record     force video/trace/screenshot capture on (sets PROBIERZ_RECORD=1)
//   timeoutMs  kill the run after this long (default 20 min)
//   spec       run only this one spec (path/substring); scopes a run to e.g.
//              a single app's suite instead of every spec in the package
//   force      skip the preflight gate and spawn even if the toolchain looks
//              incomplete (for when detection is wrong or deps are elsewhere)
//   runId      caller-assigned identity for asynchronous control planes
//   signal     AbortSignal that cancels the complete spawned process tree
// Resolves to a structured result; a failing suite is NOT an error (the exit
// code carries that). When the toolchain is not ready and force is unset, it
// resolves early with { ready:false, skipped:true, preflight } and never
// spawns. Rejects only on an unknown target or a spawn error.
export async function runSurface(target, opts = {}) {
  const t = TARGETS[target];
  if (!t) {
    return Promise.reject(
      new Error(`unknown target: ${target} (one of ${targetList().join(", ")})`),
    );
  }

  let requestedEnv = { ...(opts.env || {}) };
  const appId = segment(opts.appId || requestedEnv.PROBIERZ_APP_ID, "probierz");
  const app = appId === "probierz" ? null : appSurface(appId, target);
  if (app) {
    requestedEnv = { ...(app.surface.conditions || {}), ...requestedEnv };
    for (const name of Object.keys(app.manifest.secretRefs || {})) {
      if (requestedEnv[name] === undefined && process.env[name] !== undefined) {
        requestedEnv[name] = process.env[name];
      }
    }
    for (const [targetName, sourceName] of Object.entries(app.surface.env || {})) {
      const value = requestedEnv[sourceName] ?? process.env[sourceName];
      if (value !== undefined) {
        requestedEnv[sourceName] = value;
        requestedEnv[targetName] = value;
      }
    }
  }
  const configuredSpec = opts.spec || app?.surface.spec || null;
  const bykAuth = target === "mobile:ios:byk-auth";
  const record = bykAuth ? false : Boolean(opts.record);
  if (bykAuth) {
    const unsupported = Object.keys(requestedEnv).filter((name) => !BYK_AUTH_CONDITION_NAMES.has(name));
    if (unsupported.length) {
      return Promise.reject(new Error("mobile:ios:byk-auth accepts only app, device, runtime, Appium, and Xcode path conditions"));
    }
  }

  const startedDate = new Date();
  const startedMs = startedDate.getTime();
  const runId = segment(opts.runId, runIdentifier(startedDate));
  const artifactsDir = path.join(
    ROOT,
    "test-results",
    appId,
    segment(target, "target"),
    isoDay(startedDate),
    runId,
  );
  mkdirSync(path.join(artifactsDir, "media"), { recursive: true });
  mkdirSync(path.join(artifactsDir, "frames"), { recursive: true });
  mkdirSync(path.join(artifactsDir, "diagnostics"), { recursive: true });

  const reportPath = reportFor(artifactsDir);
  const manifestPath = path.join(artifactsDir, "run-manifest.json");
  const stdoutPath = path.join(artifactsDir, "stdout.log");
  const stderrPath = path.join(artifactsDir, "stderr.log");
  const build = buildIdentity(requestedEnv);
  const kind = segment(opts.kind || requestedEnv.PROBIERZ_RUN_KIND, "adhoc");
  const runJourneys = app ? surfaceJourneys(app.surface, requestedEnv) : [];
  const source = sourceIdentity(app, opts.appRepo);
  const harness = repositoryIdentity(ROOT, "probierz", null, {
    excludeRuntimeSecrets: true,
    includePackageLock: true,
  });
  const baseRun = {
    runId,
    startedAt: startedDate.toISOString(),
    appId,
    kind,
    target,
    tool: t.tool,
    pkg: t.pkg,
    script: t.script,
    artifactsDir,
    reportPath,
    manifestPath,
    stdoutPath,
    stderrPath,
    conditions: { record, ...redactConditions(requestedEnv) },
  };
  writeJsonAtomic(manifestPath, {
    schemaVersion: Number("2"),
    runId,
    appId,
    kind,
    target,
    spec: bykAuth ? "byk-auth.e2e.ts" : configuredSpec,
    status: "preflight",
    startedAt: startedDate.toISOString(),
    harness,
    source,
    build,
    appVersion: requestedEnv.PROBIERZ_APP_VERSION || null,
    appManifest: app ? {
      file: app.manifest.file,
      owner: app.manifest.owner,
      journeys: runJourneys,
    } : null,
    host: {
      hostname: os.hostname(),
      platform: process.platform,
      release: os.release(),
      arch: process.arch,
      node: process.version,
    },
    device: {
      name: requestedEnv.IOS_DEVICE || requestedEnv.ANDROID_DEVICE || null,
      runtime: requestedEnv.IOS_VERSION || requestedEnv.ANDROID_VERSION || null,
    },
    conditions: baseRun.conditions,
    paths: { artifactsDir, reportPath, stdoutPath, stderrPath },
  });
  if (opts.signal?.aborted) {
    const canceledRun = {
      ...baseRun,
      ready: false,
      canceled: true,
      passed: false,
      completedAt: new Date().toISOString(),
    };
    updateManifest(baseRun, {
      status: "canceled",
      completedAt: canceledRun.completedAt,
      canceled: true,
      artifacts: artifactHashes(artifactsDir, manifestPath),
    });
    return Promise.resolve(canceledRun);
  }

  // A blocked preflight is a first-class run outcome and gets a manifest, but
  // no suite is spawned.
  if (!opts.force) {
    const pf = preflight(bykAuth ? "mobile:ios" : target, { ...process.env, ...requestedEnv });
    if (!pf.ready) {
      updateManifest(baseRun, {
        status: "blocked",
        completedAt: new Date().toISOString(),
        preflight: pf,
        artifacts: artifactHashes(artifactsDir, manifestPath),
      });
      return Promise.resolve({
        ...baseRun,
        ready: false,
        skipped: true,
        preflight: pf,
      });
    }
  }

  const env = {
    ...process.env,
    ...requestedEnv,
    PROBIERZ_APP_ID: appId,
    PROBIERZ_RUN_ID: runId,
    PROBIERZ_ARTIFACTS: artifactsDir,
    PROBIERZ_REPORT_PATH: reportPath,
    PROBIERZ_JOURNEYS: runJourneys.join(","),
    PROBIERZ_NATIVE_CAPTURE_BIN: path.join(ROOT, "node_modules", ".cache", "probierz", "screen-capture-kit"),
  };
  if (record) env.PROBIERZ_RECORD = "1";
  const spec = bykAuth ? "byk-auth.e2e.ts" : configuredSpec;
  if (spec && !bykAuth) env.PROBIERZ_SPEC = spec;
  const resources = resourcesFor(target, requestedEnv);
  let resourceLease;
  try {
    resourceLease = await acquireResourcesWait(resources, runId, {
      timeoutMs: opts.resourceWaitMs,
      signal: opts.signal,
    });
    updateManifest(baseRun, { resources: resourceLease.resources });
  } catch (error) {
    if (error?.code === "PROBIERZ_CANCELLED") {
      const canceledRun = {
        ...baseRun,
        ready: false,
        canceled: true,
        passed: false,
        completedAt: new Date().toISOString(),
      };
      updateManifest(baseRun, {
        status: "canceled",
        completedAt: canceledRun.completedAt,
        canceled: true,
        artifacts: artifactHashes(artifactsDir, manifestPath),
      });
      return canceledRun;
    }
    const resourceLock = {
      error: error instanceof Error ? error.message : String(error),
      resource: error?.resource || null,
      owner: error?.owner || null,
    };
    updateManifest(baseRun, {
      status: "blocked",
      completedAt: new Date().toISOString(),
      resourceLock,
      artifacts: artifactHashes(artifactsDir, manifestPath),
    });
    return Promise.resolve({
      ...baseRun,
      ready: false,
      skipped: true,
      resourceLock,
    });
  }

  const lifecycle = app?.manifest.data || null;
  const hookSecrets = secretValues(requestedEnv);
  const seedResult = runDataCommand(lifecycle?.seed, env, hookSecrets, stdoutPath, stderrPath);
  if (seedResult.ok && seedResult.result?.env && typeof seedResult.result.env === "object") {
    Object.assign(requestedEnv, seedResult.result.env);
    Object.assign(env, seedResult.result.env);
    baseRun.conditions = { record, ...redactConditions(requestedEnv) };
    updateManifest(baseRun, {
      conditions: baseRun.conditions,
      seed: { ...seedResult.result, env: redactConditions(seedResult.result.env) },
    });
  }
  if (!seedResult.ok) {
    const rollback = runDataCommand(lifecycle?.cleanup, env, hookSecrets, stdoutPath, stderrPath);
    const reportValidation = { ok: false, error: `seed failed: ${seedResult.error}` };
    const run = {
      ...baseRun,
      ready: true,
      skipped: false,
      command: null,
      spec,
      exitCode: 1,
      signal: null,
      timedOut: false,
      passed: false,
      durationMs: Date.now() - startedMs,
      reportValidation,
      setupError: seedResult.error,
      cleanup: rollback,
      stdoutTail: "",
      stderrTail: seedResult.error || "",
    };
    updateManifest(run, {
      status: "failed",
      completedAt: new Date().toISOString(),
      setupError: seedResult.error,
      cleanup: rollback,
      reportValidation,
    });
    resourceLease.release();
    return Promise.resolve(run);
  }
  const dataSeeded = Boolean(lifecycle?.seed);

  const command = `npm run ${t.script}${spec && !bykAuth ? ` (PROBIERZ_SPEC=${spec})` : ""}`;
  const wanted = Number(opts.timeoutMs);
  const timeoutMs = wanted > Number("0") ? wanted : DEFAULT_TIMEOUT_MS;
  updateManifest(baseRun, { status: "running", command, timeoutMs });
  updateManifest(baseRun, { dataSeeded });

  return new Promise((resolve, reject) => {
    let child;
    try {
      child = spawn("npm", ["run", t.script], {
        cwd: ROOT,
        env,
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      resourceLease.release();
      reject(error);
      return;
    }
    const performanceSampler = startPerformanceSampler(child, artifactsDir, { target, env });
    const stdout = createWriteStream(stdoutPath, { flags: "a", mode: 0o600 });
    const stderr = createWriteStream(stderrPath, { flags: "a", mode: 0o600 });
    const secrets = secretValues(requestedEnv);
    let out = "";
    let err = "";
    let timedOut = false;
    let canceled = false;
    let settled = false;
    let hardKill = null;
    let firstOutputMs = null;
    const scheduleHardKill = () => {
      clearTimeout(hardKill);
      hardKill = setTimeout(() => terminateTree(child, "SIGKILL"), KILL_GRACE_MS);
    };
    const timer = setTimeout(() => {
      timedOut = true;
      terminateTree(child, "SIGTERM");
      scheduleHardKill();
    }, timeoutMs);
    const abort = () => {
      canceled = true;
      terminateTree(child, "SIGTERM");
      scheduleHardKill();
    };
    opts.signal?.addEventListener("abort", abort, { once: true });
    if (opts.signal?.aborted) abort();

    child.stdout.on("data", (chunk) => {
      if (firstOutputMs === null) firstOutputMs = Date.now() - startedMs;
      const safe = redactText(chunk, secrets);
      out = (out + safe).slice(-TAIL);
      stdout.write(stamped(safe));
    });
    child.stderr.on("data", (chunk) => {
      if (firstOutputMs === null) firstOutputMs = Date.now() - startedMs;
      const safe = redactText(chunk, secrets);
      err = (err + safe).slice(-TAIL);
      stderr.write(stamped(safe));
    });
    child.on("error", async (error) => {
      clearTimeout(timer);
      clearTimeout(hardKill);
      opts.signal?.removeEventListener("abort", abort);
      if (settled) return;
      settled = true;
      stdout.end();
      stderr.end();
      await Promise.allSettled([finished(stdout), finished(stderr)]);
      const performance = performanceSampler.stop(firstOutputMs);
      const platformDiagnostics = collectPlatformDiagnostics({
        target,
        env,
        artifactsDir,
        startedAt: baseRun.startedAt,
      });
      const cleanup = dataSeeded
        ? runDataCommand(lifecycle?.cleanup, env, secretValues(requestedEnv), stdoutPath, stderrPath)
        : { ok: true, result: null };
      // A runner that will not even start is a toolchain that is not installed
      // — the thing `probierz check` exists to catch. Saying so beats handing
      // the operator a bare `spawn ENOENT` and letting them guess whose fault
      // it is. Retrying an absent binary never helps, hence `config`.
      const failure = failureFrom({
        point: "run.spawn",
        error,
        detail: `npm script ${t.script} in ${t.pkg}`,
        action: `Starting the ${target} runner failed`,
        fallbackCode: CODE.CONFIG,
      });
      updateManifest(baseRun, {
        status: "failed",
        completedAt: new Date().toISOString(),
        spawnFailure: failureSummary(failure),
        cleanup,
        performance,
        platformDiagnostics,
      });
      resourceLease.release();
      reject(failure);
    });
    child.on("close", async (code, signal) => {
      clearTimeout(timer);
      opts.signal?.removeEventListener("abort", abort);
      clearTimeout(hardKill);
      terminateTree(child, "SIGTERM");
      stdout.end();
      stderr.end();
      await Promise.allSettled([finished(stdout), finished(stderr)]);
      if (settled) return;
      settled = true;

      const performance = performanceSampler.stop(firstOutputMs);
      const platformDiagnostics = collectPlatformDiagnostics({
        target,
        env,
        artifactsDir,
        startedAt: baseRun.startedAt,
      });
      const exitCode = code === null ? -1 : code;
      const reportValidation = reportIdentity(reportPath, runId, startedMs);
      const cleanup = dataSeeded
        ? runDataCommand(lifecycle?.cleanup, env, secretValues(requestedEnv), stdoutPath, stderrPath)
        : { ok: true, result: null };
      const passed = exitCode === Number("0") && !timedOut && !canceled && reportValidation.ok && cleanup.ok;
      const run = {
        ...baseRun,
        ready: true,
        command,
        spec,
        exitCode,
        signal: signal || null,
        timedOut,
        canceled,
        passed,
        durationMs: Date.now() - startedMs,
        reportValidation,
        stdoutTail: out,
        stderrTail: err,
        cleanup,
        cleanupError: cleanup.ok ? null : cleanup.error,
        performance,
        platformDiagnostics,
      };
      resourceLease.release();
      updateManifest(run, {
        status: canceled ? "canceled" : (passed ? "executed" : "failed"),
        completedAt: new Date().toISOString(),
        exitCode,
        signal: signal || null,
        timedOut,
        canceled,
        durationMs: run.durationMs,
        reportValidation,
        cleanup,
        cleanupError: cleanup.ok ? null : cleanup.error,
        performance,
        platformDiagnostics,
        artifacts: artifactHashes(artifactsDir, manifestPath),
      });
      resolve(run);
    });
  });
}
