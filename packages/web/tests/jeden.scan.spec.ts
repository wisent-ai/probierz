import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  ARTIFACTS,
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
  settledCapture,
  slashCommands,
  watchFor,
} from './helpers/tui';
import { bramaReachableSync } from './helpers/jeden-profiles';
import {
  ONE_ROW,
  SCAN_SKIP,
  SEED_SUBCOMMANDS,
  type ScanRow,
  type ScanStatus,
  classify,
  frameFits,
  harvestSubcommands,
  paintedSince,
  pickerRows,
} from './helpers/jeden-scan';

/**
 * The command surface scan: every slash command the app itself advertises,
 * opened in turn, with what happened recorded. Asking the app for its own
 * command list means a command added tomorrow is scanned tomorrow.
 */
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
