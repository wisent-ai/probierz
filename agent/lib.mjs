// Shared, read-only discovery for the probierz test toolkit. The CLI and the
// MCP server both import this, so there is one source of truth for which
// surfaces and specs exist. No test is ever executed here: running a suite
// needs Chromium / Appium / a simulator and is a heavy side-effecting action
// kept out of this read-only surface (mirrors how echo / skarbiec / stado
// expose only reads).
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// probierz/agent -> probierz project root.
const ROOT = path.resolve(HERE, "..");

// Cross-platform surfaces this toolkit drives. Single source of truth for
// `list`; the run commands are strings only (never executed here).
export const SURFACES = [
  {
    name: "web",
    pkg: "packages/web",
    tool: "Playwright",
    script: "test:web",
    targets: "Chromium / Firefox / WebKit + emulated mobile",
    env: ["BASE_URL"],
  },
  {
    name: "electron",
    pkg: "packages/electron",
    tool: "Playwright (_electron)",
    script: "test:electron",
    targets: "Electron desktop app",
    env: ["ELECTRON_APP_MAIN"],
  },
  {
    name: "mobile",
    pkg: "packages/mobile",
    tool: "WebdriverIO + Appium (XCUITest / UiAutomator2)",
    script: "test:mobile:ios | test:mobile:android",
    targets: "iOS / Android",
    env: ["APP_IOS", "APP_ANDROID", "BUNDLE_ID", "APP_PACKAGE", "IOS_DEVICE", "IOS_VERSION", "GMAIL_TOKEN"],
  },
  {
    name: "desktop-native",
    pkg: "packages/desktop-native",
    tool: "WebdriverIO + Appium (Mac2 / WinAppDriver)",
    script: "test:desktop:mac | test:desktop:win",
    targets: "native macOS / Windows",
    env: ["MAC_BUNDLE_ID", "WIN_APP"],
  },
  {
    name: "desktop-cua",
    pkg: "packages/desktop-cua",
    tool: "cua-driver",
    script: "test:desktop:cua",
    targets: "native desktop accessibility surfaces",
    env: ["CUA_APP_EXECUTABLE"],
  },
  {
    name: "tui",
    pkg: "packages/tui",
    tool: "PTY",
    script: "test:tui",
    targets: "terminal applications",
    env: ["TUI_CMD"],
  },
];

const SPEC_DIRS = ["test/specs", "tests", "specs"];
const SPEC_SUFFIXES = [".e2e.ts", ".spec.ts", ".spec.mjs"];

function listSpecFiles(pkgRel) {
  const out = [];
  for (const sub of SPEC_DIRS) {
    const dir = path.join(ROOT, pkgRel, sub);
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      if (SPEC_SUFFIXES.some((s) => name.endsWith(s))) {
        out.push(path.join(pkgRel, sub, name));
      }
    }
  }
  return out.sort();
}

// Specs discovered on disk per surface, so the list always reflects the tree.
export function listSpecs(surfaceName) {
  const chosen = surfaceName ? SURFACES.filter((s) => s.name === surfaceName) : SURFACES;
  if (surfaceName && !chosen.length) throw new Error(`unknown surface: ${surfaceName}`);
  return chosen.map((s) => ({ surface: s.name, specs: listSpecFiles(s.pkg) }));
}

// Static outline of a spec: describe / it / test titles in file order. Pure
// text scan against a spec path under the probierz root; nothing is executed.
export function describeSpec(relPath) {
  const clean = String(relPath).replace(/^[/]+/, "");
  const abs = path.resolve(ROOT, clean);
  if (abs !== ROOT && !abs.startsWith(ROOT + path.sep)) {
    throw new Error("path escapes the probierz root");
  }
  if (!existsSync(abs)) throw new Error(`spec not found: ${clean}`);
  const src = readFileSync(abs, "utf8");
  const re = /\b(describe|it|test)\s*\(\s*(['"`])([^'"`]*)\2/g;
  const outline = [];
  let m = re.exec(src);
  while (m !== null) {
    outline.push({ kind: m[Number("1")], title: m[Number("3")] });
    m = re.exec(src);
  }
  return { spec: clean, count: outline.length, outline };
}

// The exact shell command to run a surface. Returned as a string; this module
// never spawns it.
const RUN_COMMANDS = {
  web: "BASE_URL=https://example.com npm run test:web",
  electron: "ELECTRON_APP_MAIN=/abs/app/main.js npm run test:electron",
  "mobile:ios": "APP_IOS=/abs/App.app IOS_DEVICE='iPhone 17' npm run test:mobile:ios",
  "mobile:android": "APP_ANDROID=/abs/app.apk npm run test:mobile:android",
  "desktop:mac": "MAC_BUNDLE_ID=com.apple.TextEdit npm run test:desktop:mac",
  "desktop:win": "WIN_APP='Microsoft.WindowsCalculator_8wekyb3d8bbwe!App' npm run test:desktop:win",
};

export function runCommand(target) {
  const cmd = RUN_COMMANDS[target];
  if (!cmd) {
    throw new Error(`unknown target: ${target} (one of ${Object.keys(RUN_COMMANDS).join(", ")})`);
  }
  return { target, command: cmd, note: "read-only: this is the command to run yourself; probierz never executes it" };
}
