import assert from 'node:assert/strict';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
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

  const artifacts = process.env.PROBIERZ_ARTIFACTS?.trim();
  const mediaManifest = process.env.PROBIERZ_MEDIA_MANIFEST?.trim();
  assert.ok(artifacts, 'PROBIERZ_ARTIFACTS is required for the onboarding evidence trace');
  assert.ok(mediaManifest, 'PROBIERZ_MEDIA_MANIFEST is required for the onboarding evidence trace');
  const tracePath = join(artifacts, 'skarbiec-onboarding-first-use.trace.json');
  await mkdir(dirname(tracePath), { recursive: true });
  await writeFile(tracePath, `${JSON.stringify({
    schemaVersion: 1,
    kind: 'probierz-skarbiec-onboarding-trace',
    evidenceLevel: 'E2',
    runId: process.env.PROBIERZ_RUN_ID || null,
    status: 'completed',
    observation: {
      firstSuccess: 'audit_entry_observed',
      itemId,
      auditOperation: 'onboarding-demo-item-read',
    },
    redaction: {
      status: 'verified_redacted',
      credentialsIncluded: false,
      itemValuesIncluded: false,
    },
    publicationRequirements: {
      artifactKind: 'trace',
      minimumEvidence: 'E2',
      redactionStatus: 'verified_redacted',
      signedReceiptRequired: true,
    },
  }, null, 2)}\n`, { mode: 0o600 });
  await mkdir(dirname(mediaManifest), { recursive: true });
  await writeFile(mediaManifest, `${JSON.stringify([{
    file: tracePath,
    kind: 'trace',
    contentType: 'application/json',
  }], null, 2)}\n`, { mode: 0o600 });
} finally {
  if (app) await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
