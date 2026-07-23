import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { lstat } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { repositorySourceFiles } from "../source-identity.mjs";

const HOST = "charles@charless-mac-mini.tail6443b3.ts.net";
const KEY = path.join(homedir(), ".ssh/charless_mac_mini_ed25519");
const BASE = "/Users/charles/Library/Caches/probierz";
const REMOTE_NODE = "/opt/homebrew/bin/node";
const CONNECT_TIMEOUT = "ConnectTimeout=15";
const RETRY_ATTEMPTS = 3;
const RETRY_BASE_MS = 1000;
const QUARANTINE_MS = 15 * 60 * 1000;
const HOST_STATE = path.join(homedir(), "Library", "Caches", "probierz", "remote-hosts", "byk-auth.json");
const quote = (value) => `'${value.replaceAll("'", `'"'"'`)}'`;

function hostState() {
  if (!existsSync(HOST_STATE)) return null;
  try { return JSON.parse(readFileSync(HOST_STATE, "utf8")); }
  catch { return null; }
}

function assertHostAvailable() {
  const state = hostState();
  if (!state?.quarantinedUntil) return;
  const until = Date.parse(state.quarantinedUntil);
  if (Number.isFinite(until) && until > Date.now()) {
    throw new Error(`dedicated host is quarantined until ${state.quarantinedUntil}`);
  }
}

function quarantineHost(reason) {
  const previous = hostState();
  const state = {
    schemaVersion: 1,
    host: HOST,
    failures: Number(previous?.failures || 0) + 1,
    reason,
    quarantinedAt: new Date().toISOString(),
    quarantinedUntil: new Date(Date.now() + QUARANTINE_MS).toISOString(),
  };
  mkdirSync(path.dirname(HOST_STATE), { recursive: true, mode: 0o700 });
  const temporary = `${HOST_STATE}.${process.pid}.${randomUUID()}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600 });
  renameSync(temporary, HOST_STATE);
}

function clearQuarantine() {
  if (existsSync(HOST_STATE)) unlinkSync(HOST_STATE);
}

function infrastructureFailure(message) {
  const error = new Error(message);
  error.code = "REMOTE_INFRASTRUCTURE";
  return error;
}

async function retryStep(label, operation) {
  let lastResult = null;
  let lastError = null;
  for (let attempt = 1; attempt <= RETRY_ATTEMPTS; attempt += 1) {
    try {
      lastResult = await operation();
      if (lastResult.code === Number("0")) return lastResult;
    } catch (error) {
      lastError = error;
    }
    if (attempt < RETRY_ATTEMPTS) {
      await new Promise((resolve) => setTimeout(resolve, RETRY_BASE_MS * (2 ** (attempt - 1))));
    }
  }
  const detail = lastError ? lastError.message : `exit ${lastResult?.code ?? "unknown"}`;
  throw infrastructureFailure(`${label} failed after ${RETRY_ATTEMPTS} attempts (${detail})`);
}

function sshBase() {
  return [
    "-i", KEY,
    "-o", "BatchMode=yes",
    "-o", "IdentitiesOnly=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", CONNECT_TIMEOUT,
  ];
}

function spawnAndWait(command, args, options, onChild, input = null) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, options);
    onChild(child);
    let settled = false;
    child.once("error", () => {
      if (settled) return;
      settled = true;
      onChild(null);
      reject(new Error(`could not start ${command}`));
    });
    child.once("close", (code, signal) => {
      if (settled) return;
      settled = true;
      onChild(null);
      resolve({ code, signal });
    });
    if (input !== null) child.stdin.end(input);
  });
}

async function requireLocalPath(candidate, kind, name) {
  if (!path.isAbsolute(candidate) || !existsSync(candidate)) {
    throw new Error(`${name} must be an existing absolute path`);
  }
  const metadata = await lstat(candidate);
  if (kind === "directory" ? !metadata.isDirectory() : !metadata.isFile()) {
    throw new Error(`${name} has the wrong file type`);
  }
}

export function sourceFileList(root) {
  const files = repositorySourceFiles(root, {
    excludeRuntimeSecrets: true,
    includePackageLock: true,
  });
  return `${files.join("\0")}\0`;
}

export async function runRemoteBykAuth({
  root, appPath, iosDevice, iosVersion, socketPath, recipient, onChild,
}) {
  await requireLocalPath(root, "directory", "Probierz root");
  await requireLocalPath(appPath, "directory", "APP_IOS");
  await requireLocalPath(KEY, "file", "dedicated-host SSH key");
  const socketMetadata = await lstat(socketPath);
  if (!socketMetadata.isSocket()) throw new Error("local OTP broker socket is unavailable");
  assertHostAvailable();

  const runRoot = `${BASE}/runs/${randomUUID()}`;
  const remoteSource = `${runRoot}/probierz`;
  const remoteApp = `${runRoot}/Byk.app`;
  const remoteSocket = `${runRoot}/byk-otp.sock`;
  const ssh = sshBase();
  const sshTransport = ["ssh", ...ssh.map(quote)].join(" ");
  const runRemote = (command, stdio = ["ignore", "inherit", "inherit"]) =>
    spawnAndWait("ssh", [...ssh, HOST, command], { stdio }, onChild);
  let remoteCreated = false;

  try {
    await retryStep("dedicated-host preparation", () => runRemote(
      `umask 077; mkdir -p ${quote(remoteSource)}; chmod 700 ${quote(runRoot)}`,
    ));
    remoteCreated = true;

    const sourceFiles = sourceFileList(root);
    await retryStep("Probierz source sync", () => spawnAndWait("rsync", [
      "-a", "--delete",
      "--from0", "--files-from=-",
      "-e", sshTransport,
      `${root}/`, `${HOST}:${remoteSource}/`,
    ], { stdio: ["pipe", "inherit", "inherit"] }, onChild, sourceFiles));

    await retryStep("Byk app sync", () => spawnAndWait("rsync", [
      "-a", "--delete", "-e", sshTransport,
      `${appPath}/`, `${HOST}:${remoteApp}/`,
    ], { stdio: ["ignore", "inherit", "inherit"] }, onChild));

    const config = JSON.stringify({
      runRoot,
      sourceRoot: remoteSource,
      appPath: remoteApp,
      socketPath: remoteSocket,
      recipient,
      iosDevice,
      iosVersion,
    }) + "\n";
    const worker = `${remoteSource}/agent/remote/byk-auth-worker.mjs`;
    const result = await spawnAndWait("ssh", [
      ...ssh,
      "-o", "ExitOnForwardFailure=yes",
      "-o", "StreamLocalBindUnlink=yes",
      "-R", `${remoteSocket}:${socketPath}`,
      HOST,
      `umask 077; exec ${quote(REMOTE_NODE)} ${quote(worker)}`,
    ], { stdio: ["pipe", "inherit", "inherit"] }, onChild, config);
    if (result.code === 255) throw infrastructureFailure("dedicated-host worker transport failed");
    clearQuarantine();
    return result;
  } catch (error) {
    if (error?.code === "REMOTE_INFRASTRUCTURE") quarantineHost(error.message);
    throw error;
  } finally {
    if (remoteCreated) await runRemote(`rm -rf -- ${quote(runRoot)}`).catch(() => {});
  }
}
