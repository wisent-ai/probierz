#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { chmod, lstat, mkdtemp, readFile, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runRemoteBykAuth } from "./remote/byk-auth.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ROTATOR_ROOT = path.resolve(ROOT, "../entitlements-rotator");
const BROKER_BINARY = path.join(ROTATOR_ROOT, "target/debug/skarbiec-entitlements-router");
const MAILBOX = "byk-ios-login";
const SPEC = "byk-auth.e2e.ts";
const PROBE_BROKER = process.argv.includes("--probe-broker");
const SEED_RESEND = process.argv.includes("--seed-resend");
const REMOTE_BYK = !process.argv.includes("--local");
const RESEND_SOURCE = path.resolve(ROOT, "../weles/.env");
const READY_TIMEOUT_MS = Number("15000");
const STDERR_LIMIT = Number("4000");
const SHUTDOWN_TIMEOUT_MS = Number("2000");
const TEST_ENV_NAMES = new Set([
  "PATH", "HOME", "TMPDIR", "USER", "SHELL", "LANG", "TERM", "COLORTERM",
  "FORCE_COLOR", "NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "APPIUM_HOME",
  "DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS", "XCODE_DEFAULT_TOOLCHAIN_OVERRIDE",
  "XCODE_DEVELOPER_USR_PATH", "XCODE_PRODUCT_BUILD_VERSION", "XCODE_TOOLCHAIN_PATH",
  "XCODE_VERSION_ACTUAL", "XCODE_VERSION_MAJOR", "XCODE_VERSION_MINOR",
  "XCODE_XCCONFIG_FILE", "APP_IOS", "BUNDLE_ID", "IOS_DEVICE", "IOS_VERSION", "CI",
]);

class BrokerStartupError extends Error {
  constructor(message, stderr = "") { super(message); this.stderr = stderr; }
}

function validateAppConfiguration() {
  const app = process.env.APP_IOS || "";
  const bundle = process.env.BUNDLE_ID || "";
  if (app !== app.trim() || bundle !== bundle.trim()) {
    throw new Error("APP_IOS and BUNDLE_ID must not contain surrounding whitespace");
  }
  if (Boolean(app) === Boolean(bundle)) {
    throw new Error("set exactly one of APP_IOS or BUNDLE_ID");
  }
}

function sanitizeBrokerStderr(raw) {
  return raw.slice(-STDERR_LIMIT).replace(/[^\n]+/g, "[REDACTED]").trim();
}

let activeChild = null;
let brokerChild = null;
let receivedSignal = null;
function spawnAndWait(command, args, options) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, options);
    activeChild = child;
    let settled = false;
    child.once("error", () => {
      if (settled) return;
      settled = true;
      if (activeChild === child) activeChild = null;
      reject(new Error(`could not start ${command}`));
    });
    child.once("close", (code, signal) => {
      if (settled) return;
      settled = true;
      if (activeChild === child) activeChild = null;
      resolve({ code, signal });
    });
  });
}

async function ensureBrokerBinary() {
  const result = await spawnAndWait("cargo", ["build", "--bin", "skarbiec-entitlements-router"], {
    cwd: ROTATOR_ROOT, stdio: "inherit",
  });
  if (result.code !== Number("0")) throw new Error("failed to build the Skarbiec mailbox broker");
  if (!existsSync(BROKER_BINARY)) {
    throw new Error("Skarbiec mailbox broker build did not produce its binary");
  }
}

const validRecipient = (value) => typeof value === "string"
  && value.length <= Number("254") && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value);
function waitForBrokerReady(child, expectedSocket) {
  return new Promise((resolve, reject) => {
    let stdout = "";
    let stderr = "";
    let settled = false;
    let parsing = false;
    const captureStderr = (chunk) => {
      stderr = (stderr + chunk.toString("utf8")).slice(-STDERR_LIMIT);
    };
    const cleanup = () => {
      clearTimeout(timer);
      child.stdout.off("data", readStdout);
      child.stderr.off("data", captureStderr);
      child.off("error", failedToSpawn);
      child.off("exit", exitedEarly);
    };
    const fail = (message) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new BrokerStartupError(message, sanitizeBrokerStderr(stderr)));
    };
    const failedToSpawn = () => fail("could not start the Skarbiec mailbox broker");
    const exitedEarly = () => fail("Skarbiec mailbox broker exited before readiness");
    const acceptReadiness = async (line) => {
      let ready;
      try { ready = JSON.parse(line); }
      catch { fail("Skarbiec mailbox broker returned invalid readiness JSON"); return; }
      if (!ready || Array.isArray(ready) || ready.status !== "ready" || ready.mailbox !== MAILBOX) {
        fail("Skarbiec mailbox broker returned invalid readiness data"); return;
      }
      if (typeof ready.socket_path !== "string" || !path.isAbsolute(ready.socket_path)
          || ready.socket_path !== expectedSocket) {
        fail("Skarbiec mailbox broker returned an invalid socket path"); return;
      }
      if (!validRecipient(ready.recipient)) {
        fail("Skarbiec mailbox broker returned an invalid recipient"); return;
      }
      try {
        if (!(await lstat(expectedSocket)).isSocket()) {
          fail("Skarbiec mailbox broker did not create a Unix socket"); return;
        }
      } catch { fail("Skarbiec mailbox broker did not create a Unix socket"); return; }
      if (settled) return;
      settled = true;
      cleanup();
      child.stdout.resume();
      child.stderr.resume();
      resolve({ recipient: ready.recipient });
    };
    const readStdout = (chunk) => {
      if (settled || parsing) return;
      stdout += chunk.toString("utf8");
      if (stdout.length > Number("16384")) {
        fail("Skarbiec mailbox broker readiness line was too large"); return;
      }
      const newline = stdout.indexOf("\n");
      if (newline === -Number("1")) return;
      parsing = true;
      void acceptReadiness(stdout.slice(Number("0"), newline).replace(/\r$/, ""));
    };
    const timer = setTimeout(
      () => fail("timed out waiting for the Skarbiec mailbox broker"), READY_TIMEOUT_MS,
    );
    child.stderr.on("data", captureStderr);
    child.stdout.on("data", readStdout);
    child.once("error", failedToSpawn);
    child.once("exit", exitedEarly);
  });
}

function testEnvironment(socketPath, recipient) {
  const env = {};
  for (const [name, value] of Object.entries(process.env)) {
    if (TEST_ENV_NAMES.has(name) || name.startsWith("LC_")) env[name] = value;
  }
  env.PROBIERZ_SPEC = SPEC;
  env.BYK_OTP_SOCKET = socketPath;
  env.BYK_TEST_EMAIL = recipient;
  return env;
}

async function terminateBroker(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  let didClose = false;
  const closed = new Promise((resolve) => child.once("close", () => {
    didClose = true;
    resolve();
  }));
  const waitForClose = () => Promise.race([
    closed,
    new Promise((resolve) => setTimeout(resolve, SHUTDOWN_TIMEOUT_MS)),
  ]);
  child.kill("SIGTERM");
  await waitForClose();
  if (!didClose) {
    child.kill("SIGKILL");
    await waitForClose();
  }
}

const SIGNALS = ["SIGINT", "SIGTERM", "SIGHUP"];
const signalHandlers = new Map();
for (const signal of SIGNALS) {
  const handler = () => {
    if (receivedSignal) {
      activeChild?.kill("SIGKILL");
      brokerChild?.kill("SIGKILL");
      return;
    }
    receivedSignal = signal;
    activeChild?.kill(signal);
    if (brokerChild !== activeChild) brokerChild?.kill("SIGTERM");
  };
  signalHandlers.set(signal, handler);
  process.on(signal, handler);
}

async function brokerEnvironment() {
  const brokerEnv = { ...process.env };
  if (brokerEnv.SKARBIEC_UNLOCK) return brokerEnv;
  const unlockPath = brokerEnv.SKARBIEC_UNLOCK_FILE
    || path.join(brokerEnv.HOME || homedir(), ".skarbiec-unlock");
  try {
    const unlock = (await readFile(unlockPath, "utf8")).trim();
    if (unlock) brokerEnv.SKARBIEC_UNLOCK = unlock;
  } catch {
    // The broker reports a sanitized startup error if the local key needs an unlock.
  }
  return brokerEnv;
}

async function main() {
  let temporaryDirectory = null;
  let exitCode = Number("1");
  let exitSignal = null;
  try {
    if (SEED_RESEND) {
      await ensureBrokerBinary();
      const seed = await spawnAndWait(BROKER_BINARY, ["seed-resend", RESEND_SOURCE], {
        cwd: ROTATOR_ROOT, env: await brokerEnvironment(), stdio: ["ignore", "inherit", "inherit"],
      });
      return {
        exitCode: seed.code === null ? Number("1") : seed.code,
        exitSignal: receivedSignal || seed.signal,
      };
    }
    if (PROBE_BROKER) {
      await ensureBrokerBinary();
      const probe = await spawnAndWait(BROKER_BINARY, ["mailbox-probe", "--mailbox", MAILBOX], {
        cwd: ROTATOR_ROOT, env: await brokerEnvironment(), stdio: ["ignore", "inherit", "inherit"],
      });
      return {
        exitCode: probe.code === null ? Number("1") : probe.code,
        exitSignal: receivedSignal || probe.signal,
      };
    }
    validateAppConfiguration();
    await ensureBrokerBinary();
    if (receivedSignal) return { exitCode, exitSignal: receivedSignal };
    temporaryDirectory = await mkdtemp(path.join(tmpdir(), "probierz-byk-auth-"));
    await chmod(temporaryDirectory, Number.parseInt("700", Number("8")));
    const socketPath = path.join(temporaryDirectory, "byk-otp.sock");
    const brokerEnv = await brokerEnvironment();
    brokerChild = spawn(BROKER_BINARY,
      ["mailbox-broker", "--mailbox", MAILBOX, "--socket", socketPath], {
        cwd: ROTATOR_ROOT, env: brokerEnv, stdio: ["ignore", "pipe", "pipe"],
      });
    activeChild = brokerChild;
    const { recipient } = await waitForBrokerReady(brokerChild, socketPath);
    if (activeChild === brokerChild) activeChild = null;
    if (receivedSignal) return { exitCode, exitSignal: receivedSignal };
    const result = REMOTE_BYK
      ? await runRemoteBykAuth({
        root: ROOT,
        appPath: process.env.APP_IOS || "",
        iosDevice: process.env.IOS_DEVICE || "iPhone 15",
        iosVersion: process.env.IOS_VERSION || "",
        socketPath,
        recipient,
        onChild: (child) => { activeChild = child; },
      })
      : await spawnAndWait("npm", ["run", "test:mobile:ios"], {
        cwd: ROOT,
        env: testEnvironment(socketPath, recipient),
        stdio: ["ignore", "inherit", "inherit"],
      });
    exitCode = result.code === null ? Number("1") : result.code;
    exitSignal = result.signal;
  } catch (error) {
    const message = error instanceof Error ? error.message : "unknown failure";
    console.error(`byk auth runner: ${message}`);
    if (error instanceof BrokerStartupError && error.stderr) {
      console.error(`broker stderr (sanitized, truncated):\n${error.stderr}`);
    }
  } finally {
    activeChild = null;
    await terminateBroker(brokerChild);
    brokerChild = null;
    if (temporaryDirectory) await rm(temporaryDirectory, { recursive: true, force: true });
  }
  return { exitCode, exitSignal: receivedSignal || exitSignal };
}

const result = await main();
for (const [signal, handler] of signalHandlers) process.off(signal, handler);
if (result.exitSignal) process.kill(process.pid, result.exitSignal);
else process.exitCode = result.exitCode;
