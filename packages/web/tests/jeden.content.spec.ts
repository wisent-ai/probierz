import { test, expect } from '@playwright/test';
import {
  HAS_TMUX,
  TIMEOUTS,
  TuiSession,
  checked,
  flattened,
  paneTail,
  watchFor,
} from './helpers/tui';

/**
 * Read-only views must contain their own subject matter. Weak individually,
 * strong together: paired with the command scan (renders, no error, fits,
 * navigable) it is the difference between "a box appeared" and "the box is
 * about the thing the command promises".
 */
const CONTENT_CONTRACTS: [command: string, expected: RegExp][] = [
  ['/help', /\/model/],
  ['/hotkeys', /Enter|Esc/],
  ['/context', /token/i],
  ['/status', /capabilit|health|allow|ask/i],
  ['/tools', /read|write|command|search/i],
  ['/prompt', /[Jj]eden/],
  ['/login', /account|Weles|authentication/i],
  ['/usage', /usage|quota|token|event/i],
  ['/roles', /default|fast|advisor|role/i],
  ['/agents', /agent/i],
  ['/jobs', /job/i],
  ['/session', /session/i],
  ['/memory', /memory|queue|index/i],
  ['/hooks', /hook/i],
  ['/extensions', /extension|plugin|discover/i],
  ['/plugins', /plugin/i],
  ['/marketplace', /marketplace|catalog|plugin/i],
  ['/mcp', /mcp|server/i],
  ['/ssh', /ssh|host/i],
  ['/browser', /browser|runtime|chrom/i],
  ['/changelog', /\d|release|change/i],
  ['/settings', /tools\.|ui\.|context\.|secrets\./],
  ['/model', /claude|codex|kimi|catalog|model/i],
  ['/approval', /approval|ask|allow|deny/i],
  ['/collab status', /collab|host|guest|relay/i],
  ['/fast status', /fast mode/i],
  ['/plan status', /plan/i],
  ['/stats', /stat|usage|dashboard|report/i],
];

test.describe('content contracts — jeden', () => {
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

  test('every read-only view contains its own subject matter', async () => {
    const missing: string[] = [];
    for (const [command, expected] of CONTENT_CONTRACTS) {
      await session.command(command);
      // Matched against de-wrapped text: content that hits the frame edge
      // continues on the next row, and a raw match would call it absent.
      const seen = await watchFor(() => flattened(session.capture()), expected, TIMEOUTS.settle);
      if (!seen.found) {
        missing.push(`${command} (no ${expected})`);
        console.log(`[content-contract] ${command} missed ${expected}:\n${paneTail(session.capture())}`);
      }
    }
    expect(
      checked('fn.view-content', 'jeden', !missing.length, missing.join('; ')),
      `these views rendered without their own subject matter: ${missing.join(', ')}`,
    ).toEqual(true);
  });
});
