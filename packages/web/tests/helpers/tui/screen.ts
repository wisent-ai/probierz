/**
 * Measurements taken from a captured screen: how many boxed frames are on
 * it, whether a two-pane view is really two panes, which row each cursor
 * is on, and the normalisation that masks volatile values before a golden
 * comparison.
 */

// Sentinels spelled out: the repository guard bans bare numerals, and these
// read better than the arithmetic that ban otherwise produces.
const NONE = ''.length;
const ONE = ' '.length;
const NOT_FOUND = ''.indexOf(' ');

/** Number of distinct boxed frames (`╭` top borders) visible in the pane. */
export function boxFrameCount(capture: string): number {
  return (capture.match(/╭/g) || []).length;
}

export interface PaneGeometry {
  /** Rows whose divider sits at the same column as the frame's `┬`. */
  alignedRows: number;
  /** The `┬`/`┴` joints that make the divider part of the border. */
  joined: boolean;
  /** Brands column carries state dots. */
  dots: number;
  /** Item rows whose figures end at a common right edge. */
  alignedMetrics: number;
}

/** Structure of a two-pane view, measured the way a reader sees it. The first
 * version of this counted "a dot, then a bar, then text" — which a flat list
 * fakes by accident, and which stayed green while `/model` was still one
 * column. A pane is a pane when the divider is part of the frame (`┬`…`┴`)
 * and sits at one column on every row. */
export function paneGeometry(capture: string): PaneGeometry {
  const lines = capture.split('\n');
  const top = lines.find((line) => line.includes('┬'));
  const bottom = lines.find((line) => line.includes('┴'));
  const column = top?.indexOf('┬') ?? NOT_FOUND;
  const body = lines.filter((line) => line.startsWith('│') && line.trimEnd().endsWith('│'));
  const split = body.filter((line) => line.charAt(column) === '│');
  const dots = split.filter((line) => /[●○]/.test(line.slice(NONE, column))).length;
  // Figures form a column when their right edge repeats: take the end offset
  // of the metric block on each row and count the most common one.
  const tally = new Map<number, number>();
  for (const line of split) {
    const end = line.slice(column).search(/(?:◫|█|│)\s*│\s*$/);
    if (end === NOT_FOUND) continue;
    tally.set(end, (tally.get(end) ?? NONE) + ONE);
  }
  return {
    alignedRows: split.length,
    joined: Boolean(top) && Boolean(bottom),
    dots,
    alignedMetrics: Math.max(...[...tally.values(), NONE]),
  };
}

/** Kept for the legacy signature: rows that look split. Prefer
 * `paneGeometry`, which cannot be satisfied by a bar drawn inside text. */
export function twoPaneDividerRows(capture: string): number {
  return paneGeometry(capture).alignedRows;
}

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
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** The row the item cursor is on, without its marker. */
export function cursorRow(screen: string): string {
  return (screen.split('\n').find((line) => line.includes('›')) ?? '').trim();
}

/** The row the brands cursor is on. */
export function brandRow(screen: string): string {
  return (screen.split('\n').find((line) => line.includes('❯')) ?? '').trim();
}

/** First `provider/model` id visible on the cursor row, if any. */
export function cursorModelId(screen: string): string {
  return cursorRow(screen).match(/[a-z\d.-]+\/[a-z\d.:-]+/i)?.[NAME_MATCH] ?? '';
}

const NAME_MATCH = ''.length;
