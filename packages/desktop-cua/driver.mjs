// desktop:cua driver — thin node wrapper over the cua-driver CLI.
// cua-driver drives real macOS apps through the Accessibility API (no
// xcodebuild, no Appium): launch hidden, walk the AX tree, click/type by
// element_index. Specs stay plain node scripts, same shape as the TUI
// surface. The binary needs an Accessibility grant once per host.
import { spawnSync } from "node:child_process";

const CUA_BIN = process.env.CUA_DRIVER_BIN || "cua-driver";
const LAUNCH_WAIT_MS = Number("8000");
const POLL_MS = Number("400");

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function cuaCall(tool, args = {}) {
  const out = spawnSync(CUA_BIN, ["call", tool, JSON.stringify(args)], {
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

function findWindow(pid) {
  const listed = cuaCall("list_windows") || {};
  return (listed.windows || []).find((win) => win.pid === pid && win.layer === 0) || null;
}

export function launchCuaApp({ bundleId = process.env.CUA_BUNDLE_ID, name = process.env.CUA_APP_NAME } = {}) {
  if (!bundleId && !name) throw new Error("launchCuaApp needs CUA_BUNDLE_ID or CUA_APP_NAME");
  const launched = cuaCall("launch_app", bundleId ? { bundle_id: bundleId } : { name });
  const pid = launched?.pid ?? launched?.app?.pid;
  if (!pid) throw new Error(`launch_app returned no pid: ${JSON.stringify(launched).slice(-300)}`);
  const ownWindow = (launched?.windows || []).find((win) => win.window_id);
  if (ownWindow) return { pid, windowId: ownWindow.window_id };
  const deadline = Date.now() + LAUNCH_WAIT_MS;
  while (Date.now() < deadline) {
    const win = findWindow(pid);
    if (win) return { pid, windowId: win.window_id };
    sleep(POLL_MS);
  }
  throw new Error(`app ${bundleId || name} produced no window within ${LAUNCH_WAIT_MS}ms`);
}

export function snapshotTree(pid, windowId) {
  const state = cuaCall("get_window_state", { pid, window_id: windowId });
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
  throw new Error(`timed out waiting for ${JSON.stringify(needle)}; last tree:\n${last.slice(-2000)}`);
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
