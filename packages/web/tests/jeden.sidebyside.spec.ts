import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import {
  ARTIFACTS,
  HAS_OMP,
  HAS_TMUX,
  TIMEOUTS,
  TuiSession,
  checked,
  compareToGolden,
  escapeHtml,
  watchFor,
} from './helpers/tui';

/**
 * Side-by-side walkthrough: the same interaction script executed on jeden
 * and omp at identical geometry, producing a reviewable HTML report per run
 * (not a pixel diff — the apps differ by design — but an artifact a human
 * can flip through in 30 seconds). jeden steps also go through golden-PNG
 * regression with volatile values masked. Regenerate goldens with
 * PROBIERZ_UPDATE_GOLDEN=1.
 */

interface Step {
  label: string;
  command?: string;
  awaitPattern: RegExp;
  timeoutMs?: number;
  /** Golden-PNG regression only makes sense on steps whose content is ours,
   * not the live model catalog: that list reorders whenever brama's upstream
   * changes, which is a data update, not a UI regression. */
  golden?: boolean;
}

const STEPS: Step[] = [
  { label: 'welcome', awaitPattern: /Tips|Welcome back/i, timeoutMs: TIMEOUTS.ready, golden: true },
  { label: 'model-view', command: '/model --all', awaitPattern: /Select model route/, timeoutMs: TIMEOUTS.view },
  { label: 'settings-view', command: '/settings', awaitPattern: /Jeden settings/, timeoutMs: TIMEOUTS.ready, golden: true },
];

const OMP_STEPS: Step[] = [
  { label: 'welcome', awaitPattern: /Tips|sessions/i, timeoutMs: TIMEOUTS.ready },
  { label: 'model-view', command: '/models', awaitPattern: /All available models|Roles/, timeoutMs: TIMEOUTS.view },
  { label: 'settings-view', command: '/settings', awaitPattern: /Appearance/, timeoutMs: TIMEOUTS.ready },
];

function bramaConfigured(): boolean {
  if (process.env.BRAMA_URL) return true;
  const envPath = join(homedir(), '.jeden', '.env');
  return existsSync(envPath) && readFileSync(envPath, 'utf8').includes('BRAMA_URL=');
}

async function walkthrough(session: TuiSession, steps: Step[], tag: string): Promise<Map<string, string>> {
  const captures = new Map<string, string>();
  for (const step of steps) {
    if (step.command) await session.command(step.command);
    const seen = await watchFor(() => session.capture(), step.awaitPattern, step.timeoutMs ?? TIMEOUTS.view);
    console.log(`[sidebyside:${tag}] ${step.label} -> ${seen.found ? `${seen.ms}ms` : 'NOT SEEN'}`);
    expect(seen.found, `${tag} step ${step.label}: ${step.awaitPattern} never appeared`).toBe(true);
    captures.set(step.label, session.capture());
  }
  return captures;
}

test.describe('side-by-side jeden vs omp', () => {
  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
    if (!HAS_OMP) test.skip(true, 'omp not installed');
    if (!bramaConfigured()) test.skip(true, 'brama not configured');
  });

  test('walkthrough report + jeden golden regression', async ({ page }) => {
    test.setTimeout(TIMEOUTS.walkthrough);
    const jeden = TuiSession.jeden();
    const omp = TuiSession.omp();
    try {
      // allSettled, not all: with Promise.all the first rejection reaches
      // `finally`, both sessions die, and the survivor's next capture fails
      // with tmux "can't find pane" — masking the assertion that broke.
      const [jedenRun, ompRun] = await Promise.allSettled([
        walkthrough(jeden, STEPS, 'jeden'),
        walkthrough(omp, OMP_STEPS, 'omp'),
      ]);
      const jedenCaptures = jedenRun.status === 'fulfilled' ? jedenRun.value : new Map<string, string>();
      const ompCaptures = ompRun.status === 'fulfilled' ? ompRun.value : new Map<string, string>();

      const outDir = join(ARTIFACTS, 'sidebyside');
      mkdirSync(outDir, { recursive: true });
      const rows: string[] = [];
      const goldenFailures: string[] = [];
      for (const step of STEPS) {
        const jedenText = jedenCaptures.get(step.label) ?? '(no capture)';
        const ompText = ompCaptures.get(step.label) ?? '(no capture)';
        rows.push(
          `<h2>${escapeHtml(step.label)}</h2><div class="row">` +
            `<div><h3>jeden</h3><pre>${escapeHtml(jedenText)}</pre></div>` +
            `<div><h3>omp</h3><pre>${escapeHtml(ompText)}</pre></div></div>`,
        );
        if (!step.golden || !jedenCaptures.has(step.label)) continue;
        const golden = await compareToGolden(page, `jeden-${step.label}`, jedenText);
        if (golden.wroteGolden) {
          console.log(`[golden] wrote ${golden.goldenPath}`);
          continue;
        }
        checked('golden.stable', 'jeden', golden.match, `${step.label}: ${golden.diffPixels} px`);
        if (!golden.match) {
          goldenFailures.push(`${step.label} (${golden.diffPixels} px differ, see ${join(ARTIFACTS, 'golden')})`);
        }
      }

      const html =
        `<!doctype html><meta charset="utf-8"><title>jeden vs omp — side by side</title>` +
        `<style>body{font:13px/1.35 'Menlo',monospace;background:#0d1117;color:#e6edf3;padding:1em}` +
        `.row{display:flex;gap:1em}pre{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:1em;overflow:auto;max-width:49vw;white-space:pre}</style>` +
        `<h1>jeden vs omp — ${new Date().toISOString()}</h1>${rows.join('')}`;
      writeFileSync(join(outDir, 'index.html'), html);
      console.log(`[sidebyside] report: ${join(outDir, 'index.html')}`);

      // Report first, assert second: the artifact a human reviews must exist
      // even (especially) on the runs where a walkthrough broke.
      const walkthroughErrors = [jedenRun, ompRun]
        .filter((result) => result.status === 'rejected')
        .map((result) => String((result as PromiseRejectedResult).reason));
      expect(walkthroughErrors, walkthroughErrors.join(' | ')).toEqual([]);
      expect(goldenFailures, `golden regressions: ${goldenFailures.join(', ')}`).toEqual([]);
    } finally {
      jeden.kill();
      omp.kill();
    }
  });
});
