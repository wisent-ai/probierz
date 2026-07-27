import { test, expect } from '@playwright/test';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import {
  ARTIFACTS,
  HAS_OMP,
  HAS_TMUX,
  PICKER_CHROME,
  SCAN_CHUNK,
  SCAN_PAINT_MS,
  SCAN_TIMEOUT_MS,
  TIMEOUTS,
  TuiSession,
  UNMATCHABLE_QUERY,
  boxFrameCount,
  checked,
  paneGeometry,
  paneTail,
  settledCapture,
  slashCommands,
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
    // The BARE command, because that is what a user types. Asserting the
    // structure of `/model --all` only proved the flag's layout: the plain
    // view stayed a flat list for a full release while this row was green.
    modelCommand: '/model',
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
      // Four independent properties, because "a dot and a bar somewhere" is
      // what the flat list already had: the divider must be part of the frame
      // (┬…┴), hold one column across the rows, the brands column must carry
      // state dots, and the figures must end at a shared right edge.
      const geometry = paneGeometry(session.capture());
      const detail =
        `joined ${geometry.joined}, ${geometry.alignedRows} aligned rows, ` +
        `${geometry.dots} brand dots, ${geometry.alignedMetrics} aligned metric rows`;
      const split =
        geometry.joined &&
        geometry.alignedRows >= MIN_TWO_PANE_ROWS &&
        geometry.dots >= MIN_TWO_PANE_ROWS &&
        geometry.alignedMetrics >= MIN_TWO_PANE_ROWS;
      expect(
        checked('screen.two-pane', app.name, split, detail),
        `${app.name}: model view is not a brands/models split (${detail})`,
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
  /** Did the painted frame open AND close inside the visible pane? */
  fits: boolean;
  /** Picker-only interaction results; '—' when the command paints no picker. */
  navigates: string;
  filters: string;
  closes: string;
  note: string;
}

/** Verbs whose subcommands only read. A discovered subcommand outside this
 * list (purchase, set, uninstall, revoke…) is reported, never executed. */
const READ_VERBS = new Set(['list', 'show', 'status', 'get', 'describe', 'info', 'help', 'dump']);

const ONE_ROW = Number(process.env.PROBIERZ_ONE_ROW ?? '1');

/** Read-only subcommands taken from the dispatcher itself (`rust/slash/mod.rs`
 * status/list arms and `rust/cli/billing.rs::BILLING_SLASH_HANDLERS`). The
 * harvest below adds any the app prints at runtime; this seed exists because
 * jeden's pickers show labels, not the commands behind them, so harvesting
 * alone finds nothing — and the bare `/billing` failure says nothing about
 * whether `/billing policy get` works. */
const SEED_SUBCOMMANDS = [
  '/billing policy get',
  '/subscriptions list',
  '/subscriptions status',
  '/plan status',
  '/goal status',
  '/loop status',
  '/fast status',
  '/advisor status',
  '/approval status',
  '/todo list',
  '/session list',
  '/memory status',
  '/collab status',
  '/roadmap list',
  '/tools --json',
];

/** Does the view currently on screen open AND close inside the pane? Judged
 * on the live screen, never on the diff: a bottom border is byte-identical
 * between frames, so a set difference deletes it and every picker looks
 * broken. The current view is the last frame, hence the last `╭`. */
function frameFits(screen: string): boolean {
  const lines = screen.split('\n');
  const tops = lines.flatMap((line, index) => (line.includes('╭') ? [index] : []));
  const bottoms = lines.flatMap((line, index) => (line.includes('╰') ? [index] : []));
  if (!tops.length && !bottoms.length) return true;
  const lastTop = Math.max(...tops);
  return tops.length > bottoms.length ? false : bottoms.some((index) => index > lastTop);
}

/** Rows a picker actually offers, counted by their badge (`label [BADGE] —
 * detail`). A one-row picker cannot demonstrate navigation or filtering, so
 * asserting either against it manufactures failures. */
function pickerRows(screen: string): number {
  return screen.split('\n').filter((line) => /\[[A-Z][A-Z\d _-]*\]/.test(line)).length;
}

function contentLines(capture: string): number {
  return capture.split('\n').filter((line) => line.trim()).length;
}

/** Slash commands WITH a subcommand that the app itself printed — usage
 * text, error messages, picker rows — in both shapes jeden uses:
 * `/session info` and `Usage: /session [info|delete]`. Harvested instead of
 * guessed, so the scan never invents a surface the app does not document. */
function harvestSubcommands(screen: string, advertised: Set<string>, into: Set<string>): void {
  for (const match of screen.matchAll(/(\/[a-z-]+) ([a-z][a-z-]+)/g)) {
    const [, command, verb] = match;
    if (advertised.has(command) && READ_VERBS.has(verb)) into.add(`${command} ${verb}`);
  }
  for (const match of screen.matchAll(/(\/[a-z-]+) \[([a-z|-]+)\]/g)) {
    const [, command, alternatives] = match;
    if (!advertised.has(command)) continue;
    for (const verb of alternatives.split('|')) {
      if (READ_VERBS.has(verb)) into.add(`${command} ${verb}`);
    }
  }
}

const ERROR_BOX = /╭ error|panicked|internal error/i;

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
    const advertised = new Set(commands);
    const subcommands = new Set<string>();

    async function probe(session: TuiSession, command: string): Promise<ScanRow> {
      const before = session.capture();
      const framesBefore = boxFrameCount(session.captureHistory());
      await session.command(command);
      const painted = await watchFor(
        () => (session.capture() === before ? '' : 'painted'),
        /painted/,
        SCAN_PAINT_MS,
      );
      const screen = await settledCapture(session);
      const newPaint = paintedSince(before, screen);
      harvestSubcommands(screen, advertised, subcommands);
      const { status, note } = painted.found
        ? classify(screen, newPaint)
        : { status: 'silent' as ScanStatus, note: 'screen never changed' };

      // Picker interaction: move, search, close. Enter is never pressed —
      // confirming a row runs the command behind it, side effects and all.
      // A picker with a single row cannot demonstrate movement or filtering,
      // so it reports n/a instead of a manufactured failure.
      let navigates = '—';
      let filters = '—';
      let closes = '—';
      if (status === 'picker') {
        const multiRow = pickerRows(screen) > ONE_ROW;
        // A two-pane picker opens on the brands column, so step right first:
        // otherwise ↓ walks the brands and the item cursor never moves — the
        // interaction the footer promises is the one to measure.
        if (paneGeometry(screen).joined) {
          session.key('Right');
          await settledCapture(session);
        }
        const cursorLine = (frame: string) =>
          frame.split('\n').find((line) => line.includes('›')) ?? '';
        const cursorBefore = cursorLine(session.capture());
        session.key('Down');
        const moved = await settledCapture(session);
        navigates = multiRow
          ? cursorBefore && cursorLine(moved) !== cursorBefore
            ? 'yes'
            : 'NO'
          : 'n/a';
        // Rows carry badges; an unmatchable query must leave none standing.
        // Counting all visible lines cannot see this in a two-pane frame,
        // where the brands column keeps its rows whatever the query is.
        const badgesBefore = pickerRows(moved);
        session.type(UNMATCHABLE_QUERY);
        const filtered = await settledCapture(session);
        filters = multiRow ? (pickerRows(filtered) < badgesBefore ? 'yes' : 'NO') : 'n/a';
        session.key('C-u');
        await settledCapture(session);
        session.key('Escape');
        const closed = await settledCapture(session);
        closes = PICKER_CHROME.test(closed) ? 'NO' : 'yes';
      }
      return {
        command,
        status,
        paintMs: painted.ms,
        frames: boxFrameCount(session.captureHistory()) - framesBefore,
        fits: painted.found ? frameFits(screen) : false,
        navigates,
        filters,
        closes,
        note,
      };
    }

    for (const chunk of chunks.values()) {
      const session = TuiSession.jeden();
      try {
        const ready = await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready);
        expect(ready.found, 'jeden did not reach its welcome screen').toBe(true);
        for (const command of chunk) {
          if (SCAN_SKIP[command]) continue;
          rows.push(await probe(session, command));
        }
      } finally {
        session.kill();
      }
    }

    // Second pass: read-only subcommands — the dispatcher-derived seed plus
    // anything the app printed at us during the first pass.
    const subRows: ScanRow[] = [];
    const subList = [...new Set([...SEED_SUBCOMMANDS, ...subcommands])];
    const subChunks = new Map<number, string[]>();
    for (const [index, sub] of subList.entries()) {
      const bucket = Math.floor(index / SCAN_CHUNK);
      subChunks.set(bucket, [...(subChunks.get(bucket) ?? []), sub]);
    }
    for (const chunk of subChunks.values()) {
      const session = TuiSession.jeden();
      try {
        expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
        for (const sub of chunk) {
          subRows.push(await probe(session, sub));
        }
      } finally {
        session.kill();
      }
    }

    const tableHeader = [
      '| command | status | paint ms | frames | fits | ↑↓ | search | esc | note |',
      '|---|---|---|---|---|---|---|---|---|',
    ];
    const asRow = (row: ScanRow) =>
      `| ${row.command} | ${row.status} | ${row.paintMs} | ${row.frames} | ${row.fits ? 'yes' : 'NO'} | ` +
      `${row.navigates} | ${row.filters} | ${row.closes} | ${row.note} |`;
    const report = [
      `# jeden command-surface scan — ${new Date().toISOString()}`,
      '',
      `scanned ${rows.length} of ${commands.length} advertised commands, plus ${subRows.length} read-only subcommands`,
      '',
      '## bare commands',
      ...tableHeader,
      ...rows.map(asRow),
      ...Object.entries(SCAN_SKIP).map(
        ([command, why]) => `| ${command} | skipped | — | — | — | — | — | — | ${why} |`,
      ),
      '',
      '## read-only subcommands the app documented',
      ...tableHeader,
      ...subRows.map(asRow),
      '',
    ].join('\n');
    mkdirSync(ARTIFACTS, { recursive: true });
    const reportPath = join(ARTIFACTS, 'command-scan.md');
    writeFileSync(reportPath, report);
    console.log(`[command-scan] ${reportPath}`);

    const all = [...rows, ...subRows];
    const silent = all.filter((row) => row.status === 'silent').map((row) => row.command);
    const errored = all.filter((row) => row.status === 'error');
    const pickers = all.filter((row) => row.status === 'picker');
    console.log(
      `[command-scan] ${all.length} probed (${rows.length} bare + ${subRows.length} sub) · ` +
        `${pickers.length} pickers · ${errored.length} errors · ${silent.length} silent`,
    );
    if (errored.length) {
      console.log(
        `[command-scan] errors: ${errored.map((row) => `${row.command} (${row.note})`).join('; ')}`,
      );
    }

    // Soft: one hard assertion would abort the test and the checks below it
    // would never reach the ledger, so a single red row could hide five more.
    expect
      .soft(
        checked('scan.no-silent-commands', 'jeden', !silent.length, `${silent.length} silent`),
        `these commands put nothing on the screen: ${silent.join(', ')}`,
      )
      .toBe(true);
    const panics = all.filter((row) => /panic/i.test(row.note)).map((row) => row.command);
    expect
      .soft(
        checked('scan.no-panics', 'jeden', !panics.length, panics.join(' ')),
        `these commands panicked: ${panics.join(', ')}`,
      )
      .toBe(true);
    // Advertising a command in /help that the dispatcher does not route is a
    // broken promise to the user, not a mere error message.
    const unrouted = all
      .filter((row) => /unknown .*command|no such command/i.test(row.note))
      .map((row) => row.command);
    expect
      .soft(
        checked('scan.no-unrouted-commands', 'jeden', !unrouted.length, unrouted.join(' ')),
        `/help advertises these commands but the dispatcher rejects them: ${unrouted.join(', ')}`,
      )
      .toBe(true);
    const overflowing = all.filter((row) => !row.fits).map((row) => row.command);
    expect
      .soft(
        checked('ui.frame-fits', 'jeden', !overflowing.length, overflowing.join(' ')),
        `these views opened off-screen (top border above the viewport): ${overflowing.join(', ')}`,
      )
      .toBe(true);
    const stuck = all.filter((row) => row.navigates === 'NO').map((row) => row.command);
    expect
      .soft(
        checked('ui.picker-navigation', 'jeden', !stuck.length, stuck.join(' ')),
        `↑↓ moved no cursor in these pickers: ${stuck.join(', ')}`,
      )
      .toBe(true);
    const unfiltered = all.filter((row) => row.filters === 'NO').map((row) => row.command);
    expect
      .soft(
        checked('ui.picker-search', 'jeden', !unfiltered.length, unfiltered.join(' ')),
        `typing an unmatchable query filtered nothing in these pickers: ${unfiltered.join(', ')}`,
      )
      .toBe(true);
    const unclosable = all.filter((row) => row.closes === 'NO').map((row) => row.command);
    expect
      .soft(
        checked('ui.picker-close', 'jeden', !unclosable.length, unclosable.join(' ')),
        `Esc did not close these pickers: ${unclosable.join(', ')}`,
      )
      .toBe(true);
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
