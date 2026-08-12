import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary = process.env.TUI_CMD?.trim();
assert.ok(
  binary,
  'TUI_CMD is required: provide the released Skarbiec executable; gpg and an isolated owner keyring are external runtime prerequisites',
);

const tempDir = await mkdtemp('/tmp/probierz-skarbiec-first-use-');
const vaultFile = join(tempDir, 'onboarding.vault.json');
const env = {
  HOME: tempDir,
  GNUPGHOME: tempDir,
  SKARBIEC_VAULT_FILE: vaultFile,
  SKARBIEC_AUDIT_FILE: join(tempDir, 'audit.jsonl'),
  USER: 'probierz-skarbiec-first-use',
  HOSTNAME: 'probierz-isolated-host',
};

async function run(args, marker, timeoutMs = 30_000) {
  const app = spawnTui(binary, args, { env });
  try {
    await app.waitFor(marker, { timeoutMs, useFullLog: true });
    return app.fullLog();
  } finally {
    await app.close();
  }
}

let app;
try {
  const initialized = await run(
    ['init', 'probierz-skarbiec-onboarding-owner'],
    /"owner_fpr"\s*:/,
    120_000,
  );
  assert.match(initialized, /"ok"\s*:\s*true/);
  assert.doesNotMatch(initialized, /"status"\s*:\s*"completed"/);

  app = spawnTui(binary, ['onboarding'], { env, cols: 120, rows: 40 });
  await app.waitFor('Your agents never hold a credential', {
    timeoutMs: 30_000,
    useFullLog: true,
  });
  app.key('enter');
  await app.waitFor('From .env copies to one-use capabilities', {
    timeoutMs: 15_000,
    useFullLog: true,
  });
  app.key('enter');
  await app.waitFor('Create and read a safe local note', {
    timeoutMs: 15_000,
    useFullLog: true,
  });
  app.send('n');
  app.key('enter');
  const paused = await app.waitFor(/"status"\s*:\s*"paused"/, {
    timeoutMs: 15_000,
    useFullLog: true,
  });
  assert.match(paused, /"resume"\s*:\s*"skarbiec onboarding"/);
  assert.doesNotMatch(paused, /Created and decrypted non-secret note/);
  await app.close();

  app = spawnTui(binary, ['onboarding', '--yes'], { env, cols: 120, rows: 40 });
  const completed = await app.waitFor(/"first_success"\s*:\s*"audit_entry_observed"/, {
    timeoutMs: 120_000,
    useFullLog: true,
  });
  assert.match(completed, /Created and decrypted non-secret note: onboarding-safe-note-[0-9a-f]{8}/);
  assert.match(completed, /Observed hash-chained audit entry for item: onboarding-safe-note-[0-9a-f]{8}/);
  assert.match(completed, /The note value is not present in the audit record\./);
  assert.match(completed, /"status"\s*:\s*"completed"/);
  const itemId = completed.match(/onboarding-safe-note-[0-9a-f]{8}/)?.[0];
  assert.ok(itemId, 'expected the isolated demo item id in canonical completion output');
  await app.close();
  app = null;

  const audit = await run(
    ['audit-query', '--op', 'onboarding-demo-item-read', '--item', itemId],
    /"matched"\s*:\s*[1-9]\d*/,
  );
  assert.match(audit, /"op"\s*:\s*"onboarding-demo-item-read"/);
  assert.match(audit, new RegExp(`"item"\\s*:\\s*"${itemId}"`));
  assert.doesNotMatch(audit, /Skarbiec onboarding note; explicitly not a secret/);
} finally {
  if (app) await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
