import { closeSync, existsSync, mkdirSync, openSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";

const commandTimeoutMs = 15_000;
const probeTimeoutMs = 5_000;
const startupTimeoutMs = 60_000;
const socket = path.join(homedir(), "Library", "Caches", "cua-driver", "probierz.sock");
const daemonLog = path.join(path.dirname(socket), "probierz-daemon.log");
const bundledDriver = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";
const cuaDriver = process.env.CUA_DRIVER_BIN
  || (process.platform === "darwin" && existsSync(bundledDriver) ? bundledDriver : "cua-driver");

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function run(args, timeout = commandTimeoutMs) {
  return spawnSync(cuaDriver, args, { encoding: "utf8", timeout });
}

function call(tool, args = {}, timeout = commandTimeoutMs) {
  return run(["call", tool, JSON.stringify(args), "--socket", socket], timeout);
}

function probe() {
  const listed = call("list_windows");
  if (listed.status !== 0) return null;
  let payload;
  try {
    payload = JSON.parse(String(listed.stdout || "{}"));
  } catch {
    return null;
  }
  const windows = payload.windows || [];
  const candidates = [
    ...windows.filter((candidate) => candidate?.is_on_screen && candidate.title),
    ...windows.filter((candidate) => candidate?.pid && candidate?.window_id),
  ].slice(0, 5);
  for (const window of candidates) {
    const state = call(
      "get_window_state",
      { pid: window.pid, window_id: window.window_id },
      probeTimeoutMs,
    );
    if (state.status === 0 && String(state.stdout || "").includes("AXApplication")) return window;
  }
  return null;
}

function waitForProbe() {
  const deadline = Date.now() + startupTimeoutMs;
  while (Date.now() < deadline) {
    const window = probe();
    if (window) return window;
    sleep(250);
  }
  return null;
}

let window = existsSync(socket) ? probe() : null;
if (!window) {
  run(["stop", "--socket", socket]);
  rmSync(socket, { force: true });
  mkdirSync(path.dirname(socket), { recursive: true });
  rmSync(daemonLog, { force: true });
  const logFd = openSync(daemonLog, "a", 0o600);


  const launched = spawn(
    cuaDriver,
    ["serve", "--socket", socket],
    {
      detached: true,
      stdio: ["ignore", logFd, logFd],
    },
  );
  closeSync(logFd);
  launched.unref();

  const deadline = Date.now() + startupTimeoutMs;
  while (Date.now() < deadline && !existsSync(socket)) sleep(100);
  if (!existsSync(socket)) {
    const detail = existsSync(daemonLog) ? readFileSync(daemonLog, "utf8").slice(-2000) : "";
    process.stderr.write(`CuaDriver did not create ${socket}${detail ? `:\n${detail}` : "\n"}`);
    process.exit(1);
  }
  window = waitForProbe();
}

if (!window) {
  process.stderr.write("Probierz CuaDriver daemon cannot read a live Accessibility tree\n");
  process.exit(1);
}

process.stdout.write(`${JSON.stringify({ ready: true, socket, pid: window.pid, windowId: window.window_id })}\n`);
