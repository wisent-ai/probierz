import { test, expect } from '@playwright/test';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  ARTIFACTS,
  HAS_TMUX,
  type JourneyStepResult,
  PICKER_CHROME,
  TIMEOUTS,
  TuiSession,
  brandRow,
  checked,
  cursorModelId,
  cursorRow,
  paneGeometry,
  paneTail,
  runJourney,
  settledCapture,
  watchFor,
} from './helpers/tui';
import { bramaReachableSync } from './helpers/jeden-profiles';
/**
 * Interaction journeys. The contracts above grade a screen; these grade what
 * the keys DO across screens — cross the panes, walk a list, choose a model,
 * land back with the choice applied. A layout that looks right and answers to
 * nothing passes every static check ever written.
 */
test.describe('interaction journeys — jeden', () => {
  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
    if (!bramaReachableSync()) test.skip(true, 'brama not configured');
  });

  test('arrows cross the panes and the cursor follows', async () => {
    test.setTimeout(TIMEOUTS.test);
    const session = TuiSession.jeden();
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      const journey = await runJourney(session, [
        {
          label: 'open the model hub',
          command: '/model',
          expect: (after) => /Select model route/.test(after) && paneGeometry(after).joined,
        },
        {
          label: 'brands hold the keyboard first',
          key: 'Down',
          expect: (after, before) => brandRow(after) !== brandRow(before) && !cursorRow(after),
        },
        {
          label: '→ hands the keyboard to the models',
          key: 'Right',
          expect: (after) => Boolean(cursorRow(after)) && !brandRow(after),
        },
        {
          label: '↓ walks the models, not the brands',
          key: 'Down',
          expect: (after, before) => cursorRow(after) !== cursorRow(before) && !brandRow(after),
        },
        {
          label: '← hands it back to the brands',
          key: 'Left',
          expect: (after) => Boolean(brandRow(after)) && !cursorRow(after),
        },
        {
          label: 'a different brand narrows the model pane',
          key: 'Down',
          expect: (after, before) => brandRow(after) !== brandRow(before),
        },
      ]);
      const failed = journey.filter((step) => !step.ok);
      writeJourney('pane-crossing', journey);
      expect(
        checked('ux.pane-crossing', 'jeden', !failed.length, failed.map((step) => step.label).join('; ')),
        `these steps did nothing: ${failed.map((step) => step.label).join(', ')}`,
      ).toBe(true);
    } finally {
      session.kill();
    }
  });

  test('choosing a model applies it and closes the view', async () => {
    test.setTimeout(TIMEOUTS.test);
    const session = TuiSession.jeden();
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await session.command('/model', { escapeFirst: false });
      expect((await watchFor(() => session.capture(), /Select model route/, TIMEOUTS.view)).found).toBe(true);
      session.key('Right');
      await settledCapture(session);
      // Walk to a row that names a route; the first entries are the AUTO
      // pseudo-routes and the active model, which cannot be re-selected.
      let chosen = '';
      for (const _attempt of Array.from({ length: Number(process.env.PROBIERZ_PICK_STEPS ?? '8') })) {
        const id = cursorModelId(session.capture());
        if (id && !cursorRow(session.capture()).includes('[ACTIVE]')) {
          chosen = id;
          break;
        }
        session.key('Down');
        await settledCapture(session);
      }
      expect(chosen, 'no selectable model route under the cursor').not.toEqual('');
      session.key('Enter');
      const closed = await watchFor(
        () => (PICKER_CHROME.test(session.capture()) ? '' : 'closed'),
        /closed/,
        TIMEOUTS.view,
      );
      const settled = await settledCapture(session);
      const onStatusLine = settled.includes(chosen);
      const configPath = join(session.home, '.jeden', 'config.yml');
      const persisted = existsSync(configPath) && readFileSync(configPath, 'utf8').includes(chosen);
      const applied = closed.found && onStatusLine && persisted;
      expect(
        checked(
          'ux.selection-applies',
          'jeden',
          applied,
          `${chosen}: closed ${closed.found}, status ${onStatusLine}, config ${persisted}`,
        ),
        `choosing ${chosen} did not close the view and switch the route:\n${paneTail(settled)}`,
      ).toBe(true);
    } finally {
      session.kill();
    }
  });

  test('a destructive row asks before it acts, and Esc means no', async () => {
    test.setTimeout(TIMEOUTS.test);
    const session = TuiSession.jeden();
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      const journey = await runJourney(session, [
        {
          label: 'open usage',
          command: '/usage',
          expect: (after) => PICKER_CHROME.test(after),
        },
        {
          label: 'search reaches the reset row',
          type: 'reset',
          expect: (after) => /reset/i.test(after),
        },
        {
          label: 'Enter opens a confirm panel instead of resetting',
          key: 'Enter',
          expect: (after) => /confirm|cancel/i.test(after),
        },
        {
          label: 'Esc cancels it',
          key: 'Escape',
          expect: (after) => !/confirm .*action/i.test(after),
        },
      ]);
      const failed = journey.filter((step) => !step.ok);
      writeJourney('destructive-guard', journey);
      expect(
        checked('ux.confirm-guards', 'jeden', !failed.length, failed.map((step) => step.label).join('; ')),
        `these steps did nothing: ${failed.map((step) => step.label).join(', ')}`,
      ).toBe(true);
    } finally {
      session.kill();
    }
  });
});

/** Journeys are reviewed by a human as often as by an assertion, so each run
 * leaves the screen after every step on disk. */
function writeJourney(name: string, steps: JourneyStepResult[]): void {
  const dir = join(ARTIFACTS, 'journeys');
  mkdirSync(dir, { recursive: true });
  const body = steps
    .map((step) => `## ${step.ok ? 'ok' : 'FAILED'} — ${step.label}\n\n\`\`\`\n${step.screen}\n\`\`\`\n`)
    .join('\n');
  writeFileSync(join(dir, `${name}.md`), `# journey: ${name}\n\n${body}`);
}
