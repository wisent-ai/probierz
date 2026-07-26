import { test, expect } from '@playwright/test';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import {
  HAS_OMP,
  HAS_TMUX,
  TIMEOUTS,
  TuiSession,
  boxFrameCount,
  checked,
  paneTail,
  twoPaneDividerRows,
  watchFor,
} from './helpers/tui';

/**
 * Screen-semantics contracts, run identically against jeden and omp (the
 * reference control). These assert HOW the screen behaves, not WHAT text it
 * contains: overlays must fit the viewport, replace each other instead of
 * appending to the transcript, keep transcript growth bounded, and the model
 * picker must have a two-pane structure. A red row here is a parity gap that
 * content-level tests cannot see; a red row on the omp side means the
 * harness itself is miscalibrated.
 */

const FRAME_BUDGET_AFTER_TWO_OVERLAYS = Number(process.env.PROBIERZ_FRAME_BUDGET ?? '4');
const MIN_TWO_PANE_ROWS = Number(process.env.PROBIERZ_MIN_TWO_PANE_ROWS ?? '3');

interface AppProfile {
  name: 'jeden' | 'omp';
  start: () => TuiSession;
  ready: RegExp;
  modelCommand: string;
  modelTitle: RegExp;
  modelFooter: RegExp;
  settingsCommand: string;
  settingsTitle: RegExp;
}

function bramaReachableSync(): boolean {
  if (process.env.BRAMA_URL) return true;
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return false;
  return readFileSync(envPath, 'utf8').includes('BRAMA_URL=');
}

const PROFILES: AppProfile[] = [
  {
    name: 'jeden',
    start: () => TuiSession.jeden(),
    ready: /Tips|Welcome back/,
    modelCommand: '/model --all',
    modelTitle: /Select model route/,
    modelFooter: /Esc close/,
    settingsCommand: '/settings',
    settingsTitle: /Jeden settings/,
  },
  {
    name: 'omp',
    start: () => TuiSession.omp(),
    ready: /Tips|sourcekit|sessions/i,
    modelCommand: '/models',
    modelTitle: /All available models|Roles/,
    modelFooter: /Esc close/,
    settingsCommand: '/settings',
    settingsTitle: /Appearance/,
  },
];

for (const app of PROFILES) {
  test.describe(`screen semantics — ${app.name}`, () => {
    let session: TuiSession;

    test.beforeAll(() => {
      if (!HAS_TMUX) test.skip(true, 'tmux not installed');
      if (app.name === 'omp' && !HAS_OMP) test.skip(true, 'omp not installed');
      if (app.name === 'jeden' && !bramaReachableSync()) {
        test.skip(true, 'brama not configured');
      }
    });

    test.beforeEach(async () => {
      test.setTimeout(TIMEOUTS.test);
      session = app.start();
      const ready = await watchFor(() => session.capture(), app.ready, TIMEOUTS.ready);
      expect(ready.found, `${app.name} did not reach its ready screen`).toBe(true);
    });

    test.afterEach(() => {
      session?.kill();
    });

    test('overlay keeps its title and footer inside the viewport', async () => {
      await session.command(app.modelCommand, { escapeFirst: false });
      const opened = await watchFor(() => session.capture(), app.modelTitle, TIMEOUTS.view);
      expect(
        opened.found,
        `${app.name} model view never opened; last screen:\n${paneTail(session.capture())}`,
      ).toBe(true);
      const capture = session.capture();
      const fits = app.modelTitle.test(capture) && app.modelFooter.test(capture);
      const detail = `title ${app.modelTitle.test(capture)}, footer ${app.modelFooter.test(capture)}`;
      expect(
        checked('screen.geometry', app.name, fits, detail),
        `${app.name}: title and footer must be visible in the same frame (${detail}) — the view is taller than the viewport`,
      ).toBe(true);
    });

    test('a new overlay REPLACES the previous one instead of appending', async () => {
      await session.command(app.modelCommand, { escapeFirst: false });
      expect(
        (await watchFor(() => session.capture(), app.modelTitle, TIMEOUTS.view)).found,
        `${app.name} model view never opened;\n${paneTail(session.capture())}`,
      ).toBe(true);
      await session.command(app.settingsCommand);
      expect(
        (await watchFor(() => session.capture(), app.settingsTitle, TIMEOUTS.view)).found,
        `${app.name} settings view never opened;\n${paneTail(session.capture())}`,
      ).toBe(true);
      // Scrollback, not the visible window: an old frame that merely scrolled
      // out of sight is still appended transcript, not a replaced view.
      const replaced = !app.modelTitle.test(session.captureHistory());
      expect(
        checked('screen.replacement', app.name, replaced),
        `${app.name}: the previous view (${app.modelTitle}) is still in the transcript after opening settings — overlays append instead of replacing`,
      ).toBe(true);
    });

    test('transcript growth stays bounded across overlays', async () => {
      await session.command(app.modelCommand, { escapeFirst: false });
      expect(
        (await watchFor(() => session.capture(), app.modelTitle, TIMEOUTS.view)).found,
        `${app.name} model view never opened;\n${paneTail(session.capture())}`,
      ).toBe(true);
      await session.command(app.settingsCommand);
      expect(
        (await watchFor(() => session.capture(), app.settingsTitle, TIMEOUTS.view)).found,
        `${app.name} settings view never opened;\n${paneTail(session.capture())}`,
      ).toBe(true);
      const frames = boxFrameCount(session.captureHistory());
      const bounded = frames <= FRAME_BUDGET_AFTER_TWO_OVERLAYS;
      expect(
        checked('screen.transcript-budget', app.name, bounded, `${frames} frames`),
        `${app.name}: ${frames} boxed frames in the transcript after two overlays (budget ${FRAME_BUDGET_AFTER_TWO_OVERLAYS}) — the transcript grows with every command`,
      ).toBe(true);
    });

    test('the model view has a two-pane structure (brands left, models right)', async () => {
      await session.command(app.modelCommand, { escapeFirst: false });
      expect(
        (await watchFor(() => session.capture(), app.modelTitle, TIMEOUTS.view)).found,
        `${app.name} model view never opened;\n${paneTail(session.capture())}`,
      ).toBe(true);
      const dividerRows = twoPaneDividerRows(session.capture());
      const split = dividerRows >= MIN_TWO_PANE_ROWS;
      expect(
        checked('screen.two-pane', app.name, split, `${dividerRows} divider rows`),
        `${app.name}: ${dividerRows} two-pane divider rows found (need ≥ ${MIN_TWO_PANE_ROWS}) — the model view is a flat list, not a brands/models split`,
      ).toBe(true);
    });
  });
}
