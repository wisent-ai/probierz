/**
 * The two applications the screen-semantics contracts run against — jeden,
 * and omp as the reference control — and the budgets those contracts judge a
 * screen by.
 *
 * A red row on the omp side means the harness itself is miscalibrated, which
 * is why both profiles describe the same screens through the same fields.
 */

import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { TuiSession } from './tui';

export const FRAME_BUDGET_AFTER_TWO_OVERLAYS = Number(process.env.PROBIERZ_FRAME_BUDGET ?? '4');
export const MIN_TWO_PANE_ROWS = Number(process.env.PROBIERZ_MIN_TWO_PANE_ROWS ?? '3');

export interface AppProfile {
  name: 'jeden' | 'omp';
  start: () => TuiSession;
  ready: RegExp;
  modelCommand: string;
  modelTitle: RegExp;
  modelFooter: RegExp;
  settingsCommand: string;
  settingsTitle: RegExp;
}

/** Whether a Brama is configured for this run, read from the same file the
 * app reads. A contract that needs the catalog skips rather than fails when
 * there is none. */
export function bramaReachableSync(): boolean {
  if (process.env.BRAMA_URL) return true;
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return false;
  return readFileSync(envPath, 'utf8').includes('BRAMA_URL=');
}
export const PROFILES: AppProfile[] = [
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

