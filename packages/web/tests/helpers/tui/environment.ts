/**
 * What the environment gives this harness: which binaries exist, where
 * artifacts and fixtures live, the sandbox HOME a session runs in, and
 * every waiting budget and geometry the specs share.
 *
 * All of it is operator-tunable through `PROBIERZ_*` variables, so a
 * slow machine is tuned in one place instead of four specs.
 */

import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

export const HAS_TMUX = (() => {
  try {
    execFileSync('tmux', ['-V'], { encoding: 'utf8' });
    return true;
  } catch {
    return false;
  }
})();

export const JEDEN = process.env.JEDEN_BIN || 'jeden';
export const OMP = process.env.OMP_BIN || 'omp';

/** How long the version probe for the comparison binary may take. */
const VERSION_PROBE_MS = 10_000;

export const HAS_OMP = (() => {
  try {
    execFileSync(OMP, ['--version'], { encoding: 'utf8', timeout: VERSION_PROBE_MS });
    return true;
  } catch {
    return false;
  }
})();

// Playwright transpiles specs to CJS and runs them from packages/web, so
// paths anchor at process.cwd() (import.meta is unavailable there).
export const GOLDEN_DIR = join(process.cwd(), 'tests', 'golden');
export const ARTIFACTS = process.env.PROBIERZ_ARTIFACTS || 'test-results';

/** Fixtures seeded into sandboxes so views have something real to discover. */
export const FIXTURES = join(process.cwd(), 'harness', 'fixtures');

/** Chrome only an open picker paints — the marker for "a view owns the
 * keyboard right now". */
export const PICKER_CHROME = /Esc close|Type to search/;

/** Shared waiting budgets. Named (not inline) so a slow machine is tuned in
 * one place instead of scattered across four specs. */
export const TIMEOUTS = {
  ready: Number(process.env.PROBIERZ_READY_TIMEOUT_MS ?? '30000'),
  view: Number(process.env.PROBIERZ_VIEW_TIMEOUT_MS ?? '60000'),
  settle: Number(process.env.PROBIERZ_SETTLE_TIMEOUT_MS ?? '15000'),
  spinner: Number(process.env.PROBIERZ_SPINNER_TIMEOUT_MS ?? '20000'),
  turn: Number(process.env.PROBIERZ_TURN_TIMEOUT_MS ?? '90000'),
  test: Number(process.env.PROBIERZ_TEST_TIMEOUT_MS ?? '180000'),
  walkthrough: Number(process.env.PROBIERZ_WALKTHROUGH_TIMEOUT_MS ?? '240000'),
};

export const POLL_MS = Number(process.env.PROBIERZ_POLL_MS ?? '200');
export const SLOW_POLL_MS = Number(process.env.PROBIERZ_SLOW_POLL_MS ?? '500');

/** Both apps are compared at identical geometry; a narrower pane would
 * change wrapping and make "does it fit" meaningless. */
export const PANE_WIDTH = Number(process.env.PROBIERZ_PANE_WIDTH ?? '200');
export const PANE_HEIGHT = Number(process.env.PROBIERZ_PANE_HEIGHT ?? '50');

export const TYPE_ATTEMPTS = Array.from({
  length: Number(process.env.PROBIERZ_TYPE_ATTEMPTS ?? '3'),
});
export const TAIL_LINES = Number(process.env.PROBIERZ_TAIL_LINES ?? '12');

/** How long a typed command may take to show up in the prompt. */
export const ECHO_TIMEOUT_MS = Number(process.env.PROBIERZ_ECHO_TIMEOUT_MS ?? '5000');
export const MAX_OUTPUT_BYTES = Number(process.env.PROBIERZ_MAX_OUTPUT_BYTES ?? '8388608');

/** `/help` prints `<command><spaces><description>`; capture group one. */
const HELP_COMMAND_GROUP = Number(process.env.PROBIERZ_HELP_GROUP ?? '1');

/** Per-command budget in the command-surface scan: any paint at all — a
 * spinner counts — must land inside it. */
export const SCAN_PAINT_MS = Number(process.env.PROBIERZ_SCAN_PAINT_MS ?? '15000');
/** Polling step while waiting for a painted view to stop changing. */
export const SCAN_SETTLE_MS = Number(process.env.PROBIERZ_SCAN_SETTLE_MS ?? '400');
/** Typed into a picker's search to prove it filters: no row can match it. */
export const UNMATCHABLE_QUERY = process.env.PROBIERZ_UNMATCHABLE_QUERY ?? 'qzxwvj';
/** Commands per session; a fresh session bounds mode/state bleed without
 * paying app startup for all ~70 commands. */
export const SCAN_CHUNK = Number(process.env.PROBIERZ_SCAN_CHUNK ?? '12');
export const SCAN_TIMEOUT_MS = Number(process.env.PROBIERZ_SCAN_TIMEOUT_MS ?? '900000');

/** Terminals merge a lone ESC with whatever follows within their escape
 * timeout; the default clears every parser we drive (jeden ratatui, omp
 * ink). Both are operator-tunable for slower machines. */
export const ESC_FLUSH_MS = Number(process.env.PROBIERZ_ESC_FLUSH_MS ?? '400');
export const TYPE_SETTLE_MS = Number(process.env.PROBIERZ_TYPE_SETTLE_MS ?? '150');

/** Language the sandbox pins, because every assertion here is written
 * against the English strings. */
const SANDBOX_LANGUAGE = process.env.PROBIERZ_SANDBOX_LANGUAGE ?? 'en';

/** tmux's own output cap for one capture. */
const TMUX_MAX_BUFFER = 8 * 1024 * 1024;

export function tmux(args: string[]): string {
  return execFileSync('tmux', args, { encoding: 'utf8', maxBuffer: TMUX_MAX_BUFFER });
}

export function tmpCwd(): string {
  return mkdtempSync(join(tmpdir(), 'probierz-tui-cwd-'));
}

/** Every slash command the binary itself advertises. Asking the app beats a
 * hand-written list: a command added tomorrow is scanned tomorrow, and one
 * that disappears stops being scanned instead of failing forever. */
export function slashCommands(): string[] {
  const help = execFileSync(JEDEN, ['--cwd', tmpCwd()], {
    input: '/help\n',
    encoding: 'utf8',
    timeout: TIMEOUTS.ready,
    maxBuffer: MAX_OUTPUT_BYTES,
  });
  return [...help.matchAll(/^(\/[a-z-]+)\s\s+\S/gm)].map((match) => match[HELP_COMMAND_GROUP]);
}

/** Fresh HOME for one session. `warmCache` brings the brama catalog cache
 * along (structural contracts must not pay a cold 5 700-model fetch; latency
 * contracts deliberately keep it cold), `credentials` decides whether the
 * operator's `.env` comes with it — the setup checklist can only be verified
 * against a home that has none. */
export function isolatedHome(warmCache: boolean, credentials = true): string {
  const home = mkdtempSync(join(tmpdir(), 'probierz-tui-home-'));
  mkdirSync(join(home, '.jeden'), { recursive: true });
  const files = credentials ? ['.env', 'config.yml'] : ['config.yml'];
  for (const file of files) {
    const source = join(homedir(), '.jeden', file);
    if (existsSync(source)) {
      copyFileSync(source, join(home, '.jeden', file));
    }
  }
  const cache = join(homedir(), '.jeden', 'cache');
  if (warmCache && existsSync(cache)) {
    cpSync(cache, join(home, '.jeden', 'cache'), { recursive: true });
  }
  // Pin the UI language in the sandbox. Every assertion here is written
  // against the English strings, so inheriting the operator's `ui.language`
  // turns the whole suite red for a reason that has nothing to do with the
  // app — which is exactly what one stray `/settings set` did.
  const configPath = join(home, '.jeden', 'config.yml');
  if (existsSync(configPath)) {
    try {
      const config = JSON.parse(readFileSync(configPath, 'utf8')) as {
        ui?: Record<string, unknown>;
      };
      config.ui = { ...config.ui, language: SANDBOX_LANGUAGE };
      writeFileSync(configPath, JSON.stringify(config));
    } catch {
      // Not JSON — leave the operator's file shape untouched.
    }
  }
  return home;
}

export function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}
