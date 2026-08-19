// Journey: capabilities-screen (skarbiec-desktop, desktop:cua)
//
// The Capabilities screen is where an operator learns *why* a consumer was
// refused: a route whose item is absent and a route whose field is absent are
// different failures, and the screen must name both beside the routes that do
// resolve. Its one write - adding a route - must refuse without a typed reason.
//
// Everything is read from an isolated vault and its own capability-routes
// table under ~/Library/Caches/probierz-vg-journeys/, never the operator's
// vault and never a gateway host: `routes add` is never allowed to complete
// here, and the table's bytes are fingerprinted before and after the refusal.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import {
  cuaCall,
  launchCuaProcess,
  quitApp,
  snapshotState,
  snapshotTree,
} from "../driver.mjs";

const SKARBIEC_CLI = process.env.PROBIERZ_SKARBIEC_CLI
  || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec";
const EXECUTABLE = process.env.CUA_APP_EXECUTABLE
  || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec-desktop/.build/Skarbiec.app/Contents/MacOS/Skarbiec";
// Never /tmp: a sweeper on this host deleted another run's live fixtures out of it.
const SCRATCH = path.join(homedir(), "Library", "Caches", "probierz-vg-journeys", "skarbiec-capabilities");
const VAULT = path.join(SCRATCH, "skarbiec.vault");
const AUDIT = path.join(SCRATCH, "audit.log");
const ROUTES_TABLE = path.join(SCRATCH, "capability-routes.json");
const ROUTES_AUDIT = path.join(SCRATCH, "capability-routes.audit.jsonl");
const CLI_PATH = `/opt/homebrew/bin:${process.env.PATH || "/usr/bin:/bin"}`;

// The three resolutions the screen exists to tell apart, as CapabilityRoutes.swift
// derives them from item_present/field_present.
const FIXTURE_ROUTES = [
  { resource: "https://login.example.com", item: "example-login", field: "password", resolution: "Resolves" },
  { resource: "https://sso.example.com", item: "example-login", field: "totp", resolution: "Field missing" },
  { resource: "https://absent.example.com", item: "missing-login", field: "password", resolution: "Item unreadable" },
];

const artifactsDir = path.resolve(process.env.PROBIERZ_ARTIFACTS || "test-results");
const specName = process.env.PROBIERZ_SPEC_NAME || "skarbiec-desktop-capabilities-screen";
const media = [];

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function cli(args) {
  return execFileSync(SKARBIEC_CLI, args, {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: CLI_PATH,
      SKARBIEC_VAULT_FILE: VAULT,
      SKARBIEC_AUDIT_FILE: AUDIT,
    },
  });
}

// `skarbiec` prints its refusals on stdout as `Error: …` and still exits 0, so
// a fixture step that silently failed would otherwise surface as a UI assertion
// four minutes later.
function cliJSON(args) {
  const raw = cli(args);
  try {
    return JSON.parse(raw);
  } catch {
    throw new Error(`skarbiec ${args.join(" ")} did not answer with JSON: ${raw.trim().slice(0, 300)}`);
  }
}

function buildFixture() {
  assert.ok(existsSync(SKARBIEC_CLI), `the skarbiec release CLI is required at ${SKARBIEC_CLI}`);
  mkdirSync(SCRATCH, { recursive: true });
  // The table is rebuilt every run; the vault (and the gpg identity `init`
  // generates for it) is kept so repeated runs do not accumulate host keys.
  for (const name of readdirSync(SCRATCH)) {
    if (name.startsWith("capability-routes")) rmSync(path.join(SCRATCH, name), { force: true });
  }
  if (!existsSync(VAULT)) cliJSON(["init", "probierz-capabilities-fixture", "--json"]);
  cliJSON([
    "set", "example-login",
    "username=agent@example.com",
    "password=synthetic-fixture-value",
    "--json",
  ]);
  for (const route of FIXTURE_ROUTES) {
    cliJSON([
      "routes", "add",
      "--resource", route.resource,
      "--item", route.item,
      "--field", route.field,
      "--reason", "probierz capabilities-screen fixture",
      "--json",
    ]);
  }
  const listed = cliJSON(["routes", "list", "--json"]);
  const byResource = new Map(listed.routes.map((row) => [row.resource, row]));
  for (const route of FIXTURE_ROUTES) {
    const row = byResource.get(route.resource);
    assert.ok(row, `fixture route ${route.resource} should be in the table the app will read`);
    assert.equal(row.item, route.item);
    assert.equal(row.field, route.field);
    assert.equal(row.item_present, route.resolution !== "Item unreadable");
    assert.equal(row.field_present, route.resolution === "Resolves");
  }
  assert.equal(listed.routes.length, FIXTURE_ROUTES.length);
}

// Every byte the mutating action could touch: the table, its audit journal, and
// the backup series `routes add` publishes beside them.
function tableFingerprint() {
  return readdirSync(SCRATCH)
    .filter((name) => name.startsWith("capability-routes"))
    .sort()
    .map((name) => {
      const digest = createHash("sha256").update(readFileSync(path.join(SCRATCH, name))).digest("hex");
      return `${name}:${digest}`;
    })
    .join("\n");
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

// Write into one named field and read the value back off the element, because
// a background AX insert that did not land looks exactly like one that did.
function typeInto(pid, windowId, label, value) {
  const named = (element) => String(element.role || "").includes("TextField")
    && String(element.label || "") === label;
  let last = "";
  for (const mode of ["background", "foreground"]) {
    const state = snapshotState(pid, windowId) || {};
    const field = elementsOf(state).find(named);
    if (!field) {
      dumpTree(`type-${label}-missing`, String(state.tree_markdown || ""));
      throw new Error(`no ${label} field to type into`);
    }
    cuaCall("type_text", {
      pid,
      text: value,
      delivery_mode: mode,
      ...(field.element_token
        ? { element_token: field.element_token }
        : {
          window_id: windowId,
          element_index: field.element_index,
          ...(state.snapshot_id ? { snapshot_id: state.snapshot_id } : {}),
        }),
    });
    sleep(600);
    const written = elementsOf(snapshotState(pid, windowId) || {}).find(named);
    last = String(written?.value || "");
    if (last.includes(value)) return;
  }
  throw new Error(`typing ${JSON.stringify(value)} into ${label} did not land; the field holds ${JSON.stringify(last)}`);
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

buildFixture();
const fingerprintBefore = tableFingerprint();
const routesBefore = readFileSync(ROUTES_TABLE, "utf8");
const routesAuditBefore = existsSync(ROUTES_AUDIT) ? readFileSync(ROUTES_AUDIT, "utf8") : "";
const backupsBefore = new Set(readdirSync(SCRATCH).filter((name) => name.includes("json.before-")));

const app = launchCuaProcess({
  executable: EXECUTABLE,
  env: {
    SKARBIEC_CLI,
    SKARBIEC_VAULT_FILE: VAULT,
    SKARBIEC_AUDIT_FILE: AUDIT,
    PATH: CLI_PATH,
  },
});

try {
  // A directly spawned bundle stays in the background, where its SwiftUI scene
  // never composes its content; the window has to be the foreground one before
  // anything is on it.
  cuaCall("bring_to_front", { pid: app.pid, window_id: app.windowId });
  sleep(1500);
  const shell = waitForWindowText(app.pid, "AXButton (Capabilities)", 60_000);

  activate(
    app.pid,
    shell.windowId,
    "the Capabilities destination",
    (element) => String(element.label || "") === "Capabilities" && String(element.role || "").includes("Button"),
    (tree) => tree.includes("AXButton (Read routes)"),
    15_000,
  );

  // The screen has finished reading when its routes are on it.
  const loaded = waitForWindowText(app.pid, `AXStaticText = "${FIXTURE_ROUTES[0].resource}"`, 90_000);
  const tree = loaded.tree;
  dumpTree("capabilities-loaded", tree);
  capture(app.pid, loaded.windowId, "capabilities-loaded");

  const texts = staticTexts(tree);
  assert.ok(texts.has("Capabilities"), "the screen should render its title");
  assert.ok(tree.includes("(every consumer)"), "the screen should say which consumer it read routes for");
  assert.match(tree, /AXButton \(Read routes\)/, "the screen should render its read control");

  // Loaded: read, not erroring, not spinning.
  assert.ok(
    texts.has(`${FIXTURE_ROUTES.length} routes, 2 unresolved`),
    "the context bar should count the table and the routes that do not resolve",
  );
  assert.doesNotMatch(
    tree,
    /AXStaticText = "Reading capability routes"/,
    "the screen should not still be loading once the routes are on it",
  );
  assert.doesNotMatch(tree, /AXStaticText = "No capability routes"/, "the fixture table is not empty");
  assert.doesNotMatch(tree, /AXStaticText = "not read"/, "the screen should have read the table");
  assert.doesNotMatch(
    tree,
    /Verifying routes against the vault/,
    "no verification should be in flight",
  );

  // Every route, with the item and field it names.
  for (const column of ["RESOURCE", "ITEM", "FIELD", "RESOLVES"]) {
    assert.ok(tree.includes(`AXButton "${column}"`), `the table should render the ${column} column`);
  }
  for (const route of FIXTURE_ROUTES) {
    assert.ok(texts.has(route.resource), `the table should render the route for ${route.resource}`);
    assert.ok(texts.has(route.item), `the table should render the item ${route.item} for ${route.resource}`);
    assert.ok(texts.has(route.field), `the table should render the field ${route.field} for ${route.resource}`);
    assert.ok(
      tree.includes(`(${route.resolution})`),
      `the table should resolve ${route.resource} as ${route.resolution}`,
    );
  }

  // Which routes the vault cannot resolve, each named as the incident it is.
  assert.ok(
    tree.includes("One route names a field its item does not carry"),
    "a route whose item lacks the named field should be called out",
  );
  assert.ok(
    tree.includes("One route names an item this host cannot read"),
    "a route whose item this host cannot read should be called out separately",
  );

  // Unresolved routes are ordered above healthy ones: the row the operator
  // opened this screen for is not buried under whatever sorts above it by name.
  const positionOf = (resource) => tree.indexOf(`AXStaticText = "${resource}"`);
  assert.ok(
    positionOf("https://sso.example.com") < positionOf("https://absent.example.com"),
    "the field-missing route should sort above the unreadable-item route",
  );
  assert.ok(
    positionOf("https://absent.example.com") < positionOf("https://login.example.com"),
    "unresolved routes should sort above the route that resolves",
  );

  // The mutating action refuses without a typed reason.
  activate(
    app.pid,
    loaded.windowId,
    "the Add route action",
    (element) => String(element.label || "") === "Add route" && String(element.role || "").includes("Button"),
    (after) => after.includes('AXStaticText = "Add a route"'),
    15_000,
  );
  const drawer = { windowId: loaded.windowId };

  // Resource, Item and Field, in the order the drawer declares them. Reason is
  // deliberately left empty.
  typeInto(app.pid, drawer.windowId, "Resource", "https://probierz.example.com");
  typeInto(app.pid, drawer.windowId, "Item", "example-login");
  typeInto(app.pid, drawer.windowId, "Field", "password");

  const filled = snapshotTree(app.pid, drawer.windowId);
  dumpTree("add-route-empty-reason", filled);
  assert.match(
    filled,
    /AXStaticText = "A reason is required and is recorded with the change\."/,
    "an empty reason should be refused in the drawer's own words",
  );
  // The command the drawer would run carries an empty reason, so it is not one
  // the backend would accept either.
  assert.match(filled, /--reason '' --json/, "the previewed command should show the empty reason it would carry");
  capture(app.pid, drawer.windowId, "add-route-refused");

  // Invoke it anyway. The drawer's own Add route button is the second control
  // of that name on the screen, and while the reason gate is unmet it is
  // published with no element index and no action at all - the press cannot
  // even be addressed, which is the refusal. If it can be addressed, it is
  // pressed, and the table must still be untouched afterwards.
  const drawerState = snapshotState(app.pid, drawer.windowId) || {};
  const named = elementsOf(drawerState).filter((element) =>
    String(element.label || "") === "Add route" && String(element.role || "").includes("Button"));
  const rendered = filled.split("AXButton (Add route)").length - 1;
  assert.ok(rendered >= 2, "the drawer should render its own Add route button beside the action bar's");
  let pressRefusal = named.length < rendered
    ? "the drawer's Add route button exposes no press action while the reason is empty"
    : null;
  if (pressRefusal === null) {
    const submit = named.reduce((lower, element) =>
      (element.frame?.y || 0) > (lower.frame?.y || 0) ? element : lower);
    try {
      cuaCall("click", {
        pid: app.pid,
        ...(submit.element_token
          ? { element_token: submit.element_token }
          : {
            window_id: drawer.windowId,
            element_index: submit.element_index,
            ...(drawerState.snapshot_id ? { snapshot_id: drawerState.snapshot_id } : {}),
          }),
      });
    } catch (error) {
      pressRefusal = error instanceof Error ? error.message : String(error);
    }
  }
  sleep(2500);

  const afterPress = snapshotTree(app.pid, drawer.windowId);
  dumpTree("add-route-after-press", afterPress);
  assert.match(
    afterPress,
    /AXStaticText = "A reason is required and is recorded with the change\."/,
    `the drawer should still refuse after the action was invoked${pressRefusal ? ` (press refused: ${pressRefusal})` : ""}`,
  );
  assert.doesNotMatch(afterPress, /Route added/, "no route may be written without a reason");

  // Nothing was written: same bytes, same audit journal, no new backup.
  assert.equal(tableFingerprint(), fingerprintBefore, "the capability routes table must be untouched");
  assert.equal(readFileSync(ROUTES_TABLE, "utf8"), routesBefore, "the routes table content must be unchanged");
  assert.equal(
    existsSync(ROUTES_AUDIT) ? readFileSync(ROUTES_AUDIT, "utf8") : "",
    routesAuditBefore,
    "no audit line may be written for a refused route",
  );
  const backupsAfter = readdirSync(SCRATCH).filter((name) => name.includes("json.before-"));
  assert.deepEqual(
    backupsAfter.filter((name) => !backupsBefore.has(name)),
    [],
    "a refused add must not publish a new table backup",
  );
} finally {
  publishMedia();
  quitApp(app.pid);
}
