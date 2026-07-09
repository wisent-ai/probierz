// Execution + recording layer for the probierz test toolkit. Unlike the
// read-only agent/lib.mjs discovery surface, this module SPAWNS a real test run
// (an npm script -> Playwright or WebdriverIO+Appium) under caller-chosen
// conditions, forces recording on when asked, and reports where the artifacts
// and the machine-readable report landed. Heavy and side-effecting, so only the
// CLI/MCP `run` tool reaches it -- never the read-only discovery path.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { preflight } from "./preflight.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// probierz/agent -> probierz project root.
const ROOT = path.resolve(HERE, "..");

// Runnable targets -> the workspace package + root npm script that drives them,
// and whether Playwright or WebdriverIO produces the report.
export const TARGETS = {
  web: { pkg: "packages/web", script: "test:web", tool: "playwright" },
  electron: { pkg: "packages/electron", script: "test:electron", tool: "playwright" },
  "mobile:ios": { pkg: "packages/mobile", script: "test:mobile:ios", tool: "wdio", kind: "mobile" },
  "mobile:android": { pkg: "packages/mobile", script: "test:mobile:android", tool: "wdio", kind: "mobile" },
  "desktop:mac": { pkg: "packages/desktop-native", script: "test:desktop:mac", tool: "wdio", kind: "native" },
  "desktop:win": { pkg: "packages/desktop-native", script: "test:desktop:win", tool: "wdio", kind: "native" },
};

export function targetList() {
  return Object.keys(TARGETS);
}

// Where a target writes its machine-readable report. Playwright uses the JSON
// reporter (report.json); the WDIO confs write probierz-<kind>-results.json.
// Both land in the package's test-results dir (the run's artifactsDir).
function reportFor(t, artifactsDir) {
  if (t.tool === "playwright") return path.join(artifactsDir, "report.json");
  return path.join(artifactsDir, `probierz-${t.kind}-results.json`);
}

// Keep only the last N chars of each stream; a real run is noisy and the full
// log lives on disk anyway.
const TAIL = 4000;
const DEFAULT_TIMEOUT_MS = 20 * 60 * 1000;

// Spawn a real run. opts:
//   env        extra condition vars (BASE_URL, APP_IOS, PROBIERZ_LOCALE, ...)
//   record     force video/trace/screenshot capture on (sets PROBIERZ_RECORD=1)
//   timeoutMs  kill the run after this long (default 20 min)
//   spec       run only this one spec (path/substring); scopes a run to e.g.
//              a single app's suite instead of every spec in the package
//   force      skip the preflight gate and spawn even if the toolchain looks
//              incomplete (for when detection is wrong or deps are elsewhere)
// Resolves to a structured result; a failing suite is NOT an error (the exit
// code carries that). When the toolchain is not ready and force is unset, it
// resolves early with { ready:false, skipped:true, preflight } and never
// spawns. Rejects only on an unknown target or a spawn error.
export function runSurface(target, opts = {}) {
  const t = TARGETS[target];
  if (!t) {
    return Promise.reject(
      new Error(`unknown target: ${target} (one of ${targetList().join(", ")})`),
    );
  }
  const pkgDir = path.join(ROOT, t.pkg);
  const artifactsDir = path.join(pkgDir, "test-results");
  const reportPath = reportFor(t, artifactsDir);
  const record = Boolean(opts.record);

  // Preflight gate: unless forced, refuse to spawn when the toolchain is not
  // ready and return exactly what is missing + how to fix it, rather than an
  // opaque failure deep inside npm/Playwright/Appium.
  if (!opts.force) {
    const pf = preflight(target);
    if (!pf.ready) {
      return Promise.resolve({
        target,
        tool: t.tool,
        pkg: t.pkg,
        script: t.script,
        ready: false,
        skipped: true,
        preflight: pf,
        artifactsDir,
        reportPath,
      });
    }
  }

  const env = { ...process.env, ...(opts.env || {}), PROBIERZ_ARTIFACTS: artifactsDir };
  if (record) env.PROBIERZ_RECORD = "1";
  // Single-spec filter via env, not CLI args: the run goes through nested npm
  // workspace scripts (npm run test:mobile:ios -> npm --workspace ... run
  // test:ios -> wdio), and each npm layer eats `--` passthrough differently. An
  // env var the configs read is immune to that. wdio.shared.conf and the
  // Playwright configs override their spec set when PROBIERZ_SPEC is present.
  if (opts.spec) env.PROBIERZ_SPEC = opts.spec;

  const command = `npm run ${t.script}${opts.spec ? ` (PROBIERZ_SPEC=${opts.spec})` : ""}`;
  const wanted = Number(opts.timeoutMs);
  const timeoutMs = wanted > Number("0") ? wanted : DEFAULT_TIMEOUT_MS;

  return new Promise((resolve, reject) => {
    const child = spawn("npm", ["run", t.script], {
      cwd: ROOT,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    let err = "";
    let timedOut = false;
    const started = Date.now();
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);

    child.stdout.on("data", (d) => { out = (out + d).slice(-TAIL); });
    child.stderr.on("data", (d) => { err = (err + d).slice(-TAIL); });
    child.on("error", (e) => { clearTimeout(timer); reject(e); });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      const exitCode = code === null ? -1 : code;
      resolve({
        target,
        tool: t.tool,
        pkg: t.pkg,
        script: t.script,
        command,
        spec: opts.spec || null,
        conditions: { record, ...(opts.env || {}) },
        exitCode,
        signal: signal || null,
        timedOut,
        passed: exitCode === 0 && !timedOut,
        durationMs: Date.now() - started,
        artifactsDir,
        reportPath,
        stdoutTail: out,
        stderrTail: err,
      });
    });
  });
}
