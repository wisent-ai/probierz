import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { ARTIFACTS, readChecks, type CheckOutcome } from './tui';

export type { CheckOutcome };

/**
 * Executable verdict matrix: the parity claims from UI-COMPARISON.md as data.
 * A "parity" verdict is only EARNED when at least one structural check backs
 * it; verdicts without checks are printed as the explicit backlog instead of
 * silently believed (the hand-written doc lied exactly this way).
 */

export const CHECK_IDS = {
  'screen.geometry': 'overlay keeps title+footer inside the viewport',
  'screen.replacement': 'a new overlay replaces the previous one',
  'screen.transcript-budget': 'transcript growth stays bounded across overlays',
  'screen.two-pane': 'model view has a two-pane brands/models structure',
  'screen.loading-state': 'spinner precedes content on network-bound views',
  'golden.stable': 'step renders identically to its golden PNG',
  'replay.hang': '/model opens within 30s (the ^C report)',
  'replay.identity': 'agent identifies as jeden',
  'scan.no-silent-commands': 'every advertised slash command paints something',
  'scan.no-panics': 'no advertised slash command panics',
  'scan.no-unrouted-commands': 'every advertised slash command is routed by the dispatcher',
} as const;

export type CheckId = keyof typeof CHECK_IDS;

export interface VerdictEntry {
  view: string;
  verdict: 'parity' | 'omp richer' | 'jeden richer' | 'omp missing' | 'jeden missing' | 'partial';
  checks: CheckId[];
}

export const VERDICT_MATRIX: VerdictEntry[] = [
  // "Shell" is the app-wide view lifecycle the user reported first: in omp a
  // command REPLACES the screen, in jeden each one appends another frame to
  // the session. Without this row the transcript check graded nothing.
  { view: 'Shell / view lifecycle', verdict: 'parity', checks: ['screen.replacement', 'screen.transcript-budget'] },
  { view: 'Command surface', verdict: 'parity', checks: ['scan.no-silent-commands', 'scan.no-panics', 'scan.no-unrouted-commands'] },
  { view: 'Models', verdict: 'parity', checks: ['screen.geometry', 'screen.two-pane', 'screen.replacement', 'screen.transcript-budget', 'screen.loading-state', 'golden.stable'] },
  { view: 'Settings', verdict: 'parity', checks: ['screen.geometry', 'screen.replacement', 'golden.stable'] },
  { view: 'Usage', verdict: 'parity', checks: ['screen.loading-state'] },
  { view: 'Login / auth', verdict: 'parity', checks: ['replay.hang', 'replay.identity'] },
  { view: 'Collab', verdict: 'parity', checks: [] },
  { view: 'Marketplace', verdict: 'parity', checks: [] },
  { view: 'Extensions', verdict: 'parity', checks: [] },
  { view: 'Agents', verdict: 'parity', checks: [] },
  { view: 'Setup', verdict: 'parity', checks: [] },
];

export interface BrokenVerdict {
  entry: VerdictEntry;
  /** Check ids that ran and came back red. */
  failed: string[];
}

export interface UnverifiedVerdict {
  entry: VerdictEntry;
  /** Check ids the verdict leans on that produced no outcome this run. */
  missing: string[];
}

export interface MatrixReport {
  earned: VerdictEntry[];
  unearned: VerdictEntry[];
  unverified: UnverifiedVerdict[];
  broken: BrokenVerdict[];
  unknownCheckIds: string[];
  outcomes: CheckOutcome[];
}

/** Evaluate the matrix against the check ledger of THIS run. `app` selects
 * whose outcomes count: 'omp' evaluates the harness itself (a red omp check
 * means the contract is miscalibrated, not that jeden regressed). */
export function evaluateMatrix(app = 'jeden', outcomes = readChecks()): MatrixReport {
  const known = new Set<string>(Object.keys(CHECK_IDS));
  const unknownCheckIds = [
    ...new Set(VERDICT_MATRIX.flatMap((entry) => entry.checks).filter((id) => !known.has(id))),
  ];
  const mine = outcomes.filter((outcome) => outcome.app === app);
  const ran = new Set(mine.map((outcome) => outcome.id));
  const red = new Set(mine.filter((outcome) => !outcome.ok).map((outcome) => outcome.id));

  const claimsParity = VERDICT_MATRIX.filter((entry) => entry.verdict === 'parity');
  const unearned = claimsParity.filter((entry) => !entry.checks.length);
  const broken = claimsParity
    .map((entry) => ({ entry, failed: entry.checks.filter((id) => red.has(id)) }))
    .filter((row) => row.failed.length);
  const brokenViews = new Set(broken.map((row) => row.entry.view));
  // Missing evidence is NOT evidence: a verdict whose check never executed
  // stays unverified, which is also why an empty ledger earns nothing.
  const unverified = claimsParity
    .filter((entry) => entry.checks.length && !brokenViews.has(entry.view))
    .map((entry) => ({ entry, missing: entry.checks.filter((id) => !ran.has(id)) }))
    .filter((row) => row.missing.length);
  const earned = VERDICT_MATRIX.filter(
    (entry) =>
      entry.checks.length &&
      entry.checks.every((id) => ran.has(id)) &&
      !entry.checks.some((id) => red.has(id)),
  );
  return { earned, unearned, unverified, broken, unknownCheckIds, outcomes };
}

export function writeMatrixReport(report: MatrixReport, app = 'jeden'): string {
  mkdirSync(ARTIFACTS, { recursive: true });
  const lines = [
    `# Executable verdict matrix — ${app}`,
    '',
    `earned (checks ran and passed): ${report.earned.map((row) => row.view).join(', ') || '—'}`,
    `BROKEN (parity claimed, check failed): ${
      report.broken.map((row) => `${row.entry.view} [${row.failed.join(' ')}]`).join(', ') || '—'
    }`,
    `unverified (parity claimed, check never ran): ${
      report.unverified.map((row) => `${row.entry.view} [${row.missing.join(' ')}]`).join(', ') || '—'
    }`,
    `unearned (parity claimed, no structural check exists): ${
      report.unearned.map((row) => row.view).join(', ') || '—'
    }`,
    `unknown check ids: ${report.unknownCheckIds.join(', ') || '—'}`,
    '',
    '## check ledger',
    ...report.outcomes.map(
      (outcome) =>
        `${outcome.ok ? 'PASS' : 'FAIL'} [${outcome.app}] ${outcome.id}${
          outcome.detail ? ` — ${outcome.detail}` : ''
        }`,
    ),
    '',
  ];
  const path = join(ARTIFACTS, `verdict-matrix-${app}.txt`);
  writeFileSync(path, lines.join('\n'));
  return path;
}
