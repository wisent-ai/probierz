import { closeSync, existsSync, mkdirSync, openSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { cuaPermissions } from "../../agent/preflight.mjs";

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



function waitForProbe() {
  const deadline = Date.now() + startupTimeoutMs;
  while (Date.now() < deadline) {
    const permissions = cuaPermissions(cuaDriver);
    if (permissions) return permissions;
    sleep(250);
  }
  return null;
}

let permissions = existsSync(socket) ? cuaPermissions(cuaDriver) : null;
if (!permissions) {
  // Stado may already own this socket. Never stop its daemon or unlink its
  // endpoint because a readiness read failed.
  mkdirSync(path.dirname(socket), { recursive: true });
  const logFd = openSync(daemonLog, "a", 0o600);

  const bundle = cuaDriver.match(/^(.*\.app)\/Contents\/MacOS\/[^/]+$/)?.[1];
  const args = ["serve", "--socket", socket, "--no-permissions-gate"];
  if (process.platform === "darwin" && bundle) {
    // LaunchServices supplies the Aqua/WindowServer context AppKit needs.
    // Direct launchd children cannot acquire NSPasteboard on a remote worker.
    const launched = spawnSync("/usr/bin/open", [
      "-n", "-g", "-a", bundle,
      "--stdout", daemonLog, "--stderr", daemonLog,
      "--args", ...args,
    ], { stdio: ["ignore", logFd, logFd], timeout: commandTimeoutMs });
    closeSync(logFd);
    if (launched.status !== 0) {
      process.stderr.write(`CuaDriver LaunchServices startup failed: ${launched.error?.message || launched.status}\n`);
      process.exit(1);
    }
  } else {
    const launched = spawn(cuaDriver, args, {
      detached: true,
      stdio: ["ignore", logFd, logFd],
    });
    closeSync(logFd);
    launched.unref();
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

if (!permissions?.accessibility) {
  process.stderr.write(permissions
    ? "CuaDriver has no Accessibility grant; no permission prompt was requested\n"
    : "Probierz cannot read the serving CuaDriver daemon's permission status\n");
  process.exit(1);
}

process.stdout.write(`${JSON.stringify({ ready: true, socket, source: permissions.source })}\n`);
