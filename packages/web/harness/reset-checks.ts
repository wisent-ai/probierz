import { rmSync } from 'node:fs';
import { CHECKS_FILE } from '../tests/helpers/tui';

/**
 * Playwright globalSetup. The structural check ledger is per-run: without
 * this reset a check that passed in an earlier run could earn today's parity
 * verdict even though it never executed now.
 */
export default function resetChecks(): void {
  rmSync(CHECKS_FILE, { force: true });
}
