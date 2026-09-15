/**
 * The live tmux session: launching either binary, sending keys, capturing
 * the pane, and the verified command path.
 *
 * Shared tmux-driven TUI harness for jeden/omp comparison testing. Text
 * assertions check content; this harness exists to check SCREEN SEMANTICS —
 * view replacement, transcript growth, pane structure, viewport geometry —
 * the class of divergence plain text assertions cannot see.
 */

import { homedir } from 'node:os';
import {
  delay,
  ECHO_TIMEOUT_MS,
  ESC_FLUSH_MS,
  isolatedHome,
  JEDEN,
  OMP,
  PANE_HEIGHT,
  PANE_WIDTH,
  PICKER_CHROME,
  POLL_MS,
  SCAN_PAINT_MS,
  SCAN_SETTLE_MS,
  TAIL_LINES,
  tmpCwd,
  tmux,
  TYPE_ATTEMPTS,
  TYPE_SETTLE_MS,
} from './environment';

export interface TuiLaunchOptions {
  /** Shell command executed inside the tmux session. */
  command: string;
  /** HOME override for the launched process. */
  home?: string;
  width?: number;
  height?: number;
}

/** Random suffix so two sessions in one process never share a name. */
const NAME_SUFFIX_RADIX = 36;
const NAME_SUFFIX_START = 2;
const NAME_SUFFIX_END = 8;

export class TuiSession {
  readonly name = `probierz-cmp-${process.pid}-${Math.random()
    .toString(NAME_SUFFIX_RADIX)
    .slice(NAME_SUFFIX_START, NAME_SUFFIX_END)}`;
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
    tmux([
      'new-session',
      '-d',
      '-s',
      session.name,
      '-x',
      String(width),
      '-y',
      String(height),
      command,
    ]);
    return session;
  }

  static jeden(
    options: {
      args?: string;
      /** Pre-seeded working directory — discovery contracts plant fixtures
       * there before the app starts. */
      cwd?: string;
      isolateHome?: boolean;
      warmCache?: boolean;
      credentials?: boolean;
    } = {},
  ): TuiSession {
    const isolate = options.isolateHome ?? true;
    const cwd = options.cwd ?? tmpCwd();
    const session = TuiSession.launch({
      command: `${JEDEN} ${options.args ?? `--cwd ${cwd}`}`,
      home: isolate
        ? isolatedHome(options.warmCache ?? true, options.credentials ?? true)
        : undefined,
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
      await this.closeOverlay();
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

  /** Escape whatever owns the keyboard, and confirm it actually closed. */
  private async closeOverlay(): Promise<void> {
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

/** Capture once the screen stops moving. Classifying a mid-paint frame flips
 * pickers into "text" and misses error boxes that arrive a beat late — two
 * scan runs disagreed by three commands before this existed. */
export async function settledCapture(
  session: TuiSession,
  budgetMs = SCAN_PAINT_MS,
): Promise<string> {
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
