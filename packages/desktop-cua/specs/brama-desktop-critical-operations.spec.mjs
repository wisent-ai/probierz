// Journey: critical-operations (brama-desktop, desktop:cua)
//
// The real Brama Desktop bundle owns both standalone mutations: provider
// credentials in Keychain and route aliases in the bundled Brama registry.
// This journey performs add, replace and delete through the native UI, then
// reads those two durable stores after every mutation.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  cuaCall,
  launchCuaProcess,
  quitApp,
  snapshotState,
  snapshotTree,
} from "../driver.mjs";

function reservePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => {
        if (error) reject(error);
        else resolve(address.port);
      });
    });
  });
}

const BUNDLE_EXECUTABLE = process.env.CUA_APP_EXECUTABLE
  || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/brama-desktop/.build/Brama.app/Contents/MacOS/Brama";
const artifactsDir = path.resolve(process.env.PROBIERZ_ARTIFACTS || "test-results");
const specName = process.env.PROBIERZ_SPEC_NAME || "brama-desktop-critical-operations";
const media = [];
const suffix = randomUUID().replaceAll("-", "");
const stateRoot = path.join(artifactsDir, `${specName}-state-${suffix}`);
const routes = path.join(stateRoot, "Runtime", "routes.json");
const keychainNamespace = `ai.wisent.brama.desktop.probierz.${suffix}`;
const keychainService = `${keychainNamespace}.providers`;
const keychainRuntimeService = `${keychainNamespace}.runtime`;
const runtimeOrigin = `http://127.0.0.1:${await reservePort()}`;
const provider = "openai";
const firstKey = `qa-${randomUUID()}`;
const replacementKey = `qa-${randomUUID()}`;
const alias = `probierz/${suffix}`;

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function elementsOf(state) {
  return state?.elements || state?.structuredContent?.elements || [];
}

function windowsOf(pid) {
  const listed = cuaCall("list_windows") || {};
  return (listed.windows || []).filter((window) => window.pid === pid && window.layer === 0);
}

function dumpTree(name, tree) {
  const file = path.join(artifactsDir, `${specName}-${name}.tree.txt`);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, tree);
  return file;
}

function waitForWindowText(pid, needle, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let widest = "";
  while (Date.now() < deadline) {
    for (const window of windowsOf(pid)) {
      const tree = snapshotTree(pid, window.window_id);
      if (tree.includes(needle)) return { windowId: window.window_id, tree };
      if (tree.length > widest.length) widest = tree;
    }
    sleep(300);
  }
  const dumped = dumpTree(`timeout-${needle.replace(/[^A-Za-z0-9]+/g, "-").slice(0, 50)}`, widest);
  throw new Error(`timed out waiting for ${JSON.stringify(needle)}; tree: ${dumped}`);
}

function waitUntil(check, description, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const value = check();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    sleep(300);
  }
  throw new Error(`timed out waiting for ${description}${lastError ? `: ${lastError.message}` : ""}`);
}

function activate(pid, windowId, label, matches, settled, timeoutMs = 15_000) {
  const rungs = [
    (element, state) => cuaCall("click", {
      pid,
      ...(element.element_token
        ? { element_token: element.element_token }
        : {
          window_id: windowId,
          element_index: element.element_index,
          ...(state.snapshot_id ? { snapshot_id: state.snapshot_id } : {}),
        }),
    }),
    (element) => cuaCall("click", {
      pid,
      window_id: windowId,
      x: Math.round(element.frame.x + element.frame.w / 2),
      y: Math.round(element.frame.y + element.frame.h / 2),
    }),
    (element) => cuaCall("click", {
      pid,
      window_id: windowId,
      x: Math.round(element.frame.x + element.frame.w / 2),
      y: Math.round(element.frame.y + element.frame.h / 2),
      delivery_mode: "foreground",
    }),
  ];

  let lastError = null;
  for (const [index, press] of rungs.entries()) {
    const state = snapshotState(pid, windowId) || {};
    const element = elementsOf(state).find(matches);
    if (!element) {
      dumpTree(`activate-${label}-missing`, String(state.tree_markdown || ""));
      throw new Error(`no element to press for ${label}`);
    }
    if (index > 0 && !element.frame) throw new Error(`element for ${label} has no click frame`);
    try {
      press(element, state);
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
    }
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      sleep(350);
      const tree = snapshotTree(pid, windowId);
      if (settled(tree)) return tree;
    }
    dumpTree(`activate-${label}-rung-${index + 1}`, snapshotTree(pid, windowId));
  }
  throw new Error(`pressing ${label} never settled${lastError ? `: ${lastError}` : ""}`);
}

function typeField(pid, windowId, label, value) {
  const state = snapshotState(pid, windowId) || {};
  const element = elementsOf(state).find((candidate) =>
    String(candidate.label || "") === label
    && ["AXTextField", "AXSecureTextField", "TextField", "SecureTextField"].some((role) =>
      String(candidate.role || "").includes(role)));
  if (!element) {
    dumpTree(`field-${label.replace(/[^A-Za-z0-9]+/g, "-")}-missing`, String(state.tree_markdown || ""));
    throw new Error(`no editable field labelled ${label}`);
  }
  cuaCall("type_text", {
    pid,
    ...(element.element_token
      ? { element_token: element.element_token }
      : {
        window_id: windowId,
        element_index: element.element_index,
        ...(state.snapshot_id ? { snapshot_id: state.snapshot_id } : {}),
      }),
    text: value,
  });
  const after = snapshotState(pid, windowId) || {};
  const afterElement = elementsOf(after).find((candidate) => String(candidate.label || "") === label);
  assert.ok(afterElement, `${label} must remain addressable after typing`);
  if (!String(afterElement.role || "").includes("Secure")) {
    assert.equal(String(afterElement.value || ""), value, `${label} must contain the exact entered value`);
  }
}

function capture(pid, windowId, name) {
  const file = path.join(artifactsDir, `${specName}-${name}.png`);
  mkdirSync(path.dirname(file), { recursive: true });
  snapshotState(pid, windowId, { screenshotOutFile: file });
  assert.ok(existsSync(file), `cua-driver produced no screenshot at ${file}`);
  media.push({ kind: "screenshot", file, contentType: "image/png" });
}

function publishMedia() {
  const manifest = process.env.PROBIERZ_SPEC_MEDIA_PATH;
  if (!manifest) return;
  mkdirSync(path.dirname(manifest), { recursive: true });
  writeFileSync(manifest, `${JSON.stringify(media, null, 2)}\n`);
}

function keychainValue() {
  const result = spawnSync("security", [
    "find-generic-password", "-w", "-s", keychainService, "-a", provider,
  ], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function deleteKeychainState() {
  for (const [service, account] of [
    [keychainService, provider],
    [keychainRuntimeService, "brama-desktop"],
  ]) {
    spawnSync("security", [
      "delete-generic-password", "-s", service, "-a", account,
    ], { stdio: "ignore" });
  }
}

function routeRegistry() {
  return JSON.parse(readFileSync(routes, "utf8"));
}

mkdirSync(stateRoot, { recursive: true });
deleteKeychainState();
assert.equal(keychainValue(), null, "the isolated provider must start absent from Keychain");

const app = launchCuaProcess({
  executable: BUNDLE_EXECUTABLE,
  env: {
    BRAMA_LOCAL_RUNTIME: "1",
    BRAMA_BASE_URL: runtimeOrigin,
    BRAMA_DESKTOP_STATE_DIR: stateRoot,
    BRAMA_DESKTOP_KEYCHAIN_NAMESPACE: keychainNamespace,
  },
});

try {
  cuaCall("bring_to_front", { pid: app.pid, window_id: app.windowId });
  sleep(1500);

  const shell = waitForWindowText(app.pid, "Subscriptions", 60_000);
  activate(
    app.pid,
    shell.windowId,
    "Subscriptions",
    (element) => String(element.label || "") === "Subscriptions" && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Add local key"),
    30_000,
  );

  let subscriptions = waitForWindowText(app.pid, "Add local key", 30_000);
  activate(
    app.pid,
    subscriptions.windowId,
    "Add local key",
    (element) => String(element.label || "") === "Add local key" && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Add a local provider key"),
  );
  let dialog = waitForWindowText(app.pid, "Add a local provider key", 20_000);
  typeField(app.pid, dialog.windowId, "Provider", provider);
  typeField(app.pid, dialog.windowId, "API key or subscription credential", firstKey);
  activate(
    app.pid,
    dialog.windowId,
    "Add key",
    (element) => String(element.label || "") === "Add key" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes("Add a local provider key"),
    30_000,
  );
  waitUntil(() => keychainValue() === firstKey, "the exact first provider credential in Keychain");
  subscriptions = waitForWindowText(app.pid, provider, 30_000);
  capture(app.pid, subscriptions.windowId, "provider-key-added");

  activate(
    app.pid,
    subscriptions.windowId,
    provider,
    (element) => String(element.label || "").startsWith(provider) && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Replace this provider key"),
  );
  let inspector = waitForWindowText(app.pid, "Replace this provider key", 20_000);
  activate(
    app.pid,
    inspector.windowId,
    "Replace this provider key",
    (element) => String(element.label || "").startsWith("Replace this provider key") && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Replace a local provider key"),
  );
  dialog = waitForWindowText(app.pid, "Replace a local provider key", 20_000);
  typeField(app.pid, dialog.windowId, "API key or subscription credential", replacementKey);
  activate(
    app.pid,
    dialog.windowId,
    "Replace credential",
    (element) => String(element.label || "") === "Replace credential" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes("Replace a local provider key"),
    30_000,
  );
  waitUntil(() => keychainValue() === replacementKey, "the exact replacement provider credential in Keychain");
  subscriptions = waitForWindowText(app.pid, `${provider} is saved`, 30_000);
  capture(app.pid, subscriptions.windowId, "provider-key-replaced");

  activate(
    app.pid,
    subscriptions.windowId,
    "Routing",
    (element) => String(element.label || "") === "Routing" && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Add alias"),
    30_000,
  );
  let routing = waitForWindowText(app.pid, "Add alias", 30_000);
  activate(
    app.pid,
    routing.windowId,
    "Add alias",
    (element) => String(element.label || "") === "Add alias" && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Create an alias"),
  );
  dialog = waitForWindowText(app.pid, "Create an alias", 20_000);
  typeField(app.pid, dialog.windowId, "Alias", alias);
  typeField(app.pid, dialog.windowId, "Primary target", "openai/default");
  activate(
    app.pid,
    dialog.windowId,
    "Create alias",
    (element) => String(element.label || "") === "Create alias" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes("Create an alias"),
    30_000,
  );
  waitUntil(() => routeRegistry().routes?.[alias] === "openai/default", "the added route in routes.json");
  routing = waitForWindowText(app.pid, alias, 30_000);
  capture(app.pid, routing.windowId, "route-alias-added");

  activate(
    app.pid,
    routing.windowId,
    alias,
    (element) => String(element.label || "").startsWith(alias) && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Review change"),
  );
  inspector = waitForWindowText(app.pid, "Review change", 20_000);
  typeField(app.pid, inspector.windowId, "Primary target", "openai/fail");
  activate(
    app.pid,
    inspector.windowId,
    "Review change",
    (element) => String(element.label || "").startsWith("Review change") && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Rewrite this route"),
  );
  dialog = waitForWindowText(app.pid, "Rewrite this route", 20_000);
  activate(
    app.pid,
    dialog.windowId,
    "Rewrite the route",
    (element) => String(element.label || "") === "Rewrite the route" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes("Rewrite this route"),
    30_000,
  );
  waitUntil(() => routeRegistry().routes?.[alias] === "openai/fail", "the replaced route in routes.json");
  routing = waitForWindowText(app.pid, "openai/fail", 30_000);
  capture(app.pid, routing.windowId, "route-alias-replaced");

  activate(
    app.pid,
    routing.windowId,
    "Delete this alias",
    (element) => String(element.label || "").startsWith("Delete this alias") && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Delete this alias?"),
  );
  dialog = waitForWindowText(app.pid, "Delete this alias?", 20_000);
  activate(
    app.pid,
    dialog.windowId,
    "Delete the alias",
    (element) => String(element.label || "") === "Delete the alias" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes("Delete this alias?"),
    30_000,
  );
  waitUntil(() => !(alias in (routeRegistry().routes || {})), "the alias deletion in routes.json");
  routing = waitForWindowText(app.pid, "Add alias", 30_000);
  assert.ok(!routing.tree.includes(alias), "the deleted alias must be absent from the UI");
  capture(app.pid, routing.windowId, "route-alias-deleted");

  activate(
    app.pid,
    routing.windowId,
    "Subscriptions after route deletion",
    (element) => String(element.label || "") === "Subscriptions" && String(element.role || "").includes("Button"),
    (tree) => tree.includes(provider),
    30_000,
  );
  subscriptions = waitForWindowText(app.pid, provider, 30_000);
  activate(
    app.pid,
    subscriptions.windowId,
    "saved provider",
    (element) => String(element.label || "").startsWith(provider) && String(element.role || "").includes("Button"),
    (tree) => tree.includes("Remove this provider key"),
  );
  inspector = waitForWindowText(app.pid, "Remove this provider key", 20_000);
  activate(
    app.pid,
    inspector.windowId,
    "Remove this provider key",
    (element) => String(element.label || "").startsWith("Remove this provider key") && String(element.role || "").includes("Button"),
    (tree) => tree.includes(`Remove the ${provider} API key?`),
  );
  dialog = waitForWindowText(app.pid, `Remove the ${provider} API key?`, 20_000);
  activate(
    app.pid,
    dialog.windowId,
    "Remove it",
    (element) => String(element.label || "") === "Remove it" && String(element.role || "").includes("Button"),
    (tree) => !tree.includes(`Remove the ${provider} API key?`),
    30_000,
  );
  waitUntil(() => keychainValue() === null, "provider deletion from Keychain");
  subscriptions = waitForWindowText(app.pid, "Add local key", 30_000);
  assert.ok(!subscriptions.tree.includes(provider), "the deleted provider must be absent from the UI");
  capture(app.pid, subscriptions.windowId, "provider-key-deleted");
} finally {
  publishMedia();
  quitApp(app.pid);
  deleteKeychainState();
  rmSync(stateRoot, { recursive: true, force: true });
}
