/**
 * What the harness records and compares: the structural check ledger the
 * verdict matrix consumes, the deterministic PNG render and its golden
 * comparison, and the scripted interaction journeys.
 */

import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';
import { ARTIFACTS, delay, GOLDEN_DIR, SCAN_SETTLE_MS, TIMEOUTS } from './environment';
import { escapeHtml, normalizeForGolden } from './screen';
import { settledCapture, type TuiSession } from './session';

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
 * Deterministic PNG rendering + golden comparison                     *
 * ------------------------------------------------------------------ */

/** The page the capture is painted onto. Fixed so the same text renders to
 * the same pixels on every machine. */
const RENDER_WIDTH = 1720;
const RENDER_HEIGHT = 900;

/** How different one pixel may be before pixelmatch counts it, and the
 * share of counted pixels a comparison still passes with. */
const PIXEL_THRESHOLD = 0.1;
const DEFAULT_MAX_DIFF_RATIO = 0.015;

/** Reported when the goldens differ in size, where a pixel count would be
 * meaningless. */
const UNCOMPARABLE = -1;
const NO_PIXELS = 0;

/** The page one capture is painted onto: a dark terminal background and a
 * monospace block, so only the text can differ between runs. */
const RENDER_PAGE = (text: string) =>
  `<!doctype html><body style="margin:0;background:#0d1117"><pre style="margin:0;padding:12px;font:13px/1.25 'Menlo',monospace;color:#e6edf3">${text}</pre></body>`;

export async function renderTextToPng(page: Page, text: string): Promise<Buffer> {
  await page.setViewportSize({ width: RENDER_WIDTH, height: RENDER_HEIGHT });
  await page.setContent(RENDER_PAGE(escapeHtml(text)));
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
 * PROBIERZ_UPDATE_GOLDEN is set or when no golden exists yet. */
export async function compareToGolden(
  page: Page,
  name: string,
  capture: string,
  maxDiffRatio = DEFAULT_MAX_DIFF_RATIO,
): Promise<GoldenResult> {
  mkdirSync(GOLDEN_DIR, { recursive: true });
  const goldenPath = join(GOLDEN_DIR, `${name}.png`);
  const actualPng = await renderTextToPng(page, normalizeForGolden(capture));
  if (process.env.PROBIERZ_UPDATE_GOLDEN || !existsSync(goldenPath)) {
    writeFileSync(goldenPath, actualPng);
    return {
      match: true,
      diffPixels: NO_PIXELS,
      totalPixels: NO_PIXELS,
      goldenPath,
      wroteGolden: true,
    };
  }
  const expected = PNG.sync.read(readFileSync(goldenPath));
  const actual = PNG.sync.read(actualPng);
  if (expected.width !== actual.width || expected.height !== actual.height) {
    writeActual(name, actualPng);
    return {
      match: false,
      diffPixels: UNCOMPARABLE,
      totalPixels: UNCOMPARABLE,
      goldenPath,
      wroteGolden: false,
    };
  }
  const diff = new PNG({ width: expected.width, height: expected.height });
  const diffPixels = pixelmatch(
    expected.data,
    actual.data,
    diff.data,
    expected.width,
    expected.height,
    { threshold: PIXEL_THRESHOLD },
  );
  if (diffPixels > NO_PIXELS) {
    writeActual(name, actualPng);
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

/** Keep the render that failed, beside the golden it was compared to. */
function writeActual(name: string, actualPng: Buffer): void {
  mkdirSync(join(ARTIFACTS, 'golden'), { recursive: true });
  writeFileSync(join(ARTIFACTS, 'golden', `${name}-actual.png`), actualPng);
}

/* ------------------------------------------------------------------ *
 * Interaction journeys                                                *
 * ------------------------------------------------------------------ */

export interface JourneyStep {
  label: string;
  /** Slash command submitted through the verified path. */
  command?: string;
  /** A tmux key name (`Down`, `Right`, `Enter`, `C-u`, `Escape`). */
  key?: string;
  /** Literal characters typed into whatever has the keyboard. */
  type?: string;
  /** What must hold once the screen settles. Receives the screen after the
   * step and the screen before it, because half of an interaction contract
   * is "something changed" — a cursor that never moves passes any regex. */
  expect: (after: string, before: string) => boolean;
  /** How long the expectation may take to hold; defaults to the view budget
   * so a network-bound step is not judged on its spinner. */
  timeoutMs?: number;
}

export interface JourneyStepResult {
  label: string;
  ok: boolean;
  screen: string;
}

/** Drive a scripted interaction, asserting after every keystroke. A journey
 * is how a user meets the app: open, move, cross panes, choose, land
 * somewhere. Grading one static screenshot cannot see any of that. */
export async function runJourney(
  session: TuiSession,
  steps: JourneyStep[],
): Promise<JourneyStepResult[]> {
  const results: JourneyStepResult[] = [];
  for (const step of steps) {
    const before = session.capture();
    if (step.command) await session.command(step.command, { escapeFirst: false });
    if (step.key) session.key(step.key);
    if (step.type) session.type(step.type);
    // Poll, never sample once: a view that needs a network round trip is
    // still a spinner when the screen first settles, and judging it there
    // reports "the keys did nothing" for a step that simply had not landed.
    const deadline = Date.now() + (step.timeoutMs ?? TIMEOUTS.view);
    let after = await settledCapture(session);
    let ok = step.expect(after, before);
    while (!ok && Date.now() < deadline) {
      await delay(SCAN_SETTLE_MS);
      after = session.capture();
      ok = step.expect(after, before);
    }
    results.push({ label: step.label, ok, screen: after });
    if (!ok) break;
  }
  return results;
}
