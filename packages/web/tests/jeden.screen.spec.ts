import { test, expect } from '@playwright/test';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import {
  ARTIFACTS,
  HAS_OMP,
  HAS_TMUX,
  SCAN_CHUNK,
  SCAN_PAINT_MS,
  SCAN_TIMEOUT_MS,
  TIMEOUTS,
  TuiSession,
  boxFrameCount,
  checked,
  paneTail,
  settledCapture,
  slashCommands,
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

/**
 * Full command-surface scan. Every slash command the binary advertises is
 * driven in a live TUI and classified by what it puts on the screen. The
 * per-view contracts above cover five views deeply; this covers all ~70
 * shallowly, so a command that renders nothing, panics, or errors out
 * cannot hide in the long tail.
 */

/** Commands excluded from the scan, with the reason printed in the report —
 * a silent skip is how a scan starts lying. */
const SCAN_SKIP: Record<string, string> = {
  '/update': 'runs the automated self-update',
  '/rebuild': 'rebuilds the binary and restarts the session',
  '/refresh': 'mutates live Weles credentials other tests depend on',
  '/compact': 'spawns a model turn (quota + minutes)',
  '/btw': 'spawns a model turn (quota + minutes)',
  '/exit': 'terminates the session; covered by its own contract',
};

type ScanStatus = 'picker' | 'text' | 'error' | 'silent';

interface ScanRow {
  command: string;
  status: ScanStatus;
  paintMs: number;
  frames: number;
  note: string;
}

const ERROR_BOX = /╭ error|panicked|internal error/i;
const PICKER_CHROME = /Esc close|Type to search/;

/** Only what THIS command painted. jeden keeps earlier frames on screen, so
 * classifying the whole pane makes one error box mark every later command as
 * failing — the first version of this scan did exactly that. */
function paintedSince(before: string, after: string): string {
  const seen = new Set(before.split('\n'));
  return after
    .split('\n')
    .filter((line) => line.trim() && !seen.has(line))
    .join('\n');
}

/** Two signals, two sources — mixing them is how the first two versions of
 * this scan lied. An ERROR is judged on new paint only, because jeden leaves
 * old error frames on screen. A PICKER is judged on the live screen, because
 * picker chrome is byte-identical between views and a set difference erases
 * it — and every command is preceded by Esc, so an open picker belongs to
 * the command just submitted. */
function classify(screen: string, newPaint: string): { status: ScanStatus; note: string } {
  if (ERROR_BOX.test(newPaint)) {
    const lines = newPaint.split('\n');
    const title = lines.findIndex((row) => ERROR_BOX.test(row));
    const body = lines.slice(title).find((row) => /│/.test(row));
    return { status: 'error', note: (body ?? '').replace(/[│╭╮╯╰─]/g, '').trim() };
  }
  if (PICKER_CHROME.test(screen)) return { status: 'picker', note: '' };
  const complaint = newPaint.split('\n').find((row) => /cannot|failed|unknown|not found/i.test(row));
  return { status: 'text', note: (complaint ?? '').replace(/[│╭╮╯╰─]/g, '').trim() };
}

test.describe('command surface scan — jeden', () => {
  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
    if (!bramaReachableSync()) test.skip(true, 'brama not configured');
  });

  test('every advertised command paints something, and nothing panics', async () => {
    test.setTimeout(SCAN_TIMEOUT_MS);
    const commands = slashCommands();
    expect(commands, '/help advertised no commands — the scan would be vacuous').not.toEqual([]);

    // A fresh session every SCAN_CHUNK commands bounds mode/state bleed
    // (/plan, /fast and friends are toggles) without paying app startup ~70x.
    const chunks = new Map<number, string[]>();
    for (const [index, command] of commands.entries()) {
      const bucket = Math.floor(index / SCAN_CHUNK);
      chunks.set(bucket, [...(chunks.get(bucket) ?? []), command]);
    }

    const rows: ScanRow[] = [];
    for (const chunk of chunks.values()) {
      const session = TuiSession.jeden();
      try {
        const ready = await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready);
        expect(ready.found, 'jeden did not reach its welcome screen').toBe(true);
        for (const command of chunk) {
          if (SCAN_SKIP[command]) continue;
          const before = session.capture();
          const framesBefore = boxFrameCount(session.captureHistory());
          await session.command(command);
          const painted = await watchFor(
            () => (session.capture() === before ? '' : 'painted'),
            /painted/,
            SCAN_PAINT_MS,
          );
          const screen = await settledCapture(session);
          const { status, note } = painted.found
            ? classify(screen, paintedSince(before, screen))
            : { status: 'silent' as ScanStatus, note: 'screen never changed' };
          rows.push({
            command,
            status,
            paintMs: painted.ms,
            frames: boxFrameCount(session.captureHistory()) - framesBefore,
            note,
          });
        }
      } finally {
        session.kill();
      }
    }

    const skipped = Object.entries(SCAN_SKIP).map(
      ([command, why]) => `| ${command} | skipped | — | — | ${why} |`,
    );
    const report = [
      `# jeden command-surface scan — ${new Date().toISOString()}`,
      '',
      `scanned ${rows.length} of ${commands.length} advertised commands`,
      '',
      '| command | status | paint ms | frames added | note |',
      '|---|---|---|---|---|',
      ...rows.map(
        (row) => `| ${row.command} | ${row.status} | ${row.paintMs} | ${row.frames} | ${row.note} |`,
      ),
      ...skipped,
      '',
    ].join('\n');
    mkdirSync(ARTIFACTS, { recursive: true });
    const reportPath = join(ARTIFACTS, 'command-scan.md');
    writeFileSync(reportPath, report);
    console.log(`[command-scan] ${reportPath}`);

    const silent = rows.filter((row) => row.status === 'silent').map((row) => row.command);
    const errored = rows.filter((row) => row.status === 'error');
    const pickers = rows.filter((row) => row.status === 'picker');
    console.log(
      `[command-scan] ${rows.length} scanned · ${pickers.length} pickers · ` +
        `${rows.length - pickers.length - errored.length - silent.length} text · ` +
        `${errored.length} errors · ${silent.length} silent`,
    );
    if (errored.length) {
      console.log(
        `[command-scan] errors: ${errored.map((row) => `${row.command} (${row.note})`).join('; ')}`,
      );
    }

    expect(
      checked('scan.no-silent-commands', 'jeden', !silent.length, `${silent.length} silent`),
      `these commands put nothing on the screen: ${silent.join(', ')}`,
    ).toBe(true);
    const panics = rows.filter((row) => /panic/i.test(row.note)).map((row) => row.command);
    expect(
      checked('scan.no-panics', 'jeden', !panics.length, panics.join(' ')),
      `these commands panicked: ${panics.join(', ')}`,
    ).toBe(true);
    // Advertising a command in /help that the dispatcher does not route is a
    // broken promise to the user, not a mere error message.
    const unrouted = rows
      .filter((row) => /unknown .*command|no such command/i.test(row.note))
      .map((row) => row.command);
    expect(
      checked('scan.no-unrouted-commands', 'jeden', !unrouted.length, unrouted.join(' ')),
      `/help advertises these commands but the dispatcher rejects them: ${unrouted.join(', ')}`,
    ).toBe(true);
  });

  test('/exit terminates the session', async () => {
    test.setTimeout(TIMEOUTS.test);
    const session = TuiSession.jeden();
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await session.command('/exit');
      const gone = await watchFor(() => (session.alive() ? '' : 'gone'), /gone/, TIMEOUTS.settle);
      expect(gone.found, '/exit left the session running').toBe(true);
    } finally {
      session.kill();
    }
  });
});
