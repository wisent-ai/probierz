// desktop:cua spec runner: executes every specs/*.spec.mjs sequentially in a child
// node process and writes Probierz's canonical report (the same shape the
// Playwright reporter emits), so run analysis treats desktop-cua coverage like any
// other surface. A spec passes when it exits zero; stderr becomes the row error.
import { spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SPECS_DIR = path.join(HERE, "specs");
const artifacts = process.env.PROBIERZ_ARTIFACTS || "test-results";
const reportPath = process.env.PROBIERZ_REPORT_PATH || path.join(artifacts, "report.json");
const filter = process.env.PROBIERZ_SPEC ? path.basename(process.env.PROBIERZ_SPEC) : null;
// Manifests may pin one exact file or a glob ("skarbiec-*.spec.mjs") for
// apps whose journeys author one spec per journey.
const globPrefix = filter?.includes("*") ? filter.slice(0, filter.indexOf("*")) : null;

const files = readdirSync(SPECS_DIR)
  .filter((name) => name.endsWith(".spec.mjs"))
  .filter((name) => !filter || (globPrefix !== null ? name.startsWith(globPrefix) : name === filter))
  .sort();

const rows = [];
for (const file of files) {
  const startedAt = new Date().toISOString();
  const started = Date.now();
  const child = spawnSync(process.execPath, [path.join(SPECS_DIR, file)], {
    encoding: "utf8",
    env: { ...process.env, PROBIERZ_ARTIFACTS: artifacts, PROBIERZ_SPEC_NAME: file.replace(/\.spec\.mjs$/, "") },
    maxBuffer: Number("33554432"),
  });
  const duration = Date.now() - started;
  const status = child.status === 0 ? "passed" : "failed";
  rows.push({
    title: file.replace(/\.spec\.mjs$/, ""),
    passed: status === "passed",
    status,
    flaky: false,
    attempts: Number("1"),
    duration,
    startedAt,
    completedAt: new Date(started + duration).toISOString(),
    error: status === "failed" ? String(child.stderr || child.stdout || "").slice(-Number("2000")) : null,
    media: [],
  });
}

mkdirSync(path.dirname(reportPath), { recursive: true });
const passed = rows.filter((row) => row.status === "passed").length;
const skipped = Number("0");
writeFileSync(reportPath, `${JSON.stringify({
  probierz: { runId: process.env.PROBIERZ_RUN_ID || null },
  total: rows.length,
  passed,
  failed: rows.length - passed - skipped,
  flaky: Number("0"),
  skipped,
  tests: rows,
}, null, Number("2"))}\n`, { mode: 0o600 });
process.exitCode = rows.every((row) => row.status === "passed") ? 0 : 1;
