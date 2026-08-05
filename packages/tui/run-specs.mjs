// TUI spec runner: executes every specs/*.spec.mjs sequentially in a child
// node process and writes Probierz's canonical report (the same shape the
// Playwright reporter emits), so run analysis treats TUI coverage like any
// other surface. A spec passes when it exits zero; stderr becomes the row error.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
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

// Row errors keep the failure headline AND the state dump: head for the
// "what was expected" line, tail for the final app state, so a long tree
// never drowns the actual reason.
function clipRowError(text) {
  const limit = Number("2000");
  if (text.length <= limit) return text;
  return `${text.slice(0, Number("600"))}\n…\n${text.slice(-Number("1400"))}`;
}

const captureErrors = [];

const rows = [];
for (const file of files) {
  const title = file.replace(/\.spec\.mjs$/, "");
  const startedAt = new Date().toISOString();
  const started = Date.now();
  const mediaManifestPath = path.join(artifacts, ".media", `${title}.json`);
  rmSync(mediaManifestPath, { force: true });
  const child = spawnSync(process.execPath, [path.join(SPECS_DIR, file)], {
    encoding: "utf8",
    env: {
      ...process.env,
      PROBIERZ_ARTIFACTS: artifacts,
      PROBIERZ_MEDIA_MANIFEST: mediaManifestPath,
      PROBIERZ_SPEC_NAME: title,
    },
    maxBuffer: Number("33554432"),
  });
  const duration = Date.now() - started;
  const status = child.status === 0 ? "passed" : "failed";
  let media = [];
  if (existsSync(mediaManifestPath)) {
    try {
      const declared = JSON.parse(readFileSync(mediaManifestPath, "utf8"));
      if (!Array.isArray(declared)) throw new Error("media manifest must be an array");
      const artifactsRoot = path.resolve(artifacts);
      media = declared.map((entry) => {
        if (!entry || typeof entry !== "object") throw new Error("media entry must be an object");
        if (!["screenshot", "trace", "video"].includes(entry.kind)) {
          throw new Error(`unsupported media kind ${entry.kind}`);
        }
        const resolved = path.resolve(String(entry.file || ""));
        if (resolved !== artifactsRoot && !resolved.startsWith(`${artifactsRoot}${path.sep}`)) {
          throw new Error("media path escapes the artifacts directory");
        }
        if (!existsSync(resolved)) throw new Error(`declared media does not exist: ${resolved}`);
        return {
          file: resolved,
          kind: entry.kind,
          ...(entry.contentType ? { contentType: String(entry.contentType) } : {}),
        };
      });
    } catch (error) {
      captureErrors.push(`${title}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  rows.push({
    title,
    passed: status === "passed",
    status,
    flaky: false,
    attempts: Number("1"),
    duration,
    startedAt,
    completedAt: new Date(started + duration).toISOString(),
    error: status === "failed" ? clipRowError(String(child.stderr || child.stdout || "")) : null,
    media,
  });
}

mkdirSync(path.dirname(reportPath), { recursive: true });
const passed = rows.filter((row) => row.status === "passed").length;
const skipped = Number("0");
writeFileSync(reportPath, `${JSON.stringify({
  probierz: { runId: process.env.PROBIERZ_RUN_ID || null, captureErrors },
  total: rows.length,
  passed,
  failed: rows.length - passed - skipped,
  flaky: Number("0"),
  skipped,
  tests: rows,
}, null, Number("2"))}\n`, { mode: 0o600 });
process.exitCode = rows.every((row) => row.status === "passed") ? 0 : 1;
