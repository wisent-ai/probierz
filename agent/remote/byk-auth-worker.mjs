import { spawn } from "node:child_process";
import { chmod, lstat, mkdir, rm, symlink } from "node:fs/promises";
import path from "node:path";

const INPUT_LIMIT = Number("4096");
const SPEC = "byk-auth.e2e.ts";
const NPM = "/opt/homebrew/bin/npm";
const SYSTEM_PATH = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
const ENV_NAMES = new Set([
  "PATH", "HOME", "TMPDIR", "USER", "SHELL", "LANG", "TERM", "COLORTERM",
  "FORCE_COLOR", "NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "APPIUM_HOME",
  "DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS", "XCODE_DEFAULT_TOOLCHAIN_OVERRIDE",
  "XCODE_DEVELOPER_USR_PATH", "XCODE_PRODUCT_BUILD_VERSION", "XCODE_TOOLCHAIN_PATH",
  "XCODE_VERSION_ACTUAL", "XCODE_VERSION_MAJOR", "XCODE_VERSION_MINOR",
  "XCODE_XCCONFIG_FILE", "IOS_DEVICE", "IOS_VERSION", "CI",
]);
const SIGNALS = ["SIGINT", "SIGTERM", "SIGHUP"];
let activeChild = null;
let receivedSignal = null;

function fail(message) {
  throw new Error(message);
}

async function inputConfig() {
  const chunks = [];
  let size = Number("0");
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > INPUT_LIMIT) fail("remote Byk configuration is too large");
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8");
  if (!raw.endsWith("\n") || raw.indexOf("\n") !== raw.length - Number("1")) {
    fail("remote Byk configuration must be one JSON line");
  }
  let value;
  try { value = JSON.parse(raw); }
  catch { fail("remote Byk configuration is invalid JSON"); }
  const keys = [
    "runRoot", "sourceRoot", "appPath", "socketPath", "recipient",
    "iosDevice", "iosVersion",
  ];
  const required = keys.filter((key) => key !== "iosVersion");
  if (!value || Array.isArray(value) || Object.keys(value).length !== keys.length
      || keys.some((key) => typeof value[key] !== "string")
      || required.some((key) => !value[key])) {
    fail("remote Byk configuration has invalid schema");
  }
  return value;
}

function requireChildPath(root, candidate, name) {
  const normalizedRoot = path.resolve(root);
  const normalized = path.resolve(candidate);
  if (!path.isAbsolute(candidate) || !normalized.startsWith(normalizedRoot + path.sep)) {
    fail(`${name} must stay inside the protected run directory`);
  }
  return normalized;
}

function baseEnvironment() {
  const environment = {};
  for (const [name, value] of Object.entries(process.env)) {
    if (ENV_NAMES.has(name) || name.startsWith("LC_")) environment[name] = value;
  }
  environment.PATH = environment.PATH ? `${SYSTEM_PATH}:${environment.PATH}` : SYSTEM_PATH;
  return environment;
}

function testEnvironment(config, appiumHome) {
  const environment = baseEnvironment();
  environment.PROBIERZ_SPEC = SPEC;
  environment.BYK_OTP_SOCKET = config.socketPath;
  environment.BYK_TEST_EMAIL = config.recipient;
  environment.APP_IOS = config.appPath;
  environment.APPIUM_HOME = appiumHome;
  environment.IOS_DEVICE = config.iosDevice;
  if (config.iosVersion) environment.IOS_VERSION = config.iosVersion;
  return environment;
}

function spawnAndWait(command, args, options) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, options);
    activeChild = child;
    child.once("error", () => {
      if (activeChild === child) activeChild = null;
      reject(new Error(`could not start ${command}`));
    });
    child.once("close", (code, signal) => {
      if (activeChild === child) activeChild = null;
      resolve({ code, signal });
    });
  });
}

for (const signal of SIGNALS) {
  process.on(signal, () => {
    if (receivedSignal) {
      activeChild?.kill("SIGKILL");
      return;
    }
    receivedSignal = signal;
    activeChild?.kill(signal);
  });
}

async function main() {
  const config = await inputConfig();
  const runRoot = path.resolve(config.runRoot);
  const sourceRoot = requireChildPath(runRoot, config.sourceRoot, "sourceRoot");
  config.appPath = requireChildPath(runRoot, config.appPath, "appPath");
  config.socketPath = requireChildPath(runRoot, config.socketPath, "socketPath");
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(config.recipient)) {
    fail("remote Byk recipient is invalid");
  }
  if (config.iosDevice.trim() !== config.iosDevice || /[\r\n]/u.test(config.iosDevice)) {
    fail("remote iOS device name is invalid");
  }
  if (config.iosVersion && (!config.iosVersion.split(".").every((part) =>
    part && [...part].every((character) => character >= "0" && character <= "9")))) {
    fail("remote iOS version is invalid");
  }
  const appMetadata = await lstat(config.appPath);
  const socketMetadata = await lstat(config.socketPath);
  if (!appMetadata.isDirectory() || !socketMetadata.isSocket()) {
    fail("remote Byk app or protected OTP socket is unavailable");
  }
  const lockPath = path.join(path.dirname(path.dirname(runRoot)), "byk-auth.lock");
  const npmCache = path.join(runRoot, "npm-cache");
  const appiumHome = path.join(runRoot, "appium-home");
  const xcuitestDriver = path.join(sourceRoot, "node_modules/appium-xcuitest-driver");
  let locked = false;
  try {
    try {
      await mkdir(lockPath);
      await chmod(lockPath, Number.parseInt("700", Number("8")));
      locked = true;
    } catch {
      fail("dedicated iOS host is already running a Byk device test");
    }
    const sdk = await spawnAndWait("/usr/bin/xcrun", [
      "--sdk", "iphonesimulator", "--show-sdk-version",
    ], {
      cwd: sourceRoot,
      env: baseEnvironment(),
      stdio: ["ignore", "ignore", "ignore"],
    });
    if (sdk.code !== Number("0")) {
      fail("dedicated iOS host is missing the Xcode iOS Simulator SDK");
    }
    const installEnvironment = baseEnvironment();
    installEnvironment.NPM_CONFIG_CACHE = npmCache;
    const install = await spawnAndWait(NPM, [
      "ci", "--workspace", "packages/mobile", "--include-workspace-root=false",
    ], {
      cwd: sourceRoot,
      env: installEnvironment,
      stdio: ["ignore", "inherit", "inherit"],
    });
    if (install.code !== Number("0")) return install;
    const appiumModules = path.join(appiumHome, "node_modules");
    await mkdir(appiumModules, { recursive: true });
    await symlink(xcuitestDriver,
      path.join(appiumModules, "appium-xcuitest-driver"), "dir");
    await rm(npmCache, { recursive: true, force: true });
    if (receivedSignal) return { code: null, signal: receivedSignal };
    return await spawnAndWait(NPM, ["run", "test:mobile:ios"], {
      cwd: sourceRoot,
      env: testEnvironment(config, appiumHome),
      stdio: ["ignore", "inherit", "inherit"],
    });
  } finally {
    if (locked) await rm(lockPath, { recursive: true, force: true });
    await rm(runRoot, { recursive: true, force: true });
  }
}

try {
  const result = await main();
  if (result.signal) process.kill(process.pid, result.signal);
  process.exitCode = result.code === null ? Number("1") : result.code;
} catch (error) {
  console.error(`remote Byk runner: ${error instanceof Error ? error.message : "unknown failure"}`);
  process.exitCode = Number("1");
}
