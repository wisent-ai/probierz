// Bridge to the stado GPU job queue: run a probierz target on a chosen
// remote host and bring the evidence back into the local test-results tree,
// so history, status, and the gate treat remote runs like local ones.
// Inputs travel as tarballs through stado://probierz/inputs (the probierz
// checkout is private), the job script provisions node and the app's binary
// on the worker, and results are persisted under stado://probierz/results.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stadoModelRouterUrl } from "./model-router.mjs";
import { loadAppManifest } from "./apps.mjs";
import { CODE, FailureError, failureFrom, failureSummary } from "./failure.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const STADO_BIN = "stado";
const NODE_VERSION = "v22.20.0";
const WATCH_INTERVAL_MS = Number("30000");
const WATCH_BUDGET_MS = Number("3600000");
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
// Worker-relative spec dirs per target (mirror of TARGET_SPEC_DIRS in
// author-spec.mjs) so author-mode evidence tarballs include the new spec.
const TARGET_SPEC_DIRS_REL = {
  web: "packages/web/tests",
  electron: "packages/electron/tests",
  "mobile:ios": "packages/mobile/test/specs",
  "mobile:android": "packages/mobile/test/specs",
  "desktop:mac": "packages/desktop-native/test/specs",
  "desktop:win": "packages/desktop-native/test/specs",
  tui: "packages/tui/specs",
};

function stateUri(kind) {
  return `stado://probierz/${kind}`;
}
function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\"'\"'")}'`;
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
    { host: "stado:mini", kind: "stado", platform: "darwin", target: "charless-mac-mini", request: { provider: "local", pin_to_provider: true, pinned_host: "local-charless-mac-mini.local" }, description: "stado queue, dedicated Mac mini consumer" },
    { host: "stado:macbook", kind: "stado", platform: "darwin", target: "lukasz-macbook", apiUrl: "http://127.0.0.1:18765", request: { provider: "local", pin_to_provider: true, pinned_host: "local-lukaszs-macbook-pro-5485.local" }, description: "stado queue, dedicated MacBook consumer" },
    { host: "stado:t4", kind: "stado", request: { gpu_type: "nvidia-tesla-t4" }, description: "stado queue, nvidia-tesla-t4 capacity" },
  ];
}

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  // `error` carries the spawn failure itself (a missing `stado` binary), which
  // neither stream reports and which classifies very differently from a
  // command that ran and refused.
  return { status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || ""), error: out.error || null };
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
  const status = sh(STADO_BIN, ["host", "gui-automation", "status", hostDef.target], {
    env: hostDef.apiUrl ? { ...process.env, STADO_API_URL: hostDef.apiUrl } : process.env,
  });
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
  if (fields.get("gui-ready") !== "yes") {
    const consoleOwner = fields.get("console") || "unknown";
    const accessibility = fields.get("accessibility") || "unknown";
    throw new FailureError({
      point: "stado.preflight",
      code: CODE.CONFIG,
      detail: `target=${hostDef.target}; console=${consoleOwner}; accessibility=${accessibility}; gui-ready=${fields.get("gui-ready") || "not-reported"}`,
      message: `The selected host "${hostDef.host}" is not ready for desktop:cua: it needs an active macOS console session and a granted CuaDriver.`,
    });
  }
}

function packRepo(appIds) {
  const hash = createHash("sha256").update(`${Date.now()}-${Math.random()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `probierz-${hash}.tar.gz`);
  const includes = ["agent", "packages", "apps", "package.json", "package-lock.json", "tsconfig.base.json", ".git"];
  const args = ["-czf", file, ...includes.filter((entry) => existsSync(path.join(ROOT, entry)))];
  const packed = sh("tar", args, { cwd: ROOT });
  if (packed.status !== Number("0")) throw localFailure("stado.pack", "Packing the probierz checkout failed", packed);
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
  return { file, hash };
}

function packAppSource(appId, repoRoot) {
  const hash = createHash("sha256").update(`${appId}-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `${appId}-${hash}.tar.gz`);
  const args = ["-czf", file, "--exclude=target", "--exclude=node_modules", "--exclude=.build", "."];
  const packed = sh("tar", args, { cwd: repoRoot });
  if (packed.status !== Number("0")) throw localFailure("stado.pack", `Packing the ${appId} source tree failed`, packed);
  return { file, hash };
}

function packAppBundle(appId, bundlePath) {
  const bundleName = path.basename(bundlePath);
  const hash = createHash("sha256").update(`${appId}-app-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `${appId}-app-${hash}.tar.gz`);
  // -C into the bundle's parent so the tarball root is <Bundle>.app itself.
  const packed = sh("tar", ["-czf", file, "-C", path.dirname(bundlePath), bundleName]);
  if (packed.status !== Number("0")) throw localFailure("stado.pack", `Packing the ${appId} application bundle failed`, packed);
  return { file, hash, bundleName };
}

function manifestRepoRoot(appId) {
  const manifest = path.join(ROOT, "apps", appId, "probierz.yaml");
  if (!existsSync(manifest)) return null;
  const match = readFileSync(manifest, "utf8").match(/^\s*- root:\s*(\S.+)$/m);
  return match ? match[1].trim() : null;
}

function upload(localFile, name) {
  const destination = `${stateUri("inputs")}/${name}`;
  const out = sh(STADO_BIN, ["storage", "put", destination, localFile]);
  // The local path is diagnostic; it rides the log line, not the message.
  if (out.status !== Number("0")) {
    throw remoteFailure("stado.upload", `Uploading ${name} to the stado object store failed`, { ...out, stderr: `${out.stderr} (source ${localFile})` });
  }
  return destination;
}

function runScript({ target, appId, spec, provision, hash, platform = "linux", mode = "run", author = null, sourceRoot = null, modelRouterUrl = null, record = false }) {
  // Remote jobs may receive secret_env values. Shell xtrace would copy any
  // expanded bearer into the canonical Stado command log.
  const lines = ["set -euo pipefail"];
  lines.push('JOB_ROOT="$PWD"', "mkdir -p output work");
  if (platform === "darwin") {
    // macOS runner: jobs spawned by the stado agent get a bare /bin/sh PATH,
    // so put homebrew on PATH first (stado and node live there on the mini).
    // Then use the node already on the box when present, else fetch the
    // darwin-arm64 tarball.
    lines.push(
      "export PATH=$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH",
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
    `mkdir -p "$JOB_ROOT/work/probierz" && tar -xzf "$JOB_ROOT/inputs/probierz.tar.gz" -C "$JOB_ROOT/work/probierz"`,
  );
  if (provision?.kind === "installed-tui") {
    lines.push(`export TUI_CMD=${shellQuote(provision.path)}`);
  }
  if (provision?.kind === "cargo-release") {
    const manifestPath = provision.manifestPath || "Cargo.toml";
    const manifestDir = path.posix.dirname(manifestPath);
    const targetPrefix = manifestDir === "." ? "" : `${manifestDir}/`;
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}"`,
      "curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal",
      "export PATH=\"$HOME/.cargo/bin:$PATH\"",
      `cargo build --release --manifest-path "$JOB_ROOT/work/${provision.appId}/${manifestPath}"`,
      `export TUI_CMD="$JOB_ROOT/work/${provision.appId}/${targetPrefix}target/release/${provision.binary || provision.appId}"`,
    );
  }
  if (provision?.kind === "app-bundle") {
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar -xzf "$JOB_ROOT/inputs/${provision.appId}-app.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export MAC_APP_PATH="$JOB_ROOT/work/${provision.appId}/${provision.bundleName}"`,
      `mkdir -p "$JOB_ROOT/work/${provision.appId}-src" && tar -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}-src"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}-src"`,
    );
  }
  if (provision?.kind === "app-bundle" && target === "desktop:cua") {
    lines.push(
      'CUA_EXECUTABLE=$(/usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$MAC_APP_PATH/Contents/Info.plist")',
      'export CUA_APP_EXECUTABLE="$MAC_APP_PATH/Contents/MacOS/$CUA_EXECUTABLE"',
    );
  }
  if (provision?.kind === "node-source") {
    // Generic JS app sources are staged as immutable job inputs.
    lines.push(
      `mkdir -p "$JOB_ROOT/work/${provision.appId}" && tar -xzf "$JOB_ROOT/inputs/${provision.appId}.tar.gz" -C "$JOB_ROOT/work/${provision.appId}"`,
      `export PROBIERZ_APP_SOURCE="$JOB_ROOT/work/${provision.appId}"`,
    );
    for (const [key, value] of Object.entries(provision.env ?? {})) {
      lines.push(`export ${key}=${JSON.stringify(String(value))}`);
    }
  }
  lines.push(
    `cd "$JOB_ROOT/work/probierz"`,
  );
  if (provision?.kind === "app-bundle" || provision?.kind === "cargo-release" || provision?.kind === "node-source") {
    // The manifest ships the submitter's absolute repo root; on the worker
    // the sources live under /tmp/w, so rewrite it before the run.
    const srcDir = provision.kind === "app-bundle" ? `$JOB_ROOT/work/${provision.appId}-src` : `$JOB_ROOT/work/${provision.appId}`;
    lines.push(
      `perl -pi -e "s|^  - root: .*|  - root: ${srcDir}|" apps/${appId}/probierz.yaml`,
    );
  }
  if (["mobile:ios", "mobile:android", "desktop:mac", "desktop:win"].includes(target)) {
    const appiumEnvironment = target === "desktop:mac" ? "appium-2-mac2-2.2.2" : "appium-2";
    lines.push(`export APPIUM_HOME="$HOME/.cache/probierz/${appiumEnvironment}"`);
  }
  lines.push(
    "npm install --no-audit --no-fund --loglevel=error",
    // Fresh worker: provision the target's host-level deps (appium drivers,
    // native helpers) exactly as a local `probierz setup <target>` would.
    `node agent/cli.mjs setup ${target}`,
  );
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
    // Remote authoring returns the verified spec, manifest, and run artifacts
    // with the submitter-local repository root restored.
    if (!modelRouterUrl) throw new Error("remote authoring needs STADO_MODEL_ROUTER_URL");
    lines.push(
      `export STADO_MODEL_ROUTER_URL=${shellQuote(modelRouterUrl)}`,
      ': "${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}"',
      "export PROBIERZ_MODEL_AGENT_ID=probierz",
      ': "${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}"',
    );
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
    lines.push(
      "set +e",
      `node agent/cli.mjs author-spec ${shellQuote(appId)} ${shellQuote(author.journey)} --target ${shellQuote(target)} --desc ${shellQuote(author.desc)} --app-path "${target === "tui" ? "$TUI_CMD" : "$MAC_APP_PATH"}"`,
      "PROBIERZ_RC=$?",
      "set -e",
      "mkdir -p test-results",
      `perl -pi -e "s|^  - root: .*|  - root: ${(sourceRoot || "").replace(/\//g, "\\/")}|" apps/${appId}/probierz.yaml`,
      `tar -czf "$JOB_ROOT/output/probierz-author-${hash}.tar.gz" test-results ${TARGET_SPEC_DIRS_REL[target]} apps/${appId}/probierz.yaml`,
      "exit $PROBIERZ_RC",
    );
    return lines.join("\n");
  }
  lines.push(
    // Evidence must survive a failing run: capture the exit code, tar and
    // upload whatever test-results exist, then re-emit the run's status so
    // the job's success/failure still reflects the tests.
    "set +e",
    `node agent/cli.mjs run ${target} --app ${appId}${spec ? ` --spec ${spec}` : ""}${record ? " --record" : ""} PROBIERZ_RUN_KIND=pull-request`,
    "PROBIERZ_RUN_RC=$?",
    "set -e",
    `tar -czf "$JOB_ROOT/output/probierz-run-${hash}.tar.gz" test-results`,
    "exit $PROBIERZ_RUN_RC",
  );
  return lines.join("\n");
}

function submitMachine(hostDef, hash, kind, inputObjects, secretEnv = {}) {
  const requestFile = path.join(tmpdir(), `probierz-machine-${hash}.json`);
  const request = {
    client_request_id: `probierz-${kind}-${hash}`,
    command: "bash inputs/run.sh",
    output_uri: stateUri("results"),
    input_objects: inputObjects,
    secret_env: secretEnv,
    ...(hostDef.request || {}),
  };
  writeFileSync(requestFile, JSON.stringify(request));
  const submit = sh(STADO_BIN, ["machine", "submit", "--request-file", requestFile], {
    env: hostDef.apiUrl ? { ...process.env, STADO_API_URL: hostDef.apiUrl } : process.env,
  });
  rmSync(requestFile, { force: true });
  let jobId = null;
  try {
    const payload = JSON.parse(submit.stdout);
    jobId = payload?.ok ? payload.result?.job?.job_id || null : null;
  } catch {
    // A queue that answers with something other than its own protocol is a
    // queue that is not working; the text lands on the log line below.
  }
  if (jobId) return { jobId, failure: null };
  // The raw submit output is the operator's evidence, so it is logged in full
  // by `remoteFailure`; the caller gets the verdict, not the transcript.
  return { jobId: null, failure: failureSummary(remoteFailure("stado.submit", "The stado queue did not accept the job", submit)) };
}

/**
 * A `machine status` call that itself fails is a blip until it is a pattern.
 * Three consecutive failures is the queue being unreachable, and waiting out
 * the full hour to say so would be the silence this contract forbids.
 */
const STATUS_FAILURE_TOLERANCE = Number("3");

async function watchJob(jobId, hostDef) {
  const deadline = Date.now() + WATCH_BUDGET_MS;
  let consecutiveStatusFailures = Number("0");
  while (Date.now() < deadline) {
    const out = sh(STADO_BIN, ["machine", "status", jobId], {
      env: hostDef.apiUrl ? { ...process.env, STADO_API_URL: hostDef.apiUrl } : process.env,
    });
    let payload = null;
    try { payload = JSON.parse(out.stdout); } catch {}
    const answered = Boolean(payload?.ok);
    if (!answered) {
      consecutiveStatusFailures += Number("1");
      if (consecutiveStatusFailures >= STATUS_FAILURE_TOLERANCE) {
        return {
          state: "unreachable",
          failure: failureSummary(remoteFailure("stado.watch", `The stado queue stopped answering about job ${jobId}`, out)),
        };
      }
    } else {
      consecutiveStatusFailures = Number("0");
    }
    const state = String(answered ? payload.result?.job?.state || "" : "").toLowerCase();
    // A job the fleet failed or cancelled is a run result, not an outage: the
    // queue did its part. The worker's own text is the operator's evidence.
    if (["failed", "cancelled"].includes(state)) {
      const detail = (out.stdout || out.stderr).slice(Number("0"), Number("500"));
      return {
        state: "failed",
        failure: failureSummary(new FailureError({
          point: "stado.worker",
          code: CODE.UNKNOWN,
          detail,
          message: `Job ${jobId} ${state} on the remote host; its retained evidence describes the failure.`,
        })),
      };
    }
    if (["uploaded", "completed"].includes(state)) return { state: "completed", failure: null };
    await new Promise((resolve) => { setTimeout(resolve, WATCH_INTERVAL_MS); });
  }
  return {
    state: "watch-timeout",
    failure: failureSummary(failureFrom({
      point: "stado.watch",
      error: `watching job ${jobId} timed out`,
      detail: `no terminal state within ${WATCH_BUDGET_MS}ms`,
      action: `Job ${jobId} was still running when probierz stopped waiting`,
    })),
  };
}

function provisionInputs({ appId, provision, appRepo }) {
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
    return { sourceRoot: null, inputs };
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
    return { sourceRoot: appRepo, inputs };
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
    const sourceRepo = appRepo || manifestRepoRoot(appId);
    if (!sourceRepo) {
      throw new FailureError({
        point: "stado.pack",
        code: CODE.CONFIG,
        detail: `app-bundle needs the app source repo for ${appId}`,
        message: `Remote app-bundle runs need the app source repo: pass --app-repo, or set repositories[0].root in apps/${appId}/probierz.yaml.`,
      });
    }
    const source = packAppSource(appId, sourceRepo);
    inputs.app = {
      stado_uri: upload(source.file, `${appId}-${source.hash}.tar.gz`),
      relative_path: `inputs/${appId}.tar.gz`,
    };
    return { sourceRoot: sourceRepo, inputs };
  }
  return { sourceRoot: null, inputs };
}

function seoRunScript({ appId, baseUrl, mode, policyPath, briefPath, primaryModel, secondaryModel, adjudicatorModel, agentId, routerUrl, productionEvidence, signatureRequired, hash, platform = "linux" }) {
  const lines = ["set -euo pipefail", 'JOB_ROOT="$PWD"', "mkdir -p output work"];
  if (platform === "darwin") {
    lines.push(
      "export PATH=$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH",
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
    'mkdir -p "$JOB_ROOT/work/probierz" && tar -xzf "$JOB_ROOT/inputs/probierz.tar.gz" -C "$JOB_ROOT/work/probierz"',
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
  const scriptFile = path.join(tmpdir(), `probierz-seo-${packedRepo.hash}.sh`);
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
  const { jobId, failure } = submitMachine(hostDef, packedRepo.hash, "seo", inputObjects, remoteRunSecretEnv(appId, secretNames));
  const result = { host, jobId, appId, mode, submitted: Boolean(jobId) };
  if (!jobId) return { ...result, state: "submit-failed", failure };
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef);
  result.state = watched.state;
  result.failure = watched.failure;
  if (watched.state === "completed") result.resultsDir = fetchResults(packedRepo.hash, appId, jobId, "seo");
  return result;
}

function fetchFailedRun(jobId, hostDef) {
  const destDir = path.join(ROOT, "test-results", ".remote", jobId);
  rmSync(destDir, { recursive: true, force: true });
  mkdirSync(destDir, { recursive: true });
  const downloaded = sh(STADO_BIN, ["machine", "artifacts", jobId, "--output-dir", destDir], {
    env: hostDef.apiUrl ? { ...process.env, STADO_API_URL: hostDef.apiUrl } : process.env,
  });
  let payload;
  try {
    payload = JSON.parse(downloaded.stdout);
  } catch {
    return null;
  }
  const artifact = (payload?.result?.artifacts || []).find(({ relative_path: relativePath }) =>
    /^probierz-run-.*\.tar\.gz$/.test(String(relativePath || "")));
  if (!artifact) return null;
  const tarball = path.join(destDir, artifact.relative_path);
  const listed = sh("tar", ["-tzf", tarball], { cwd: ROOT });
  if (listed.status !== Number("0")) return null;
  const manifestEntry = listed.stdout.split("\n").find((entry) => entry.endsWith("/run-manifest.json"));
  const untar = sh("tar", ["-xzf", tarball, "-C", ROOT], { cwd: ROOT });
  if (untar.status !== Number("0")) return null;
  let manifest = null;
  if (manifestEntry) {
    try {
      manifest = JSON.parse(readFileSync(path.join(ROOT, manifestEntry), "utf8"));
    } catch {}
  }
  return { resultsDir: destDir, manifest };
}

export async function submitRemoteRun({ target, appId, spec = null, host = "stado:gcp", provision = null, appRepo = null, watch = true, mode = "run", record = false }) {
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
  const packedRepo = packRepo([appId]);
  const repoUri = upload(packedRepo.file, `probierz-${packedRepo.hash}.tar.gz`);
  const provisioned = provisionInputs({ appId, provision, appRepo });
  const script = runScript({ target, appId, spec, provision, hash: packedRepo.hash, platform: hostDef.platform, mode, record });
  const scriptFile = path.join(tmpdir(), `probierz-run-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  const scriptUri = upload(scriptFile, `run-${packedRepo.hash}.sh`);
  const inputObjects = {
    repo: { stado_uri: repoUri, relative_path: "inputs/probierz.tar.gz" },
    script: { stado_uri: scriptUri, relative_path: "inputs/run.sh" },
    ...provisioned.inputs,
  };
  const { jobId, failure } = submitMachine(
    hostDef,
    packedRepo.hash,
    "run",
    inputObjects,
    remoteRunSecretEnv(appId, ["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"]),
  );
  const result = { host, jobId, target, appId, submitted: Boolean(jobId) };
  // `failure` replaces the raw submit transcript that used to travel here: the
  // transcript is on the log line, the verdict is what a caller can act on.
  if (!jobId) return { ...result, state: "submit-failed", failure };
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef);
  result.state = watched.state;
  result.failure = watched.failure;
  if (watched.state === "completed") {
    result.resultsDir = fetchResults(packedRepo.hash, appId, jobId);
  }
  if (watched.state === "failed") {
    const retained = fetchFailedRun(jobId, hostDef);
    if (retained) {
      result.resultsDir = retained.resultsDir;
      const preflight = retained.manifest?.preflight;
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

export async function submitRemoteAuthor({ appId, journey, target, desc, host = "stado:gcp", provision = null, appRepo = null, watch = true }) {
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
  const modelRouterUrl = stadoModelRouterUrl();
  const packedRepo = packRepo([appId]);
  const repoUri = upload(packedRepo.file, `probierz-${packedRepo.hash}.tar.gz`);
  const provisioned = provisionInputs({ appId, provision, appRepo });
  const script = runScript({
    target, appId, spec: null, provision, hash: packedRepo.hash,
    platform: hostDef.platform, mode: "author",
    author: { journey, desc }, sourceRoot: provisioned.sourceRoot, modelRouterUrl,
  });
  const scriptFile = path.join(tmpdir(), `probierz-author-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  const scriptUri = upload(scriptFile, `author-${packedRepo.hash}.sh`);
  const inputObjects = {
    repo: { stado_uri: repoUri, relative_path: "inputs/probierz.tar.gz" },
    script: { stado_uri: scriptUri, relative_path: "inputs/run.sh" },
    ...provisioned.inputs,
  };
  const { jobId, failure } = submitMachine(
    hostDef,
    packedRepo.hash,
    "author",
    inputObjects,
    remoteRunSecretEnv(appId, ["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"]),
  );
  const result = { host, jobId, target, appId, journey, submitted: Boolean(jobId) };
  if (!jobId) return { ...result, state: "submit-failed", failure };
  if (!watch) return { ...result, state: "queued", failure: null };
  const watched = await watchJob(jobId, hostDef);
  result.state = watched.state;
  result.failure = watched.failure;
  if (watched.state === "completed") {
    result.resultsDir = fetchResults(packedRepo.hash, appId, jobId, "author");
    result.specDir = TARGET_SPEC_DIRS_REL[target] || null;
  }
  return result;
}

export function fetchResults(hash, appId, jobId, kind = "run") {
  const destDir = path.join(ROOT, "test-results", ".remote", jobId);
  rmSync(destDir, { recursive: true, force: true });
  mkdirSync(destDir, { recursive: true });
  const tarball = path.join(destDir, "results.tar.gz");
  const down = sh(STADO_BIN, ["storage", "get", `${stateUri("results")}/probierz-${kind}-${hash}.tar.gz`, tarball]);
  if (down.status !== Number("0")) throw remoteFailure("stado.download", `Downloading the evidence for job ${jobId} failed`, down);
  // Every remote archive is rooted at the repository: run archives contain
  // test-results/, while author archives add accepted specs and the manifest.
  const untar = sh("tar", ["-xzf", tarball, "-C", ROOT], { cwd: ROOT });
  if (untar.status !== Number("0")) throw localFailure("stado.download", `Unpacking the evidence for job ${jobId} failed`, untar);
  return destDir;
}
