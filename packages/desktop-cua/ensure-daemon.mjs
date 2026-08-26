import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const commandTimeoutMs = 15_000;
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
  const checked = call("check_permissions", { prompt: false });
  if (checked.status !== 0) return null;
  try {
    const payload = JSON.parse(String(checked.stdout || "{}"));
    return (payload?.permissions?.accessibility ?? payload?.accessibility) === true
      ? (payload.permissions ?? payload)
      : null;
  } catch {
    return null;
  }
}

function waitForProbe() {
  const deadline = Date.now() + startupTimeoutMs;
  while (Date.now() < deadline) {
    const permissions = probe();
    if (permissions) return permissions;
    sleep(250);
  }
  return null;
}

let permissions = existsSync(socket) ? probe() : null;
if (!permissions) {
  run(["stop", "--socket", socket]);
  rmSync(socket, { force: true });
  mkdirSync(path.dirname(socket), { recursive: true });
  rmSync(daemonLog, { force: true });

  const launched = spawnSync(
    "/usr/bin/open",
    ["-n", "-g", "-a", "CuaDriver", "--args", "server", "--socket", socket],
    { encoding: "utf8", timeout: commandTimeoutMs },
  );
  if (launched.status !== 0) {
    process.stderr.write(
      `CuaDriver app launch failed: ${String(launched.stderr || launched.stdout || "").trim()}\n`,
    );
    process.exit(1);
  }

  const deadline = Date.now() + startupTimeoutMs;
  while (Date.now() < deadline && !existsSync(socket)) sleep(100);
  if (!existsSync(socket)) {
    const detail = existsSync(daemonLog) ? readFileSync(daemonLog, "utf8").slice(-2000) : "";
    process.stderr.write(`CuaDriver did not create ${socket}${detail ? `:\n${detail}` : "\n"}`);
    process.exit(1);
  }
  permissions = waitForProbe();
}

if (!permissions) {
  process.stderr.write("Probierz CuaDriver daemon cannot use macOS Accessibility\n");
  process.exit(1);
}

process.stdout.write(`${JSON.stringify({ ready: true, socket, permissions })}\n`);
