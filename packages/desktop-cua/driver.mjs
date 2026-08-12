// desktop:cua driver — thin node wrapper over the cua-driver CLI.
// cua-driver drives real macOS apps through the Accessibility API (no
// xcodebuild, no Appium): launch hidden, walk the AX tree, click/type by
// element_index. Specs stay plain node scripts, same shape as the TUI
// surface. The binary needs an Accessibility grant once per host.
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import path from "node:path";

const CUA_BIN = process.env.CUA_DRIVER_BIN || "cua-driver";
const CUA_SOCKET = process.env.CUA_DRIVER_SOCKET
  || path.join(homedir(), "Library", "Caches", "cua-driver", "probierz.sock");
const LAUNCH_WAIT_MS = Number("8000");
const POLL_MS = Number("400");

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function cuaCall(tool, args = {}) {
  const out = spawnSync(CUA_BIN, ["call", tool, JSON.stringify(args), "--socket", CUA_SOCKET], {
    encoding: "utf8",
    maxBuffer: Number("33554432"),
  });
  if (out.status !== 0) {
    throw new Error(`cua-driver ${tool} failed: ${String(out.stderr || out.stdout || "").slice(-400)}`);
  }
  const text = String(out.stdout || "").trim();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function findWindow(pid, name) {
  const listed = cuaCall("list_windows") || {};
  // Apps spawn untitled utility windows (menus, status items) next to the
  // real content window; prefer titled, then largest area. LaunchServices may
  // return pid -1 while a newly registered app is still starting, so a caller
  // that supplied the app name can resolve the concrete pid from its window.
  let candidates = (listed.windows || []).filter((win) => win.pid === pid && win.layer === 0);
  if (!candidates.length && name) {
    candidates = (listed.windows || []).filter((win) => win.app_name === name && win.layer === 0);
  }
  candidates.sort((a, b) => {
    const titleDelta = Number(Boolean(b.title)) - Number(Boolean(a.title));
    if (titleDelta !== 0) return titleDelta;
    return ((b.bounds?.width || 0) * (b.bounds?.height || 0)) - ((a.bounds?.width || 0) * (a.bounds?.height || 0));
  });
  return candidates[0] || null;
}

export function launchCuaApp({
  bundleId = process.env.CUA_BUNDLE_ID,
  name = process.env.CUA_APP_NAME,
  args = [],
  expectedName = name,
  urls = [],
  newInstance = false,
} = {}) {
  if (!bundleId && !name) throw new Error("launchCuaApp needs CUA_BUNDLE_ID or CUA_APP_NAME");
  const launched = cuaCall("launch_app", {
    ...(bundleId ? { bundle_id: bundleId } : { name }),
    ...(args.length ? { additional_arguments: args } : {}),
    ...(urls.length ? { urls } : {}),
    ...(newInstance ? { creates_new_application_instance: true } : {}),
  });
  const pid = launched?.pid ?? launched?.app?.pid;
  if (pid == null) throw new Error(`launch_app returned no pid: ${JSON.stringify(launched).slice(-300)}`);
  const ownWindow = (launched?.windows || []).find((win) => win.window_id);
  if (ownWindow) return { pid: ownWindow.pid ?? pid, windowId: ownWindow.window_id };
  return waitForWindow(pid, expectedName);
}

export function launchCuaBundle({
  bundlePath,
  expectedName,
  args = [],
  background = true,
  urls = [],
} = {}) {
  if (!bundlePath || !expectedName) throw new Error("launchCuaBundle needs bundlePath and expectedName");
  const launched = spawnSync("/usr/bin/open", [
    "-n",
    ...(background ? ["-g"] : []),
    "-a", bundlePath,
    ...urls,
    ...(args.length ? ["--args", ...args] : []),
  ], { encoding: "utf8" });
  if (launched.status !== 0) {
    throw new Error(`open could not launch ${bundlePath}: ${String(launched.stderr || launched.stdout || "").slice(-400)}`);
  }
  return waitForWindow(-1, expectedName);
}

// Launch an app by executing its binary directly with a custom environment
// (e.g. TAMA_TEST_IDENTITY=1 to bypass a sign-in gate in a debug build).
// cua-driver's launch_app has no env support, so specs spawn the process
// themselves and then drive it by pid like any launch_app-launched app.
export function launchCuaProcess({
  executable = process.env.CUA_APP_EXECUTABLE,
  env = {},
  args = [],
} = {}) {
  if (!executable) throw new Error("launchCuaProcess needs CUA_APP_EXECUTABLE or an executable path");
  const child = spawnSync("sh", ["-c", `nohup "$0" "$@" > /dev/null 2>&1 & echo $!`, executable, ...args], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
  const pid = Number(String(child.stdout || "").trim());
  if (!pid) throw new Error(`failed to spawn ${executable}: ${String(child.stderr || "").slice(-300)}`);
  return waitForWindow(pid);
}

function waitForWindow(pid, name) {
  const deadline = Date.now() + LAUNCH_WAIT_MS;
  while (Date.now() < deadline) {
    const win = findWindow(pid, name);
    if (win) return { pid: win.pid, windowId: win.window_id };
    sleep(POLL_MS);
  }
  throw new Error(`pid ${pid} produced no window within ${LAUNCH_WAIT_MS}ms`);
}

export function snapshotState(pid, windowId, { screenshotOutFile } = {}) {
  const args = { pid, window_id: windowId, exact_window: true, max_elements: 5000, max_depth: 64 };
  if (screenshotOutFile) args.screenshot_out_file = screenshotOutFile;
  return cuaCall("get_window_state", args);
}

export function snapshotTree(pid, windowId) {
  const state = snapshotState(pid, windowId);
  return String(state?.tree_markdown || "");
}

export function waitForText(pid, windowId, needle, timeoutMs = Number("10000")) {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < deadline) {
    last = snapshotTree(pid, windowId);
    if (last.includes(needle)) return last;
    sleep(POLL_MS);
  }
  throw new Error(`timed out waiting for ${JSON.stringify(needle)}; last tree (tail): ${last.slice(-600)}`);
}

// First [element_index N] whose tree line contains the needle. The index is
// only valid for the snapshot it came from — re-snapshot after every action.
export function elementIndexOf(tree, needle) {
  for (const line of String(tree).split("\n")) {
    if (!line.includes(needle)) continue;
    const match = line.match(/\[(\d+)\]/);
    if (match) return Number(match[1]);
  }
  throw new Error(`no indexed element matching ${JSON.stringify(needle)} in tree`);
}

export function clickElement(pid, windowId, elementIndex) {
  return cuaCall("click", { pid, window_id: windowId, element_index: elementIndex });
}

export function typeText(pid, text, { windowId, elementIndex } = {}) {
  const args = { pid, text };
  if (elementIndex !== undefined) {
    args.window_id = windowId;
    args.element_index = elementIndex;
  }
  return cuaCall("type_text", args);
}

export function pressKey(pid, key, { windowId } = {}) {
  const args = { pid, key };
  if (windowId) args.window_id = windowId;
  return cuaCall("press_key", args);
}

export function quitApp(pid) {
  spawnSync("kill", [String(pid)]);
}

export function windowBounds(pid, windowId) {
  const listed = cuaCall("list_windows") || {};
  const win = (listed.windows || []).find((w) => w.window_id === windowId);
  if (!win?.bounds) throw new Error(`window ${windowId} not found for pid ${pid}`);
  return win.bounds;
}

// SwiftUI outline rows reject AX actions (-25206/-25200). The reliable path
// into a sidebar is a coordinate click into its band (focus), then keyboard.
export function focusSidebar(pid, windowId, { fraction = Number("0.12") } = {}) {
  const b = windowBounds(pid, windowId);
  cuaCall("click", { pid, x: Math.round(b.x + b.width * fraction), y: Math.round(b.y + b.height * Number("0.5")) });
}

// Select the Nth sidebar row (0-based, in the probe's outline order): focus,
// clamp the selection to the first row with up-spam, walk down, confirm.
export function selectSidebarRow(pid, windowId, rowIndex) {
  focusSidebar(pid, windowId);
  for (let i = 0; i < Number("20"); i += 1) pressKey(pid, "up", { windowId });
  for (let i = 0; i < rowIndex; i += 1) pressKey(pid, "down", { windowId });
  pressKey(pid, "return", { windowId });
}
