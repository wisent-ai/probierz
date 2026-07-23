import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

// Minimal Playwright reporter that writes Probierz's canonical, run-scoped
// report. The run ID comes from the runner environment and is emitted by the
// producer, so consumers can reject stale or cross-run reports.
export default class ProbierzPlaywrightReporter {
  constructor() {
    this.rows = new Map();
  }

  onTestEnd(test, result) {
    const status = result.status === "passed"
      ? "passed"
      : result.status === "skipped"
        ? "skipped"
        : "failed";
    const previous = this.rows.get(test.id);
    const media = [
      ...(previous?.media || []),
      ...(result.attachments || [])
        .filter((attachment) => attachment.path)
        .map((attachment) => ({
          file: attachment.path,
          kind: attachment.name,
          contentType: attachment.contentType,
        })),
    ];
    this.rows.set(test.id, {
      title: test.titlePath().join(" › "),
      passed: status === "passed",
      status,
      flaky: Boolean(previous?.flaky || (previous?.status === "failed" && status === "passed")),
      attempts: Number(result.retry || 0) + Number("1"),
      duration: Number(previous?.duration || 0) + Number(result.duration || 0),
      startedAt: previous?.startedAt || result.startTime?.toISOString?.() || null,
      completedAt: result.startTime instanceof Date
        ? new Date(result.startTime.getTime() + Number(result.duration || 0)).toISOString()
        : null,
      error: status === "failed" ? (result.error?.message || "failed") : null,
      media,
    });
  }

  onEnd() {
    const reportPath = process.env.PROBIERZ_REPORT_PATH
      || path.join(process.env.PROBIERZ_ARTIFACTS || "test-results", "report.json");
    mkdirSync(path.dirname(reportPath), { recursive: true });
    const rows = [...this.rows.values()];
    const passed = rows.filter((row) => row.status === "passed").length;
    const skipped = rows.filter((row) => row.status === "skipped").length;
    const failed = rows.length - passed - skipped;
    const flaky = rows.filter((row) => row.flaky).length;
    writeFileSync(
      reportPath,
      `${JSON.stringify({
        probierz: { runId: process.env.PROBIERZ_RUN_ID || null },
        total: rows.length,
        passed,
        failed,
        flaky,
        skipped,
        tests: rows,
      }, null, 2)}\n`,
      { mode: 0o600 },
    );
  }
}
