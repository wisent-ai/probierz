/**
 * What the command surface scan needs to judge one screen.
 *
 * The scan opens every slash command the app itself advertises and records
 * what happened: did anything paint, does the frame fit the pane, is it a
 * picker, does it navigate, filter and close. Everything here is the
 * judgement, not the driving — the spec owns the sessions.
 */

import { join } from 'node:path';
import { PICKER_CHROME } from './tui';

export type ScanStatus = 'picker' | 'text' | 'error' | 'silent';

export interface ScanRow {
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

/** A picker offering exactly one row cannot demonstrate navigation. */
export const ONE_ROW = Number(process.env.PROBIERZ_ONE_ROW ?? '1');
/**
 * Full command-surface scan. Every slash command the binary advertises is
 * driven in a live TUI and classified by what it puts on the screen. The
 * per-view contracts above cover five views deeply; this covers all ~70
 * shallowly, so a command that renders nothing, panics, or errors out
 * cannot hide in the long tail.
 */

/** Commands excluded from the scan, with the reason printed in the report —
 * a silent skip is how a scan starts lying. */
export const SCAN_SKIP: Record<string, string> = {
  '/update': 'runs the automated self-update',
  '/rebuild': 'rebuilds the binary and restarts the session',
  '/refresh': 'mutates live Weles credentials other tests depend on',
  '/compact': 'spawns a model turn (quota + minutes)',
  '/btw': 'spawns a model turn (quota + minutes)',
  '/exit': 'terminates the session; covered by its own contract',
};

/** Verbs whose subcommands only read. A discovered subcommand outside this
 * list (purchase, set, uninstall, revoke…) is reported, never executed. */
export const READ_VERBS = new Set(['list', 'show', 'status', 'get', 'describe', 'info', 'help', 'dump']);
/** Read-only subcommands taken from the dispatcher itself (`rust/slash/mod.rs`
 * status/list arms and `rust/cli/billing.rs::BILLING_SLASH_HANDLERS`). The
 * harvest below adds any the app prints at runtime; this seed exists because
 * jeden's pickers show labels, not the commands behind them, so harvesting
 * alone finds nothing — and the bare `/billing` failure says nothing about
 * whether `/billing policy get` works. */
export const SEED_SUBCOMMANDS = [
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
export function frameFits(screen: string): boolean {
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
export function pickerRows(screen: string): number {
  return screen.split('\n').filter((line) => /\[[A-Z][A-Z\d _-]*\]/.test(line)).length;
}

export function contentLines(capture: string): number {
  return capture.split('\n').filter((line) => line.trim()).length;
}

/** Slash commands WITH a subcommand that the app itself printed — usage
 * text, error messages, picker rows — in both shapes jeden uses:
 * `/session info` and `Usage: /session [info|delete]`. Harvested instead of
 * guessed, so the scan never invents a surface the app does not document. */
export function harvestSubcommands(screen: string, advertised: Set<string>, into: Set<string>): void {
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

export const ERROR_BOX = /╭ error|panicked|internal error/i;

/** Only what THIS command painted. jeden keeps earlier frames on screen, so
 * classifying the whole pane makes one error box mark every later command as
 * failing — the first version of this scan did exactly that. */
export function paintedSince(before: string, after: string): string {
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
export function classify(screen: string, newPaint: string): { status: ScanStatus; note: string } {
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

