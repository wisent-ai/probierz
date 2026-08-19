// Journey: subscription-pool-screen (brama-desktop, desktop:cua)
//
// Every `best`-aliased model call failed for hours because both pooled
// credentials were burnt and no surface reported pool state. This screen is
// that surface: it must render each pooled subscription's provider, identity,
// redemption state, expiry and the provider's own refusal - and never a
// credential value or capability identifier. Its one write, refreshing a
// provider's pool, must refuse without a typed reason.
//
// The read is the product's own path: the bundled `brama-runtime` answering
// `subscriptions list --json` over a synthetic usage ledger in
// ~/Library/Caches/probierz-vg-journeys/, with the entitlements router stubbed
// out so the deployment's real subscriptions are not enumerated. The gateway
// rollout is production, so `BRAMA_BIN` is a wrapper that forwards the read to
// the real binary and refuses `subscription refresh` outright, and records
// every argv it was handed - which is how "the refresh did not proceed" is a
// fact here rather than a hope.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import {
  cuaCall,
  launchCuaProcess,
  quitApp,
  snapshotState,
  snapshotTree,
} from "../driver.mjs";

const BUNDLE_EXECUTABLE = process.env.CUA_APP_EXECUTABLE
  || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/brama-desktop/.build/Brama.app/Contents/MacOS/Brama";
// The gateway CLI Brama Desktop ships inside its own bundle.
const REAL_CLI = path.join(path.dirname(BUNDLE_EXECUTABLE), "brama-runtime");
// Never /tmp: a sweeper on this host deleted another run's live fixtures out of it.
const SCRATCH = path.join(homedir(), "Library", "Caches", "probierz-vg-journeys", "brama-subscription-pool");
const WRAPPER_CLI = path.join(SCRATCH, "brama");
const LEDGER = path.join(SCRATCH, "subscription-usage.json");
const ROUTER_STUB = path.join(SCRATCH, "entitlements-router");
const INVOCATIONS = path.join(SCRATCH, "invocations.log");

const DAY_MS = 24 * 60 * 60 * 1000;
const now = Date.now();

// One pooled subscription in each state `brama subscriptions list` publishes,
// as subscription_dispatch/pool.rs derives them from this ledger.
const EXPECTED = [
  { id: "sub-anthropic-7f21", provider: "anthropic", state: "Live", error: null, expiry: "dated" },
  {
    id: "sub-google-93bd",
    provider: "google",
    state: "Expired",
    error: "the pooled grant expired before the last dispatch",
    expiry: "dated",
  },
  { id: "sub-mistral-4a88", provider: "mistral", state: "Unknown", error: null, expiry: "none" },
  {
    id: "sub-openai-1c04",
    provider: "openai",
    state: "Burnt",
    error: "the provider refused the stored grant: seat revoked",
    expiry: "none",
  },
];

// A key nothing in the product decodes. If it reaches the window, the screen is
// rendering material out of the ledger it was never asked to show.
const DECOY = "sk-probierz-NEVERRENDER";
const SECRET_PATTERNS = [
  { name: "the ledger decoy", pattern: /NEVERRENDER/ },
  { name: "an API-key-shaped string", pattern: /\bsk-[A-Za-z0-9_-]{6,}/ },
  { name: "a bearer token", pattern: /Bearer\s+[A-Za-z0-9._-]{8,}/ },
  { name: "a JWT", pattern: /\bey[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/ },
];

const REFRESH_PROVIDER = "openai";

const artifactsDir = path.resolve(process.env.PROBIERZ_ARTIFACTS || "test-results");
const specName = process.env.PROBIERZ_SPEC_NAME || "brama-desktop-subscription-pool-screen";
const media = [];

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function buildFixture() {
  assert.ok(existsSync(REAL_CLI), `the bundled brama runtime is required at ${REAL_CLI}`);
  rmSync(SCRATCH, { recursive: true, force: true });
  mkdirSync(SCRATCH, { recursive: true });

  writeFileSync(LEDGER, `${JSON.stringify({
    subscriptions: {
      "sub-anthropic-7f21": {
        provider: "anthropic",
        credential: { state: "active", recorded_at_ms: now - 3_600_000, expires_at_ms: now + 120 * DAY_MS },
        credential_value: `${DECOY}-anthropic`,
      },
      "sub-openai-1c04": {
        provider: "openai",
        credential: {
          state: "needs_reauthorization",
          cause: "the provider refused the stored grant: seat revoked",
          recorded_at_ms: now - 3_600_000,
        },
        credential_value: `${DECOY}-openai`,
      },
      "sub-google-93bd": {
        provider: "google",
        credential: { state: "active", recorded_at_ms: now - 3_600_000, expires_at_ms: now - 9 * DAY_MS },
        probe: {
          attempted_at_ms: now - 3_600_000,
          ok: false,
          detail: "the pooled grant expired before the last dispatch",
        },
        credential_value: `${DECOY}-google`,
      },
      "sub-mistral-4a88": { provider: "mistral", credential_value: `${DECOY}-mistral` },
    },
  }, null, 2)}\n`);

  // No broker on this host for this journey: the pool is exactly the ledger,
  // and no real subscription of the deployment is enumerated into the evidence.
  writeFileSync(ROUTER_STUB, "#!/bin/sh\nexit 1\n", { mode: 0o755 });

  writeFileSync(
    WRAPPER_CLI,
    [
      "#!/bin/sh",
      `printf '%s\\n' "$*" >> ${JSON.stringify(INVOCATIONS)}`,
      'if [ "$1" = "subscription" ] && [ "$2" = "refresh" ]; then',
      "  printf 'error: this probierz journey never refreshes a pooled subscription\\n' >&2",
      "  exit 3",
      "fi",
      `BRAMA_SUBSCRIPTION_USAGE_FILE=${JSON.stringify(LEDGER)} \\`,
      `ENTITLEMENTS_ROUTER_BIN=${JSON.stringify(ROUTER_STUB)} \\`,
      `exec ${JSON.stringify(REAL_CLI)} "$@"`,
      "",
    ].join("\n"),
    { mode: 0o755 },
  );
  writeFileSync(INVOCATIONS, "");
}

function invocations() {
  return readFileSync(INVOCATIONS, "utf8").split("\n").map((line) => line.trim()).filter(Boolean);
}

function windowsOf(pid) {
  const listed = cuaCall("list_windows") || {};
  return (listed.windows || []).filter((win) => win.pid === pid && win.layer === 0);
}

// Sheets are their own windows, so a needle is looked for across every window
// the app owns rather than only the one it launched with.
function waitForWindowText(pid, needle, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < deadline) {
    for (const win of windowsOf(pid)) {
      const tree = snapshotTree(pid, win.window_id);
      if (tree.includes(needle)) return { windowId: win.window_id, tree };
      if (tree.length > last.length) last = tree;
    }
    sleep(300);
  }
  const dumped = dumpTree(`timeout-${needle.replace(/[^A-Za-z0-9]+/g, "-").slice(0, 60)}`, last);
  throw new Error(
    `timed out waiting for ${JSON.stringify(needle)}; widest tree written to ${dumped}; tail: ${last.slice(-1200)}`,
  );
}

function elementsOf(state) {
  return state?.elements || state?.structuredContent?.elements || [];
}

// Press a control and confirm the app took it.
//
// cua-driver's own ladder: the accessibility action first, then a pixel click
// at the element's own frame, then the same click with the window fronted. A
// SwiftUI button that ignores AXPress still takes a real mouse event, and a
// press that changed nothing is not a press.
function activate(pid, windowId, label, matches, settled, timeoutMs = 12_000) {
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
  for (const [rung, press] of rungs.entries()) {
    const state = snapshotState(pid, windowId) || {};
    const element = elementsOf(state).find(matches);
    if (!element) {
      dumpTree(`activate-${label}-missing`, String(state.tree_markdown || ""));
      throw new Error(`no element to press for ${label}`);
    }
    if (rung > 0 && !element.frame) throw new Error(`element for ${label} carries no frame to click`);
    try {
      press(element, state);
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
    }
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      sleep(400);
      const tree = snapshotTree(pid, windowId);
      if (settled(tree)) return tree;
    }
    dumpTree(`activate-${label}-rung-${rung + 1}`, snapshotTree(pid, windowId));
  }
  throw new Error(`pressing ${label} never settled${lastError ? ` (last press error: ${lastError})` : ""}`);
}

function capture(pid, windowId, name) {
  const file = path.join(artifactsDir, `${specName}-${name}.png`);
  mkdirSync(path.dirname(file), { recursive: true });
  snapshotState(pid, windowId, { screenshotOutFile: file });
  if (!existsSync(file)) throw new Error(`cua-driver produced no screenshot at ${file}`);
  media.push({ kind: "screenshot", file, contentType: "image/png" });
  return file;
}

function publishMedia() {
  const manifest = process.env.PROBIERZ_SPEC_MEDIA_PATH;
  if (!manifest) return;
  mkdirSync(path.dirname(manifest), { recursive: true });
  writeFileSync(manifest, `${JSON.stringify(media, null, 2)}\n`);
}

// The tree is the evidence for anything that did not render, so it is kept
// beside the screenshots instead of only tailed into the row error.
function dumpTree(name, tree) {
  const file = path.join(artifactsDir, `${specName}-${name}.tree.txt`);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, tree);
  return file;
}

function staticTexts(tree) {
  return new Set([...tree.matchAll(/AXStaticText = "([^"]*)"/g)].map((match) => match[1]));
}

function assertNoSecretMaterial(tree, where) {
  for (const { name, pattern } of SECRET_PATTERNS) {
    assert.doesNotMatch(tree, pattern, `${where} must render no credential material (${name})`);
  }
}

buildFixture();

const app = launchCuaProcess({
  executable: BUNDLE_EXECUTABLE,
  env: {
    BRAMA_BIN: WRAPPER_CLI,
    BRAMA_SUBSCRIPTION_USAGE_FILE: LEDGER,
    ENTITLEMENTS_ROUTER_BIN: ROUTER_STUB,
  },
});

try {
  // A directly spawned bundle stays in the background, where its SwiftUI scene
  // never composes its content; the window has to be the foreground one before
  // anything is on it.
  cuaCall("bring_to_front", { pid: app.pid, window_id: app.windowId });
  sleep(1500);
  const shell = waitForWindowText(app.pid, "Subscription Pool", 60_000);
  activate(
    app.pid,
    shell.windowId,
    "the Subscription Pool destination",
    (element) => String(element.label || "") === "Subscription Pool"
      && String(element.role || "").includes("Button"),
    (after) => after.includes('AXStaticText = "LAST REDEEM ERROR"'),
    20_000,
  );

  const loaded = waitForWindowText(app.pid, 'AXStaticText = "LAST REDEEM ERROR"', 60_000);
  const tree = loaded.tree;
  dumpTree("pool-loaded", tree);
  capture(app.pid, loaded.windowId, "pool-loaded");

  const texts = staticTexts(tree);

  // Loaded: read through the CLI, not erroring, not spinning.
  assert.ok(texts.has("Subscription Pool"), "the screen should render its title");
  assert.ok(texts.has("brama CLI"), "the screen should scope itself to the CLI it read");
  assert.doesNotMatch(
    tree,
    /AXStaticText = "The subscription pool could not be read"/,
    "the pool read should not have failed",
  );
  assert.doesNotMatch(
    tree,
    /AXStaticText = "Reading the subscription pool"/,
    "the screen should not still be reading once the pool is on it",
  );
  assert.doesNotMatch(tree, /AXStaticText = "Not read yet"/, "the screen should report when it read the pool");
  assert.doesNotMatch(tree, /AXStaticText = "Reading…"/, "no read should still be in flight");
  assert.doesNotMatch(tree, /AXStaticText = "The pool holds no subscription"/, "the ledger pool is not empty");
  assert.doesNotMatch(
    tree,
    /AXStaticText = "No subscription in the pool is live"/,
    "one ledger subscription is live",
  );
  assert.ok([...texts].some((value) => value.startsWith("read ")), "the screen should show the read's freshness");

  // Every contracted column, and every row's contracted fields. A row is one
  // combined button: provider, subscription, state, expiry, the provider's own
  // refusal - in that order.
  for (const column of ["PROVIDER", "SUBSCRIPTION", "STATE", "EXPIRES", "LAST REDEEM ERROR"]) {
    assert.ok(texts.has(column), `the table should render the ${column} column`);
  }
  for (const entry of EXPECTED) {
    const row = `(${entry.provider}, ${entry.id}, ${entry.state}, `;
    assert.ok(tree.includes(row), `the table should render ${entry.provider} as ${entry.state} with its identity`);
    if (entry.error) {
      assert.ok(
        tree.includes(`${row}`) && tree.includes(entry.error),
        `the table should render the provider's own refusal for ${entry.provider}`,
      );
    }
  }
  assert.ok(
    tree.includes("No expiry recorded"),
    "a pooled subscription whose credential states no expiry should say so",
  );

  // The pool's own arithmetic over those rows.
  for (const [signal, count] of [["Live", 1], ["Burnt", 1], ["Expired", 1], ["Unknown", 1]]) {
    assert.ok(texts.has(`${signal}: ${count}`), `the pool should count ${count} ${signal.toLowerCase()} subscription`);
  }

  // Unusable rows first: the rows that stop work are the rows an operator came for.
  const positionOf = (provider) => tree.indexOf(`(${provider}, `);
  for (const unusable of ["google", "mistral", "openai"]) {
    assert.ok(
      positionOf(unusable) < positionOf("anthropic"),
      `the unusable ${unusable} row should sort above the live one`,
    );
  }

  assertNoSecretMaterial(tree, "the loaded pool");

  // The inspector joins one row's state to its identity and its expiry.
  activate(
    app.pid,
    loaded.windowId,
    `the ${REFRESH_PROVIDER} row`,
    (element) => String(element.label || "").startsWith(`${REFRESH_PROVIDER}, `)
      && String(element.role || "").includes("Button"),
    (after) => after.includes("POOLED SUBSCRIPTION"),
    15_000,
  );
  const inspector = waitForWindowText(app.pid, "POOLED SUBSCRIPTION", 30_000);
  dumpTree("pool-inspector", inspector.tree);
  for (const label of ["IDENTITY", "PROVIDER", "SUBSCRIPTION ID", "STATE", "EXPIRY", "REFRESH"]) {
    assert.ok(inspector.tree.includes(label), `the inspector should render its ${label} field`);
  }
  assert.ok(
    inspector.tree.includes("sub-openai-1c04"),
    "the inspector should name the pooled subscription it is describing",
  );
  assert.ok(
    inspector.tree.includes("Never available here"),
    "the inspector should state what this screen never shows",
  );
  assert.ok(
    inspector.tree.includes("Reading the credential value behind a pooled subscription"),
    "the inspector should name the credential value as unavailable",
  );
  assertNoSecretMaterial(inspector.tree, "the inspector");

  // The mutating action refuses without a typed reason.
  const invocationsBeforeRefresh = invocations();
  activate(
    app.pid,
    inspector.windowId,
    `the Refresh ${REFRESH_PROVIDER} action`,
    (element) => String(element.label || "").startsWith(`Refresh ${REFRESH_PROVIDER}`)
      && String(element.role || "").includes("Button"),
    (after) => after.includes(`Refresh the ${REFRESH_PROVIDER} subscription pool?`),
    20_000,
  );
  const dialog = waitForWindowText(
    app.pid,
    `AXStaticText = "Refresh the ${REFRESH_PROVIDER} subscription pool?"`,
    30_000,
  );
  dumpTree("refresh-empty-reason", dialog.tree);
  assert.match(
    dialog.tree,
    /AXStaticText = "A reason is required\. The command refuses without one\."/,
    "an empty reason should be refused in the dialog's own words",
  );
  assert.ok(
    dialog.tree.includes(`brama subscription refresh ${REFRESH_PROVIDER} --reason '' --json`),
    "the previewed command should show the empty reason it would carry",
  );
  capture(app.pid, dialog.windowId, "refresh-refused");

  // Invoke it anyway. While the reason gate is unmet the confirm button is
  // published with no element index and no action at all - the press cannot
  // even be addressed, which is the refusal. If it can be addressed, it is
  // pressed, and the gateway must still never be asked.
  const dialogState = snapshotState(app.pid, dialog.windowId) || {};
  const confirms = elementsOf(dialogState).filter((element) =>
    String(element.label || "") === "Refresh it" && String(element.role || "").includes("Button"));
  assert.ok(
    dialog.tree.includes("AXButton (Refresh it)"),
    "the dialog should render the confirm button it is refusing to run",
  );
  let pressRefusal = confirms.length === 0
    ? "the Refresh it button exposes no press action while the reason is empty"
    : null;
  if (pressRefusal === null) {
    try {
      cuaCall("click", {
        pid: app.pid,
        ...(confirms[0].element_token
          ? { element_token: confirms[0].element_token }
          : {
            window_id: dialog.windowId,
            element_index: confirms[0].element_index,
            ...(dialogState.snapshot_id ? { snapshot_id: dialogState.snapshot_id } : {}),
          }),
      });
    } catch (error) {
      pressRefusal = error instanceof Error ? error.message : String(error);
    }
  }
  sleep(2500);

  const afterPress = snapshotTree(app.pid, dialog.windowId);
  dumpTree("refresh-after-press", afterPress);
  assert.match(
    afterPress,
    /AXStaticText = "A reason is required\. The command refuses without one\."/,
    `the dialog should still refuse after the action was invoked${pressRefusal ? ` (press refused: ${pressRefusal})` : ""}`,
  );

  // Nothing was dispatched: the CLI was never asked to refresh anything.
  const invocationsAfter = invocations();
  assert.deepEqual(invocationsAfter, invocationsBeforeRefresh, "a refused refresh must not invoke the Brama CLI");
  assert.ok(
    !invocationsAfter.some((line) => line.startsWith("subscription refresh")),
    `the CLI must never be asked to refresh a pool here: ${invocationsAfter.join(" | ")}`,
  );
  assert.ok(
    invocationsAfter.some((line) => line.startsWith("subscriptions list")),
    "the pool on screen must come from a real CLI read",
  );

  const shellAfter = snapshotTree(app.pid, loaded.windowId);
  assert.doesNotMatch(
    shellAfter,
    new RegExp(`AXStaticText = "Refreshing ${REFRESH_PROVIDER} credentials\\.`),
    "no refresh may be in flight",
  );
} finally {
  publishMedia();
  quitApp(app.pid);
}
