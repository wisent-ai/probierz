// Journey: mailbox-lifecycle.
//
// skrzynka already owns this test, in its own repository, at
// tests/mailboxes/mailbox-lifecycle.probierz.spec.mjs -- written in exactly the
// shape packages/tui/run-specs.mjs endorses for a product that keeps its test
// beside its source. What it never had was a manifest naming it, so nothing ran
// it. This spec is that name, and nothing more: it executes that file and
// reports its verdict.
//
// Deliberately a delegation rather than a copy. The mailbox assertions belong
// next to the code they describe. Restating them here would be two sources of
// truth, and the copy would be the one that silently stopped matching.
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

function readTapSummary(tap) {
  let planned;
  const results = [];

  for (const line of tap.split(/\r?\n/)) {
    const plan = /^1\.\.(\d+)$/.exec(line);
    if (plan) {
      planned = Number(plan[1]);
      continue;
    }

    const result = /^(not ok|ok)\s+\d+\b/.exec(line);
    if (result) {
      const directive = /\s+#\s+(SKIP|TODO)\b/i.exec(line)?.[1]?.toUpperCase() || null;
      results.push({ ok: result[1] === 'ok', directive });
    }
  }

  if (planned === undefined) {
    throw new Error('top-level TAP plan (`1..N`) was absent');
  }
  if (results.length !== planned) {
    throw new Error(`top-level TAP plan declared ${planned} tests but reported ${results.length} results`);
  }

  const skipped = results.filter((result) => result.directive === 'SKIP').length;
  const todo = results.filter((result) => result.directive === 'TODO').length;
  const passed = results.filter((result) => result.ok && result.directive === null).length;
  const failed = results.filter((result) => !result.ok && result.directive !== 'TODO').length;
  return { tests: planned, passed, failed, skipped, todo };
}

const run = spawnSync(process.execPath, ['--test', '--test-reporter=tap', owned], {
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

let summary;
try {
  summary = readTapSummary(run.stdout || '');
} catch (error) {
  assert.fail(
    `Probierz could not read the repository spec's TAP result: ${error instanceof Error ? error.message : String(error)}\nTAP stdout:\n${run.stdout || ''}\nstderr:\n${run.stderr || ''}`,
  );
}

assert.deepEqual(
  summary,
  { tests: 1, passed: 1, failed: 0, skipped: 0, todo: 0 },
  `the repository's TAP result was not one completed passing test\nTAP stdout:\n${run.stdout || ''}\nstderr:\n${run.stderr || ''}`,
);
