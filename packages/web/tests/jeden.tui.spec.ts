import { test, expect } from '@playwright/test';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { HAS_TMUX, SLOW_POLL_MS, TIMEOUTS, TuiSession, checked, watchFor } from './helpers/tui';

/**
 * jeden TUI loading states. Verifies that network-bound views show the
 * background-turn spinner BEFORE their content renders, and that a model
 * turn shows the busy spinner before the answer streams in. Requires tmux
 * and a reachable brama control plane; skips cleanly otherwise.
 */
const LOADING_TIMEOUT_MS = TIMEOUTS.test;

const SPINNER = /working…|esc to cancel/;
// Footer chrome is bottom-anchored (always visible); the prompt line scrolls
// off tall boxes. Capital E matches the rendered "Esc close" footer.
const PICKER_CHROME = /Type to search|Esc close/;

function bramaUrl(): string | undefined {
  if (process.env.BRAMA_URL) return process.env.BRAMA_URL;
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return undefined;
  for (const line of readFileSync(envPath, 'utf8').split('\n')) {
    const match = line.match(/^BRAMA_URL=(.*)$/);
    if (match) return match[1].trim().replace(/^["']|["']$/g, '');
  }
  return undefined;
}

async function bramaReachable(url: string): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 5_000);
  try {
    const res = await fetch(`${url.replace(/\/+$/, '')}/health`, { signal: controller.signal });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

test.describe('jeden tui loading states', () => {
  let session: TuiSession;

  test.beforeAll(async () => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
    const url = bramaUrl();
    if (!url || !(await bramaReachable(url))) {
      test.skip(true, 'brama unreachable: BRAMA_URL missing or /health probe failed');
    }
  });

  test.beforeEach(async () => {
    test.setTimeout(LOADING_TIMEOUT_MS);
    // Cold cache on purpose: a warm catalog opens instantly and there is no
    // loading state left to observe.
    session = TuiSession.jeden({ warmCache: false });
    const ready = await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready);
    expect(ready.found, 'jeden TUI did not reach the welcome screen').toBe(true);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('/model shows a loading state before the picker opens', async () => {
    await session.command('/model --all', { escapeFirst: false });
    const spinner = await watchFor(() => session.capture(), SPINNER, TIMEOUTS.spinner);
    const picker = await watchFor(() => session.capture(), PICKER_CHROME, TIMEOUTS.view);
    const ordered = spinner.found && picker.found && spinner.ms <= picker.ms;
    expect(spinner.found, 'no spinner/loading state while the catalog was loading').toBe(true);
    expect(picker.found, 'model picker never opened').toBe(true);
    expect(
      checked('screen.loading-state', 'jeden', ordered, `spinner ${spinner.ms}ms, picker ${picker.ms}ms`),
      'loading state must precede the picker',
    ).toBe(true);
  });

  test('/usage shows a loading state before quota rows render', async () => {
    await session.command('/usage', { escapeFirst: false });
    const spinner = await watchFor(() => session.capture(), SPINNER, TIMEOUTS.spinner);
    const view = await watchFor(() => session.capture(), /Provider usage|quota/, TIMEOUTS.view);
    expect(spinner.found, 'no spinner/loading state while quota was loading').toBe(true);
    expect(view.found, 'usage view never rendered').toBe(true);
    expect(spinner.ms, 'loading state must precede the quota rows').toBeLessThanOrEqual(view.ms);
  });

  test('a model turn shows the busy spinner before the answer', async () => {
    // The answer (Paris) must not appear in the prompt or anywhere in the
    // startup chrome (status line numbers can collide with numeric answers).
    session.submit('Name the capital of France, one word only.');
    const spinner = await watchFor(() => session.capture(), SPINNER, TIMEOUTS.spinner);
    const answer = await watchFor(() => session.capture(), /Paris/, TIMEOUTS.turn, SLOW_POLL_MS);
    expect(spinner.found, 'no busy spinner during the model turn').toBe(true);
    expect(answer.found, 'model answer never arrived').toBe(true);
    expect(spinner.ms, 'busy spinner must precede the answer').toBeLessThanOrEqual(answer.ms);
  });
});
