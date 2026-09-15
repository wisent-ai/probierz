/**
 * Shared tmux-driven TUI harness for jeden/omp comparison testing. Text
 * assertions check content; this harness exists to check SCREEN SEMANTICS —
 * view replacement, transcript growth, pane structure, viewport geometry —
 * the class of divergence plain text assertions cannot see.
 *
 * | part | what it owns |
 * |---|---|
 * | `environment` | binaries, paths, the sandbox HOME, every budget and geometry |
 * | `session` | the live tmux session, its keys, its captures, its commands |
 * | `screen` | what a captured screen measures: frames, panes, cursor rows |
 * | `evidence` | the check ledger, the golden comparison, the journeys |
 *
 * Specs import from `./helpers/tui`, which is this file, so the split is
 * invisible to them.
 */

export * from './environment';
export * from './evidence';
export * from './screen';
export * from './session';
