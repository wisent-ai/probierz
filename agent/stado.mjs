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
const GCS_INPUTS = "gs://stado/probierz-inputs";
const GCS_RESULTS = "gs://stado/probierz-results";
const NODE_VERSION = "v22.20.0";
const WATCH_INTERVAL_MS = Number("30000");
const WATCH_BUDGET_MS = Number("3600000");

export function listHosts() {
  return [
    { host: "local", kind: "local", description: "this machine (default)" },
    { host: "stado:gcp", kind: "stado", submit: ["--provider", "gcp", "--pin-provider"], description: "stado queue, GCP consumers only" },
    { host: "stado:azure", kind: "stado", submit: ["--provider", "azure", "--pin-provider"], description: "stado queue, Azure consumers only" },
    { host: "stado:aws", kind: "stado", submit: ["--provider", "aws", "--pin-provider"], description: "stado queue, AWS consumers only" },
    { host: "stado:any", kind: "stado", submit: ["--any-provider"], description: "stado queue, any consumer with capacity" },
    { host: "stado:spot", kind: "stado", submit: ["--spot"], description: "stado queue, cheapest preemptible capacity" },
    { host: "stado:local", kind: "stado", submit: ["--provider", "local", "--pin-provider"], description: "stado queue, local-kind consumers only" },
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

function upload(localFile, name) {
  const dest = `${GCS_INPUTS}/${name}`;
  const out = sh("gcloud", ["storage", "cp", localFile, dest]);
  if (out.status !== 0) throw new Error(`upload ${localFile} failed: ${out.stderr}`);
  return dest;
}

function runScript({ target, appId, spec, provision, hash }) {
  const lines = [
    "set -euxo pipefail",
    `curl -fsSL https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz -o /tmp/node.tar.xz`,
    "tar -xJf /tmp/node.tar.xz -C /tmp",
    `export PATH=/tmp/node-${NODE_VERSION}-linux-x64/bin:$PATH`,
    `gcloud storage cp ${GCS_INPUTS}/probierz.tar.gz /tmp/`,
    "mkdir -p /tmp/w/probierz && tar -xzf /tmp/probierz.tar.gz -C /tmp/w/probierz",
  ];
  if (provision?.kind === "cargo-release") {
    lines.push(
      `gcloud storage cp ${GCS_INPUTS}/${provision.appId}.tar.gz /tmp/`,
      `mkdir -p /tmp/w/${provision.appId} && tar -xzf /tmp/${provision.appId}.tar.gz -C /tmp/w/${provision.appId}`,
      "curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal",
      "export PATH=\"$HOME/.cargo/bin:$PATH\"",
      `cargo build --release --manifest-path /tmp/w/${provision.appId}/Cargo.toml`,
      `export TUI_CMD=/tmp/w/${provision.appId}/target/release/${provision.binary || provision.appId}`,
    );
  }
  lines.push(
    "cd /tmp/w/probierz",
    "npm install --no-audit --no-fund --loglevel=error",
    `node agent/cli.mjs run ${target} --app ${appId}${spec ? ` --spec ${spec}` : ""} PROBIERZ_RUN_KIND=pull-request`,
    "tar -czf /tmp/results.tar.gz test-results",
    `gcloud storage cp /tmp/results.tar.gz ${GCS_RESULTS}/probierz-run-${hash}.tar.gz`,
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
  const script = runScript({ target, appId, spec, provision, hash: packedRepo.hash });
  const scriptFile = path.join(tmpdir(), `probierz-run-${packedRepo.hash}.sh`);
  writeFileSync(scriptFile, script);
  upload(scriptFile, `run-${packedRepo.hash}.sh`);
  const submit = sh(WC_BIN, [
    "submit",
    `gcloud storage cp ${GCS_INPUTS}/run-${packedRepo.hash}.sh /tmp/run.sh && bash /tmp/run.sh`,
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
  const down = sh("gcloud", ["storage", "cp", `${GCS_RESULTS}/probierz-run-${hash}.tar.gz`, tarball]);
  if (down.status !== 0) throw new Error(`download results for ${jobId} failed: ${down.stderr}`);
  const untar = sh("tar", ["-xzf", tarball, "-C", path.join(ROOT, "test-results")], { cwd: ROOT });
  if (untar.status !== 0) throw new Error(`extract results for ${jobId} failed: ${untar.stderr}`);
  return destDir;
}
