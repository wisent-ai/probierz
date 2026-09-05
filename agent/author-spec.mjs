// Autonomous journey spec authoring: probe the real app, draft a candidate
// spec with a headless model, verify it with an actual probierz run, and
// iterate with failure feedback until the run is green (or rounds run out).
// A green spec moves into the target package's specs directory and the
// journey is registered in the app manifest. Every candidate must drive the
// real product: no mocks, no fake selectors, no stubbing the app under test.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import { loadAppManifest } from "./apps.mjs";
import { runHistory } from "./history.mjs";
import { draftStructuredArtifact } from "./model-router.mjs";

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

async function probeTui(command) {
  const { spawnTui } = await import("../packages/tui/pty.mjs");
  const session = spawnTui(command, [], {});
  await session.sleep(Number("3000"));
  try {
    const screen = session.screen();
    return [`kind: tui`, `command: ${command}`, "initial screen (pty frame, ANSI stripped):", screen].join("\n").slice(0, PROBE_CHARS);
  } finally {
    await session.close();
  }
}

async function probeCua(bundleId) {
  const { launchCuaApp, launchCuaProcess, snapshotTree, quitApp } = await import("../packages/desktop-cua/driver.mjs");
  // CUA_APP_EXECUTABLE switches the probe to a direct-process launch with the
  // author's environment (e.g. TAMA_TEST_IDENTITY=1 for gated debug builds).
  const app = process.env.CUA_APP_EXECUTABLE ? launchCuaProcess({}) : launchCuaApp({ bundleId });
  try {
    const tree = snapshotTree(app.pid, app.windowId);
    return [`kind: desktop:cua`, `app: ${process.env.CUA_APP_EXECUTABLE || bundleId}`, "accessibility tree (cua-driver element_index rendering):", tree].join("\n").slice(0, PROBE_CHARS);
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
      "  import { spawnTui } from '../pty.mjs';",
      "  const app = spawnTui(process.env.TUI_CMD || '<command>');",
      "  await app.waitFor('<text from the probe>');",
      "  app.send('/some-input'); app.key('enter');",
      "  await app.waitFor('<expected text>');",
      "  await app.close();",
      "Assertions use node:assert. Drive only via app.send/app.key; read state via app.screen() (current",
      "frame) or app.fullLog() (whole session). Keys: enter, tab, esc, backspace, ctrl-c, ctrl-d, arrows.",
      "The journey must reach real app UI states from the probe. NEVER assert on driver/harness errors,",
      "launch failures, or nonzero exits as the outcome — if the app fails to launch or the journey",
      "cannot be completed, the spec must throw (fail), not pass.",
    ].join("\n");
  }
  if (target === "desktop:cua") {
    return [
      "Spec style: plain node script (.mjs) driving the real macOS app through the cua-driver.",
      "  import assert from 'node:assert/strict';",
      "  import { launchCuaApp, snapshotTree, waitForText, elementIndexOf, clickElement, typeText, pressKey, quitApp } from '../driver.mjs';",
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

function buildBrief({ appId, journey, target, desc, probe, round, rounds, previousSpec, failures, stagedPath }) {
  const lines = [
    `Write an e2e journey spec for the app "${appId}" (target ${target}), journey "${journey}".`,
    `Journey goal: ${desc}`,
    "",
    `Produce the complete contents for this spec path: ${stagedPath}`,
    "Call submit_probierz_spec exactly once with those contents; do not modify files or return prose.",
    "",
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
  lines.push("", "Submit only that one spec, then stop.");
  return lines.join("\n");
}


function runStagedSpec({ appId, journey, target, stagedPath, baseUrl, appPath }) {
  const env = { ...process.env };
  if (baseUrl) env.BASE_URL = baseUrl;
  if (appPath && target.startsWith("mobile:")) {
    env.APP_IOS = appPath;
    if (!env.IOS_VERSION && latestIosRuntimeVersion()) env.IOS_VERSION = latestIosRuntimeVersion();
  } else if (appPath && target === "tui") {
    env.TUI_CMD = appPath;
  } else if (appPath && target === "desktop:cua") {
    env.CUA_BUNDLE_ID = appPath;
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

function acceptSpec({ appId, journey, target, stagedPath, mappingPaths }) {
  const finalPath = path.join(TARGET_SPEC_DIRS[target], `${appId}-${journey}${specExtension(target)}`);
  renameSync(stagedPath, finalPath);
  const manifest = loadAppManifest(appId);
  const document = parseYaml(readFileSync(manifest.file, "utf8"));
  document.journeys[journey] = document.journeys[journey] || { owner: document.owner || "probierz", timeoutMs: Number("300000") };
  const surface = document.surfaces[target];
  surface.journeys = [...new Set([...(surface.journeys || []), journey])].sort();
  if (mappingPaths.length) {
    document.repositories[0].mappings = [...(document.repositories[0].mappings || []), { paths: mappingPaths, journeys: [journey] }];
  }
  writeFileSync(manifest.file, stringifyYaml(document));
  return { spec: finalPath, manifest: manifest.file };
}

export async function authorSpec({ appId, journey, target, desc, baseUrl = null, appPath = null, mappingPaths = [], rounds = Number("3"), dryRun = false }) {
  if (!TARGET_SPEC_DIRS[target]) throw new Error(`unsupported target: ${target}`);
  const manifest = loadAppManifest(appId);
  if (!manifest.surfaces[target]) throw new Error(`app ${appId} has no ${target} surface`);
  if (target === "web" && !baseUrl) throw new Error("web authoring needs --base-url");
  if (target !== "web" && target !== "electron" && !appPath) throw new Error(`${target} authoring needs --app-path`);
  const probe = target === "web" || target === "electron"
    ? await probeWeb(baseUrl)
    : (target === "tui" ? await probeTui(appPath) : (target === "desktop:cua" ? await probeCua(appPath) : await probeNative(target, appPath)));
  const stagedPath = path.join(TARGET_SPEC_DIRS[target], `.author-staging-${journey}${specExtension(target)}`);
  mkdirSync(path.dirname(stagedPath), { recursive: true });
  let previousSpec = null;
  let failures = [];
  for (let round = 1; round <= rounds; round += 1) {
    const brief = buildBrief({ appId, journey, target, desc, probe, round, rounds, previousSpec, failures, stagedPath });
    if (dryRun) return { ok: true, dryRun: true, brief, stagedPath };
    try {
      const drafted = await draftStructuredArtifact({
        brief,
        toolName: "submit_probierz_spec",
        description: "Submit the complete Probierz journey spec for the current authoring round.",
      });
      writeFileSync(stagedPath, drafted.content);
    } catch (error) {
      return {
        ok: false,
        reason: "Stado model-router authoring failed",
        detail: error instanceof Error ? error.message : String(error),
      };
    }
    const run = runStagedSpec({ appId, journey, target, stagedPath, baseUrl, appPath });
    if (run.status === "passed") {
      const accepted = acceptSpec({ appId, journey, target, stagedPath, mappingPaths });
      return { ok: true, journey, target, spec: accepted.spec, manifest: accepted.manifest, runId: run.runId, rounds: round };
    }
    previousSpec = readFileSync(stagedPath, "utf8");
    failures = run.failures;
  }
  rmSync(stagedPath, { force: true });
  return { ok: false, reason: `authoring did not converge in ${rounds} rounds`, lastFailures: failures };
}
