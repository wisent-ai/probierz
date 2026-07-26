import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * jeden TUI loading states, driven through tmux (same technique as
 * reference/ui-peek.sh). Verifies that network-bound views show the
 * background-turn spinner BEFORE their content renders, and that a model
 * turn shows the busy spinner before the answer streams in. Requires tmux
 * and a reachable brama control plane; skips cleanly otherwise.
 */
const JEDEN = process.env.JEDEN_BIN || 'jeden';
const TEST_TIMEOUT_MS = 180_000;

const HAS_TMUX = (() => {
  try {
    execFileSync('tmux', ['-V'], { encoding: 'utf8' });
    return true;
  } catch {
    return false;
  }
})();

function tmux(args: string[]): string {
  return execFileSync('tmux', args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
}

function tmpCwd(): string {
  return mkdtempSync(join(tmpdir(), 'jeden-tui-'));
}

class TuiSession {
  readonly name = `probierz-tui-${process.pid}-${Math.random().toString(36).slice(2, 8)}`;

  start(): void {
    tmux(['new-session', '-d', '-s', this.name, '-x', '200', '-y', '50', `${JEDEN} --cwd ${tmpCwd()}`]);
  }

  submit(text: string): void {
    tmux(['send-keys', '-t', this.name, '-l', text]);
    tmux(['send-keys', '-t', this.name, 'Enter']);
  }

  capture(): string {
    return tmux(['capture-pane', '-t', this.name, '-p']);
  }

  kill(): void {
    try {
      tmux(['kill-session', '-t', this.name]);
    } catch {
      // already gone
    }
  }
}

/** Milliseconds until `pattern` first appears in the pane, or -1 on timeout. */
async function waitFor(
  capture: () => string,
  pattern: RegExp,
  timeoutMs: number,
  intervalMs = 200,
): Promise<number> {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (pattern.test(capture())) return Date.now() - started;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  return -1;
}

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
    if (!HAS_TMUX) {
      test.skip(true, 'tmux not installed');
    }
    const url = bramaUrl();
    if (!url || !(await bramaReachable(url))) {
      test.skip(true, 'brama unreachable: BRAMA_URL missing or /health probe failed');
    }
  });

  test.beforeEach(async () => {
    test.setTimeout(TEST_TIMEOUT_MS);
    session = new TuiSession();
    session.start();
    const ready = await waitFor(() => session.capture(), /Tips|Welcome back/, 30_000);
    expect(ready, 'jeden TUI did not reach the welcome screen').toBeGreaterThanOrEqual(0);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('/model shows a loading state before the picker opens', async () => {
    session.submit('/model --all');
    const spinnerMs = await waitFor(() => session.capture(), SPINNER, 20_000);
    const pickerMs = await waitFor(() => session.capture(), PICKER_CHROME, 40_000);
    expect(spinnerMs, 'no spinner/loading state while the catalog was loading').toBeGreaterThanOrEqual(0);
    expect(pickerMs, 'model picker never opened').toBeGreaterThanOrEqual(0);
    expect(spinnerMs, 'loading state must precede the picker').toBeLessThanOrEqual(pickerMs);
  });

  test('/usage shows a loading state before quota rows render', async () => {
    session.submit('/usage');
    const spinnerMs = await waitFor(() => session.capture(), SPINNER, 20_000);
    const viewMs = await waitFor(() => session.capture(), /Provider usage|quota/, 40_000);
    expect(spinnerMs, 'no spinner/loading state while quota was loading').toBeGreaterThanOrEqual(0);
    expect(viewMs, 'usage view never rendered').toBeGreaterThanOrEqual(0);
    expect(spinnerMs).toBeLessThanOrEqual(viewMs);
  });

  test('a model turn shows the busy spinner before the answer', async () => {
    // The answer (Paris) must not appear in the prompt or anywhere in the
    // startup chrome (status line numbers can collide with numeric answers).
    session.submit('Name the capital of France, one word only.');
    const spinnerMs = await waitFor(() => session.capture(), SPINNER, 20_000);
    const answerMs = await waitFor(() => session.capture(), /Paris/, 90_000, 500);
    expect(spinnerMs, 'no busy spinner during the model turn').toBeGreaterThanOrEqual(0);
    expect(answerMs, 'model answer never arrived').toBeGreaterThanOrEqual(0);
    expect(spinnerMs).toBeLessThanOrEqual(answerMs);
  });
});
