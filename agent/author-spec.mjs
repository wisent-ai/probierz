// Autonomous journey spec authoring: probe the real app, draft a candidate
// spec with a headless model, verify it with an actual probierz run, and
// iterate with failure feedback until the run is green (or rounds run out).
// A green spec is installed in the product's primary repository and exposed to
// the toolkit through a relative registration symlink. Every candidate must
// drive the real product: no mocks, no fake selectors, no stubbing the app.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import { loadAppManifest } from "./apps.mjs";
import { runHistory } from "./history.mjs";
import { draftStructuredArtifact } from "./model-router.mjs";
import { appSourceIdentity } from "./runner.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const CLI = path.join(HERE, "cli.mjs");
const PROBE_CHARS = Number("9000");
const BODY_CHARS = Number("1500");

const TARGET_SPEC_DIRS = {
  web: path.join(ROOT, "packages", "web", "tests"),
  electron: path.join(ROOT, "packages", "electron", "tests"),
  "mobile:ios": path.join(ROOT, "packages", "mobile", "test", "specs"),
  "mobile:android": path.join(ROOT, "packages", "mobile", "test", "specs"),
  "desktop:mac": path.join(ROOT, "packages", "desktop-native", "test", "specs"),
  "desktop:win": path.join(ROOT, "packages", "desktop-native", "test", "specs"),
  "desktop:cua": path.join(ROOT, "packages", "desktop-cua", "specs"),
  tui: path.join(ROOT, "packages", "tui", "specs"),
};

function specExtension(target) {
  if (target === "web" || target === "electron") return ".spec.ts";
  if (target === "tui" || target === "desktop:cua") return ".spec.mjs";
  return ".e2e.ts";
}

function productSpecExtension(target) {
  return target === "tui" || target === "desktop:cua" ? "mjs" : "ts";
}

function safeName(value, label) {
  const clean = String(value || "").trim();
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(clean)) {
    throw new Error(`${label} must be one safe path name: ${value}`);
  }
  return clean;
}

function inside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
}

function assertPhysicalProductPath(repositoryRoot, testsRoot, productSpec) {
  const destination = lstatSync(productSpec, { throwIfNoEntry: false });
  if (destination && !destination.isFile()) {
    throw new Error("authored spec destination must be a regular product file");
  }
  const parent = path.dirname(productSpec);
  let existingParent = parent;
  while (!existsSync(existingParent)) existingParent = path.dirname(existingParent);
  const physicalProductRoot = realpathSync(repositoryRoot);
  const physicalParent = realpathSync(existingParent);
  if (physicalParent !== physicalProductRoot && !inside(physicalProductRoot, physicalParent)) {
    throw new Error("authored spec path escapes the selected product tests directory");
  }
  if (!existsSync(testsRoot) || !existsSync(parent)) return;
  const physicalTests = realpathSync(testsRoot);
  const physicalProduct = path.join(realpathSync(parent), path.basename(productSpec));
  if (!inside(physicalProductRoot, physicalTests) || !inside(physicalTests, physicalProduct)) {
    throw new Error("authored spec path escapes the selected product tests directory");
  }
}

function authoredPaths({ manifest, journey, target, area = null, productRoot = null }) {
  const selectedJourney = safeName(journey, "journey");
  const selectedArea = safeName(area || selectedJourney, "authoring area");
  const repositoryRoot = path.resolve(productRoot || manifest.repositories[Number("0")].root);
  const testsRoot = path.resolve(repositoryRoot, manifest.surfaces[target]?.testDirectory || "tests");
  const productSpec = path.resolve(
    testsRoot,
    selectedArea,
    `${selectedJourney}.probierz.spec.${productSpecExtension(target)}`,
  );
  if (!inside(repositoryRoot, testsRoot) || !inside(testsRoot, productSpec)) {
    throw new Error("authored spec path escapes the selected product tests directory");
  }
  assertPhysicalProductPath(repositoryRoot, testsRoot, productSpec);
  const registration = path.join(
    TARGET_SPEC_DIRS[target],
    `${manifest.appId}-${selectedJourney}${specExtension(target)}`,
  );
  return {
    area: selectedArea,
    journey: selectedJourney,
    productRoot: repositoryRoot,
    testsRoot,
    productSpec,
    registration,
  };
}

function authoringArtifactRoot(appId, journey) {
  const parent = path.join(ROOT, "test-results", ".authoring", safeName(appId, "app ID"));
  mkdirSync(parent, { recursive: true, mode: 0o700 });
  const requested = process.env.PROBIERZ_AUTHOR_RECEIPT_ID;
  if (requested) {
    const directory = path.join(parent, safeName(requested, "authoring receipt ID"));
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    return directory;
  }
  return mkdtempSync(path.join(parent, `${safeName(journey, "journey")}-`));
}

function surfaceEnvironment(manifest, target, journey) {
  const surface = manifest.surfaces[target];
  const environment = { ...process.env, ...(surface.conditions || {}) };
  const selection = (surface.journeyOverrides || []).find(
    (override) => override.journeys.length === Number("1") && override.journeys[Number("0")] === journey,
  );
  if (selection) Object.assign(environment, selection.when);
  for (const [targetName, sourceName] of Object.entries(surface.env || {})) {
    const value = environment[sourceName];
    if (value !== undefined) {
      environment[sourceName] = value;
      environment[targetName] = value;
    }
  }
  const missingManifestEnvironment = [...new Set(
    Object.entries(surface.env || {})
      .filter(([targetName, sourceName]) => targetName !== sourceName && String(environment[sourceName] ?? "").trim() === "")
      .map(([, sourceName]) => sourceName),
  )];
  if (target === "tui" && missingManifestEnvironment.length) {
    throw new Error(`tui authoring probe needs resolved manifest environment: ${missingManifestEnvironment.join(", ")}`);
  }
  return environment;
}

function isolatedTuiProbe(manifest, journey, artifactsRoot) {
  const home = path.join(artifactsRoot, "home");
  const session = path.join(artifactsRoot, "session");
  const temporary = path.join(artifactsRoot, "temporary");
  const xdgConfig = path.join(artifactsRoot, "xdg", "config");
  const xdgCache = path.join(artifactsRoot, "xdg", "cache");
  const xdgData = path.join(artifactsRoot, "xdg", "data");
  const xdgState = path.join(artifactsRoot, "xdg", "state");
  for (const directory of [home, session, temporary, xdgConfig, xdgCache, xdgData, xdgState]) {
    mkdirSync(directory, { recursive: true, mode: 0o700 });
  }
  return {
    cwd: session,
    env: {
      ...surfaceEnvironment(manifest, "tui", journey),
      HOME: home,
      XDG_CONFIG_HOME: xdgConfig,
      XDG_CACHE_HOME: xdgCache,
      XDG_DATA_HOME: xdgData,
      XDG_STATE_HOME: xdgState,
      TMPDIR: temporary,
      TMP: temporary,
      TEMP: temporary,
    },
  };
}

export { probeWeb, probeNative, probeTui, probeCua };

async function probeWeb(baseUrl) {
  const { chromium } = await import("playwright");
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(baseUrl, { waitUntil: "load" });
    const title = await page.title();
    const snapshot = await page.evaluate(() => {
      const items = [];
      const textOf = (el) => (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("placeholder") || "")
        .trim().replace(/\s+/g, " ");
      for (const el of document.querySelectorAll("h1,h2,h3")) {
        const text = textOf(el);
        if (text) items.push(`${el.tagName.toLowerCase()}: ${text.slice(0, 80)}`);
      }
      for (const el of document.querySelectorAll("button,[role='button'],a,input,select,textarea,[aria-label],[data-testid]")) {
        const text = textOf(el);
        const id = el.id ? `#${el.id}` : (el.getAttribute("data-testid") ? `[data-testid=${el.getAttribute("data-testid")}]` : el.tagName.toLowerCase());
        if (text || id) items.push(`${id}: ${text.slice(0, 80)}`);
      }
      return items.slice(0, Number("120")).join("\n");
    });
    const body = (await page.locator("body").innerText()).replace(/\s+/g, " ").slice(0, BODY_CHARS);
    return [`kind: web`, `url: ${baseUrl}`, `title: ${title}`, `body text: ${body}`, "interactive/headings:", snapshot].join("\n").slice(0, PROBE_CHARS);
  } finally {
    await browser.close();
  }
}

function latestIosRuntimeVersion() {
  const out = spawnSync("xcrun", ["simctl", "list", "runtimes", "-j"], { encoding: "utf8" });
  if (out.status !== 0) return null;
  try {
    const runtimes = JSON.parse(out.stdout).runtimes || [];
    const ios = runtimes.filter((runtime) => runtime.platform === "iOS" && runtime.version);
    if (!ios.length) return null;
    const match = ios[ios.length - 1].version.match(/^(\d+\.\d+)/);
    return match ? match[1] : null;
  } catch {
    return null;
  }
}

async function probeNative(target, appPath) {
  const { remote } = await import("webdriverio");
  const capabilities = target === "desktop:mac"
    ? { platformName: "Mac", "appium:automationName": "Mac2", "appium:appPath": appPath, "appium:showServerLogs": false }
    : {
      platformName: "iOS",
      "appium:automationName": "XCUITest",
      "appium:app": appPath,
      ...(latestIosRuntimeVersion() ? { "appium:platformVersion": latestIosRuntimeVersion() } : {}),
    };
  const driver = await remote({ hostname: "127.0.0.1", port: Number("4723"), capabilities, logLevel: "error" });
  try {
    const source = await driver.getPageSource();
    return [`kind: ${target}`, `app: ${appPath}`, "accessibility tree (truncated):", source].join("\n").slice(0, PROBE_CHARS);
  } finally {
    await driver.deleteSession().catch(() => {});
  }
}

async function probeTui(command, options = {}) {
  const { spawnTui } = await import("../packages/tui/pty.mjs");
  const session = spawnTui(command, [], options);
  await session.sleep(Number("3000"));
  try {
    const screen = session.screen();
    // A finite CLI is still a terminal application. Its exit status and output
    // are authoring context, not a successful journey or a reason to invent an
    // interactive wrapper. The subsequently executed spec decides the verdict.
    const state = session.child.exitCode === null ? "running" : `exited with code ${session.child.exitCode}`;
    if (!screen.trim()) throw new Error("tui authoring probe returned an empty initial screen");
    return [`kind: tui`, `command: ${command}`, `process: ${state}`, "observed output (pty frame, ANSI stripped):", screen].join("\n").slice(0, PROBE_CHARS);
  } finally {
    await session.close();
  }
}

function cuaExecutablePath(appPath) {
  if (!appPath || !appPath.endsWith(".app")) return null;
  const plist = path.join(appPath, "Contents", "Info.plist");
  const result = spawnSync(
    "/usr/libexec/PlistBuddy",
    ["-c", "Print :CFBundleExecutable", plist],
    { encoding: "utf8" },
  );
  const executable = String(result.stdout || "").trim();
  return result.status === Number("0") && executable
    ? path.join(appPath, "Contents", "MacOS", executable)
    : null;
}

async function probeCua(appPath) {
  const { launchCuaApp, launchCuaProcess, snapshotTree, quitApp } = await import("../packages/desktop-cua/driver.mjs");
  const executable = process.env.CUA_APP_EXECUTABLE || cuaExecutablePath(appPath);
  const app = executable
    ? launchCuaProcess({ executable })
    : launchCuaApp({ bundleId: appPath });
  try {
    const tree = snapshotTree(app.pid, app.windowId);
    return [`kind: desktop:cua`, `app: ${executable || appPath}`, "accessibility tree (cua-driver element_index rendering):", tree].join("\n").slice(0, PROBE_CHARS);
  } finally {
    quitApp(app.pid);
  }
}

function styleGuide(target) {
  if (target === "web" || target === "electron") {
    return [
      "Spec style: Playwright with @playwright/test.",
      "  import { test, expect } from '@playwright/test';",
      "  test('<journey name>', async ({ page }) => { await page.goto('/'); ... });",
      "The harness provides BASE_URL; navigate with page.goto('/'). Prefer role/text selectors from the probe dump.",
    ].join("\n");
  }
  if (target === "tui") {
    return [
      "Spec style: plain node script (.mjs) driving the real binary through the TUI PTY driver.",
      "  import assert from 'node:assert/strict';",
      "  import { mkdirSync, writeFileSync } from 'node:fs';",
      "  import { dirname, join } from 'node:path';",
      "  import { pathToFileURL } from 'node:url';",
      "  const toolkitRoot = process.env.PROBIERZ_TOOLKIT_ROOT;",
      "  const retained = process.env.PROBIERZ_ARTIFACTS;",
      "  assert.ok(toolkitRoot, 'PROBIERZ_TOOLKIT_ROOT is required');",
      "  assert.ok(retained, 'PROBIERZ_ARTIFACTS is required');",
      "  const { spawnTui } = await import(pathToFileURL(join(toolkitRoot, 'packages/tui/pty.mjs')).href);",
      "  const home = join(retained, 'home'); const session = join(retained, 'session'); const temporary = join(retained, 'temporary');",
      "  for (const directory of [home, session, temporary]) mkdirSync(directory, { recursive: true });",
      "  const app = spawnTui(process.env.TUI_CMD || '<command>', [], { cwd: session, env: {",
      "    HOME: home, XDG_CONFIG_HOME: join(home, '.config'), XDG_CACHE_HOME: join(home, '.cache'),",
      "    XDG_DATA_HOME: join(home, '.local', 'share'), XDG_STATE_HOME: join(home, '.local', 'state'),",
      "    TMPDIR: temporary, TMP: temporary, TEMP: temporary,",
      "  } });",
      "  await app.waitFor('<text from the probe>');",
      "  app.send('/some-input'); app.key('enter');",
      "  await app.waitFor('<expected text>');",
      "  await app.close();",
      "  let transcript = app.fullLog();",
      "  for (const [name, value] of Object.entries(process.env)) {",
      "    if (/(auth|cookie|credential|key|password|secret|token)/i.test(name) && String(value).length >= 4)",
      "      transcript = transcript.replaceAll(String(value), `[REDACTED:${name}]`);",
      "  }",
      "  const trace = join(retained, 'journey-trace.txt');",
      "  writeFileSync(trace, transcript, { mode: 0o600 });",
      "  const mediaManifest = process.env.PROBIERZ_MEDIA_MANIFEST;",
      "  assert.ok(mediaManifest, 'PROBIERZ_MEDIA_MANIFEST is required');",
      "  mkdirSync(dirname(mediaManifest), { recursive: true });",
      "  writeFileSync(mediaManifest, JSON.stringify([{ file: trace, kind: 'trace', contentType: 'text/plain' }]), { mode: 0o600 });",
      "Assertions use node:assert. Drive only via app.send/app.key; read state via app.screen() (current",
      "frame) or app.fullLog() (whole session). Keys: enter, tab, esc, backspace, ctrl-c, ctrl-d, arrows.",
      "The retained artifact directory owns HOME, every XDG directory, the working session and temporary",
      "files. Retain a redacted terminal trace and declare it through PROBIERZ_MEDIA_MANIFEST as shown.",
      "Use the already resolved manifest credentials; never open login, browser, consent or notification UI.",
      "The journey must reach real app UI states from the probe. NEVER assert on driver/harness errors,",
      "launch failures, or nonzero exits as the outcome — if the app fails to launch or the journey",
      "cannot be completed, the spec must throw (fail), not pass.",
    ].join("\n");
  }
  if (target === "desktop:cua") {
    return [
      "Spec style: plain node script (.mjs) driving the real macOS app through the cua-driver.",
      "  import assert from 'node:assert/strict';",
      "  import { join } from 'node:path';",
      "  import { pathToFileURL } from 'node:url';",
      "  const toolkitRoot = process.env.PROBIERZ_TOOLKIT_ROOT;",
      "  assert.ok(toolkitRoot, 'PROBIERZ_TOOLKIT_ROOT is required');",
      "  const { launchCuaApp, snapshotTree, waitForText, elementIndexOf, clickElement, typeText, pressKey, quitApp } =",
      "    await import(pathToFileURL(join(toolkitRoot, 'packages/desktop-cua/driver.mjs')).href);",
      "  const app = launchCuaApp({ bundleId: process.env.CUA_BUNDLE_ID || '<bundle id>' });",
      "  const tree = waitForText(app.pid, app.windowId, '<text from the probe>');",
      "  clickElement(app.pid, app.windowId, elementIndexOf(tree, '<button>'));",
      "  await waitForText(app.pid, app.windowId, '<expected text>');",
      "  quitApp(app.pid);",
      "element_index values are valid ONLY for the snapshot they came from: re-snapshot (snapshotTree or",
      "waitForText) after every click/key before using another index. SwiftUI renders progressively:",
      "before touching an interactive element, waitForText for THAT element's own role/label (e.g.",
      "'AXTextField', 'AXButton (Send') — a static title appearing first does not mean inputs exist yet.",
      "Sidebar/outline rows REJECT AX actions (-25206/-25200): never click them by element_index. Use",
      "selectSidebarRow(pid, windowId, N) with the row's 0-based ordinal in the probe's outline order —",
      "it focuses the band and walks the selection with arrow keys — then waitForText the panel content.",
      "The spec must pass with NO environment setup: when the app needs a test seam (e.g.",
      "TAMA_TEST_IDENTITY), launch with launchCuaProcess({ executable: process.env.CUA_APP_EXECUTABLE ||",
      "'<absolute app binary path>', env: { TAMA_TEST_IDENTITY: '1' } }) so the flag travels inside the",
      "spec itself. Typing: pass the field's",
      "element_index to typeText (it focuses before writing); AX does not always expose a field's typed value, so prefer",
      "asserting a STATE CHANGE (button enabled, new screen, new tree text) over reading field contents",
      "back. The journey must reach real app UI states from the probe. NEVER assert on driver/harness",
      "errors or launch failures as the outcome — if the app fails to launch or the journey cannot be",
      "completed, the spec must throw (fail), not pass.",
    ].join("\n");
  }
  return [
    "Spec style: WebdriverIO with @wdio/globals.",
    "  import { $, $$, browser } from '@wdio/globals';",
    "Use accessibility-id selectors (`~identifier`) or XPath by visible text from the probe dump. The harness",
    "launches the real app; do not launch it yourself.",
  ].join("\n");
}

function buildBrief({ appId, journey, target, desc, probe, productSpec, round, rounds, previousSpec, failures }) {
  const lines = [
    `Write an e2e journey spec for the app "${appId}" (target ${target}), journey "${journey}".`,
    `Journey goal: ${desc}`,
    `Accepted product-owned path: ${productSpec}`,
    "",
    "Return the complete contents of exactly one self-contained spec through the submit_probierz_spec tool.",
    "Do not use Markdown fences, modify files, or return any other artifact.",
    "Probe of the real app (use these selectors; anything else must be discovered by the spec itself):",
    probe,
    "",
    styleGuide(target),
    "",
    "Hard rules:",
    "- Drive the real app only: no mocks, no fake selectors, no stubbing, no screenshots-only assertions.",
    "- One focused journey; readable, deterministic, no sleeps beyond explicit waits for real conditions.",
    "- The file must be self-contained and pass on the first run.",
  ];
  if (previousSpec && failures.length) {
    lines.push("", `Round ${round} of ${rounds}: your previous spec FAILED. Fix it based on the run failures.`);
    lines.push("--- PREVIOUS SPEC ---", previousSpec, "--- RUN FAILURES ---", ...failures);
  } else {
    lines.push("", `Round ${round} of ${rounds}.`);
  }
  lines.push("", "Call submit_probierz_spec exactly once with the complete spec, then stop.");
  return lines.join("\n");
}


function runStagedSpec({ appId, journey, target, stagedPath, baseUrl, appPath }) {
  const manifest = loadAppManifest(appId);
  const env = surfaceEnvironment(manifest, target, journey);
  if (baseUrl) env.BASE_URL = baseUrl;
  if (appPath && target.startsWith("mobile:")) {
    env.APP_IOS = appPath;
    if (!env.IOS_VERSION && latestIosRuntimeVersion()) env.IOS_VERSION = latestIosRuntimeVersion();
  } else if (appPath && target === "tui") {
    env.TUI_CMD = appPath;
  } else if (appPath && target === "desktop:cua") {
    const executable = cuaExecutablePath(appPath);
    if (executable) env.CUA_APP_EXECUTABLE = executable;
    else env.CUA_BUNDLE_ID = appPath;
  } else if (appPath) {
    env.MAC_APP_PATH = appPath;
  }
  const run = spawnSync(process.execPath, [
    CLI, "run", target, "--app", appId, "--spec", stagedPath,
    "PROBIERZ_RUN_KIND=pull-request", `PROBIERZ_JOURNEY=${journey}`,
  ], {
    encoding: "utf8",
    env,
    maxBuffer: Number("33554432"),
  });
  let runId = null;
  try {
    runId = JSON.parse(run.stdout)?.runId || null;
  } catch {}
  const history = runHistory({ appId, limit: Number("20") });
  const exact = history.runs.find((candidate) => candidate.runId === runId) || null;
  const result = { exit: run.status, runId, status: exact?.status || "unknown", failures: [] };
  if (exact?.manifestPath) {
    const analysisFile = path.join(path.dirname(exact.manifestPath), "analysis.json");
    if (existsSync(analysisFile)) {
      try {
        const analysis = JSON.parse(readFileSync(analysisFile, "utf8"));
        result.failures = (analysis.failures || []).map((failure) => String(failure.error || failure.message || "").slice(0, Number("400"))).filter(Boolean).slice(0, Number("6"));
      } catch { result.failures = []; }
    }
  }
  return result;
}

export function installAcceptedSpec({
  appId,
  journey,
  target,
  area = null,
  content,
  mappingPaths = [],
  productRoot = null,
  runId = null,
  sourceIdentity = null,
  authoringRoot = null,
}) {
  const manifest = loadAppManifest(appId);
  const paths = authoredPaths({ manifest, journey, target, area, productRoot });
  const bytes = Buffer.isBuffer(content) ? content : Buffer.from(String(content), "utf8");
  const document = parseYaml(readFileSync(manifest.file, "utf8"));
  if (!document.surfaces?.[target]) {
    throw new Error(`app ${appId} has no ${target} surface`);
  }
  document.journeys[journey] = document.journeys[journey]
    || { owner: document.owner || "probierz", timeoutMs: Number("300000") };
  const surface = document.surfaces[target];
  surface.journeys = [...new Set([...(surface.journeys || []), journey])].sort();
  if (mappingPaths.length) {
    const mappings = document.repositories[Number("0")].mappings || [];
    const duplicate = mappings.some((mapping) =>
      JSON.stringify(mapping.paths || []) === JSON.stringify(mappingPaths)
      && JSON.stringify(mapping.journeys || []) === JSON.stringify([journey]));
    if (!duplicate) mappings.push({ paths: mappingPaths, journeys: [journey] });
    document.repositories[Number("0")].mappings = mappings;
  }
  mkdirSync(path.dirname(paths.productSpec), { recursive: true });
  assertPhysicalProductPath(paths.productRoot, paths.testsRoot, paths.productSpec);
  let replacedProductSpec = null;
  try {
    const registrationMetadata = lstatSync(paths.registration);
    if (registrationMetadata.isSymbolicLink()) {
      const candidate = path.resolve(path.dirname(paths.registration), readlinkSync(paths.registration));
      if (candidate !== paths.productSpec && inside(paths.testsRoot, candidate)) {
        replacedProductSpec = candidate;
      }
    }
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  writeFileSync(paths.productSpec, bytes);
  if (replacedProductSpec) rmSync(replacedProductSpec, { force: true });
  mkdirSync(path.dirname(paths.registration), { recursive: true });
  rmSync(paths.registration, { force: true });
  const linkTarget = path.relative(path.dirname(paths.registration), paths.productSpec);
  symlinkSync(linkTarget, paths.registration);

  writeFileSync(manifest.file, stringifyYaml(document));

  let receipt = null;
  if (authoringRoot) {
    const acceptedArtifact = path.join(authoringRoot, `accepted-spec.${productSpecExtension(target)}`);
    writeFileSync(acceptedArtifact, bytes);
    receipt = path.join(authoringRoot, "accepted.json");
    writeFileSync(receipt, `${JSON.stringify({
      schemaVersion: Number("1"),
      appId,
      journey,
      area: paths.area,
      target,
      runId,
      sourceSha256: sourceIdentity?.app?.sha256 || null,
      harnessSha256: sourceIdentity?.harness?.sha256 || null,
      spec: {
        relativePath: path.relative(paths.productRoot, paths.productSpec).split(path.sep).join("/"),
        artifact: path.relative(ROOT, acceptedArtifact).split(path.sep).join("/"),
        bytes: bytes.length,
        sha256: createHash("sha256").update(bytes).digest("hex"),
      },
      mappingPaths,
      registration: {
        relativePath: path.relative(ROOT, paths.registration).split(path.sep).join("/"),
      },
    }, null, Number("2"))}\n`, { mode: 0o600 });
  }
  return {
    spec: paths.productSpec,
    registration: paths.registration,
    manifest: manifest.file,
    receipt,
  };
}

export async function authorSpec({
  appId,
  journey,
  target,
  desc,
  area = null,
  baseUrl = null,
  appPath = null,
  mappingPaths = [],
  rounds = Number("3"),
  dryRun = false,
}) {
  if (!TARGET_SPEC_DIRS[target]) throw new Error(`unsupported target: ${target}`);
  const manifest = loadAppManifest(appId);
  if (!manifest.surfaces[target]) throw new Error(`app ${appId} has no ${target} surface`);
  const paths = authoredPaths({ manifest, journey, target, area });
  const sourceIdentity = appSourceIdentity(appId, { primaryRoot: paths.productRoot });
  const selectedAppPath = target === "tui"
    && appPath
    && !path.isAbsolute(appPath)
    && /[\\/]/.test(appPath)
    ? path.resolve(appPath)
    : appPath;
  if (target === "web" && !baseUrl) throw new Error("web authoring needs --base-url");
  if (target !== "web" && target !== "electron" && !selectedAppPath) throw new Error(`${target} authoring needs --app-path`);
  const artifactsRoot = authoringArtifactRoot(appId, journey);
  const tuiProbe = target === "tui" ? isolatedTuiProbe(manifest, journey, artifactsRoot) : null;
  const probe = target === "web" || target === "electron"
    ? await probeWeb(baseUrl)
    : (target === "tui" ? await probeTui(selectedAppPath, tuiProbe) : (target === "desktop:cua" ? await probeCua(selectedAppPath) : await probeNative(target, selectedAppPath)));
  writeFileSync(path.join(artifactsRoot, "probe.txt"), `${probe}\n`, { mode: 0o600 });
  const stagedPath = path.join(artifactsRoot, `candidate${specExtension(target)}`);
  let previousSpec = null;
  let failures = [];
  for (let round = 1; round <= rounds; round += 1) {
    const brief = buildBrief({
      appId,
      journey,
      target,
      desc,
      probe,
      productSpec: paths.productSpec,
      round,
      rounds,
      previousSpec,
      failures,
    });
    writeFileSync(path.join(artifactsRoot, `round-${round}-brief.txt`), `${brief}\n`, { mode: 0o600 });
    if (dryRun) return { ok: true, dryRun: true, brief, stagedPath, artifactsRoot, productSpec: paths.productSpec, registration: paths.registration };
    rmSync(stagedPath, { force: true });
    try {
      const drafted = await draftStructuredArtifact({
        brief,
        toolName: "submit_probierz_spec",
        description: "Submit the complete Probierz journey spec for the current authoring round.",
        model: manifest.surfaces[target].conditions?.PROBIERZ_MODEL,
      });
      writeFileSync(stagedPath, drafted.content);
    } catch (error) {
      return {
        ok: false,
        reason: "Stado model-router authoring failed",
        detail: error instanceof Error ? error.message : String(error),
        artifactsRoot,
      };
    }
    const run = runStagedSpec({ appId, journey, target, stagedPath, baseUrl, appPath: selectedAppPath });
    if (run.status === "passed") {
      const currentSourceIdentity = appSourceIdentity(appId, { primaryRoot: paths.productRoot });
      if (currentSourceIdentity.app?.sha256 !== sourceIdentity.app?.sha256) {
        throw new Error("product source changed during authoring; refusing to register a stale verified spec");
      }
      const accepted = installAcceptedSpec({
        appId,
        journey,
        target,
        area,
        content: readFileSync(stagedPath),
        mappingPaths,
        runId: run.runId,
        sourceIdentity,
        authoringRoot: artifactsRoot,
      });
      rmSync(stagedPath, { force: true });
      return {
        ok: true,
        journey,
        area: paths.area,
        target,
        spec: accepted.spec,
        registration: accepted.registration,
        manifest: accepted.manifest,
        receipt: accepted.receipt,
        artifactsRoot,
        runId: run.runId,
        rounds: round,
      };
    }
    previousSpec = readFileSync(stagedPath, "utf8");
    failures = run.failures;
  }
  rmSync(stagedPath, { force: true });
  return { ok: false, reason: `authoring did not converge in ${rounds} rounds`, lastFailures: failures, artifactsRoot };
}
