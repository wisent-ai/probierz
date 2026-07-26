import { evaluateMatrix, writeMatrixReport } from '../tests/helpers/verdict-matrix';

/**
 * Playwright globalTeardown. Runs whatever the outcome, so the parity report
 * exists exactly on the runs that matter — the red ones. The matrix spec
 * turns the same data into assertions; this only guarantees the artifact.
 */
export default function reportVerdicts(): void {
  for (const app of ['jeden', 'omp']) {
    const report = evaluateMatrix(app);
    if (!report.outcomes.length) continue;
    const path = writeMatrixReport(report, app);
    console.log(
      `[verdict-matrix] ${app}: earned ${report.earned.length}, broken ${report.broken.length}, ` +
        `unverified ${report.unverified.length}, unearned ${report.unearned.length} — ${path}`,
    );
  }
}
