// Bridge to the stado GPU job queue: run a probierz target on a chosen
// remote host and bring the evidence back into the local test-results tree,
// so history, status, and the gate treat remote runs like local ones.
// Inputs travel as tarballs through stado://probierz/inputs (the probierz
// checkout is private), the job script provisions node and the app's binary
// on the worker, and results are persisted under stado://probierz/results.
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, closeSync, cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, openSync, readFileSync, readSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { installAcceptedSpec } from "./author-spec.mjs";
import { stadoModelRouterUrl } from "./model-router.mjs";
import { loadAppManifest, surfaceJourneys } from "./apps.mjs";
import { appSourceIdentity } from "./runner.mjs";
import { CODE, EXIT_RETRY, FailureError, failureFrom, failureSummary } from "./failure.mjs";
import { repositorySourceFiles } from "./source-identity.mjs";
import { SETUP_STEP_TIMEOUT_MS, setupSteps } from "./preflight.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const STADO_BIN = "stado";
const NODE_VERSION = "v22.20.0";
const WATCH_INTERVAL_MS = Number("30000");
const STATUS_CALL_TIMEOUT_MS = Number("180000");
const GUI_STATUS_CALL_TIMEOUT_MS = Number("1800000");
const WATCH_BUDGET_ENV = "PROBIERZ_WATCH_BUDGET_MS";

function positiveBudget(value) {
  const budget = Number(value);
  return Number.isFinite(budget) && budget > Number("0") ? Math.ceil(budget) : null;
}

function needsStadoDesktopCli(target, provision) {
  return target === "desktop:cua"
    && provision?.kind === "app-bundle"
    && provision.appId === "stado";
}

function provisioningBudget(target, provision) {
  // The remote dependency install and every target setup command receive the
  // same per-step allowance that `probierz setup` publicly promises. A source
  // Cargo build is one additional provisioning step.
  const setupCount = target === "tui" ? Number("0") : setupSteps(target).length;
  const sourceBuildCount = provision?.kind === "cargo-release"
    || needsStadoDesktopCli(target, provision)
    ? Number("1")
    : Number("0");
  return (Number("1") + setupCount + sourceBuildCount) * SETUP_STEP_TIMEOUT_MS;
}

function selectedRunBudget({ appId, target, environment, provision }) {
  const manifest = loadAppManifest(appId);
  const surface = manifest.surfaces[target];
  if (!surface) throw new Error(`app ${appId} has no ${target} surface`);
  const selectedEnvironment = {
    ...(surface.conditions || {}),
    ...Object.fromEntries(environment),
  };
  for (const [targetName, sourceName] of Object.entries(surface.env || {})) {
    const value = selectedEnvironment[sourceName] ?? process.env[sourceName];
    if (value !== undefined) {
      selectedEnvironment[sourceName] = value;
      selectedEnvironment[targetName] = value;
    }
  }
  const journeys = surfaceJourneys(surface, selectedEnvironment);
  const journeyBudget = journeys.reduce(
    (total, journey) => total + Number(manifest.journeys[journey].timeoutMs),
    Number("0"),
  );
  return journeyBudget + provisioningBudget(target, provision);
}

function conservativeWatchBudget(appId) {
  const manifest = loadAppManifest(appId);
  const journeyBudget = Object.values(manifest.journeys).reduce(
    (total, journey) => total + positiveBudget(journey.timeoutMs),
    Number("0"),
  );
  let provisionBudget = Number("0");
  for (const target of Object.keys(manifest.surfaces)) {
    if (target in TARGET_REGISTRATION_DIRS_REL || target === "desktop:cua") {
      provisionBudget = Math.max(provisionBudget, provisioningBudget(target, { kind: "cargo-release" }));
    }
  }
  return journeyBudget + provisionBudget;
}

function legacyWatchBudget(job, hostDef) {
  const directory = mkdtempSync(workPath("watch-contract-"));
  const options = {
    env: process.env,
  };
  try {
    const file = path.join(directory, "job.json");
    const prefix = job.state === "queued" ? "queue" : job.state;
    const downloaded = sh(STADO_BIN, ["storage", "get", stateUri(`${prefix}/${job.job_id}.json`), file], options);
    if (downloaded.status !== Number("0")) {
      throw remoteFailure("stado.watch", "Reading the original run inputs failed", downloaded);
    }
    const original = JSON.parse(readFileSync(file, "utf8"));
    const inputs = original.resolved_input_artifacts || {};
    const appInput = String(inputs.app?.relative_path || "").match(/^inputs\/([a-z0-9][a-z0-9._-]*)\.tar\.gz$/i);
    let appId = appInput?.[Number("1")];
    if (!appId && inputs.script?.stado_uri) {
      const scriptFile = path.join(directory, "run.sh");
      const scriptDownload = sh(STADO_BIN, ["storage", "get", inputs.script.stado_uri, scriptFile], options);
      if (scriptDownload.status !== Number("0")) {
        throw remoteFailure("stado.watch", "Reading the original run command failed", scriptDownload);
      }
      const script = readFileSync(scriptFile, "utf8");
      appId = script.match(/(?:--app|author-spec)\s+['"]?([a-z0-9][a-z0-9._-]*)/i)?.[Number("1")];
    }
    if (!appId) {
      throw new FailureError({
        point: "stado.watch",
        code: CODE.CONFIG,
        detail: `job ${job.job_id} has neither a saved watch budget nor identifiable application inputs`,
        message: "The original run contract cannot be recovered from this job.",
      });
    }
    return conservativeWatchBudget(appId);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function budgetFromJob(job) {
  const escaped = WATCH_BUDGET_ENV.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = String(job?.command || "").match(new RegExp(`(?:^|\\s)${escaped}=(\\d+)(?:\\s|$)`));
  return positiveBudget(match?.[Number("1")]);
}

function workPath(name) {
  const directory = path.join(homedir(), ".stado", "work", "probierz");
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  return path.join(directory, name);
}

// An input upload rides through a control-plane restart rather than failing
// the submission: six attempts, five seconds further apart each time, so a
// redeploy has about a minute and a half to come back before an operator is
// told the store is down.
const UPLOAD_ATTEMPTS = Number("6");
const UPLOAD_BACKOFF_MS = Number("5000");
const REMOTE_SECRET_ENV = {
  STADO_MODEL_ROUTER_TOKEN: {
    reference: "vault://wisent/probierz/model-router-token",
    item: "probierz-model-router",
    field: "token",
  },
  PROBIERZ_MODEL_AGENT_SECRET: {
    reference: "vault://wisent/probierz/model-agent-secret",
    item: "probierz-agent-auth",
    field: "agent_auth_secret",
  },
  PROBIERZ_SEO_RECEIPT_PRIVATE_KEY: {
    reference: "vault://wisent/probierz/seo-receipt-private-key",
    item: "probierz-seo-receipt-signing",
    field: "private_key",
  },
};

function remoteModelSecretEnv(names) {
  const secretEnv = {};
  for (const name of names) {
    const { item, field } = REMOTE_SECRET_ENV[name];
    secretEnv[name] = { item, field };
  }
  return secretEnv;
}

function remoteRunSecretEnv(appId, names = Object.keys(REMOTE_SECRET_ENV)) {
  const configured = loadAppManifest(appId).secretRefs || {};
  const secretEnv = {};
  for (const name of names) {
    const binding = REMOTE_SECRET_ENV[name];
    const reference = configured[name];
    if (!reference) continue;
    if (reference !== binding.reference) {
      throw new FailureError({
        point: "stado.submit",
        code: CODE.CONFIG,
        detail: `unsupported ${name} reference: ${reference}`,
        message: `Remote runs require ${binding.reference} for ${name}.`,
      });
    }
    secretEnv[name] = { item: binding.item, field: binding.field };
  }
  return secretEnv;
}
// Worker-relative toolkit registration dirs. Accepted implementations are
// transported through retained authoring artifacts and installed in products.
const TARGET_REGISTRATION_DIRS_REL = {
  web: "packages/web/tests",
  electron: "packages/electron/tests",
  "mobile:ios": "packages/mobile/test/specs",
  "mobile:android": "packages/mobile/test/specs",
  "desktop:mac": "packages/desktop-native/test/specs",
  "desktop:win": "packages/desktop-native/test/specs",
  "desktop:cua": "packages/desktop-cua/specs",
  tui: "packages/tui/specs",
};

function registeredSpecExtension(target) {
  if (target === "web" || target === "electron") return ".spec.ts";
  if (target === "tui" || target === "desktop:cua") return ".spec.mjs";
  return ".e2e.ts";
}

function productSpecExtension(target) {
  return target === "tui" || target === "desktop:cua" ? "mjs" : "ts";
}

function safeAuthorName(value, label) {
  const clean = String(value || "").trim();
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(clean)) {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.CONFIG,
      message: `${label} must be one safe path name: ${value}`,
    });
  }
  return clean;
}

function stateUri(kind) {
  return `stado://probierz/${kind}`;
}
function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\"'\"'")}'`;
}

function dedicatedStadoHost({ host, target, consumer, platform, description }) {
  return {
    host,
    kind: "stado",
    platform,
    target,
    request: {
      provider: "local",
      pin_to_provider: true,
      // The queue matches a worker consumer, not the registry target id.
      pinned_host: consumer.toLowerCase(),
    },
    description,
  };
}


export function listHosts() {
  return [
    { host: "local", kind: "local", description: "this machine (default)" },
    { host: "stado:gcp", kind: "stado", request: { provider: "gcp", pin_to_provider: true }, description: "stado queue, GCP consumers only" },
    { host: "stado:azure", kind: "stado", request: { provider: "azure", pin_to_provider: true }, description: "stado queue, Azure consumers only" },
    { host: "stado:aws", kind: "stado", request: { provider: "aws", pin_to_provider: true }, description: "stado queue, AWS consumers only" },
    { host: "stado:any", kind: "stado", request: {}, description: "stado queue, any consumer with capacity" },
    { host: "stado:spot", kind: "stado", request: { max_cost_per_hour_usd: Number("4") }, description: "stado queue, cost-capped capacity" },
    { host: "stado:local", kind: "stado", request: { provider: "local", pin_to_provider: true }, description: "stado queue, local-kind consumers only" },
    dedicatedStadoHost({ host: "stado:mini", target: "charless-mac-mini", consumer: "local-charless-mac-mini.local", platform: "darwin", description: "stado queue, dedicated Mac mini consumer" }),
    dedicatedStadoHost({ host: "stado:ubuntu", target: "ubuntu-server-rtx-pro-6000", consumer: "local-ubuntu-server", platform: "linux", description: "stado queue, dedicated Ubuntu consumer" }),
    dedicatedStadoHost({ host: "stado:macbook", target: "lukasz-macbook", consumer: "local-lukaszs-macbook-pro-5485.local", platform: "darwin", description: "stado queue, dedicated MacBook consumer" }),
    { host: "stado:t4", kind: "stado", request: { gpu_type: "nvidia-tesla-t4" }, description: "stado queue, nvidia-tesla-t4 capacity" },
  ];
}

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  // `error` carries the spawn failure itself (a missing `stado` binary), which
  // neither stream reports and which classifies very differently from a
  // command that ran and refused.
  return { command, args, status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || ""), error: out.error || null, signal: out.signal || null };
}

/**
 * Everything a spawned command told us, in the order that classifies best:
 * the spawn error first, then stderr, then stdout.
 */
function processText(out) {
  return [out.error?.message, out.stderr, out.stdout].filter(Boolean).join(" ").trim();
}

function exitText(out) {
  return out.status === null ? "none" : String(out.status);
}

/**
 * A call into the stado queue or its object store that did not complete.
 * Unrecognisable wording defaults to `infra_down` rather than `unknown`: the
 * remote side failed to do what we asked, and "we cannot tell why" must not be
 * downgraded into a shrug the operator ignores.
 */
function remoteFailure(point, action, out) {
  // The bounded failure summary can end inside a URL before the actual cause.
  process.stderr.write(`probierz-process-failure ${JSON.stringify({
    failure_point: point,
    command: out.command || STADO_BIN,
    args: out.args || [],
    exit_code: out.status,
    stdout: out.stdout,
    stderr: out.stderr,
    error: out.error?.message || null,
  })}\n`);
  return failureFrom({
    point,
    error: processText(out) || null,
    detail: `${STADO_BIN} exit ${exitText(out)}`,
    action,
    fallbackCode: CODE.INFRA_DOWN,
  });
}

/** A local packaging step. Nothing remote is implicated, so no outage default. */
function localFailure(point, action, out) {
  return failureFrom({ point, error: processText(out) || null, detail: `tar exit ${exitText(out)}`, action });
}
function requireGuiReady(hostDef, target) {
  if (target !== "desktop:cua") return;
  if (!hostDef.target) {
    throw new FailureError({
      point: "stado.preflight",
      code: CODE.CONFIG,
      detail: `host ${hostDef.host} has no registry target for GUI readiness`,
      message: `The selected host "${hostDef.host}" cannot prove a usable macOS GUI session.`,
    });
  }
  const started = Date.now();
  const status = sh(STADO_BIN, ["host", "gui-automation", "status", hostDef.target], {
    env: process.env,
    timeout: GUI_STATUS_CALL_TIMEOUT_MS,
  });
  if (status.error?.code === "ETIMEDOUT") {
    throw new FailureError({
      point: "stado.preflight",
      code: CODE.TIMEOUT,
      detail: `target=${hostDef.target}; deadline_ms=${GUI_STATUS_CALL_TIMEOUT_MS}; elapsed_ms=${Date.now() - started}; signal=${status.signal}; terminated_before_gui_job_submission=true; ${processText(status)}`,
      message: `The GUI readiness audit for ${hostDef.target} exceeded its deadline. Readiness is unknown; no GUI job was submitted.`,
    });
  }
  if (status.status !== Number("0")) {
    throw remoteFailure("stado.preflight", `Reading GUI readiness for ${hostDef.target} failed`, status);
  }
  const fields = new Map(
    status.stdout
      .split("\n")
      .map((line) => line.trim().split("\t"))
      .filter((parts) => parts.length >= Number("3"))
      .map((parts) => [parts[Number("1")], parts.slice(Number("2")).join("\t")]),
  );
  const consoleOwner = fields.get("console") || "unknown";
  const accessibility = fields.get("accessibility") || "unknown";
  // The worker's setup owns daemon startup. Requiring its socket here would
  // prevent that setup from repairing an absent daemon, even with GUI access.
  const guiAccess = !["", "root", "loginwindow", "unknown"].includes(consoleOwner)
    && fields.get("accessibility-user") === consoleOwner
    && fields.get("automated-session-declared") === "yes"
    && fields.get("cua-driver-app") === "present"
    && accessibility === "granted";
  if (!guiAccess) {
    throw new FailureError({
      point: "stado.preflight",
      code: CODE.CONFIG,
      detail: `target=${hostDef.target}; console=${consoleOwner}; accessibility=${accessibility}; gui-ready=${fields.get("gui-ready") || "not-reported"}`,
      message: `The selected host "${hostDef.host}" is not ready for desktop:cua: it needs an active macOS console session and a granted CuaDriver.`,
    });
  }
}

function packSource(repoRoot, file, extraPaths = []) {
  const workspace = mkdtempSync(workPath("source-"));
  const snapshot = path.join(workspace, "checkout");
  try {
    const cloned = sh("git", ["clone", "--no-checkout", "--no-local", "--single-branch", repoRoot, snapshot]);
    if (cloned.status !== 0) throw localFailure("stado.pack", `Copying the Git source identity for ${repoRoot} failed`, cloned);
    const indexed = sh("git", ["-C", snapshot, "reset", "--mixed", "HEAD"]);
    if (indexed.status !== 0) throw localFailure("stado.pack", `Preparing the source index for ${repoRoot} failed`, indexed);
    for (const relative of repositorySourceFiles(repoRoot, { includePackageLock: true })) {
      const destination = path.join(snapshot, relative);
      mkdirSync(path.dirname(destination), { recursive: true });
      cpSync(path.join(repoRoot, relative), destination, { verbatimSymlinks: true });
    }
    for (const relative of extraPaths) {
      cpSync(path.join(repoRoot, relative), path.join(snapshot, relative), { recursive: true, verbatimSymlinks: true });
    }
    const packed = sh("tar", ["-czf", file, "."], { cwd: snapshot });
    if (packed.status !== 0) throw localFailure("stado.pack", `Packing ${repoRoot} failed`, packed);
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
}

function packRepo(appIds) {
  const hash = createHash("sha256").update(`${Date.now()}-${Math.random()}`).digest("hex").slice(0, Number("12"));
  const file = workPath(`probierz-${hash}.tar.gz`);
  for (const appId of appIds) {
    if (!existsSync(path.join(ROOT, "apps", appId))) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.NOT_FOUND,
        detail: `app manifest not found: apps/${appId}`,
        message: `No app manifest for "${appId}". Register it under apps/ before submitting a remote run.`,
      });
    }
  }
  packSource(ROOT, file, appIds.map((appId) => `apps/${appId}`));
  return { file, hash };
}

function packAppSource(appId, repoRoot) {
  const hash = createHash("sha256").update(`${appId}-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = workPath(`${appId}-${hash}.tar.gz`);
  packSource(repoRoot, file);
  return { file, hash };
}

function packAppBundle(appId, bundlePath) {
  const bundleName = path.basename(bundlePath);
  const hash = createHash("sha256").update(`${appId}-app-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = workPath(`${appId}-app-${hash}.tar.gz`);
  // -C into the bundle's parent so the tarball root is <Bundle>.app itself.
  const packed = sh("tar", ["-czf", file, "-C", path.dirname(bundlePath), bundleName]);
  if (packed.status !== Number("0")) throw localFailure("stado.pack", `Packing the ${appId} application bundle failed`, packed);
  return { file, hash, bundleName };
}

function fileSha256(file) {
  const digest = createHash("sha256");
  const chunk = Buffer.allocUnsafe(1024 * 1024);
  const descriptor = openSync(file, "r");
  try {
    while (true) {
      const bytes = readSync(descriptor, chunk, Number("0"), chunk.length, null);
      if (bytes === Number("0")) return digest.digest("hex");
      digest.update(chunk.subarray(Number("0"), bytes));
    }
  } finally {
    closeSync(descriptor);
  }
}

function appBinaryInput(appId, binaryPath) {
  const resolved = path.resolve(binaryPath || "");
  if (!binaryPath || !existsSync(resolved) || !lstatSync(resolved).isFile()) {
    throw new FailureError({
      point: "stado.pack",
      code: CODE.NOT_FOUND,
      detail: `app binary path missing or not a file: ${binaryPath || "(empty)"}`,
      message: "The --app-binary-path you gave is not a file. Supply the signed native executable.",
    });
  }
  const staged = workPath(`${appId}-binary-${Date.now()}-${process.pid}`);
  cpSync(resolved, staged);
  const sha256 = fileSha256(staged);
  return {
    file: staged,
    sha256,
    name: path.basename(resolved),
    inputName: `${appId}-binary-${sha256}`,
  };
}

/**
 * The exact source identity of this submission, measured here and carried to
 * the worker.
 *
 * Workers receive archives, not the original checkouts. Measure the source
 * identity on the submitter and retain it with sourceIdentityOrigin "submitter".
 */
function packSourceIdentity(appId, appRepo = null) {
  const identity = appSourceIdentity(appId, { primaryRoot: appRepo });
  const hash = createHash("sha256").update(JSON.stringify(identity)).digest("hex").slice(0, Number("12"));
  const file = workPath(`${appId}-source-${hash}.json`);
  writeFileSync(file, `${JSON.stringify(identity, null, Number("2"))}\n`);
  return { ...identity, file, hash };
}

function requireImmutableNativeProvision({ target, provision, appRepo, identity }) {
  if (provision?.kind !== "native-binary") return;
  if (target !== "tui") {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.CONFIG,
      detail: `native-binary provisioning requested for target ${target}`,
      message: "--app-binary-path is supported only for remote TUI runs and authoring.",
    });
  }
  if (!appRepo) {
    throw new FailureError({
      point: "stado.pack",
      code: CODE.CONFIG,
      detail: "native-binary provisioning needs the app source repository",
      message: "Remote native-binary provisioning needs --app-repo <path>.",
    });
  }
  const primarySource = identity.app?.repositories?.find(({ index }) => index === Number("0"));
  if (!primarySource?.gitSha || primarySource.dirty) {
    throw new FailureError({
      point: "stado.pack",
      code: CODE.CONFIG,
      detail: `git_sha=${primarySource?.gitSha || "(missing)"}; dirty=${primarySource?.dirty ?? "unknown"}`,
      message: "--app-binary-path requires --app-repo to be a clean committed source checkout.",
    });
  }
}

function submissionIdentityMetadata({ receiptDir, identity, provision, inputObjects }) {
  const sourceIdentityPath = path.join(receiptDir, "source-identity.json");
  cpSync(identity.file, sourceIdentityPath);
  const primarySource = identity.app?.repositories?.find(({ index }) => index === Number("0"));
  const binary = provision?.kind === "native-binary"
    ? {
      name: provision.binaryName,
      sha256: provision.binarySha256,
      sourceRevision: primarySource?.gitSha || null,
      input: inputObjects.binary.relative_path,
    }
    : null;
  const binaryIdentityPath = binary ? path.join(receiptDir, "binary-identity.json") : null;
  if (binaryIdentityPath) writeFileSync(binaryIdentityPath, `${JSON.stringify(binary, null, Number("2"))}\n`);
  return {
    receiptDir,
    sourceIdentityPath,
    sourceRevision: primarySource?.gitSha || null,
    ...(binary ? { binary, binaryIdentityPath } : {}),
  };
}

function manifestRepoRoot(appId) {
  const manifest = path.join(ROOT, "apps", appId, "probierz.yaml");
  if (!existsSync(manifest)) return null;
  const match = readFileSync(manifest, "utf8").match(/^\s*- root:\s*(\S.+)$/m);
  return match ? match[1].trim() : null;
}

function submittingProductRoot(appId, appRepo = null) {
  return path.resolve(appRepo || loadAppManifest(appId).repositories[Number("0")].root);
}

function authorSubmissionFile(jobId) {
  if (!/^job-[0-9a-f]{24}$/.test(jobId || "")) {
    throw new Error(`invalid remote authoring job ID: ${jobId}`);
  }
  return path.join(ROOT, "test-results", ".remote", jobId, "authoring-submission.json");
}

function saveAuthorSubmission({
  jobId,
  appId,
  journey,
  area,
  target,
  productRoot,
  sourceSha256,
  harnessSha256,
  installedSourceSha256 = null,
}) {
  const file = authorSubmissionFile(jobId);
  mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  writeFileSync(file, `${JSON.stringify({
    schemaVersion: Number("1"),
    jobId,
    appId,
    journey,
    area,
    target,
    productRoot,
    sourceSha256,
    harnessSha256,
    installedSourceSha256,
  }, null, Number("2"))}\n`, { mode: 0o600 });
  return file;
}

function readAuthorSubmission(jobId) {
  const file = authorSubmissionFile(jobId);
  if (!existsSync(file)) return null;
  try {
    const parsed = JSON.parse(readFileSync(file, "utf8"));
    return parsed?.schemaVersion === Number("1") && parsed.jobId === jobId ? { ...parsed, file } : null;
  } catch {
    return null;
  }
}

/** Sleep without a timer: `upload` is synchronous, and so is everything that
 * calls it. */
function pause(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(Number("4"))), Number("0"), Number("0"), ms);
}

function upload(localFile, name) {
  const destination = `${stateUri("inputs")}/${name}`;
  let out;
  for (let attempt = Number("1"); ; attempt += Number("1")) {
    out = sh(STADO_BIN, ["storage", "put", destination, localFile]);
    if (out.status === Number("0")) return destination;
    // A multi-megabyte input goes up in 128 KB chunks, and the object store
    // restarts under it whenever the control plane redeploys: one chunk of
    // sixty-eight answers "infrastructure we depend on is unreachable" and the
    // whole submission dies. Exit 69 is the queue's own word for "not your
    // fault, try later" — so try later, here, instead of returning a verdict
    // of "outage" to an operator who can only run the same command again.
    if (out.status !== EXIT_RETRY || attempt >= UPLOAD_ATTEMPTS) break;
    pause(attempt * UPLOAD_BACKOFF_MS);
  }
  // The local path is diagnostic; it rides the log line, not the message.
  throw remoteFailure("stado.upload", `Uploading ${name} to the stado object store failed`, { ...out, stderr: `${out.stderr} (source ${localFile}, ${UPLOAD_ATTEMPTS} attempts)` });
}

function runScript({ target, appId, spec, provision, hash, platform = null, mode = "run", author = null, modelRouterUrl = null, record = false, environment = [] }) {
  // Remote jobs may receive secret_env values. Shell xtrace would copy any
  // expanded bearer into the canonical Stado command log.
  const lines = ["set -euo pipefail"];
  lines.push('JOB_ROOT="$PWD"', "mkdir -p output work", 'export TMPDIR="$JOB_ROOT/work"');
  lines.push(
    'export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"',
    'export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"',
    'export PATH="$CARGO_HOME/bin:$PATH"',
  );
  if (platform === "darwin") {
    // macOS runner: jobs spawned by the stado agent get a bare /bin/sh PATH,
    // so put homebrew on PATH first (stado and node live there on the mini).
    // Then use the node already on the box when present, else fetch the
    // darwin-arm64 tarball.
    lines.push(
      "export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH",
      `command -v node >/dev/null 2>&1 || { curl -fsSL https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-darwin-arm64.tar.gz -o "$TMPDIR/node.tar.gz" && tar -xzf "$TMPDIR/node.tar.gz" -C "$TMPDIR" && export PATH="$TMPDIR/node-${NODE_VERSION}-darwin-arm64/bin:$PATH"; }`,
    );
  } else if (platform === "linux") {
    lines.push(
      `curl -fsSL https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz -o "$TMPDIR/node.tar.xz"`,
      'tar -xJf "$TMPDIR/node.tar.xz" -C "$TMPDIR"',
      `export PATH="$TMPDIR/node-${NODE_VERSION}-linux-x64/bin:$PATH"`,
    );
  } else {
    // Generic selectors can land on either supported worker family. Resolve
    // the archive coordinate on the worker rather than assuming Linux x64.
    lines.push(
      'readonly PROBIERZ_WORKER_OS="$(uname -s)"',
      'readonly PROBIERZ_WORKER_ARCH="$(uname -m)"',
      'case "$PROBIERZ_WORKER_OS:$PROBIERZ_WORKER_ARCH" in',
      '  Darwin:arm64) PROBIERZ_NODE_PLATFORM=darwin-arm64; PROBIERZ_NODE_EXTENSION=tar.gz ;;',
      '  Darwin:x86_64) PROBIERZ_NODE_PLATFORM=darwin-x64; PROBIERZ_NODE_EXTENSION=tar.gz ;;',
      '  Linux:aarch64|Linux:arm64) PROBIERZ_NODE_PLATFORM=linux-arm64; PROBIERZ_NODE_EXTENSION=tar.xz ;;',
      '  Linux:x86_64|Linux:amd64) PROBIERZ_NODE_PLATFORM=linux-x64; PROBIERZ_NODE_EXTENSION=tar.xz ;;',
      '  *) printf \'Unsupported Stado worker OS/architecture: %s/%s (supported: Darwin or Linux on arm64 or x64)\\n\' "$PROBIERZ_WORKER_OS" "$PROBIERZ_WORKER_ARCH" >&2; exit 1 ;;',
      "esac",
      'if [ "$PROBIERZ_WORKER_OS" = Darwin ]; then export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH; fi',
      "if ! command -v node >/dev/null 2>&1; then",
      `  PROBIERZ_NODE_ARCHIVE="node-${NODE_VERSION}-$PROBIERZ_NODE_PLATFORM.$PROBIERZ_NODE_EXTENSION"`,
      `  curl -fsSL "https://nodejs.org/dist/${NODE_VERSION}/$PROBIERZ_NODE_ARCHIVE" -o "$TMPDIR/$PROBIERZ_NODE_ARCHIVE"`,
      '  case "$PROBIERZ_NODE_EXTENSION" in',
      '    tar.gz) tar -xzf "$TMPDIR/$PROBIERZ_NODE_ARCHIVE" -C "$TMPDIR" ;;',
      '    tar.xz) tar -xJf "$TMPDIR/$PROBIERZ_NODE_ARCHIVE" -C "$TMPDIR" ;;',
      "  esac",
      `  export PATH="$TMPDIR/node-${NODE_VERSION}-$PROBIERZ_NODE_PLATFORM/bin:$PATH"`,
      "fi",
    );
  }
  lines.push(
    `mkdir -p "$JOB_ROOT/work/probierz" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/probierz.tar.gz" -C "$JOB_ROOT/work/probierz"`,
  );
  if (provision?.kind === "installed-tui") {
    lines.push(`export TUI_CMD=${shellQuote(provision.path)}`);
  }
  if (provision?.kind === "native-binary") {
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}"`,
      `cp "$JOB_ROOT/inputs/${provision.appId}.binary" "$JOB_ROOT/work/${provision.appId}-binary"`,
      `chmod 0755 "$JOB_ROOT/work/${provision.appId}-binary"`,
      `export TUI_CMD="$JOB_ROOT/work/${provision.appId}-binary"`,
      'export PROBIERZ_BUILD_PATH="$TUI_CMD"',
    );
  }
  if (provision?.kind === "cargo-release") {
    const manifestPath = provision.manifestPath || "Cargo.toml";
    const manifestDir = path.posix.dirname(manifestPath);
    const targetPrefix = manifestDir === "." ? "" : `${manifestDir}/`;
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}"`,
      `readonly PROBIERZ_CARGO_TARGET_DIR="$PROBIERZ_APP_SOURCE/${targetPrefix}target"`,
      'export CARGO_TARGET_DIR="$PROBIERZ_CARGO_TARGET_DIR"',
      'trap \'rm -rf -- "$PROBIERZ_CARGO_TARGET_DIR"\' EXIT',
      "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }",
      `(cd "$PROBIERZ_APP_SOURCE/${manifestDir}" && cargo build --locked --release --bins)`,
      `export TUI_CMD="$JOB_ROOT/work/${provision.appId}/${targetPrefix}target/release/${provision.binary || provision.appId}"`,
    );
  }
  if (provision?.kind === "app-bundle") {
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${provision.appId}-app.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export MAC_APP_PATH="$JOB_ROOT/work/${provision.appId}/${provision.bundleName}"`,
      `mkdir -p "$JOB_ROOT/work/${provision.appId}-src" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}-src"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}-src"`,
    );
  }
  if (provision?.kind === "app-bundle" && target === "desktop:cua") {
    lines.push(
      'CUA_EXECUTABLE=$(/usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$MAC_APP_PATH/Contents/Info.plist")',
      'export CUA_APP_EXECUTABLE="$MAC_APP_PATH/Contents/MacOS/$CUA_EXECUTABLE"',
    );
    if (needsStadoDesktopCli(target, provision)) {
      lines.push(
        "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }",
        'export CARGO_TARGET_DIR="$PROBIERZ_APP_SOURCE/stado-rs/target"',
        '(cd "$PROBIERZ_APP_SOURCE/stado-rs" && cargo build --locked --bin stado)',
        'export PROBIERZ_STADO_BIN="$CARGO_TARGET_DIR/debug/stado"',
        'export PATH="$CARGO_TARGET_DIR/debug:$PATH"',
      );
    }
  }
  if (provision?.kind === "node-source") {
    // Generic JS app sources are staged as immutable job inputs.
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}"`,
    );
  }
  if (mode === "author" && (!provision || provision.kind === "installed-tui")) {
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${appId}" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/${appId}.tar.gz" -C "$JOB_ROOT/work/${appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${appId}"`,
    );
  }
  for (const [key, value] of environment) {
    lines.push(`export ${key}=${shellQuote(value)}`);
  }
  lines.push(
    `cd "$JOB_ROOT/work/probierz"`,
    // The submitter measured the source; the worker records that answer rather
    // than hashing checkouts it does not have. This replaces rewriting the
    // manifest's repository roots to the staged copies, which only ever
    // produced an identity of the copy — and, for a target with no staged app
    // source, produced nothing at all and killed the run.
    'export PROBIERZ_SOURCE_IDENTITY="$JOB_ROOT/inputs/source-identity.json"',
  );
  if (mode === "author" || (mode !== "run" && ["app-bundle", "cargo-release", "native-binary", "node-source"].includes(provision?.kind))) {
    // Authoring and custom scripts read staged source from the worker's
    // manifest. Ordinary runs retain the submitting source identity without
    // rewriting their source, and the submitter's manifest is updated only
    // from an accepted authoring receipt after the product bytes return.
    const srcDir = provision?.kind === "app-bundle"
      ? `$JOB_ROOT/work/${provision.appId}-src`
      : `$JOB_ROOT/work/${provision?.appId || appId}`;
    lines.push(
      `perl -pi -e "s|^  - root: .*|  - root: ${srcDir}|" apps/${appId}/probierz.yaml`,
    );
  }
  if (["mobile:ios", "mobile:android", "desktop:mac", "desktop:win"].includes(target)) {
    const appiumEnvironment = target === "desktop:mac" ? "appium-2-mac2-2.2.2" : "appium-2";
    lines.push(`export APPIUM_HOME="$HOME/.cache/probierz/${appiumEnvironment}"`);
  }
  lines.push("npm ci --no-audit --no-fund --loglevel=error");
  // TUI setup only installs npm dependencies, already locked above. Other
  // surfaces also need host drivers; every run still performs its preflight.
  if (target !== "tui") lines.push(`node agent/cli.mjs setup ${target}`);
  if (mode === "script") {
    // Custom app job (e.g. game_asset_creator sculpt/eval): run an app-owned
    // script from the probierz checkout after provisioning. The script writes
    // its artifacts into test-results/, which always comes back.
    lines.push(
      "mkdir -p test-results",
      "set +e",
      `bash apps/${appId}/${provision.script}`,
      "PROBIERZ_RUN_RC=$?",
      "set -e",
      `tar -czf "$JOB_ROOT/output/probierz-run-${hash}.tar.gz" test-results`,
      "exit $PROBIERZ_RUN_RC",
    );
    return lines.join("\n");
  }
  if (mode === "author") {
    // Remote authoring returns retained run evidence plus receipt-bound accepted
    // bytes; only the submitter installs the product spec and local manifest.
    if (!modelRouterUrl) throw new Error("remote authoring needs STADO_MODEL_ROUTER_URL");
    lines.push(
      `export STADO_MODEL_ROUTER_URL=${shellQuote(modelRouterUrl)}`,
      ': "${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}"',
      "export PROBIERZ_MODEL_AGENT_ID=probierz",
      ': "${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}"',
    );
    lines.push(`export PROBIERZ_AUTHOR_RECEIPT_ID=${shellQuote(author.receiptId || hash)}`);
    if (["mobile:ios", "mobile:android", "desktop:mac", "desktop:win"].includes(target)) {
      lines.push(
        "pkill -f '[a]ppium.*--port 4723' >/dev/null 2>&1 || true",
        "npx appium --relaxed-security --port 4723 > /tmp/appium.log 2>&1 &",
        "APPIUM_PID=$!",
        "trap 'kill \"$APPIUM_PID\" >/dev/null 2>&1 || true' EXIT",
        "export PROBIERZ_EXTERNAL_APPIUM=1",
        "for i in $(seq 1 30); do nc -z 127.0.0.1 4723 && break; sleep 2; done",
        "nc -z 127.0.0.1 4723",
      );
    }
    const appPathArgument = target === "web"
      ? ""
      : ` --app-path "${target === "tui" ? "$TUI_CMD" : "$MAC_APP_PATH"}"`;
    lines.push(
      "set +e",
      `node agent/cli.mjs author-spec ${shellQuote(appId)} ${shellQuote(author.journey)} --area ${shellQuote(author.area)} --target ${shellQuote(target)} --desc ${shellQuote(author.desc)}${appPathArgument}`,
      "PROBIERZ_RC=$?",
      "set -e",
      "mkdir -p test-results",
      `tar -czf "$JOB_ROOT/output/probierz-author-${hash}.tar.gz" test-results`,
      "exit $PROBIERZ_RC",
    );
    return lines.join("\n");
  }
  const hasAppSource = ["app-bundle", "cargo-release", "native-binary", "node-source"].includes(provision?.kind);
  const runConditions = [
    "PROBIERZ_RUN_KIND=pull-request",
    target === "tui" && provision?.kind !== "node-source" ? 'TUI_CMD="$TUI_CMD"' : null,
    hasAppSource ? 'PROBIERZ_APP_SOURCE="$PROBIERZ_APP_SOURCE"' : null,
    target === "desktop:cua" ? 'CUA_APP_EXECUTABLE="$CUA_APP_EXECUTABLE"' : null,
    ...environment.map(([key, value]) => shellQuote(`${key}=${value}`)),
  ].filter(Boolean).join(" ");
  lines.push(
    // Evidence must survive a failing run: capture the exit code, tar and
    // upload whatever test-results exist, then re-emit the run's status so
    // the job's success/failure still reflects the tests.
    "set +e",
    `node agent/cli.mjs run ${target} --app ${appId}${hasAppSource ? ' --app-repo "$PROBIERZ_APP_SOURCE"' : ""}${spec ? ` --spec ${spec}` : ""}${record ? " --record" : ""} ${runConditions}`,
    "PROBIERZ_RUN_RC=$?",
    "set -e",
    `tar -czf "$JOB_ROOT/output/probierz-run-${hash}.tar.gz" test-results`,
    "exit $PROBIERZ_RUN_RC",
  );
  return lines.join("\n");
}

function submitMachine(hostDef, hash, kind, inputObjects, secretEnv = {}, requestedWatchBudgetMs = null) {
  const watchBudgetMs = positiveBudget(requestedWatchBudgetMs);
  if (!watchBudgetMs) throw new Error("remote submission requires a positive watch budget");
  const receiptDir = path.join(ROOT, "test-results", ".remote", `probierz-${kind}-${hash}`);
  mkdirSync(receiptDir, { recursive: true });
  const requestFile = path.join(receiptDir, "request.json");
  const request = {
    client_request_id: `probierz-${kind}-${hash}`,
    // `machine status` preserves the command, so the submitted budget remains
    // recoverable by a later `stado resume` without trusting today's manifest.
    command: `${WATCH_BUDGET_ENV}=${watchBudgetMs} bash inputs/run.sh`,
    output_uri: stateUri("results"),
    input_objects: inputObjects,
    secret_env: secretEnv,
    ...(hostDef.request || {}),
  };
  writeFileSync(requestFile, JSON.stringify(request));
  console.error(`probierz-remote-request ${JSON.stringify({ requestId: request.client_request_id, requestFile })}`);
  const submit = sh(STADO_BIN, ["machine", "submit", "--request-file", requestFile], {
    env: process.env,
  });
  writeFileSync(path.join(receiptDir, "submission.json"), JSON.stringify({
    status: submit.status,
    signal: submit.signal,
    error: submit.error ? { code: submit.error.code, message: submit.error.message } : null,
    stdout: submit.stdout,
    stderr: submit.stderr,
  }, null, Number("2")));
  let jobId = null;
  try {
    const payload = JSON.parse(submit.stdout);
    jobId = payload?.ok ? payload.result?.job?.job_id || null : null;
  } catch {
    // A queue that answers with something other than its own protocol is a
    // queue that is not working; the text lands on the log line below.
  }
  if (jobId) {
    console.error(`probierz-remote-job ${JSON.stringify({ jobId, requestId: request.client_request_id, receiptDir })}`);
    return { jobId, watchBudgetMs, receiptDir, failure: null };
  }
  // The raw submit output is the operator's evidence, so it is logged in full
  // by `remoteFailure`; the caller gets the verdict, not the transcript.
  return {
    jobId: null,
    watchBudgetMs,
    receiptDir,
    failure: failureSummary(remoteFailure("stado.submit", "The stado queue did not accept the job", submit)),
  };
}

/**
 * A `machine status` call that itself fails is a blip until it is a pattern.
 * Three consecutive failures is the queue being unreachable, regardless of
 * how long the submitted run contract allows the job to execute.
 */
const STATUS_FAILURE_TOLERANCE = Number("3");

function terminalJobFailure(jobId, state, job) {
  const reported = job?.error;
  const detail = reported
    ? (typeof reported === "string" ? reported : JSON.stringify(reported))
    : state === "cancelled" && !job?.started_at
      ? "cancelled before the worker started; no run evidence was produced"
      : `worker reported ${state}`;
  return failureSummary(new FailureError({
    point: "stado.worker",
    code: CODE.UNKNOWN,
    detail,
    message: state === "cancelled" && !job?.started_at
      ? `Job ${jobId} was cancelled before a worker started; no run evidence was produced.`
      : `Job ${jobId} ${state} on the remote host: ${detail}`,
  }));
}

async function watchJob(jobId, hostDef, requestedWatchBudgetMs = null) {
  const requestedBudget = positiveBudget(requestedWatchBudgetMs);
  let watchBudgetMs = requestedBudget;
  let budgetSource = requestedBudget ? "submitted" : "original run";
  let deadline = Date.now() + (watchBudgetMs || SETUP_STEP_TIMEOUT_MS);
  let resolvedBudget = false;
  let anchoredStartAt = null;
  let consecutiveStatusFailures = Number("0");
  let lastAnsweredJob = null;
  let lastPollAnswered = false;
  while (Date.now() < deadline) {
    const out = sh(STADO_BIN, ["machine", "status", jobId], {
      env: process.env,
      timeout: Math.max(Number("1"), Math.min(STATUS_CALL_TIMEOUT_MS, deadline - Date.now())),
    });
    let payload = null;
    try { payload = JSON.parse(out.stdout); } catch {}
    const answered = Boolean(payload?.ok);
    lastPollAnswered = answered;
    const job = answered ? payload.result?.job : null;
    if (answered) lastAnsweredJob = job;
    if (answered && !resolvedBudget && !["failed", "cancelled", "completed", "uploaded"].includes(job?.state)) {
      const savedBudget = budgetFromJob(job);
      watchBudgetMs = savedBudget || requestedBudget || legacyWatchBudget(job, hostDef);
      budgetSource = savedBudget ? "saved submission" : requestedBudget ? "submitted" : "original application fallback";
      const submittedAt = Date.parse(String(job?.created_at || ""));
      deadline = Number.isFinite(submittedAt)
        ? submittedAt + watchBudgetMs
        : Date.now() + watchBudgetMs;
      resolvedBudget = true;
    }
    const startedAt = Date.parse(String(job?.started_at || ""));
    if (answered && startedAt !== anchoredStartAt && Number.isFinite(startedAt)) {
      // Queue wait is bounded from submission above, but once work starts the
      // declared budget belongs wholly to provisioning plus execution.
      deadline = startedAt + watchBudgetMs;
      anchoredStartAt = startedAt;
    }
    if (!answered) {
      consecutiveStatusFailures += Number("1");
      if (consecutiveStatusFailures >= STATUS_FAILURE_TOLERANCE) {
        return {
          state: "unreachable",
          watchBudgetMs,
          failure: failureSummary(remoteFailure("stado.watch", `The stado queue stopped answering about job ${jobId}`, out)),
        };
      }
    } else {
      consecutiveStatusFailures = Number("0");
    }
    const state = String(job?.state || "").toLowerCase();
    // A job the fleet failed or cancelled is a run result, not an outage: the
    // queue did its part. Preserve cancellation as its own terminal state so
    // evidence collection cannot turn it into a worker failure or object-store
    // outage.
    if (["failed", "cancelled"].includes(state)) {
      return {
        state,
        source: job?.resolved_input_artifacts?.source || null,
        job,
        watchBudgetMs,
        failure: terminalJobFailure(jobId, state, job),
        ...(state === "cancelled" && !job?.started_at
          ? { evidence: { required: false, collected: false, reason: "cancelled-before-start", retryable: false } }
          : {}),
      };
    }
    if (["uploaded", "completed"].includes(state)) {
      return {
        state: "completed",
        job,
        source: job?.resolved_input_artifacts?.source || null,
        watchBudgetMs,
        failure: null,
      };
    }
    if (Date.now() < deadline) {
      await new Promise((resolve) => { setTimeout(resolve, WATCH_INTERVAL_MS); });
    }
  }
  const lastState = String(lastAnsweredJob?.state || "running").toLowerCase();
  return {
    state: "watch-expired",
    job: lastAnsweredJob,
    source: lastAnsweredJob?.resolved_input_artifacts?.source || null,
    watchBudgetMs,
    failure: failureSummary(new FailureError({
      point: "stado.watch",
      code: CODE.UNKNOWN,
      detail: `job=${jobId}; state=${lastState}; watch_budget_ms=${watchBudgetMs}; budget_source=${budgetSource}; last_status_answered=${lastPollAnswered}`,
      message: lastPollAnswered
        ? `Probierz stopped watching job ${jobId} after its ${watchBudgetMs}ms ${budgetSource} budget; Stado was still answering and the job remains ${lastState}. Resume this job to continue waiting.`
        : `Probierz stopped watching job ${jobId} after its ${watchBudgetMs}ms ${budgetSource} budget; the last status read did not answer, but the queue-unreachable threshold was not reached. Resume this job to continue waiting.`,
    })),
  };
}

function provisionInputs({ appId, provision, appRepo, sourceRequired = false }) {
  const inputs = {};
  if (provision?.kind === "installed-tui") {
    if (!provision.path || !path.isAbsolute(provision.path)) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.CONFIG,
        detail: `installed TUI path must be absolute: ${provision.path || "(empty)"}`,
        message: "Remote installed-TUI authoring needs --app-path <absolute-path>.",
      });
    }
    if (sourceRequired) {
      const source = packAppSource(appId, submittingProductRoot(appId, appRepo));
      inputs.app = {
        stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
        relative_path: `inputs/${appId}.tar.gz`,
      };
    }
    return inputs;
  }
  if (provision?.kind === "native-binary") {
    if (!appRepo) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.CONFIG,
        detail: "native-binary provisioning needs the app source repository",
        message: "Remote native-binary provisioning needs --app-repo <path>.",
      });
    }
    const binary = appBinaryInput(appId, provision.binaryPath);
    provision.binarySha256 = binary.sha256;
    provision.binaryName = binary.name;
    inputs.binary = {
      stado_uri: upload(binary.file, binary.inputName),
      relative_path: `inputs/${appId}.binary`,
    };
    const source = packAppSource(appId, appRepo);
    inputs.app = {
      stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
      relative_path: `inputs/${appId}.tar.gz`,
    };
    return inputs;
  }
  if (provision?.kind === "cargo-release" || provision?.kind === "node-source") {
    if (!appRepo) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.CONFIG,
        detail: `${provision.kind} provisioning needs the app source repository`,
        message: `Remote ${provision.kind} provisioning needs --app-repo <path>.`,
      });
    }
    if (provision.kind === "cargo-release") {
      const manifestPath = provision.manifestPath || "Cargo.toml";
      if (!/^[A-Za-z0-9._/-]+$/.test(manifestPath) || path.posix.isAbsolute(manifestPath) || manifestPath.split("/").includes("..")) {
        throw new FailureError({
          point: "stado.pack",
          code: CODE.CONFIG,
          detail: `invalid cargo manifest path: ${manifestPath}`,
          message: "--cargo-manifest must be a safe path relative to --app-repo.",
        });
      }
      provision.manifestPath = manifestPath;
    }
    const source = packAppSource(appId, appRepo);
    inputs.app = {
      stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
      relative_path: `inputs/${appId}.tar.gz`,
    };
    return inputs;
  }
  if (provision?.kind === "app-bundle") {
    if (!provision.bundlePath || !existsSync(provision.bundlePath)) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.NOT_FOUND,
        detail: `app-bundle path missing: ${provision.bundlePath}`,
        message: "The --app-bundle-path you gave does not exist. Build the bundle first.",
      });
    }
    const bundle = packAppBundle(appId, provision.bundlePath);
    provision.bundleName = bundle.bundleName;
    inputs.bundle = {
      stado_uri: upload(bundle.file, `${appId}-app-${bundle.hash}.tar.gz`),
      relative_path: `inputs/${appId}-app.tar.gz`,
    };
    const sourceRepo = submittingProductRoot(appId, appRepo || manifestRepoRoot(appId));
    const source = packAppSource(appId, sourceRepo);
    inputs.app = {
      stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
      relative_path: `inputs/${appId}.tar.gz`,
    };
    return inputs;
  }
  if (sourceRequired) {
    const source = packAppSource(appId, submittingProductRoot(appId, appRepo));
    inputs.app = {
      stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
      relative_path: `inputs/${appId}.tar.gz`,
    };
  }
  return inputs;
}

function seoRunScript({ appId, baseUrl, mode, policyPath, briefPath, primaryModel, secondaryModel, adjudicatorModel, agentId, routerUrl, productionEvidence, signatureRequired, hash, platform = "linux" }) {
  const lines = ["set -euo pipefail", 'JOB_ROOT="$PWD"', "mkdir -p output work"];
  if (platform === "darwin") {
    lines.push(
      "export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH",
      `command -v node >/dev/null 2>&1 || { curl -fsSL https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-darwin-arm64.tar.gz -o /tmp/node.tar.gz && tar -xzf /tmp/node.tar.gz -C /tmp && export PATH=/tmp/node-${NODE_VERSION}-darwin-arm64/bin:$PATH; }`,
    );
  } else {
    lines.push(
      `curl -fsSL https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz -o /tmp/node.tar.xz`,
      "tar -xJf /tmp/node.tar.xz -C /tmp",
      `export PATH=/tmp/node-${NODE_VERSION}-linux-x64/bin:$PATH`,
    );
  }
  lines.push(
    'mkdir -p "$JOB_ROOT/work/probierz" && tar --no-same-owner -xzf "$JOB_ROOT/inputs/probierz.tar.gz" -C "$JOB_ROOT/work/probierz"',
    'cd "$JOB_ROOT/work/probierz"',
    "npm ci --no-audit --no-fund --loglevel=error",
    "node agent/cli.mjs setup web",
    `export STADO_MODEL_ROUTER_URL=${shellQuote(routerUrl)}`,
    `export PROBIERZ_MODEL_AGENT_ID=${shellQuote(agentId)}`,
    ': "${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}"',
    ': "${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}"',
  );
  if (signatureRequired) {
    lines.push(': "${PROBIERZ_SEO_RECEIPT_PRIVATE_KEY:?PROBIERZ_SEO_RECEIPT_PRIVATE_KEY was not materialized by Stado}"');
  }
  const args = [
    "node", "agent/cli.mjs", "seo-evaluate",
    "--app", appId,
    "--base-url", baseUrl,
    "--mode", mode,
    "--policy", policyPath,
    "--brief", briefPath,
    "--primary-model", primaryModel,
    "--secondary-model", secondaryModel,
    "--adjudicator-model", adjudicatorModel,
    "--agent-id", agentId,
  ];
  if (productionEvidence) args.push("--production-evidence", "$JOB_ROOT/inputs/production-evidence.json");
  lines.push(
    "set +e",
    args.map((value) => value.startsWith("$JOB_ROOT/") ? `\"${value}\"` : shellQuote(value)).join(" "),
    "PROBIERZ_SEO_RC=$?",
    "set -e",
    `tar -czf "$JOB_ROOT/output/probierz-seo-${hash}.tar.gz" test-results`,
    "exit $PROBIERZ_SEO_RC",
  );
  return lines.join("\n");
}

export async function submitRemoteSeo({
  appId = "landing-page",
  baseUrl,
  mode = "release",
  policyPath = null,
  briefPath = null,
  primaryModel,
  secondaryModel,
  adjudicatorModel,
  agentId = "probierz",
  productionEvidencePath = null,
  host = "stado:mini",
  watch = true,
} = {}) {
  const hostDef = listHosts().find((entry) => entry.host === host);
  if (!hostDef || hostDef.kind !== "stado") {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.NOT_FOUND,
      detail: `unknown stado host: ${host}`,
      message: `No such stado host: "${host}". Run \`probierz hosts\` for the list.`,
    });
  }
  if (!baseUrl || !primaryModel || !secondaryModel || !adjudicatorModel) {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.CONFIG,
      detail: "remote SEO evaluation needs base URL and three pinned model IDs",
      message: "Remote SEO evaluation needs --base-url, --primary-model, --secondary-model, and --adjudicator-model.",
    });
  }
  const manifest = loadAppManifest(appId);
  const profile = manifest.seo?.profiles?.[mode];
  if (!profile) throw new Error(`app ${appId} has no SEO profile for ${mode}`);
  if (profile.requireProductionEvidence && !productionEvidencePath) throw new Error(`${mode} SEO profile requires --production-evidence`);
  policyPath ||= manifest.seo.policy;
  briefPath ||= manifest.seo.brief;
  const packedRepo = packRepo([appId]);
  const repoUri = upload(packedRepo.file, `probierz-${packedRepo.hash}.tar.gz`);
  const routerUrl = stadoModelRouterUrl();
  const script = seoRunScript({
    appId, baseUrl, mode, policyPath, briefPath, primaryModel, secondaryModel,
    adjudicatorModel, agentId, routerUrl, productionEvidence: Boolean(productionEvidencePath),
    signatureRequired: profile.requireSignature, hash: packedRepo.hash, platform: hostDef.platform,
  });
  const scriptFile = workPath(`probierz-seo-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  const inputObjects = {
    repo: { stado_uri: repoUri, relative_path: "inputs/probierz.tar.gz" },
    script: { stado_uri: upload(scriptFile, `seo-${packedRepo.hash}.sh`), relative_path: "inputs/run.sh" },
  };
  if (productionEvidencePath) {
    if (!existsSync(productionEvidencePath)) throw new Error(`production SEO evidence not found: ${productionEvidencePath}`);
    inputObjects.productionEvidence = {
      stado_uri: upload(productionEvidencePath, `seo-production-${packedRepo.hash}.json`),
      relative_path: "inputs/production-evidence.json",
    };
  }
  const secretNames = ["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"];
  if (profile.requireSignature) secretNames.push("PROBIERZ_SEO_RECEIPT_PRIVATE_KEY");
  const requestedWatchBudgetMs = conservativeWatchBudget(appId);
  const { jobId, watchBudgetMs, failure } = submitMachine(
    hostDef,
    packedRepo.hash,
    "seo",
    inputObjects,
    remoteModelSecretEnv(secretNames),
    requestedWatchBudgetMs,
  );
  const result = { host, jobId, appId, mode, submitted: Boolean(jobId), watchBudgetMs };
  if (!jobId) return { ...result, state: "submit-failed", failure };
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef, watchBudgetMs);
  result.state = watched.state;
  result.watchBudgetMs = watched.watchBudgetMs;
  result.failure = watched.failure;
  if (watched.job) result.job = watched.job;
  if (watched.evidence) result.evidence = watched.evidence;
  if (["completed", "failed"].includes(watched.state)) {
    const retained = fetchRunEvidence(jobId, hostDef);
    if (retained) result.resultsDir = retained.resultsDir;
    if (retained?.artifactError) result.artifactError = retained.artifactError;
    if (watched.state === "completed" && !retained?.resultsDir) {
      result.state = "evidence-unavailable";
      result.failure = missingRunEvidenceFailure(
        jobId,
        `artifact_error=${JSON.stringify(retained?.artifactError || null)}`,
      );
    }
  }
  return result;
}

function missingRunEvidenceFailure(jobId, detail) {
  return failureSummary(failureFrom({
    point: "stado.download",
    error: Object.assign(new Error("required run evidence is missing"), { failureCode: CODE.NOT_FOUND }),
    detail: `job=${jobId}; ${detail}`,
    action: `Job ${jobId} completed without the required Probierz run evidence`,
    fallbackCode: CODE.NOT_FOUND,
  }));
}

function fetchRunEvidence(jobId, hostDef) {
  const jobDir = path.join(ROOT, "test-results", ".remote", jobId);
  // Recovery can be repeated. Keep the source receipt and every earlier
  // collection, but download into a staging directory so a failed transfer
  // never replaces retained evidence.
  mkdirSync(jobDir, { recursive: true });
  const stagingDir = workPath(`artifacts-${jobId}-${Date.now()}-${process.pid}`);
  mkdirSync(stagingDir, { recursive: true });
  let committed = false;
  try {
    const downloaded = sh(STADO_BIN, ["machine", "artifacts", jobId, "--output-dir", stagingDir], {
      env: process.env,
    });
    let payload;
    try {
      payload = JSON.parse(downloaded.stdout);
    } catch {
      throw remoteFailure("stado.download", "The queue returned invalid artifact metadata", downloaded);
    }
    if (!payload?.ok || downloaded.status !== Number("0")) {
      const upstream = payload?.error;
      if (upstream?.code === "NO_ARTIFACTS" && upstream.retryable === false) {
        process.stderr.write(`probierz-remote-artifacts ${JSON.stringify({ jobId, error: upstream })}\n`);
        return { resultsDir: null, manifest: null, authorReceipt: null, artifactError: upstream };
      }
      throw remoteFailure("stado.download", "Downloading the worker's retained artifacts failed", downloaded);
    }
    const artifacts = payload.result?.artifacts || [];
    if (!artifacts.length) return null;
    const artifact = artifacts.find(({ relative_path: relativePath }) =>
      /^probierz-(?:run|author|seo)-.*\.tar\.gz$/.test(String(relativePath || "")));
    let entries = [];
    let artifactRelative = null;
    if (artifact) {
      artifactRelative = String(artifact.relative_path || "");
      const stagedTarball = path.resolve(stagingDir, artifactRelative);
      if (stagedTarball === stagingDir || !stagedTarball.startsWith(`${stagingDir}${path.sep}`)) {
        throw new FailureError({
          point: "stado.download",
          code: CODE.CONFIG,
          message: `Remote evidence named an artifact outside its collection directory: ${artifact.relative_path}`,
        });
      }
      const listed = sh("tar", ["-tzf", stagedTarball], { cwd: ROOT });
      if (listed.status !== Number("0")) throw localFailure("stado.download", "Listing the retained evidence archive failed", listed);
      entries = listed.stdout.split("\n").filter(Boolean);
      for (const entry of entries) {
        const normalized = entry.replace(/^\.\/+/, "");
        const resolved = path.resolve(ROOT, normalized);
        if (
          (normalized !== "test-results" && !normalized.startsWith("test-results/"))
          || resolved === ROOT
          || !resolved.startsWith(`${ROOT}${path.sep}`)
        ) {
          throw new FailureError({
            point: "stado.download",
            code: CODE.CONFIG,
            message: `Remote evidence contains an unsafe retained path: ${entry}`,
          });
        }
      }
    }

    const destDir = mkdtempSync(path.join(jobDir, "collection-"));
    rmSync(destDir, { recursive: true, force: true });
    renameSync(stagingDir, destDir);
    committed = true;
    if (!artifactRelative) {
      return { resultsDir: destDir, manifest: null, authorReceipt: null };
    }

    const tarball = path.resolve(destDir, artifactRelative);
    const manifestEntries = entries.filter((entry) => entry.endsWith("/run-manifest.json"));
    const authorReceiptEntry = entries.find((entry) => entry.endsWith("/accepted.json"));
    const untar = sh("tar", ["-xzf", tarball, "-C", ROOT], { cwd: ROOT });
    if (untar.status !== Number("0")) throw localFailure("stado.download", "Extracting the retained evidence failed", untar);
    let authorReceipt = null;
    let authorReceiptFile = null;
    if (authorReceiptEntry) {
      authorReceiptFile = path.resolve(ROOT, authorReceiptEntry);
      try {
        authorReceipt = JSON.parse(readFileSync(authorReceiptFile, "utf8"));
      } catch {}
    }
    let manifest = null;
    for (const manifestEntry of manifestEntries) {
      try {
        const candidate = JSON.parse(readFileSync(path.resolve(ROOT, manifestEntry), "utf8"));
        if (!authorReceipt?.runId || candidate.runId === authorReceipt.runId) {
          manifest = candidate;
          break;
        }
      } catch {}
    }
    return { resultsDir: destDir, manifest, authorReceipt, authorReceiptFile };
  } finally {
    if (!committed) rmSync(stagingDir, { recursive: true, force: true });
  }
}

function restoreRemoteAuthoring(jobId, retained, expectedAppId = null, required = false) {
  const receipt = retained?.authorReceipt;
  if (!receipt) {
    if (required) {
      throw new FailureError({
        point: "stado.download",
        code: CODE.CONFIG,
        message: `Job ${jobId} completed authoring without a usable accepted-spec receipt.`,
      });
    }
    return null;
  }
  const submission = readAuthorSubmission(jobId);
  if (!submission) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned an authored spec, but this checkout has no source-bound submission receipt.`,
    });
  }
  const expectedProductRelative = path.posix.join(
    "tests",
    submission.area,
    `${submission.journey}.probierz.spec.${productSpecExtension(submission.target)}`,
  );
  const expectedRegistrationRelative = path.posix.join(
    TARGET_REGISTRATION_DIRS_REL[submission.target] || "",
    `${submission.appId}-${submission.journey}${registeredSpecExtension(submission.target)}`,
  );
  if (
    receipt.schemaVersion !== Number("1")
    || receipt.appId !== submission.appId
    || receipt.journey !== submission.journey
    || receipt.area !== submission.area
    || receipt.target !== submission.target
    || receipt.spec?.relativePath !== expectedProductRelative
    || receipt.registration?.relativePath !== expectedRegistrationRelative
    || !Array.isArray(receipt.mappingPaths)
    || receipt.mappingPaths.length !== Number("0")
    || !retained.manifest
    || retained.manifest.runId !== receipt.runId
    || retained.manifest.appId !== receipt.appId
    || retained.manifest.target !== receipt.target
    || retained.manifest.source?.sha256 !== submission.sourceSha256
    || retained.manifest.harness?.sha256 !== submission.harnessSha256
    || retained.manifest.sourceIdentityOrigin !== "submitter"
    || retained.manifest.status !== "passed"
    || (expectedAppId && receipt.appId !== expectedAppId)
  ) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned authoring metadata that does not match its submitting checkout.`,
    });
  }
  if (
    !receipt.sourceSha256
    || receipt.sourceSha256 !== submission.sourceSha256
    || !receipt.harnessSha256
    || receipt.harnessSha256 !== submission.harnessSha256
  ) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned an authored spec for a different source identity.`,
    });
  }
  const currentSourceSha256 = appSourceIdentity(submission.appId, {
    primaryRoot: submission.productRoot,
  }).app?.sha256;
  const expectedLocalSourceSha256 = submission.installedSourceSha256 || submission.sourceSha256;
  if (!currentSourceSha256 || currentSourceSha256 !== expectedLocalSourceSha256) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} cannot publish into a checkout whose source changed after submission.`,
    });
  }
  const artifactRoot = path.resolve(ROOT, "test-results");
  const acceptedArtifact = path.resolve(ROOT, String(receipt.spec?.artifact || ""));
  if (
    acceptedArtifact === artifactRoot
    || !acceptedArtifact.startsWith(`${artifactRoot}${path.sep}`)
    || !existsSync(acceptedArtifact)
  ) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned an authored spec outside retained Probierz artifacts.`,
    });
  }
  const content = readFileSync(acceptedArtifact);
  const digest = createHash("sha256").update(content).digest("hex");
  if (content.length !== Number(receipt.spec.bytes) || digest !== receipt.spec.sha256) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned authored spec bytes that do not match its receipt.`,
    });
  }
  const installed = installAcceptedSpec({
    appId: receipt.appId,
    journey: receipt.journey,
    area: receipt.area,
    target: receipt.target,
    content,
    mappingPaths: Array.isArray(receipt.mappingPaths) ? receipt.mappingPaths : [],
    productRoot: submission.productRoot,
  });
  const productRelative = path.relative(submission.productRoot, installed.spec).split(path.sep).join("/");
  const registrationRelative = path.relative(ROOT, installed.registration).split(path.sep).join("/");
  if (productRelative !== receipt.spec.relativePath || registrationRelative !== receipt.registration?.relativePath) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} returned authored paths that do not match the local registration contract.`,
    });
  }
  const installedSourceSha256 = appSourceIdentity(submission.appId, {
    primaryRoot: submission.productRoot,
  }).app?.sha256;
  if (!installedSourceSha256) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: `Job ${jobId} installed an authored spec, but its product source identity is unavailable.`,
    });
  }
  saveAuthorSubmission({ ...submission, installedSourceSha256 });
  return {
    productSpec: installed.spec,
    registration: installed.registration,
    appManifest: installed.manifest,
    authorReceipt: retained.authorReceiptFile,
    sourceReceipt: submission.file,
  };
}

export function collectRemoteRun({ jobId, appId, host = "stado:mini" }) {
  const hostDef = listHosts().find((entry) => entry.host === host && entry.kind === "stado");
  if (!hostDef || !/^job-[0-9a-f]{24}$/.test(jobId || "")) {
    throw new FailureError({
      point: "stado.download",
      code: CODE.CONFIG,
      message: "Collection requires a canonical Stado job ID and a known Stado host.",
    });
  }
  loadAppManifest(appId);
  const status = sh(STADO_BIN, ["machine", "status", jobId], {
    env: process.env,
    timeout: STATUS_CALL_TIMEOUT_MS,
  });
  if (status.status !== Number("0")) {
    throw remoteFailure("stado.watch", `Reading job ${jobId} failed`, status);
  }
  let payload;
  try {
    payload = JSON.parse(status.stdout);
  } catch {
    throw remoteFailure("stado.watch", `Reading job ${jobId} returned invalid status`, status);
  }
  const job = payload?.result?.job;
  const state = String(job?.state || "").toLowerCase();
  if (!payload.ok || !state) throw remoteFailure("stado.watch", `Reading job ${jobId} returned no state`, status);
  const result = {
    host,
    jobId,
    appId,
    state,
    submitted: true,
    collected: false,
    job,
    source: job?.resolved_input_artifacts?.source || null,
  };
  if (!["uploaded", "completed", "failed", "cancelled"].includes(state)) return result;
  if (state === "cancelled" && !job?.started_at) {
    return {
      ...result,
      failure: terminalJobFailure(jobId, state, job),
      evidence: { required: false, collected: false, reason: "cancelled-before-start", retryable: false },
    };
  }
  const retained = fetchRunEvidence(jobId, hostDef);
  if (!retained?.manifest || retained.manifest.appId !== appId) {
    const terminalState = state === "uploaded" ? "completed" : state;
    if (terminalState !== "completed") {
      return {
        ...result,
        state: terminalState,
        artifactError: retained?.artifactError || null,
        failure: terminalJobFailure(jobId, terminalState, job),
      };
    }
    return {
      ...result,
      state: "evidence-unavailable",
      artifactError: retained?.artifactError || null,
      failure: missingRunEvidenceFailure(
        jobId,
        `app=${appId}; artifact_error=${JSON.stringify(retained?.artifactError || null)}`,
      ),
    };
  }
  const authored = restoreRemoteAuthoring(
    jobId,
    retained,
    appId,
    ["uploaded", "completed"].includes(state) && Boolean(readAuthorSubmission(jobId)),
  );
  return {
    ...result,
    state: state === "uploaded" ? "completed" : state,
    collected: true,
    resultsDir: retained.resultsDir,
    manifest: retained.manifest,
    ...(authored || {}),
  };
}

function captureRemoteLogs(jobId, hostDef, directory) {
  const logPath = path.join(directory, "command.log");
  const receiptPath = path.join(directory, "log-receipts.jsonl");
  writeFileSync(logPath, "");
  writeFileSync(receiptPath, "");
  let cursor = Number("0");
  while (true) {
    const page = sh(STADO_BIN, ["machine", "logs", jobId, "--cursor", String(cursor), "--limit", "65536"], {
      env: process.env,
      timeout: STATUS_CALL_TIMEOUT_MS,
    });
    appendFileSync(receiptPath, `${page.stdout.trim()}\n`);
    let payload;
    try {
      payload = JSON.parse(page.stdout);
    } catch {
      return {
        logPath,
        receiptPath,
        failure: failureSummary(remoteFailure("stado.download", `Reading logs for job ${jobId} returned invalid metadata`, page)),
      };
    }
    if (!payload?.ok || page.status !== Number("0")) {
      return {
        logPath,
        receiptPath,
        failure: failureSummary(remoteFailure("stado.download", `Reading logs for job ${jobId} failed`, page)),
      };
    }
    const result = payload.result || {};
    appendFileSync(logPath, String(result.text || ""));
    if (result.eof === true) return { logPath, receiptPath, failure: null };
    const nextCursor = Number(result.next_cursor);
    if (!Number.isSafeInteger(nextCursor) || nextCursor <= cursor) {
      return {
        logPath,
        receiptPath,
        failure: failureSummary(new FailureError({
          point: "stado.download",
          code: CODE.UNKNOWN,
          detail: `job=${jobId}; cursor=${cursor}; next_cursor=${result.next_cursor}`,
          message: `Stado returned an invalid log cursor for job ${jobId}; the pages received so far were retained.`,
        })),
      };
    }
    cursor = nextCursor;
  }
}

export function cancelRemoteRun({ jobId, host = "stado:any", reason }) {
  if (!/^job-[0-9a-f]{24}$/.test(jobId || "")) {
    throw new FailureError({
      point: "stado.watch",
      code: CODE.CONFIG,
      detail: "job ID must be a canonical Stado job identifier",
      message: "Cancelling remote evidence needs a canonical Stado job ID.",
    });
  }
  const hostDef = listHosts().find((entry) => entry.host === host && entry.kind === "stado");
  if (!hostDef) {
    throw new FailureError({
      point: "stado.watch",
      code: CODE.NOT_FOUND,
      detail: `unknown stado host: ${host}`,
      message: `No such stado host: "${host}". Run \`probierz hosts\` for the list.`,
    });
  }
  const cancellationReason = typeof reason === "string" ? reason.trim() : "";
  if (!cancellationReason || cancellationReason.includes("\0")) {
    throw new FailureError({
      point: "stado.watch",
      code: CODE.CONFIG,
      detail: "cancellation reason must be non-empty and contain no NUL bytes",
      message: "Cancelling a remote run needs --reason <reason>.",
    });
  }
  const requestedAt = new Date();
  const attemptId = `${requestedAt.toISOString().replace(/[^0-9]/g, "")}-${createHash("sha256").update(`${process.pid}-${Math.random()}`).digest("hex").slice(Number("0"), Number("8"))}`;
  const cancellationRoot = path.join(ROOT, "test-results", ".remote", "cancellations", jobId);
  const cancellationDir = path.join(cancellationRoot, attemptId);
  mkdirSync(cancellationDir, { recursive: true });
  const requestPath = path.join(cancellationDir, "request.json");
  writeFileSync(requestPath, `${JSON.stringify({
    schemaVersion: Number("1"),
    jobId,
    host,
    reason: cancellationReason,
    requestedAt: requestedAt.toISOString(),
  }, null, Number("2"))}\n`);
  const options = {
    env: process.env,
    timeout: STATUS_CALL_TIMEOUT_MS,
  };
  const before = sh(STADO_BIN, ["machine", "status", jobId], options);
  const statusBeforePath = path.join(cancellationDir, "status-before.json");
  writeFileSync(statusBeforePath, before.stdout);
  writeFileSync(path.join(cancellationDir, "status-before-process.json"), `${JSON.stringify({
    status: before.status,
    signal: before.signal,
    error: before.error ? { code: before.error.code, message: before.error.message } : null,
    stdout: before.stdout,
    stderr: before.stderr,
  }, null, Number("2"))}\n`);
  let beforePayload;
  try {
    beforePayload = JSON.parse(before.stdout);
  } catch {
    throw remoteFailure("stado.watch", `Reading the original state for job ${jobId} returned invalid metadata`, before);
  }
  const originalJob = beforePayload?.result?.job;
  if (!beforePayload?.ok || before.status !== Number("0") || !originalJob) {
    throw remoteFailure("stado.watch", `Reading the original state for job ${jobId} failed`, before);
  }

  const cancellation = sh(STADO_BIN, ["machine", "cancel", jobId], options);
  const receiptPath = path.join(cancellationDir, "receipt.json");
  writeFileSync(receiptPath, cancellation.stdout);
  writeFileSync(path.join(cancellationDir, "receipt-process.json"), `${JSON.stringify({
    status: cancellation.status,
    signal: cancellation.signal,
    error: cancellation.error ? { code: cancellation.error.code, message: cancellation.error.message } : null,
    stdout: cancellation.stdout,
    stderr: cancellation.stderr,
  }, null, Number("2"))}\n`);
  let cancellationPayload;
  try {
    cancellationPayload = JSON.parse(cancellation.stdout);
  } catch {
    throw remoteFailure("stado.watch", `Cancelling job ${jobId} returned an invalid receipt`, cancellation);
  }
  const job = cancellationPayload?.result?.job;
  if (!cancellationPayload?.ok || cancellation.status !== Number("0") || !job) {
    throw remoteFailure("stado.watch", `Cancelling job ${jobId} failed`, cancellation);
  }

  const logs = captureRemoteLogs(jobId, hostDef, cancellationDir);
  const state = String(job.state || "").toLowerCase();
  let retained = null;
  let evidenceFailure = null;
  if (job.started_at && ["cancelled", "completed", "uploaded", "failed"].includes(state)) {
    try {
      retained = fetchRunEvidence(jobId, hostDef);
    } catch (error) {
      evidenceFailure = failureSummary(error, "stado.download");
    }
  }
  const cancelled = state === "cancelled";
  const evidence = {
    required: Boolean(job.started_at),
    collected: Boolean(retained?.resultsDir),
    resultsDir: retained?.resultsDir || null,
    artifactError: retained?.artifactError || null,
    failure: evidenceFailure,
    reason: job.started_at ? null : "cancelled-before-start",
  };
  const evaluationFailure = cancelled
    ? terminalJobFailure(jobId, state, job)
    : failureSummary(new FailureError({
      point: "stado.worker",
      code: CODE.UNKNOWN,
      detail: `machine cancellation returned terminal state ${state || "(empty)"}`,
      message: `Job ${jobId} is ${state || "in an unknown state"} and was not cancelled.`,
    }));
  const cancellationFailure = !cancelled
    ? evaluationFailure
    : logs.failure
      || evidence.failure
      || (evidence.required && !evidence.collected
        ? failureSummary(new FailureError({
          point: "stado.download",
          code: CODE.NOT_FOUND,
          detail: `job=${jobId}; artifact_error=${JSON.stringify(evidence.artifactError || null)}`,
          message: `Cancellation of job ${jobId} succeeded, but its required worker evidence was not retained.`,
        }))
        : null);
  const cancellationSucceeded = cancelled && !cancellationFailure;
  return {
    host,
    jobId,
    submitted: false,
    state,
    cancelled,
    passed: false,
    cancellationSucceeded,
    cancellationFailure,
    reason: cancellationReason,
    cancellationRoot,
    attemptId,
    originalJob,
    job,
    source: originalJob.resolved_input_artifacts?.source || job.resolved_input_artifacts?.source || null,
    cancellationDir,
    requestPath,
    statusBeforePath,
    receiptPath,
    logsPath: logs.logPath,
    logReceiptsPath: logs.receiptPath,
    logFailure: logs.failure,
    evidence,
    ...(retained?.resultsDir ? { resultsDir: retained.resultsDir } : {}),
    failure: evaluationFailure,
  };
}

export async function resumeRemoteRun({ jobId, host = "stado:any" }) {
  if (typeof jobId !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(jobId)) {
    throw new FailureError({
      point: "stado.watch",
      code: CODE.CONFIG,
      detail: "job ID must be one non-empty identifier without path separators",
      message: "Resuming remote evidence needs a valid existing Stado job ID.",
    });
  }
  const hostDef = listHosts().find((entry) => entry.host === host);
  if (!hostDef || hostDef.kind !== "stado") {
    throw new FailureError({
      point: "stado.watch",
      code: CODE.NOT_FOUND,
      detail: `unknown stado host: ${host}`,
      message: `No such stado host: "${host}". Run \`probierz hosts\` for the list.`,
    });
  }
  const result = { host, jobId, submitted: false, ...await watchJob(jobId, hostDef) };
  if (!["completed", "failed"].includes(result.state)) return result;
  const retained = fetchRunEvidence(jobId, hostDef);
  if (retained) result.resultsDir = retained.resultsDir;
  if (retained?.artifactError) result.artifactError = retained.artifactError;
  if (retained?.manifest) {
    result.runId = retained.manifest.runId;
    result.appId = retained.manifest.appId;
    result.target = retained.manifest.target;
    const authored = restoreRemoteAuthoring(
      jobId,
      retained,
      retained.manifest.appId,
      result.state === "completed" && Boolean(readAuthorSubmission(jobId)),
    );
    if (authored) Object.assign(result, authored);
  } else if (result.state === "completed") {
    result.state = "evidence-unavailable";
    result.failure = missingRunEvidenceFailure(
      jobId,
      `artifact_error=${JSON.stringify(retained?.artifactError || null)}`,
    );
  }
  return result;
}

export async function submitRemoteRun({ target, appId, spec = null, host = "stado:gcp", provision = null, appRepo = null, watch = true, mode = "run", record = false, environment = [] }) {
  if (!Array.isArray(environment) || environment.some((assignment) =>
    !Array.isArray(assignment) || assignment.length !== Number("2")
      || typeof assignment[Number("0")] !== "string"
      || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(assignment[Number("0")])
      || typeof assignment[Number("1")] !== "string"
      || assignment[Number("1")].includes("\0"))) {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.CONFIG,
      detail: "environment must contain NAME=VALUE assignments with valid environment names and no NUL bytes",
      message: "Invalid remote environment assignment.",
    });
  }
  const hostDef = listHosts().find((entry) => entry.host === host);
  if (!hostDef || hostDef.kind !== "stado") {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.NOT_FOUND,
      detail: `unknown stado host: ${host}`,
      message: `No such stado host: "${host}". Run \`probierz hosts\` for the list.`,
    });
  }
  requireGuiReady(hostDef, target);
  // Measured before anything is uploaded: a submission whose source cannot be
  // named is a submission whose verdict would mean nothing, and finding that
  // out here costs nothing, while finding it out on the worker costs the job.
  const identity = packSourceIdentity(appId, appRepo);
  requireImmutableNativeProvision({ target, provision, appRepo, identity });
  const requestedWatchBudgetMs = selectedRunBudget({ appId, target, environment, provision });
  const packedRepo = packRepo([appId]);
  const repoUri = upload(packedRepo.file, `probierz-${packedRepo.hash}.tar.gz`);
  const identityUri = upload(identity.file, `source-${appId}-${identity.hash}.json`);
  const provisioned = provisionInputs({ appId, provision, appRepo });
  const script = runScript({ target, appId, spec, provision, hash: packedRepo.hash, platform: hostDef.platform, mode, record, environment });
  const scriptFile = workPath(`probierz-run-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  const scriptUri = upload(scriptFile, `run-${packedRepo.hash}.sh`);
  const inputObjects = {
    repo: { stado_uri: repoUri, relative_path: "inputs/probierz.tar.gz" },
    script: { stado_uri: scriptUri, relative_path: "inputs/run.sh" },
    source: { stado_uri: identityUri, relative_path: "inputs/source-identity.json" },
    ...provisioned,
  };
  const { jobId, watchBudgetMs, receiptDir, failure } = submitMachine(
    hostDef,
    packedRepo.hash,
    "run",
    inputObjects,
    remoteRunSecretEnv(appId, ["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"]),
    requestedWatchBudgetMs,
  );
  const result = {
    host,
    jobId,
    target,
    appId,
    submitted: Boolean(jobId),
    watchBudgetMs,
    ...submissionIdentityMetadata({ receiptDir, identity, provision, inputObjects }),
  };
  // `failure` replaces the raw submit transcript that used to travel here: the
  // transcript is on the log line, the verdict is what a caller can act on.
  if (!jobId) return { ...result, state: "submit-failed", failure };
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef, watchBudgetMs);
  result.state = watched.state;
  result.watchBudgetMs = watched.watchBudgetMs;
  result.failure = watched.failure;
  if (watched.job) result.job = watched.job;
  if (watched.evidence) result.evidence = watched.evidence;
  if (["completed", "failed"].includes(watched.state)) {
    const retained = fetchRunEvidence(jobId, hostDef);
    if (retained) result.resultsDir = retained.resultsDir;
    if (retained?.artifactError) result.artifactError = retained.artifactError;
    if (watched.state === "completed" && !retained?.resultsDir) {
      result.state = "evidence-unavailable";
      result.failure = missingRunEvidenceFailure(
        jobId,
        `artifact_error=${JSON.stringify(retained?.artifactError || null)}`,
      );
    }
    if (watched.state === "failed") {
      const preflight = retained?.manifest?.preflight;
      if (preflight?.ready === false) {
        result.preflight = preflight;
        const missing = (preflight.missing || []).join(", ") || "target prerequisites";
        const remediation = (preflight.remediation || []).join("; ");
        result.failure = failureSummary(new FailureError({
          point: "stado.worker",
          code: CODE.CONFIG,
          detail: `missing: ${missing}${remediation ? `; remediation: ${remediation}` : ""}`,
          message: `Job ${jobId} did not execute because the selected host is missing: ${missing}.`,
        }));
      }
    }
  }
  return result;
}

export async function submitRemoteAuthor({ appId, journey, target, desc, area = null, host = "stado:gcp", provision = null, appRepo = null, watch = true }) {
  const selectedJourney = safeAuthorName(journey, "journey");
  const selectedArea = safeAuthorName(area || selectedJourney, "authoring area");
  if (!TARGET_REGISTRATION_DIRS_REL[target]) {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.CONFIG,
      message: `Remote authoring does not support target "${target}".`,
    });
  }
  const hostDef = listHosts().find((entry) => entry.host === host);
  if (!hostDef || hostDef.kind !== "stado") {
    throw new FailureError({
      point: "stado.submit",
      code: CODE.NOT_FOUND,
      detail: `unknown stado host: ${host}`,
      message: `No such stado host: "${host}". Run \`probierz hosts\` for the list.`,
    });
  }
  requireGuiReady(hostDef, target);
  const surface = loadAppManifest(appId).surfaces[target];
  const modelRouterUrl = stadoModelRouterUrl(
    surface?.conditions?.STADO_MODEL_ROUTER_URL ?? process.env.STADO_MODEL_ROUTER_URL,
  );
  const productRoot = submittingProductRoot(appId, appRepo);
  const identity = packSourceIdentity(appId, productRoot);
  requireImmutableNativeProvision({ target, provision, appRepo, identity });
  const sourceIdentity = JSON.parse(readFileSync(identity.file, "utf8"));
  const packedRepo = packRepo([appId]);
  const repoUri = upload(packedRepo.file, `probierz-${packedRepo.hash}.tar.gz`);
  const identityUri = upload(identity.file, `source-${appId}-${identity.hash}.json`);
  const provisioned = provisionInputs({ appId, provision, appRepo: productRoot, sourceRequired: true });
  const script = runScript({
    target, appId, spec: null, provision, hash: packedRepo.hash,
    platform: hostDef.platform, mode: "author",
    author: { journey: selectedJourney, area: selectedArea, desc, receiptId: `remote-${randomUUID()}` }, modelRouterUrl,
    environment: process.env.PROBIERZ_MODEL === undefined
      ? []
      : [["PROBIERZ_MODEL", process.env.PROBIERZ_MODEL]],
  });
  const scriptFile = workPath(`probierz-author-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  const scriptUri = upload(scriptFile, `author-${packedRepo.hash}.sh`);
  const inputObjects = {
    repo: { stado_uri: repoUri, relative_path: "inputs/probierz.tar.gz" },
    script: { stado_uri: scriptUri, relative_path: "inputs/run.sh" },
    source: { stado_uri: identityUri, relative_path: "inputs/source-identity.json" },
    ...provisioned,
  };
  const requestedWatchBudgetMs = conservativeWatchBudget(appId);
  const { jobId, watchBudgetMs, receiptDir, failure } = submitMachine(
    hostDef,
    packedRepo.hash,
    "author",
    inputObjects,
    remoteModelSecretEnv(["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"]),
    requestedWatchBudgetMs,
  );
  const result = {
    host,
    jobId,
    target,
    appId,
    journey: selectedJourney,
    area: selectedArea,
    submitted: Boolean(jobId),
    watchBudgetMs,
    ...submissionIdentityMetadata({ receiptDir, identity, provision, inputObjects }),
  };
  if (!jobId) return { ...result, state: "submit-failed", failure };
  result.sourceReceipt = saveAuthorSubmission({
    jobId,
    appId,
    journey: selectedJourney,
    area: selectedArea,
    target,
    productRoot,
    sourceSha256: sourceIdentity.app.sha256,
    harnessSha256: sourceIdentity.harness.sha256,
  });
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef, watchBudgetMs);
  result.state = watched.state;
  result.watchBudgetMs = watched.watchBudgetMs;
  result.failure = watched.failure;
  if (watched.job) result.job = watched.job;
  if (watched.evidence) result.evidence = watched.evidence;
  if (["completed", "failed"].includes(watched.state)) {
    const retained = fetchRunEvidence(jobId, hostDef);
    if (retained) result.resultsDir = retained.resultsDir;
    if (retained?.artifactError) result.artifactError = retained.artifactError;
    if (watched.state === "completed" && !retained?.resultsDir) {
      result.state = "evidence-unavailable";
      result.failure = missingRunEvidenceFailure(
        jobId,
        `artifact_error=${JSON.stringify(retained?.artifactError || null)}`,
      );
    } else if (watched.state === "completed") {
      const authored = restoreRemoteAuthoring(jobId, retained, appId, true);
      if (authored) Object.assign(result, authored);
      result.specDir = TARGET_SPEC_DIRS_REL[target] || null;
    }
  }
  return result;
}
