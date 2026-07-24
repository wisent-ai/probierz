// Bridge to the stado GPU job queue: run a probierz target on a chosen
// remote host and bring the evidence back into the local test-results tree,
// so history, status, and the gate treat remote runs like local ones.
// Inputs travel as tarballs over gs://stado/probierz-inputs (the probierz
// checkout is private), the job script provisions node and the app's binary
// on the worker, and results are uploaded to gs://stado/probierz-results.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const WC_BIN = "wc";
const NODE_VERSION = "v22.20.0";
const WATCH_INTERVAL_MS = Number("30000");
const WATCH_BUDGET_MS = Number("3600000");

// State backend for the bridge: env PROBIERZ_STADO_BUCKET wins, then
// stado.config.json (storage.gcs.bucket), then the fleet default.
function stateBucket() {
  if (process.env.PROBIERZ_STADO_BUCKET) return process.env.PROBIERZ_STADO_BUCKET;
  const candidates = [
    path.join(ROOT, "..", "wisent-compute", "stado.config.json"),
    path.join(process.env.HOME || "", ".config", "stado", "config.json"),
    path.join(process.env.HOME || "", ".stado", "config.json"),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      try {
        const data = JSON.parse(readFileSync(candidate, "utf8"));
        const bucket = data?.storage?.gcs?.bucket;
        if (typeof bucket === "string" && bucket) return bucket;
      } catch { /* unreadable config: fall through to the default */ }
    }
  }
  return "stado";
}

function stateUri(kind) {
  return `gs://${stateBucket()}/probierz-${kind}`;
}

export function listHosts() {
  return [
    { host: "local", kind: "local", description: "this machine (default)" },
    { host: "stado:gcp", kind: "stado", submit: ["--provider", "gcp", "--pin-provider"], description: "stado queue, GCP consumers only" },
    { host: "stado:azure", kind: "stado", submit: ["--provider", "azure", "--pin-provider"], description: "stado queue, Azure consumers only" },
    { host: "stado:aws", kind: "stado", submit: ["--provider", "aws", "--pin-provider"], description: "stado queue, AWS consumers only" },
    { host: "stado:any", kind: "stado", submit: ["--any-provider"], description: "stado queue, any consumer with capacity" },
    { host: "stado:spot", kind: "stado", submit: ["--spot"], description: "stado queue, cheapest preemptible capacity" },
    { host: "stado:local", kind: "stado", submit: ["--provider", "local", "--pin-provider"], description: "stado queue, local-kind consumers only" },
    { host: "stado:mini", kind: "stado", platform: "darwin", submit: ["--provider", "local", "--pin-provider", "--pinned-host", "charless-mac-mini"], description: "stado queue, pinned to charless-mac-mini (macOS runner)" },
    { host: "stado:t4", kind: "stado", submit: ["--gpu-type", "nvidia-tesla-t4"], description: "stado queue, pinned to nvidia-tesla-t4 slots (GCP)" },
  ];
}

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  return { status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || "") };
}

function packRepo(appIds) {
  const hash = createHash("sha256").update(`${Date.now()}-${Math.random()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `probierz-${hash}.tar.gz`);
  const includes = ["agent", "packages", "apps", "package.json", "tsconfig.base.json", ".git"];
  const args = ["-czf", file, ...includes.filter((entry) => existsSync(path.join(ROOT, entry)))];
  const packed = sh("tar", args, { cwd: ROOT });
  if (packed.status !== 0) throw new Error(`pack probierz failed: ${packed.stderr}`);
  for (const appId of appIds) {
    if (!existsSync(path.join(ROOT, "apps", appId))) throw new Error(`app manifest not found: apps/${appId}`);
  }
  return { file, hash };
}

function packAppSource(appId, repoRoot) {
  const hash = createHash("sha256").update(`${appId}-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `${appId}-${hash}.tar.gz`);
  const args = ["-czf", file, "--exclude=target", "--exclude=node_modules", "--exclude=.build", "."];
  const packed = sh("tar", args, { cwd: repoRoot });
  if (packed.status !== 0) throw new Error(`pack ${appId} failed: ${packed.stderr}`);
  return { file, hash };
}

function packAppBundle(appId, bundlePath) {
  const bundleName = path.basename(bundlePath);
  const hash = createHash("sha256").update(`${appId}-app-${Date.now()}`).digest("hex").slice(0, Number("12"));
  const file = path.join(tmpdir(), `${appId}-app-${hash}.tar.gz`);
  // -C into the bundle's parent so the tarball root is <Bundle>.app itself.
  const packed = sh("tar", ["-czf", file, "-C", path.dirname(bundlePath), bundleName]);
  if (packed.status !== 0) throw new Error(`pack ${appId} bundle failed: ${packed.stderr}`);
  return { file, hash, bundleName };
}

function manifestRepoRoot(appId) {
  const manifest = path.join(ROOT, "apps", appId, "probierz.yaml");
  if (!existsSync(manifest)) return null;
  const match = readFileSync(manifest, "utf8").match(/^\s*- root:\s*(\S.+)$/m);
  return match ? match[1].trim() : null;
}

function upload(localFile, name) {
  const dest = `${stateUri("inputs")}/${name}`;
  const out = sh("gcloud", ["storage", "cp", localFile, dest]);
  if (out.status !== 0) throw new Error(`upload ${localFile} failed: ${out.stderr}`);
  return dest;
}

function runScript({ target, appId, spec, provision, hash, platform = "linux" }) {
  const lines = ["set -euxo pipefail"];
  if (platform === "darwin") {
    // macOS runner: jobs spawned by the stado agent get a bare /bin/sh PATH,
    // so put homebrew on PATH first (gcloud, node live there on the mini).
    // Then use the node already on the box when present, else fetch the
    // darwin-arm64 tarball.
    lines.push(
      "export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH",
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
    `gcloud storage cp ${stateUri("inputs")}/probierz.tar.gz /tmp/`,
    "mkdir -p /tmp/w/probierz && tar -xzf /tmp/probierz.tar.gz -C /tmp/w/probierz",
  );
  if (provision?.kind === "cargo-release") {
    lines.push(
      `gcloud storage cp ${stateUri("inputs")}/${provision.appId}.tar.gz /tmp/`,
      `mkdir -p /tmp/w/${provision.appId} && tar -xzf /tmp/${provision.appId}.tar.gz -C /tmp/w/${provision.appId}`,
      "curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal",
      "export PATH=\"$HOME/.cargo/bin:$PATH\"",
      `cargo build --release --manifest-path /tmp/w/${provision.appId}/Cargo.toml`,
      `export TUI_CMD=/tmp/w/${provision.appId}/target/release/${provision.binary || provision.appId}`,
    );
  }
  if (provision?.kind === "app-bundle") {
    lines.push(
      `gcloud storage cp ${stateUri("inputs")}/${provision.appId}-app.tar.gz /tmp/`,
      `mkdir -p /tmp/w/${provision.appId} && tar -xzf /tmp/${provision.appId}-app.tar.gz -C /tmp/w/${provision.appId}`,
      `export MAC_APP_PATH=/tmp/w/${provision.appId}/${provision.bundleName}`,
      `gcloud storage cp ${stateUri("inputs")}/${provision.appId}.tar.gz /tmp/`,
      `mkdir -p /tmp/w/${provision.appId}-src && tar -xzf /tmp/${provision.appId}.tar.gz -C /tmp/w/${provision.appId}-src`,
    );
  }
  lines.push(
    "cd /tmp/w/probierz",
  );
  if (provision?.kind === "app-bundle") {
    // The manifest ships the submitter's absolute repo root; on the worker
    // the sources live in /tmp/w/<appId>-src, so rewrite it before the run.
    lines.push(
      `perl -pi -e 's|^  - root: .*|  - root: /tmp/w/${provision.appId}-src|' apps/${appId}/probierz.yaml`,
    );
  }
  lines.push(
    "npm install --no-audit --no-fund --loglevel=error",
    // Fresh worker: provision the target's host-level deps (appium drivers,
    // native helpers) exactly as a local `probierz setup <target>` would.
    `node agent/cli.mjs setup ${target}`,
    `node agent/cli.mjs run ${target} --app ${appId}${spec ? ` --spec ${spec}` : ""} PROBIERZ_RUN_KIND=pull-request`,
    "tar -czf /tmp/results.tar.gz test-results",
    `gcloud storage cp /tmp/results.tar.gz ${stateUri("results")}/probierz-run-${hash}.tar.gz`,
  );
  return lines.join("\n");
}

function parseJobId(text) {
  const match = String(text).match(/[Bb]atch:\s*([a-z0-9][a-z0-9-]{4,})/)
    || String(text).match(/job[_\s-]?id["'\s:=]+([a-z0-9][a-z0-9-]{6,})/i)
    || String(text).match(/\b([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\b/);
  return match ? match[1] : null;
}

async function watchJob(jobId) {
  const deadline = Date.now() + WATCH_BUDGET_MS;
  while (Date.now() < deadline) {
    const out = sh(WC_BIN, ["status", jobId]);
    const line = `${out.stdout}\n${out.stderr}`.split("\n").find((entry) => entry.startsWith(jobId));
    const state = line ? String(line.split(/\s+/)[1] || "").toLowerCase() : "";
    if (["failed", "cancelled"].includes(state)) return { state: "failed", detail: line || `${out.stdout}${out.stderr}`.slice(-Number("2000")) };
    if (["uploaded", "completed"].includes(state)) return { state: "completed", detail: line };
    await new Promise((resolve) => { setTimeout(resolve, WATCH_INTERVAL_MS); });
  }
  return { state: "watch-timeout", detail: `job ${jobId} did not finish within ${WATCH_BUDGET_MS}ms` };
}

export async function submitRemoteRun({ target, appId, spec = null, host = "stado:gcp", provision = null, appRepo = null, watch = true }) {
  const hostDef = listHosts().find((entry) => entry.host === host);
  if (!hostDef || hostDef.kind !== "stado") throw new Error(`unknown stado host: ${host}`);
  const packedRepo = packRepo([appId]);
  upload(packedRepo.file, "probierz.tar.gz");
  if (provision?.kind === "cargo-release") {
    if (!appRepo) throw new Error("cargo-release provisioning needs the app source repository");
    upload(packAppSource(appId, appRepo).file, `${appId}.tar.gz`);
  }
  if (provision?.kind === "app-bundle") {
    if (!provision.bundlePath || !existsSync(provision.bundlePath)) throw new Error(`app-bundle path missing: ${provision.bundlePath}`);
    const bundle = packAppBundle(appId, provision.bundlePath);
    provision.bundleName = bundle.bundleName;
    upload(bundle.file, `${appId}-app.tar.gz`);
    // The runner recomputes the source inventory on the worker, so the app
    // source (with .git) must travel too; the manifest's absolute repo root
    // is rewritten on the worker to the extracted copy.
    const sourceRepo = appRepo || manifestRepoRoot(appId);
    if (!sourceRepo) throw new Error(`app-bundle needs the app source repo (--app-repo or a repositories[0].root in apps/${appId}/probierz.yaml)`);
    upload(packAppSource(appId, sourceRepo).file, `${appId}.tar.gz`);
  }
  const script = runScript({ target, appId, spec, provision, hash: packedRepo.hash, platform: hostDef.platform });
  const scriptFile = path.join(tmpdir(), `probierz-run-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  upload(scriptFile, `run-${packedRepo.hash}.sh`);
  const submit = sh(WC_BIN, [
    "submit",
    `gcloud storage cp ${stateUri("inputs")}/run-${packedRepo.hash}.sh /tmp/run.sh && bash /tmp/run.sh`,
    ...hostDef.submit,
  ]);
  const jobId = parseJobId(`${submit.stdout}\n${submit.stderr}`);
  const result = { host, jobId, target, appId, submitted: submit.status === 0, submitText: `${submit.stdout}${submit.stderr}`.slice(-Number("800")) };
  if (!jobId) return { ...result, state: "submit-failed" };
  if (!watch) return { ...result, state: "queued" };
  const watched = await watchJob(jobId);
  result.state = watched.state;
  result.detail = watched.detail;
  if (watched.state === "completed") {
    result.resultsDir = fetchResults(packedRepo.hash, appId, jobId);
  }
  return result;
}

export function fetchResults(hash, appId, jobId) {
  const destDir = path.join(ROOT, "test-results", ".remote", jobId);
  rmSync(destDir, { recursive: true, force: true });
  mkdirSync(destDir, { recursive: true });
  const tarball = path.join(destDir, "results.tar.gz");
  const down = sh("gcloud", ["storage", "cp", `${stateUri("results")}/probierz-run-${hash}.tar.gz`, tarball]);
  if (down.status !== 0) throw new Error(`download results for ${jobId} failed: ${down.stderr}`);
  const untar = sh("tar", ["-xzf", tarball, "-C", path.join(ROOT, "test-results")], { cwd: ROOT });
  if (untar.status !== 0) throw new Error(`extract results for ${jobId} failed: ${untar.stderr}`);
  return destDir;
}
