import { test, expect } from '@playwright/test';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { HAS_TMUX, TIMEOUTS, TuiSession, checked, settledCapture, watchFor } from './helpers/tui';

/**
 * Named replays of user-reported flows. Each scenario encodes an actual bug
 * report so the regression stays executable long after the fix: the original
 * `/model` hang that ended in ^C, and the agent not knowing it is jeden.
 */

const REPLAY_TIMEOUT_MS = TIMEOUTS.test;

function bramaConfigured(): boolean {
  if (process.env.BRAMA_URL) return true;
  const envPath = join(homedir(), '.jeden', '.env');
  return existsSync(envPath) && readFileSync(envPath, 'utf8').includes('BRAMA_URL=');
}

/** The secret `/token` must never print in full. Read from the same file the
 * app reads, so the assertion compares against the real value. */
function agentSecret(): string | undefined {
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return undefined;
  const [, value] = readFileSync(envPath, 'utf8').match(/^WISENT_APP_AGENT_AUTH_SECRET=(.*)$/m) ?? [];
  return value?.trim().replace(/^["']|["']$/g, '') || undefined;
}

test.describe('jeden replays of user-reported flows', () => {
  let session: TuiSession;

  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
  });

  test.beforeEach(async () => {
    test.setTimeout(REPLAY_TIMEOUT_MS);
    // The reported hang was a cold-catalog fetch; a warm cache would make
    // this replay pass for the wrong reason.
    session = TuiSession.jeden({ warmCache: false });
    const ready = await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready);
    expect(ready.found, 'jeden TUI did not reach the welcome screen').toBe(true);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('replay: /login then /model opens the picker instead of hanging (the ^C report)', async () => {
    if (!bramaConfigured()) test.skip(true, 'brama not configured');
    await session.command('/login', { escapeFirst: false });
    expect((await watchFor(() => session.capture(), /authentication status/i, TIMEOUTS.ready)).found).toBe(true);
    await session.command('/model');
    // The original report: /model sat silent until the user hit ^C. With the
    // background-turn spinner and the disk cache the picker must appear well
    // inside the ready budget even on a cold catalog (isolated HOME).
    const opened = await watchFor(() => session.capture(), /Select model route/, TIMEOUTS.ready);
    expect(
      checked('replay.hang', 'jeden', opened.found, `${opened.ms}ms`),
      `model picker did not open within ${TIMEOUTS.ready}ms of submit`,
    ).toBe(true);
  });

  test('replay: the agent knows it is jeden (identity report)', async () => {
    await session.command('/prompt', { escapeFirst: false });
    const shown = await watchFor(() => session.capture(), /[Jj]eden/, TIMEOUTS.ready);
    expect(
      checked('replay.identity', 'jeden', shown.found),
      '/prompt did not surface the jeden identity',
    ).toBe(true);
  });
});

/**
 * Functional contracts: what a command DOES, not what it paints. The command
 * scan proves every command renders something; these prove a handful of them
 * actually work end to end. Commands whose effect is unobservable without
 * side effects on the operator's machine stay out — and stay marked as
 * behaviourally uncovered in the verdict matrix rather than assumed good.
 */
test.describe('functional contracts — jeden', () => {
  let session: TuiSession;

  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
  });

  test.beforeEach(async () => {
    test.setTimeout(TIMEOUTS.test);
    session = TuiSession.jeden();
    expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('/todo add writes a todo that the reopened view still shows', async () => {
    const marker = `probierz-${Date.now().toString(36)}`;
    await session.command(`/todo add ${marker}`, { escapeFirst: false });
    await settledCapture(session);
    session.key('Escape');
    await settledCapture(session);
    await session.command('/todo');
    const shown = await watchFor(() => session.capture(), new RegExp(marker), TIMEOUTS.settle);
    expect(
      checked('fn.todo-roundtrip', 'jeden', shown.found, marker),
      `/todo add accepted "${marker}" but reopening /todo does not list it`,
    ).toBe(true);
  });

  test('/branch creates a lineage node that /tree lists', async () => {
    const marker = `probe${Date.now().toString(36)}`;
    await session.command(`/branch ${marker}`, { escapeFirst: false });
    await settledCapture(session);
    session.key('Escape');
    await settledCapture(session);
    await session.command('/tree');
    const shown = await watchFor(() => session.capture(), new RegExp(marker), TIMEOUTS.settle);
    expect(
      checked('fn.branch-roundtrip', 'jeden', shown.found, marker),
      `/branch ${marker} reported success but /tree does not show the branch`,
    ).toBe(true);
  });

  test('/token redacts the credential it prints', async () => {
    await session.command('/token', { escapeFirst: false });
    const screen = await settledCapture(session);
    const secret = agentSecret();
    test.skip(!secret, 'no agent secret configured to compare against');
    const leaked = Boolean(secret) && screen.includes(secret as string);
    expect(
      checked('fn.token-redacted', 'jeden', !leaked),
      '/token printed the raw agent secret into the transcript the model reads',
    ).toBe(true);
  });
});
