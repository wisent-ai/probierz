// Journey: mailbox-lifecycle.
//
// skrzynka already owns this test, in its own repository, at
// tests/mailboxes/mailbox-lifecycle.probierz.spec.mjs -- written in exactly the
// shape packages/tui/run-specs.mjs endorses for a product that keeps its test
// beside its source. What it never had was a manifest naming it, so nothing ran
// it. This spec is that name, and nothing more: it executes that file and
// reports its verdict.
//
// Deliberately a delegation rather than a copy. The four mailbox assertions --
// add persists only the profile, disable and enable change only enabled state,
// remove requires confirmation and preserves Skarbiec -- belong next to the
// code they describe. Restating them here would be two sources of truth, and
// the copy would be the one that silently stopped matching.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const repository =
  process.env.SKRZYNKA_REPO ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skrzynka';

const owned = join(repository, 'tests', 'mailboxes', 'mailbox-lifecycle.probierz.spec.mjs');
assert.ok(
  existsSync(owned),
  `skrzynka no longer carries ${owned}; this journey has no test to run, which is a failure and not a pass`,
);

const run = spawnSync(process.execPath, ['--test', owned], {
  cwd: repository,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
});
const output = `${run.stdout || ''}${run.stderr || ''}`;

assert.equal(
  run.status,
  0,
  `the repository's own mailbox-lifecycle spec failed with ${run.status}${run.signal ? ` (${run.signal})` : ''}\n${output}`,
);

// `node --test` exits zero for a file in which nothing ran, so the count is
// asserted as well as the status.
assert.match(
  output,
  /# pass 1\b/,
  `the repository's spec reported no passing test\n${output}`,
);
assert.match(output, /# fail 0\b/, `the repository's spec reported failures\n${output}`);
