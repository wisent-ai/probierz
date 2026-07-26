import { execFileSync } from 'node:child_process';
import {
  appendFileSync,
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
import type { Page } from '@playwright/test';
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';

/**
 * Shared tmux-driven TUI harness for jeden/omp comparison testing. Text
 * assertions check content; this harness exists to check SCREEN SEMANTICS —
 * view replacement, transcript growth, pane structure, viewport geometry —
 * the class of divergence plain text assertions cannot see.
 */

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
export const HAS_OMP = (() => {
  try {
    execFileSync(OMP, ['--version'], { encoding: 'utf8', timeout: 10_000 });
    return true;
  } catch {
    return false;
  }
})();

// Playwright transpiles specs to CJS and runs them from packages/web, so
// paths anchor at process.cwd() (import.meta is unavailable there).
export const GOLDEN_DIR = join(process.cwd(), 'tests', 'golden');
export const ARTIFACTS = process.env.PROBIERZ_ARTIFACTS || 'test-results';

function tmux(args: string[]): string {
  return execFileSync('tmux', args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
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

/** Fresh HOME carrying only the jeden credentials and config. With
 * `warmCache` the brama catalog cache comes along too: structural contracts
 * (geometry, panes, transcript) must not pay a cold 5 700-model fetch, while
 * latency contracts deliberately keep the cache cold. */
export function isolatedHome(warmCache: boolean): string {
  const home = mkdtempSync(join(tmpdir(), 'probierz-tui-home-'));
  mkdirSync(join(home, '.jeden'), { recursive: true });
  for (const file of ['.env', 'config.yml']) {
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
      const config = JSON.parse(readFileSync(configPath, 'utf8')) as { ui?: Record<string, unknown> };
      config.ui = { ...config.ui, language: SANDBOX_LANGUAGE };
      writeFileSync(configPath, JSON.stringify(config));
    } catch {
      // Not JSON — leave the operator's file shape untouched.
    }
  }
  return home;
}

const SANDBOX_LANGUAGE = process.env.PROBIERZ_SANDBOX_LANGUAGE ?? 'en';

/** Chrome only an open picker paints — the marker for "a view owns the
 * keyboard right now". */
export const PICKER_CHROME = /Esc close|Type to search/;

export interface TuiLaunchOptions {
  /** Shell command executed inside the tmux session. */
  command: string;
  /** HOME override for the launched process. */
  home?: string;
  width?: number;
  height?: number;
}

export class TuiSession {
  readonly name = `probierz-cmp-${process.pid}-${Math.random().toString(36).slice(2, 8)}`;
  /** Sandbox the app runs in — behavioural contracts assert against the files
   * it writes there, and a sandbox nobody can name cannot be asserted on. */
  home = '';
  cwd = '';

  static launch(options: TuiLaunchOptions): TuiSession {
    const session = new TuiSession();
    const width = options.width ?? PANE_WIDTH;
    const height = options.height ?? PANE_HEIGHT;
    const command = options.home ? `env HOME=${options.home} ${options.command}` : options.command;
    session.home = options.home ?? homedir();
    tmux(['new-session', '-d', '-s', session.name, '-x', String(width), '-y', String(height), command]);
    return session;
  }

  static jeden(options: { args?: string; isolateHome?: boolean; warmCache?: boolean } = {}): TuiSession {
    const isolate = options.isolateHome ?? true;
    const cwd = tmpCwd();
    const session = TuiSession.launch({
      command: `${JEDEN} ${options.args ?? `--cwd ${cwd}`}`,
      home: isolate ? isolatedHome(options.warmCache ?? true) : undefined,
    });
    session.cwd = cwd;
    return session;
  }

  static omp(options: { args?: string } = {}): TuiSession {
    return TuiSession.launch({ command: `${OMP} ${options.args ?? '--allow-home'}` });
  }

  submit(text: string): void {
    tmux(['send-keys', '-t', this.name, '-l', text]);
    tmux(['send-keys', '-t', this.name, 'Enter']);
  }

  key(name: string): void {
    tmux(['send-keys', '-t', this.name, name]);
  }

  /** Literal keystrokes with no Enter — for typing into a picker's search. */
  type(text: string): void {
    tmux(['send-keys', '-t', this.name, '-l', text]);
  }

  alive(): boolean {
    try {
      tmux(['has-session', '-t', this.name]);
      return true;
    } catch {
      return false;
    }
  }

  capture(): string {
    return this.captureWith([]);
  }

  /** Visible pane PLUS full scrollback history — where appended transcript
   * frames actually live. The visible window alone cannot distinguish
   * "overlay replaced" from "old frame scrolled out of sight". */
  captureHistory(): string {
    return this.captureWith(['-S', '-']);
  }

  /** A dead session reports an empty screen instead of throwing: when one
   * comparison branch fails and tears its sibling down, tmux's "can't find
   * pane" would otherwise replace the real assertion message. */
  private captureWith(extra: string[]): string {
    try {
      return tmux(['capture-pane', '-t', this.name, '-p', ...extra]);
    } catch {
      return '';
    }
  }

  /** Close whatever overlay is open, type a slash command, submit it — but
   * only once the prompt has echoed the text back.
   * NEVER use `submit()` straight after `key('Escape')`: tmux delivers both
   * back-to-back, the terminal parses `ESC /` as a single escape sequence,
   * and the app receives `models` — a chat prompt instead of a command.
   * The echo is POLLED, never sampled once: a single delayed check races the
   * repaint under load, and retyping on that false negative submits
   * `/model --all/model --all`. Retyping therefore also requires the clear
   * to be observed. */
  async command(text: string, options: { escapeFirst?: boolean } = {}): Promise<void> {
    if (options.escapeFirst ?? true) {
      // Settle first: an Esc sent while the PREVIOUS view is still painting
      // hits a screen with no picker yet, reads as "nothing to close", and
      // the command that follows lands in that view's search box — which is
      // how `/settings` once read as "no matching items" in the changelog.
      await settledCapture(this, ECHO_TIMEOUT_MS);
      // Escape is verified, not assumed. A view that has painted but not yet
      // mounted its key handler swallows the first Esc.
      for (const _attempt of TYPE_ATTEMPTS) {
        this.key('Escape');
        const closed = await watchFor(
          () => (PICKER_CHROME.test(this.capture()) ? '' : 'closed'),
          /closed/,
          ECHO_TIMEOUT_MS,
          TYPE_SETTLE_MS,
        );
        if (closed.found) break;
      }
      await delay(ESC_FLUSH_MS);
    }
    const echo = new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
    for (const _attempt of TYPE_ATTEMPTS) {
      tmux(['send-keys', '-t', this.name, '-l', text]);
      if ((await watchFor(() => this.capture(), echo, ECHO_TIMEOUT_MS, TYPE_SETTLE_MS)).found) {
        tmux(['send-keys', '-t', this.name, 'Enter']);
        return;
      }
      this.key('C-u');
      const cleared = await watchFor(
        () => (echo.test(this.capture()) ? '' : 'cleared'),
        /cleared/,
        ECHO_TIMEOUT_MS,
        TYPE_SETTLE_MS,
      );
      if (!cleared.found) {
        throw new Error(
          `"${text}" neither took effect nor cleared — refusing to retype into a dirty prompt:\n${paneTail(this.capture())}`,
        );
      }
    }
    throw new Error(
      `typed "${text}" ${TYPE_ATTEMPTS.length}x and the prompt never echoed it — the app is not accepting input:\n${paneTail(this.capture())}`,
    );
  }

  kill(): void {
    try {
      tmux(['kill-session', '-t', this.name]);
    } catch {
      // already gone
    }
  }
}

export interface Appearance {
  /** Whether `pattern` showed up before the window elapsed. */
  found: boolean;
  /** Milliseconds until it appeared, or the full window on timeout — usable
   * for ordering assertions ("the spinner precedes the content"). */
  ms: number;
}

export async function watchFor(
  capture: () => string,
  pattern: RegExp,
  timeoutMs: number,
  intervalMs = POLL_MS,
): Promise<Appearance> {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (pattern.test(capture())) return { found: true, ms: Date.now() - started };
    await delay(intervalMs);
  }
  return { found: false, ms: timeoutMs };
}

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

const POLL_MS = Number(process.env.PROBIERZ_POLL_MS ?? '200');
export const SLOW_POLL_MS = Number(process.env.PROBIERZ_SLOW_POLL_MS ?? '500');

/** Both apps are compared at identical geometry; a narrower pane would
 * change wrapping and make "does it fit" meaningless. */
export const PANE_WIDTH = Number(process.env.PROBIERZ_PANE_WIDTH ?? '200');
export const PANE_HEIGHT = Number(process.env.PROBIERZ_PANE_HEIGHT ?? '50');
const TYPE_ATTEMPTS = Array.from({ length: Number(process.env.PROBIERZ_TYPE_ATTEMPTS ?? '3') });
const TAIL_LINES = Number(process.env.PROBIERZ_TAIL_LINES ?? '12');
/** How long a typed command may take to show up in the prompt. */
const ECHO_TIMEOUT_MS = Number(process.env.PROBIERZ_ECHO_TIMEOUT_MS ?? '5000');
const MAX_OUTPUT_BYTES = Number(process.env.PROBIERZ_MAX_OUTPUT_BYTES ?? '8388608');
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

/** Capture once the screen stops moving. Classifying a mid-paint frame flips
 * pickers into "text" and misses error boxes that arrive a beat late — two
 * scan runs disagreed by three commands before this existed. */
export async function settledCapture(session: TuiSession, budgetMs = SCAN_PAINT_MS): Promise<string> {
  const started = Date.now();
  let previous = session.capture();
  while (Date.now() - started < budgetMs) {
    await delay(SCAN_SETTLE_MS);
    const next = session.capture();
    if (next === previous) return next;
    previous = next;
  }
  return previous;
}

/** The pane with box borders stripped, in both join shapes a terminal can
 * produce: rows glued (a path or word split at the frame edge) and rows
 * space-joined (a sentence split across rows). Matching the raw capture
 * reports "not there" for text plainly on screen — that is how a working
 * `/settings set` looked broken. */
export function flattened(capture: string): string {
  const rows = capture.split('\n').map((line) => line.replace(/[│╭╮╯╰─]/g, '').trim());
  return `${rows.join('')}\n${rows.join(' ')}`;
}

/** Last non-blank pane lines, for failure messages. "The view never opened"
 * is undiagnosable without the screen that was actually on it. */
export function paneTail(capture: string): string {
  const lines = capture.split('\n').filter((line) => line.trim());
  return lines.slice(-TAIL_LINES).join('\n');
}

/** Terminals merge a lone ESC with whatever follows within their escape
 * timeout; the default clears every parser we drive (jeden ratatui, omp
 * ink). Both are operator-tunable for slower machines. */
export const ESC_FLUSH_MS = Number(process.env.PROBIERZ_ESC_FLUSH_MS ?? '400');
const TYPE_SETTLE_MS = Number(process.env.PROBIERZ_TYPE_SETTLE_MS ?? '150');

export function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}

/* ------------------------------------------------------------------ *
 * Structural check ledger                                             *
 * ------------------------------------------------------------------ */

export interface CheckOutcome {
  id: string;
  app: string;
  ok: boolean;
  detail: string;
}

export const CHECKS_FILE = join(ARTIFACTS, 'checks.jsonl');

/** Record a structural check outcome and hand the boolean back, so the
 * ledger is written whether the assertion that follows passes or fails.
 * The verdict matrix consumes this file: a parity claim is earned only when
 * its checks actually ran and actually passed in this run. */
export function checked(id: string, app: string, ok: boolean, detail = ''): boolean {
  mkdirSync(ARTIFACTS, { recursive: true });
  const entry: CheckOutcome = { id, app, ok, detail };
  appendFileSync(CHECKS_FILE, `${JSON.stringify(entry)}\n`);
  return ok;
}

export function readChecks(): CheckOutcome[] {
  if (!existsSync(CHECKS_FILE)) return [];
  return readFileSync(CHECKS_FILE, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line) as CheckOutcome);
}

/* ------------------------------------------------------------------ *
 * Screen-semantics measurements                                       *
 * ------------------------------------------------------------------ */

/** Number of distinct boxed frames (`╭` top borders) visible in the pane. */
export function boxFrameCount(capture: string): number {
  return (capture.match(/╭/g) || []).length;
}

/** Rows carrying a provider dot AND a second vertical divider with content
 * after it — the structural signature of a two-pane layout. A flat list has
 * dots only on rows whose sole trailing bar is the outer box border. */
export function twoPaneDividerRows(capture: string): number {
  let hits = 0;
  for (const line of capture.split('\n')) {
    const dot = line.search(/[●○]/);
    if (dot === -1) continue;
    if (/[●○][^│]*│\s*\S/.test(line.slice(dot))) hits += 1;
  }
  return hits;
}

/* ------------------------------------------------------------------ *
 * Deterministic PNG rendering + golden comparison                     *
 * ------------------------------------------------------------------ */

/** Volatile values masked before golden comparison (version hashes, measured
 * perf, quota percents, catalog counts, costs, dates). */
export function normalizeForGolden(text: string): string {
  return text
    .replace(/dev\.\d+\.[0-9a-f]+(?:\.dirty)?/g, 'dev.X.HASH')
    .replace(/\d+\.\d+s \d+t\/s/g, 'Ns Nt/s')
    .replace(/\b\d{4,} models\b/g, 'N models')
    .replace(/\$\d+\.\d+/g, '$X')
    .replace(/\b\d{1,3}%/g, 'Q%')
    .replace(/\d{4}-\d{2}-\d{2}T[\d:.]+Z?/g, 'DATE');
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

export async function renderTextToPng(page: Page, text: string): Promise<Buffer> {
  await page.setViewportSize({ width: 1720, height: 900 });
  await page.setContent(
    `<!doctype html><body style="margin:0;background:#0d1117"><pre style="margin:0;padding:12px;font:13px/1.25 'Menlo',monospace;color:#e6edf3">${escapeHtml(text)}</pre></body>`,
  );
  return page.screenshot({ fullPage: true });
}

export interface GoldenResult {
  match: boolean;
  diffPixels: number;
  totalPixels: number;
  goldenPath: string;
  wroteGolden: boolean;
}

/** Compare a capture against its golden PNG; regenerate when
 * PROBIERZ_UPDATE_GOLDEN=1 or when no golden exists yet. */
export async function compareToGolden(
  page: Page,
  name: string,
  capture: string,
  maxDiffRatio = 0.015,
): Promise<GoldenResult> {
  mkdirSync(GOLDEN_DIR, { recursive: true });
  const goldenPath = join(GOLDEN_DIR, `${name}.png`);
  const actualPng = await renderTextToPng(page, normalizeForGolden(capture));
  if (process.env.PROBIERZ_UPDATE_GOLDEN === '1' || !existsSync(goldenPath)) {
    writeFileSync(goldenPath, actualPng);
    return { match: true, diffPixels: 0, totalPixels: 0, goldenPath, wroteGolden: true };
  }
  const expected = PNG.sync.read(readFileSync(goldenPath));
  const actual = PNG.sync.read(actualPng);
  if (expected.width !== actual.width || expected.height !== actual.height) {
    mkdirSync(join(ARTIFACTS, 'golden'), { recursive: true });
    writeFileSync(join(ARTIFACTS, 'golden', `${name}-actual.png`), actualPng);
    return { match: false, diffPixels: -1, totalPixels: -1, goldenPath, wroteGolden: false };
  }
  const diff = new PNG({ width: expected.width, height: expected.height });
  const diffPixels = pixelmatch(expected.data, actual.data, diff.data, expected.width, expected.height, {
    threshold: 0.1,
  });
  if (diffPixels > 0) {
    mkdirSync(join(ARTIFACTS, 'golden'), { recursive: true });
    writeFileSync(join(ARTIFACTS, 'golden', `${name}-actual.png`), actualPng);
    writeFileSync(join(ARTIFACTS, 'golden', `${name}-diff.png`), PNG.sync.write(diff));
  }
  const totalPixels = expected.width * expected.height;
  return {
    match: diffPixels / totalPixels <= maxDiffRatio,
    diffPixels,
    totalPixels,
    goldenPath,
    wroteGolden: false,
  };
}
