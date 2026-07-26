import { test, expect } from '@playwright/test';
import { evaluateMatrix, writeMatrixReport, type CheckOutcome } from './helpers/verdict-matrix';

/**
 * The verdict matrix is executable. It runs after the structural specs (see
 * the project dependency in playwright.config.ts) and grades the parity
 * claims from UI-COMPARISON.md against the check ledger that run produced:
 * a claim whose check went red is BROKEN, a claim whose check never ran is
 * unverified, a claim with no check at all is unearned backlog. The doc used
 * to say "parity" because someone decided so; now it has to be measured.
 */
test.describe('verdict matrix', () => {
  test('parity verdicts are backed by structural checks that actually passed', () => {
    const report = evaluateMatrix('jeden');
    test.skip(!report.outcomes.length, 'no structural checks ran in this invocation');
    const path = writeMatrixReport(report, 'jeden');
    console.log(`[verdict-matrix] ${path}`);
    if (report.unearned.length) {
      console.log(
        `[verdict-matrix] unearned parity verdicts (no structural check exists yet): ${report.unearned
          .map((entry) => entry.view)
          .join(', ')}`,
      );
    }
    expect(
      report.unknownCheckIds,
      `unknown check ids in the matrix: ${report.unknownCheckIds.join(', ')}`,
    ).toEqual([]);
    expect(
      report.broken.map((row) => `${row.entry.view} [${row.failed.join(' ')}]`),
      'these views claim parity while their structural check failed this run',
    ).toEqual([]);
    expect(
      report.unverified.map((row) => `${row.entry.view} [${row.missing.join(' ')}]`),
      'these views claim parity but their checks never executed — the claim is unmeasured',
    ).toEqual([]);
  });

  test('the omp control side of every contract stays green (harness calibration)', () => {
    const report = evaluateMatrix('omp');
    test.skip(!report.outcomes.length, 'no omp outcomes in this run');
    const failed = report.outcomes.filter((outcome) => outcome.app === 'omp' && !outcome.ok);
    expect(
      failed.map((outcome) => `${outcome.id}: ${outcome.detail}`),
      'a contract fails on the reference app — the harness measures the wrong thing',
    ).toEqual([]);
  });

  // The grader decides what "parity" means, so it gets graded too: these run
  // on synthetic ledgers, with no app and no tmux involved.
  test('a failing check turns its parity verdict BROKEN, never earned', () => {
    const ledger: CheckOutcome[] = [
      { id: 'screen.two-pane', app: 'jeden', ok: false, detail: '0 divider rows' },
      { id: 'screen.geometry', app: 'jeden', ok: true, detail: '' },
    ];
    const report = evaluateMatrix('jeden', ledger);
    expect(report.broken.map((row) => row.entry.view)).toContain('Models');
    expect(report.earned.map((row) => row.view)).not.toContain('Models');
  });

  test('a check that never ran leaves its verdict unverified, not earned', () => {
    const ledger: CheckOutcome[] = [{ id: 'replay.hang', app: 'jeden', ok: true, detail: '' }];
    const report = evaluateMatrix('jeden', ledger);
    const login = report.unverified.find((row) => row.entry.view === 'Login / auth');
    expect(login?.missing).toEqual(['replay.identity']);
    expect(report.earned.map((row) => row.view)).not.toContain('Login / auth');
  });

  test('another app\u2019s outcomes never earn this app\u2019s verdict', () => {
    const ledger: CheckOutcome[] = [
      { id: 'replay.hang', app: 'omp', ok: true, detail: '' },
      { id: 'replay.identity', app: 'omp', ok: true, detail: '' },
    ];
    const report = evaluateMatrix('jeden', ledger);
    expect(report.earned).toEqual([]);
    expect(report.broken).toEqual([]);
  });
});
