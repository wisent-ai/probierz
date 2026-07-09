// Toolchain preflight + self-provisioning for the probierz test toolkit.
//
// probierz OWNS the layers it can install: Playwright browsers and Appium
// drivers (the Appium server itself the WDIO configs auto-start). It does NOT
// install host-level dependencies -- Xcode, the Android SDK, simulators,
// WinAppDriver, physical devices -- those are heavy, OS-specific, and belong to
// the operator. So `preflight` detects everything a target needs and, for each
// missing piece, says exactly how to get it: either `probierz setup <target>`
// (the parts we own) or a one-line host install command (the parts we do not).
import { existsSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import os from "node:os";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const PROBE_MS = 15000;

// A binary is present if it runs and exits cleanly. `args` is its cheapest
// version/help invocation. Never touches the network or a browser.
function hasBinary(bin, args) {
  try {
    const r = spawnSync(bin, args, { stdio: "ignore", timeout: PROBE_MS });
    return r.status === 0;
  } catch {
    return false;
  }
}

// Is an Appium driver installed? Deterministic filesystem check against
// $APPIUM_HOME/node_modules/appium-<name>-driver (the install path Appium's own
// manifest records), so detection never flip-flops the way a `npx appium driver
// list` spawn can when the transient CLI resolution fails.
function appiumDriverInstalled(name) {
  const home = process.env.APPIUM_HOME || path.join(os.homedir(), ".appium");
  return existsSync(path.join(home, "node_modules", `appium-${name}-driver`));
}

// Playwright browsers are installed if the shared browser cache has entries.
// Filesystem-only; the playwright binary is never invoked here.
function playwrightBrowsersInstalled() {
  const cache = process.env.PLAYWRIGHT_BROWSERS_PATH
    || path.join(os.homedir(), "Library", "Caches", "ms-playwright");
  try {
    return existsSync(cache) && readdirSync(cache).length > 0;
  } catch {
    return false;
  }
}

// iOS runtime versions that have at least one usable simulator, e.g.
// ["17.5","26.3"]. Parsed from simctl's runtime keys
// (com.apple.CoreSimulator.SimRuntime.iOS-26-3 -> "26.3"). Empty when xcrun is
// missing or no runtime has devices.
function availableIosRuntimes() {
  try {
    const r = spawnSync("xcrun", ["simctl", "list", "devices", "available", "--json"],
      { encoding: "utf8", timeout: PROBE_MS });
    if (r.status !== Number("0") || !r.stdout) return [];
    const byRuntime = JSON.parse(r.stdout).devices || {};
    const out = [];
    for (const [key, list] of Object.entries(byRuntime)) {
      if (!Array.isArray(list) || list.length === Number("0")) continue;
      const m = /SimRuntime\.iOS-(\d+)-(\d+)/.exec(key);
      if (m) out.push(`${m[Number("1")]}.${m[Number("2")]}`);
    }
    return [...new Set(out)].sort();
  } catch {
    return [];
  }
}

// Device names that have at least one available simulator, e.g.
// ["iPhone 17","iPad (A16)"]. Same simctl source; empty when xcrun is missing.
function availableIosDevices() {
  try {
    const r = spawnSync("xcrun", ["simctl", "list", "devices", "available", "--json"],
      { encoding: "utf8", timeout: PROBE_MS });
    if (r.status !== Number("0") || !r.stdout) return [];
    const byRuntime = JSON.parse(r.stdout).devices || {};
    const out = [];
    for (const list of Object.values(byRuntime)) {
      if (!Array.isArray(list)) continue;
      for (const d of list) if (d && d.name) out.push(d.name);
    }
    return [...new Set(out)].sort();
  } catch {
    return [];
  }
}

// The iOS SDK version an .app/.ipa was built against (Info.plist
// DTPlatformVersion), e.g. "26.2", or null if unreadable. When IOS_VERSION is
// unset, XCUITest targets THIS version, so it must be an installed runtime.
function appBuildSdk(appPath) {
  try {
    const r = spawnSync("/usr/libexec/PlistBuddy",
      ["-c", "Print :DTPlatformVersion", path.join(appPath, "Info.plist")],
      { encoding: "utf8", timeout: PROBE_MS });
    if (r.status !== Number("0")) return null;
    return r.stdout.trim() || null;
  } catch {
    return null;
  }
}

// A dependency package present in the workspace (node_modules resolved).
function pkgInstalled(rel) {
  return existsSync(path.join(ROOT, "node_modules", rel))
    || existsSync(path.join(ROOT, rel, "node_modules"));
}

// One check: { name, ok, own, hint }. `own` = probierz can install it
// (surfaced by `setup`); otherwise `hint` is the host install command.
const setupHint = (target) => `probierz setup ${target}`;

function checksFor(target) {
  if (target === "web" || target === "electron") {
    return [
      { name: "@playwright/test", ok: pkgInstalled("@playwright/test"), own: true, hint: setupHint(target) },
      { name: "playwright browsers", ok: playwrightBrowsersInstalled(), own: true, hint: setupHint(target) },
    ];
  }
  if (target === "mobile:ios") {
    // wdio.ios.conf.ts pins platformVersion ONLY when IOS_VERSION is set;
    // otherwise Appium auto-picks the newest installed runtime. Mirror that: if
    // IOS_VERSION is set, that exact runtime must exist; if not, any runtime
    // will do. Either way, report what is available so a pin mismatch is
    // actionable rather than an opaque death at run.
    const runtimes = availableIosRuntimes();
    const pinned = process.env.IOS_VERSION;
    const simOk = pinned ? runtimes.includes(pinned) : runtimes.length > Number("0");
    const simName = pinned ? `iOS simulator runtime ${pinned}` : "iOS simulator runtime (any)";
    const simHint = pinned
      ? (runtimes.length
          ? `iOS ${pinned} runtime not installed. Available: ${runtimes.join(", ")}. Set IOS_VERSION to one of these, or add ${pinned} via Xcode > Settings > Platforms.`
          : "no iOS simulator runtimes found; open Xcode > Settings > Platforms and add one")
      : "no iOS simulator runtimes found; open Xcode > Settings > Platforms and add one";
    // wdio.ios.conf.ts uses deviceName = IOS_DEVICE || "iPhone 15". A device name
    // absent from the installed runtime fails session creation opaquely, so
    // verify the resolved name exists and, on a miss, list what does.
    const wantedDevice = process.env.IOS_DEVICE || "iPhone 15";
    const devices = availableIosDevices();
    const deviceHint = devices.length
      ? `simulator "${wantedDevice}" not found. Available: ${devices.join(", ")}. Set IOS_DEVICE to one of these.`
      : "no iOS simulators found; open Xcode > Settings > Platforms and add a runtime";
    const checks = [
      { name: "Xcode command-line tools (xcrun)", ok: hasBinary("xcrun", ["--version"]), own: false, hint: "install Xcode from the App Store, then: xcode-select --install" },
      { name: "xcodebuild", ok: hasBinary("xcodebuild", ["-version"]), own: false, hint: "install Xcode from the App Store" },
      { name: simName, ok: simOk, own: false, hint: simHint },
      { name: `simulator device "${wantedDevice}"`, ok: devices.includes(wantedDevice), own: false, hint: deviceHint },
      { name: "appium driver: xcuitest", ok: appiumDriverInstalled("xcuitest"), own: true, hint: setupHint(target) },
    ];
    // With no IOS_VERSION pin, XCUITest targets the app's OWN build SDK. If the
    // app was built against a runtime that is not installed, the run dies with
    // "'<sdk>' does not exist" -- catch that here and say to pin IOS_VERSION.
    if (process.env.APP_IOS && !pinned) {
      const sdk = appBuildSdk(process.env.APP_IOS);
      if (sdk && runtimes.length && !runtimes.includes(sdk)) {
        const suggest = runtimes[runtimes.length - Number("1")];
        checks.push({
          name: `app build SDK iOS ${sdk} installed`,
          ok: false,
          own: false,
          hint: `APP_IOS was built against iOS ${sdk}, which is not installed (have: ${runtimes.join(", ")}). Without IOS_VERSION, XCUITest targets the build SDK and fails. Set IOS_VERSION=${suggest} to force an installed runtime.`,
        });
      }
    }
    return checks;
  }
  if (target === "mobile:android") {
    return [
      { name: "adb", ok: hasBinary("adb", ["version"]), own: false, hint: "install Android SDK platform-tools and add them to PATH" },
      { name: "ANDROID_HOME set", ok: Boolean(process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT), own: false, hint: "export ANDROID_HOME to your Android SDK location" },
      { name: "appium driver: uiautomator2", ok: appiumDriverInstalled("uiautomator2"), own: true, hint: setupHint(target) },
    ];
  }
  if (target === "desktop:mac") {
    return [
      { name: "macOS host", ok: os.platform() === "darwin", own: false, hint: "the mac2 driver runs on macOS only" },
      { name: "appium driver: mac2", ok: appiumDriverInstalled("mac2"), own: true, hint: setupHint(target) },
    ];
  }
  if (target === "desktop:win") {
    return [
      { name: "Windows host", ok: os.platform() === "win32", own: false, hint: "WinAppDriver runs on Windows only" },
      { name: "WinAppDriver", ok: hasBinary("WinAppDriver.exe", ["--help"]), own: false, hint: "install WinAppDriver from github.com/microsoft/WinAppDriver/releases" },
    ];
  }
  return null;
}

// Detect whether a target is ready to run. Returns readiness + per-check detail
// + a de-duplicated list of remediation hints for whatever is missing. Never
// mutates anything and never launches a browser.
export function preflight(target) {
  const checks = checksFor(target);
  if (!checks) {
    throw new Error(`unknown target: ${target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win)`);
  }
  const missing = checks.filter((c) => !c.ok);
  const remediation = [...new Set(missing.map((c) => c.hint))];
  return {
    target,
    ready: missing.length === 0,
    checks,
    missing: missing.map((c) => c.name),
    remediation,
  };
}

// The ordered provisioning steps probierz can run itself for a target: npm deps
// plus the browser install (Playwright) or driver install (Appium). Host-level
// dependencies are never in here -- preflight reports those instead.
export function setupSteps(target) {
  const npmInstall = { name: "npm install (workspaces)", command: "npm", args: ["install"], cwd: ROOT };
  const pwInstall = (pkg, withDeps) => ({
    name: `playwright browsers (${pkg})`,
    command: "npm",
    args: ["--workspace", `packages/${pkg}`, "exec", "playwright", "install", ...(withDeps ? ["--with-deps"] : [])],
    cwd: ROOT,
  });
  const driverInstall = (driver) => ({
    name: `appium driver: ${driver}`,
    command: "npx",
    args: ["--no-install", "appium", "driver", "install", driver],
    cwd: ROOT,
  });
  const table = {
    web: [npmInstall, pwInstall("web", true)],
    electron: [npmInstall, pwInstall("electron", false)],
    "mobile:ios": [npmInstall, driverInstall("xcuitest")],
    "mobile:android": [npmInstall, driverInstall("uiautomator2")],
    "desktop:mac": [npmInstall, driverInstall("mac2")],
    "desktop:win": [npmInstall, driverInstall("windows")],
  };
  const steps = table[target];
  if (!steps) {
    throw new Error(`unknown target: ${target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win)`);
  }
  return steps;
}

// Execute the provisioning steps for a target, in order, stopping at the first
// failure. Each step is a real spawn (mirrors runner.mjs). Returns per-step
// results; the host-level deps preflight flags are NOT touched here.
export function runSetup(target, opts = {}) {
  const steps = setupSteps(target);
  const done = [];
  for (const step of steps) {
    const r = spawnSync(step.command, step.args, {
      cwd: step.cwd,
      encoding: "utf8",
      timeout: Number(opts.timeoutMs) || 30 * 60 * 1000,
    });
    const ok = r.status === 0;
    done.push({ step: step.name, command: `${step.command} ${step.args.join(" ")}`, ok, exitCode: r.status === null ? -1 : r.status });
    if (!ok) return { target, ok: false, steps: done, failedAt: step.name, stderrTail: String(r.stderr || "").slice(-2000) };
  }
  return { target, ok: true, steps: done };
}
