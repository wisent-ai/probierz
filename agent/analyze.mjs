// Analysis layer for the probierz test toolkit. Reads what a run produced --
// the machine-readable report (Playwright report.json OR the WDIO
// probierz-<kind>-results.json) plus the artifacts on disk -- and returns one
// normalized summary: totals, per-test status, failure reasons, a media list
// (typed from the report's own attachment metadata, never guessed from file
// names) enriched with size + recording metadata (duration/dimensions via
// ffprobe, an optional frame montage via ffmpeg; both best-effort so a missing
// binary never fails the analysis), and a raw inventory of every other file the
// run left behind.
import { readFileSync, existsSync, statSync, readdirSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { buildTimeline } from "./timeline.mjs";
import { summarizeDiagnostics } from "./collect.mjs";

const KB = 1024;
const sizeKb = (file) => Math.round(statSync(file).size / KB);

// Recursively list files under dir.
function walk(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const name of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, name.name);
    if (name.isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

function hasBinary(bin) {
  try {
    return spawnSync(bin, ["-version"], { stdio: "ignore" }).status === 0;
  } catch {
    return false;
  }
}

// ffprobe -> { durationSec, width, height } for a video, or null if ffprobe is
// unavailable or the file is not probeable.
function probeVideo(file) {
  if (!hasBinary("ffprobe")) return null;
  const r = spawnSync(
    "ffprobe",
    ["-v", "error", "-select_streams", "v:0", "-show_entries",
     "stream=width,height:format=duration", "-of", "json", file],
    { encoding: "utf8" },
  );
  if (r.status !== 0 || !r.stdout) return null;
  try {
    const j = JSON.parse(r.stdout);
    const s = (j.streams && j.streams[0]) || {};
    const durationSec = j.format && j.format.duration ? Number(j.format.duration) : null;
    return { durationSec, width: s.width || null, height: s.height || null };
  } catch {
    return null;
  }
}

// Extract up to `count` evenly spaced frames from a video into
// <artifactsDir>/frames/<name>/. Best-effort: needs ffmpeg. Returns the frame
// paths (empty if ffmpeg is missing or extraction failed). The output dir is
// cleared first, so on a re-run stale frames never linger; every file in it is
// a frame this call wrote.
export function extractFrames(video, artifactsDir, count = 6) {
  const name = path.basename(video, path.extname(video));
  const outDir = path.join(artifactsDir, "frames", name);
  // Clear any prior frames unconditionally so stale files never linger -- even
  // when ffmpeg is unavailable and this call writes nothing.
  rmSync(outDir, { recursive: true, force: true });
  if (!hasBinary("ffmpeg")) return [];
  mkdirSync(outDir, { recursive: true });
  const meta = probeVideo(video);
  const dur = meta && meta.durationSec ? meta.durationSec : 0;
  const n = Math.max(1, Number(count));
  // one frame every dur/n seconds; fall back to 1 fps when duration unknown.
  const fps = dur > 0 ? n / dur : 1;
  const pattern = path.join(outDir, "frame_%03d.png");
  const r = spawnSync("ffmpeg", ["-y", "-i", video, "-vf", `fps=${fps}`, pattern], { stdio: "ignore" });
  if (r.status !== 0) return [];
  return walk(outDir).sort();
}

// --- report normalizers -------------------------------------------------

// Playwright report.json: walk nested suites -> specs -> tests -> results.
// Attachment kind is taken from the attachment's own name (Playwright labels
// them "video" / "trace" / "screenshot"); nothing is inferred from file names.
function normalizePlaywright(report) {
  const tests = [];
  const failures = [];
  const media = [];
  const visit = (suite) => {
    for (const spec of suite.specs || []) {
      const statuses = [];
      let duration = 0;
      let errorMsg;
      for (const t of spec.tests || []) {
        for (const res of t.results || []) {
          statuses.push(res.status);
          duration += Number(res.duration || 0);
          for (const a of res.attachments || []) {
            if (a.path) media.push({ file: a.path, kind: a.name, contentType: a.contentType });
          }
          if (res.status !== "passed" && res.errors && res.errors.length) {
            errorMsg = String(res.errors[0].message || "").split("\n")[0];
          }
        }
      }
      // spec.ok is false only for a genuine failure; it stays true for passed,
      // flaky (passed on retry), and skipped. Split those: all-skipped results
      // -> skipped, otherwise passed, so skipped never inflates the pass count.
      let status;
      if (!spec.ok) status = "failed";
      else if (statuses.length && statuses.every((s) => s === "skipped")) status = "skipped";
      else status = "passed";
      tests.push({ title: spec.title, passed: status === "passed", status, durationMs: duration });
      if (status === "failed") failures.push({ title: spec.title, error: errorMsg || "failed" });
    }
    for (const child of suite.suites || []) visit(child);
  };
  for (const s of report.suites || []) visit(s);
  const stats = report.stats || {};
  const count = (st) => tests.filter((t) => t.status === st).length;
  return {
    tool: "playwright",
    total: tests.length,
    passed: count("passed"),
    failed: count("failed"),
    flaky: Number(stats.flaky || 0),
    skipped: count("skipped"),
    durationMs: Number(stats.duration || 0),
    tests,
    failures,
    reportMedia: media,
  };
}

// WDIO probierz-<kind>-results.json: already the shape the confs emit. The
// confs record the video path per test, so media kind is known structurally.
function normalizeWdio(report, tool = "wdio") {
  const rows = report.tests || [];
  const tests = rows.map((row) => {
    const status = row.status || (row.passed ? "passed" : "failed");
    return {
      title: row.title,
      passed: status === "passed",
      status,
      durationMs: Number(row.duration || 0),
      video: row.video,
    };
  });
  const failures = rows
    .filter((row) => (row.status || (row.passed ? "passed" : "failed")) === "failed")
    .map((row) => ({ title: row.title, error: row.error || "failed" }));
  const count = (status) => tests.filter((test) => test.status === status).length;
  return {
    tool,
    total: Number(report.total ?? tests.length),
    passed: count("passed"),
    failed: count("failed"),
    flaky: Number(report.flaky || 0),
    skipped: count("skipped"),
    durationMs: tests.reduce((total, test) => total + test.durationMs, 0),
    tests,
    failures,
    reportMedia: rows.flatMap((row) => [
      ...(Array.isArray(row.media) ? row.media : []),
      ...(row.video ? [{ file: row.video, kind: "video" }] : []),
    ]),
  };
}

// Analyze a completed run. args:
//   reportPath   the machine-readable report (from runSurface result)
//   artifactsDir dir to inventory for on-disk files (from runSurface result)
//   tool         "playwright" | "wdio" (hint; inferred from report if omitted)
//   frames       if >0, extract that many frames per video (needs ffmpeg)
//   runId        expected run identity; mismatch is a hard integrity failure
// Returns the normalized summary, a report-typed media list (size + recording
// metadata), and a raw artifacts inventory. Throws only if the report is
// missing/unreadable.
export function analyzeRun({ reportPath, artifactsDir, tool, frames = 0, runId } = {}) {
  if (!reportPath || !existsSync(reportPath)) {
    throw new Error(`report not found: ${reportPath} (did the run produce one?)`);
  }
  const report = JSON.parse(readFileSync(reportPath, "utf8"));
  const reportRunId = report?.probierz?.runId || null;
  if (runId && reportRunId !== runId) {
    throw new Error(`report run ID mismatch: expected ${runId}, got ${reportRunId || "missing"}`);
  }
  const canonical = Boolean(report?.probierz) && Array.isArray(report.tests);
  const isPw = Array.isArray(report.suites);
  const summary = canonical
    ? normalizeWdio(report, tool || "probierz")
    : isPw
      ? normalizePlaywright(report)
      : normalizeWdio(report, tool || "wdio");
  const { reportMedia, ...rest } = summary;

  const want = Number(frames);
  // Media typed from the report; enrich with on-disk size + video metadata.
  const media = (reportMedia || []).map((m) => {
    const entry = { file: m.file, kind: m.kind };
    if (m.contentType) entry.contentType = m.contentType;
    if (existsSync(m.file)) {
      entry.sizeKb = sizeKb(m.file);
      if (m.kind === "video") {
        const meta = probeVideo(m.file);
        if (meta) entry.recording = meta;
        if (want > 0 && artifactsDir) entry.frames = extractFrames(m.file, artifactsDir, want);
      }
    } else {
      entry.missing = true;
    }
    return entry;
  });
  let timeline = null;
  let timelinePath = null;
  let diagnostics = null;
  if (artifactsDir) {
    const manifestPath = path.join(artifactsDir, "run-manifest.json");
    const manifest = existsSync(manifestPath)
      ? JSON.parse(readFileSync(manifestPath, "utf8"))
      : {};
    timeline = buildTimeline({
      report,
      summary: rest,
      media,
      artifactsDir,
      stdoutPath: path.join(artifactsDir, "stdout.log"),
      stderrPath: path.join(artifactsDir, "stderr.log"),
      startedAt: manifest.startedAt,
    });
    timelinePath = path.join(artifactsDir, "timeline.json");
    writeFileSync(timelinePath, `${JSON.stringify(timeline, null, 2)}\n`, { mode: 0o600 });
    diagnostics = summarizeDiagnostics({ report, timeline, artifactsDir });
  }

  // Everything else the run left on disk, sizes only -- no classification. The
  // report file itself is excluded by exact-path identity, not by name pattern.
  const known = new Set([
    ...media.map((m) => m.file),
    timelinePath,
    diagnostics?.file,
  ].filter(Boolean));
  const artifacts = artifactsDir
    ? walk(artifactsDir)
        .filter((f) => f !== reportPath && !known.has(f))
        .map((f) => ({ file: f, sizeKb: sizeKb(f) }))
    : [];

  return {
    ...rest,
    runId: reportRunId,
    reportPath,
    artifactsDir: artifactsDir || null,
    captureErrors: Array.isArray(report?.probierz?.captureErrors)
      ? report.probierz.captureErrors
      : [],
    media,
    artifacts,
    timeline: timeline ? {
      path: timelinePath,
      counts: timeline.counts,
      diagnostics: timeline.diagnostics,
    } : null,
    diagnostics,
  };
}
