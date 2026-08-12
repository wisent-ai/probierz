// Toolchain preflight + self-provisioning for the probierz test toolkit.
//
// probierz OWNS the layers it can install: Playwright browsers and Appium
// drivers (the Appium server itself the WDIO configs auto-start). It does NOT
// install host-level dependencies -- Xcode, the Android SDK, simulators,
// WinAppDriver, physical devices -- those are heavy, OS-specific, and belong to
// the operator. So `preflight` detects everything a target needs and, for each
// missing piece, says exactly how to get it: either `probierz setup <target>`
// (the parts we own) or a one-line host install command (the parts we do not).
import { existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";
import os from "node:os";
import { parse as parseYaml } from "yaml";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const require = createRequire(import.meta.url);
const NATIVE_CAPTURE_SOURCE = path.join(ROOT, "packages", "desktop-native", "tools", "screen-capture-kit.swift");
const NATIVE_CAPTURE_BINARY = path.join(ROOT, "node_modules", ".cache", "probierz", "screen-capture-kit");
const PROBE_MS = 15000;
const CUA_TREE_PROBE_MS = 5000;
const CUA_SOCKET = path.join(os.homedir(), "Library", "Caches", "cua-driver", "probierz.sock");

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

function hasConsoleSession() {
  if (os.platform() !== "darwin") return false;
  try {
    const result = spawnSync("who", [], { encoding: "utf8", timeout: PROBE_MS });
    return result.status === Number("0")
      && String(result.stdout || "").split("\n").some((line) => /\sconsole\s/.test(line));
  } catch {
    return false;
  }
}

// Is an Appium driver installed? Deterministic filesystem check against
// $APPIUM_HOME/node_modules/appium-<name>-driver (the install path Appium's own
// manifest records), so detection never flip-flops the way a `npx appium driver
// list` spawn can when the transient CLI resolution fails.
function appiumDriverInstalled(name, env = process.env) {
  const home = env.APPIUM_HOME || path.join(os.homedir(), ".appium");
  const packageDir = path.join(home, "node_modules", `appium-${name}-driver`);
  if (!existsSync(packageDir)) return false;
  try {
    const extensions = parseYaml(readFileSync(path.join(home, "node_modules", ".cache", "appium", "extensions.yaml"), "utf8"));
    return Boolean(extensions?.drivers?.[name]?.pkgName === `appium-${name}-driver`);
  } catch {
    return false;
  }
}
// The daemon owns the Accessibility grant; a launchd worker cannot inspect
// another process's TCC state with AXIsProcessTrusted. Probe one real window
// through the daemon instead. This is the exact capability a journey needs,
// without walking an unbounded inventory or trusting the caller's TCC state.
function cuaAccessibilityGranted() {
  try {
    const listed = spawnSync("cua-driver", ["call", "list_windows", "{}", "--socket", CUA_SOCKET], {
      encoding: "utf8",
      timeout: PROBE_MS,
    });
    if (listed.status !== 0) return false;
    const data = JSON.parse(String(listed.stdout || "{}"));
    const windows = data.windows || [];
    const candidates = [
      ...windows.filter((candidate) => candidate?.is_on_screen && candidate.title),
      ...windows.filter((candidate) => candidate?.pid && candidate?.window_id),
    ].slice(0, 5);
    return candidates.some((win) => {
      const state = spawnSync("cua-driver", ["call", "get_window_state", JSON.stringify({
        pid: win.pid,
        window_id: win.window_id,
      }), "--socket", CUA_SOCKET], { encoding: "utf8", timeout: CUA_TREE_PROBE_MS });
      return state.status === 0 && String(state.stdout || "").includes("AXApplication");
    });
  } catch {
    return false;
  }
}

// Check the exact browser revisions required by the installed Playwright
// package. A non-empty shared cache can contain only stale revisions.
function playwrightBrowsersInstalled() {
  try {
    const { chromium, firefox, webkit } = require("playwright");
    return [chromium, firefox, webkit]
      .map((browser) => browser.executablePath())
      .every((executable) => executable && existsSync(executable));
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

function checksFor(target, env = process.env) {
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
    const pinned = env.IOS_VERSION;
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
    const wantedDevice = env.IOS_DEVICE || "iPhone 15";
    const devices = availableIosDevices();
    const deviceHint = devices.length
      ? `simulator "${wantedDevice}" not found. Available: ${devices.join(", ")}. Set IOS_DEVICE to one of these.`
      : "no iOS simulators found; open Xcode > Settings > Platforms and add a runtime";
    const checks = [
      { name: "Xcode command-line tools (xcrun)", ok: hasBinary("xcrun", ["--version"]), own: false, hint: "install Xcode from the App Store, then: xcode-select --install" },
      { name: "xcodebuild", ok: hasBinary("xcodebuild", ["-version"]), own: false, hint: "install Xcode from the App Store" },
      { name: simName, ok: simOk, own: false, hint: simHint },
      { name: `simulator device "${wantedDevice}"`, ok: devices.includes(wantedDevice), own: false, hint: deviceHint },
      { name: "appium driver: xcuitest", ok: appiumDriverInstalled("xcuitest", env), own: true, hint: setupHint(target) },
    ];
    // With no IOS_VERSION pin, XCUITest targets the app's OWN build SDK. If the
    // app was built against a runtime that is not installed, the run dies with
    // "'<sdk>' does not exist" -- catch that here and say to pin IOS_VERSION.
    if (env.APP_IOS && !pinned) {
      const sdk = appBuildSdk(env.APP_IOS);
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
      { name: "ANDROID_HOME set", ok: Boolean(env.ANDROID_HOME || env.ANDROID_SDK_ROOT), own: false, hint: "export ANDROID_HOME to your Android SDK location" },
      { name: "appium driver: uiautomator2", ok: appiumDriverInstalled("uiautomator2", env), own: true, hint: setupHint(target) },
    ];
  }
  if (target === "desktop:mac") {
    return [
      { name: "macOS host", ok: os.platform() === "darwin", own: false, hint: "the mac2 driver runs on macOS only" },
      { name: "full Xcode toolchain", ok: hasBinary("xcodebuild", ["-version"]), own: false, hint: "install Xcode from the App Store and select it with xcode-select" },
      { name: "appium driver: mac2", ok: appiumDriverInstalled("mac2", env), own: true, hint: setupHint(target) },
    ];
  }
  if (target === "desktop:win") {
    return [
      { name: "Windows host", ok: os.platform() === "win32", own: false, hint: "WinAppDriver runs on Windows only" },
      { name: "WinAppDriver", ok: hasBinary("WinAppDriver.exe", ["--help"]), own: false, hint: "install WinAppDriver from github.com/microsoft/WinAppDriver/releases" },
    ];
  }
  if (target === "tui") {
    return [
      { name: "python3 pty.spawn shim", ok: hasBinary("python3", ["--version"]), own: false, hint: "python3 stdlib provides the pty.spawn shim the TUI driver uses" },
      { name: "node runtime", ok: hasBinary(process.execPath, ["--version"]), own: false, hint: "node is required for the TUI spec runner" },
    ];
  }
  if (target === "desktop:cua") {
    return [
      { name: "macOS host", ok: os.platform() === "darwin", own: false, hint: "the cua-driver drives the macOS Accessibility API" },
      { name: "logged-in macOS console session", ok: hasConsoleSession(), own: false, hint: "select a dedicated macOS host with an active GUI login session" },
      { name: "cua-driver binary", ok: hasBinary("cua-driver", ["--version"]), own: false, hint: "install cua-driver (macOS Accessibility driver)" },
      { name: "cua-driver accessibility", ok: cuaAccessibilityGranted(), own: true, hint: "grant CuaDriver in System Settings > Privacy & Security > Accessibility (once per host)" },
    ];
  }
  return null;
}

// Detect whether a target is ready to run. Returns readiness + per-check detail
// + a de-duplicated list of remediation hints for whatever is missing. Never
// mutates anything and never launches a browser.
export function preflight(target, env = process.env) {
  const checks = checksFor(target, env);
  if (!checks) {
    throw new Error(`unknown target: ${target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win|tui)`);
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
  const driverInstall = (driver, version) => ({
    name: `appium driver: ${driver}`,
    command: "npx",
    args: ["--no-install", "appium", "driver", "install", version ? `${driver}@${version}` : driver],
    cwd: ROOT,
    skipWhen: () => appiumDriverInstalled(driver),
  });
  const nativeCaptureBuild = {
    name: "ScreenCaptureKit recorder",
    command: "xcrun",
    args: ["swiftc", "-parse-as-library", NATIVE_CAPTURE_SOURCE, "-o", NATIVE_CAPTURE_BINARY],
    cwd: ROOT,
    outputDir: path.dirname(NATIVE_CAPTURE_BINARY),
  };
  const table = {
    web: [npmInstall, pwInstall("web", true)],
    electron: [npmInstall, pwInstall("electron", false)],
    "mobile:ios": [npmInstall, driverInstall("xcuitest")],
    "mobile:android": [npmInstall, driverInstall("uiautomator2")],
    "desktop:mac": [npmInstall, driverInstall("mac2", "2.2.2"), nativeCaptureBuild],
    "desktop:win": [npmInstall, driverInstall("windows")],
    "desktop:cua": [npmInstall, {
      name: "cua-driver daemon",
      command: process.execPath,
      args: [path.join(ROOT, "packages", "desktop-cua", "ensure-daemon.mjs")],
      cwd: ROOT,
      skipWhen: () => cuaAccessibilityGranted(),
    }],
    tui: [npmInstall],
  };
  const steps = table[target];
  if (!steps) {
    throw new Error(`unknown target: ${target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui)`);
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
    // Idempotent re-runs: a step whose skipWhen holds (e.g. the Appium
    // driver is already in APPIUM_HOME) reports ok without re-executing —
    // `appium driver install` exits 1 when the driver exists, which made
    // every second `setup` on a warm host fail.
    if (step.skipWhen?.()) {
      done.push({ step: step.name, command: `${step.command} ${step.args.join(" ")}`, ok: true, skipped: true });
      continue;
    }
    if (step.outputDir) mkdirSync(step.outputDir, { recursive: true });
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
