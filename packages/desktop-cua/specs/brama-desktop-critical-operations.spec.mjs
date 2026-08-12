import assert from "node:assert/strict";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { spawnSync } from "node:child_process";
import net from "node:net";
import {
  clickElement,
  elementIndexOf,
  launchCuaApp,
  quitApp,
  selectSidebarRow,
  snapshotState,
  snapshotTree,
  typeText,
  waitForText,
} from "../driver.mjs";

const LSREGISTER = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

async function freeLoopbackPort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  if (!port) throw new Error("could not allocate an isolated Brama loopback port");
  return port;
}

function isolateBundleIdentity(bundle) {
  if (!bundle) return;
  const info = path.join(bundle, "Contents", "Info.plist");
  const identifier = `com.wisent.brama.desktop.probierz.${randomUUID().replaceAll("-", "")}`;
  const update = spawnSync("/usr/libexec/PlistBuddy", ["-c", `Set :CFBundleIdentifier ${identifier}`, info], { encoding: "utf8" });
  if (update.status !== 0) throw new Error(`could not isolate Brama bundle identity: ${update.stderr || update.stdout}`);
  const sign = spawnSync("/usr/bin/codesign", ["--force", "--deep", "--sign", "-", bundle], { encoding: "utf8" });
  if (sign.status !== 0) throw new Error(`could not sign isolated Brama bundle: ${sign.stderr || sign.stdout}`);
  return identifier;
}

const appBundle = process.env.MAC_APP_PATH;
if (!appBundle) throw new Error("Brama Desktop CUA journey needs MAC_APP_PATH");
const isolatedBundle = path.join("/Applications", `Brama Probierz ${randomUUID()}.app`);
let bundleIdentifier = null;

const artifacts = path.resolve(process.env.PROBIERZ_ARTIFACTS || "test-results");
const mediaDir = path.join(artifacts, "media");
const mediaManifest = process.env.PROBIERZ_SPEC_MEDIA_PATH;
if (!mediaManifest) throw new Error("PROBIERZ_SPEC_MEDIA_PATH is required for E3 evidence");
const provider = `probierz-${randomUUID()}`;
const runtimePort = await freeLoopbackPort();
const media = [];
mkdirSync(mediaDir, { recursive: true });

function capture(app, name) {
  const file = path.join(mediaDir, `${name}.jpg`);
  const state = snapshotState(app.pid, app.windowId, { screenshotOutFile: file });
  const tree = String(state?.tree_markdown || "");
  writeFileSync(path.join(mediaDir, `${name}.ax.txt`), tree);
  assert.match(tree, /AXApplication|AXWindow/, `${name} screenshot must accompany a live Brama accessibility tree`);
  media.push({ file, kind: "screenshot", contentType: "image/jpeg" });
  return String(state.tree_markdown || "");
}

function clickByText(app, needle) {
  const before = snapshotTree(app.pid, app.windowId);
  clickElement(app.pid, app.windowId, elementIndexOf(before, needle));
  return snapshotTree(app.pid, app.windowId);
}

function fillByText(app, needle, value) {
  const before = snapshotTree(app.pid, app.windowId);
  typeText(app.pid, value, {
    windowId: app.windowId,
    elementIndex: elementIndexOf(before, needle),
  });
  return snapshotTree(app.pid, app.windowId);
}

function waitUntil(app, predicate, message, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let tree = "";
  while (Date.now() < deadline) {
    tree = snapshotTree(app.pid, app.windowId);
    if (predicate(tree)) return tree;
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 400);
  }
  throw new Error(`${message}; last tree: ${tree.slice(-1200)}`);
}

let app = null;
let providerAdded = false;
let journeySucceeded = false;
try {
  const staged = spawnSync("/usr/bin/ditto", [appBundle, isolatedBundle], { encoding: "utf8" });
  if (staged.status !== 0) {
    throw new Error(`could not stage isolated Brama bundle: ${staged.stderr || staged.stdout}`);
  }
  bundleIdentifier = isolateBundleIdentity(isolatedBundle);
  const registration = spawnSync(LSREGISTER, ["-f", isolatedBundle], { encoding: "utf8" });
  if (registration.status !== 0) {
    throw new Error(`could not register isolated Brama bundle: ${registration.stderr || registration.stdout}`);
  }

  const launchEnvironment = {
    BRAMA_BASE_URL: `http://127.0.0.1:${runtimePort}`,
    BRAMA_LOCAL_RUNTIME: "1",
  };
  try {
    for (const [name, value] of Object.entries(launchEnvironment)) {
      const configured = spawnSync("/bin/launchctl", ["setenv", name, value], { encoding: "utf8" });
      if (configured.status !== 0) {
        throw new Error(`could not configure ${name} for isolated Brama launch: ${configured.stderr || configured.stdout}`);
      }
    }
    app = launchCuaApp({
      bundleId: bundleIdentifier,
      newInstance: true,
      args: [
        "--disable-notifications",
        "-bramaDesktop.automaticDiscovery", "0",
        "-bramaDesktop.subscriptionAutomaticDiscovery", "0",
        "-bramaDesktop.runtimeOrigin", `http://127.0.0.1:${runtimePort}`,
      ],
    });
  } finally {
    for (const name of Object.keys(launchEnvironment)) {
      spawnSync("/bin/launchctl", ["unsetenv", name], { stdio: "ignore" });
    }
  }

  const overview = capture(app, "brama-overview");
  assert.match(overview, /Overview/, "Brama Desktop must expose its overview through the accessibility tree");
  selectSidebarRow(app.pid, app.windowId, 3);
  const operational = waitForText(app.pid, app.windowId, "Operational", 30_000);
  assert.match(operational, new RegExp(`127\\.0\\.0\\.1:${runtimePort}`), "Brama Desktop must connect to its bundled loopback runtime");
  capture(app, "brama-operational");

  selectSidebarRow(app.pid, app.windowId, 4);
  waitForText(app.pid, app.windowId, "Model Sources", 15_000);
  const sources = capture(app, "brama-model-sources-before");
  assert.doesNotMatch(sources, new RegExp(provider), "the isolated provider must not exist before the journey");

  clickByText(app, 'AXButton = "Add model source"');
  waitForText(app.pid, app.windowId, "Add model source", 10_000);
  fillByText(app, "Provider (for example openai or anthropic)", provider);
  fillByText(app, "Label (optional)", "Probierz isolated QA");
  fillByText(app, "API key or subscription credential", `qa-${randomUUID()}`);
  const completedForm = snapshotTree(app.pid, app.windowId);
  assert.doesNotMatch(completedForm, /AXButton = "Add".*DISABLED/, "Add must become available after the required fields are filled");
  providerAdded = true;
  clickByText(app, 'AXButton = "Add"');

  const added = waitForText(app.pid, app.windowId, provider, 30_000);
  assert.match(added, /active/i, "the local credential must appear as an active model source");
  waitForText(app.pid, app.windowId, "Operational", 30_000);
  capture(app, "brama-model-source-added");

  clickByText(app, 'AXButton = "Remove"');
  waitForText(app.pid, app.windowId, "Remove this model source?", 10_000);
  clickByText(app, 'AXButton = "Remove model source"');
  const removed = waitUntil(
    app,
    (tree) => !tree.includes(provider) && tree.includes("Operational"),
    "the model source was not removed or the bundled runtime did not recover",
  );
  providerAdded = false;
  assert.doesNotMatch(removed, new RegExp(provider), "the removed provider must disappear from the UI");
  capture(app, "brama-model-source-removed");
  journeySucceeded = true;
} finally {
  if (app && !journeySucceeded) {
    try { capture(app, "brama-failure"); } catch {}
  }
  writeFileSync(mediaManifest, `${JSON.stringify(media, null, 2)}\n`, { mode: 0o600 });
  if (providerAdded) {
    spawnSync("security", [
      "delete-generic-password",
      "-s", "ai.wisent.brama.desktop.providers",
      "-a", provider,
    ], { stdio: "ignore" });
  }
  if (app) quitApp(app.pid);
  spawnSync(LSREGISTER, ["-u", isolatedBundle], { stdio: "ignore" });
  rmSync(isolatedBundle, { recursive: true, force: true });
}
