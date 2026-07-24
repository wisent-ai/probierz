import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-delete-restore-');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

const shellReady = '__SKARBIEC_PTY_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: join(tempDir, 'journey.vault.json'),
      SKARBIEC_AUDIT_FILE: join(tempDir, 'journey.audit.jsonl'),
    },
  },
);

let commandNumber = 0;
const runJson = async (args, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_COMMAND_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [binary, ...args].map(shellQuote).join(' ');

  app.send(`${command} && printf '\\n${marker}\\n'`);
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const output = app.fullLog().slice(logStart);
  const appOutput = output.slice(0, output.indexOf(marker));
  const objectStart = appOutput.indexOf('{');
  const arrayStart = appOutput.indexOf('[');
  const jsonStart =
    objectStart === -1
      ? arrayStart
      : arrayStart === -1
        ? objectStart
        : Math.min(objectStart, arrayStart);

  assert.notEqual(jsonStart, -1, `expected JSON output from: ${args.join(' ')}`);
  return JSON.parse(appOutput.slice(jsonStart).trim());
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  const help = await runJson(['help']);
  assert.ok(help.commands.includes('delete'));
  assert.ok(help.commands.includes('restore'));

  const initialized = await runJson(['init', 'delete-restore-e2e-owner'], 120_000);
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, join(tempDir, 'journey.vault.json'));

  const secretId = 'recoverable-note';
  const secretValue = 'restored-secret-value-7f31';
  const created = await runJson([
    'set',
    secretId,
    '--type',
    'note',
    `secret=${secretValue}`,
  ]);
  assert.deepEqual(created, { id: secretId, ok: true });

  const liveBeforeDelete = await runJson(['list']);
  assert.equal(liveBeforeDelete.length, 1);
  assert.equal(liveBeforeDelete[0].id, secretId);
  assert.equal(liveBeforeDelete[0].deleted, false);

  const deleted = await runJson(['delete', secretId]);
  assert.deepEqual(deleted, { ok: true });

  const liveAfterDelete = await runJson(['list']);
  assert.deepEqual(liveAfterDelete, []);

  const trash = await runJson(['list', '--all']);
  assert.equal(trash.length, 1);
  assert.equal(trash[0].id, secretId);
  assert.equal(trash[0].deleted, true);

  const restored = await runJson(['restore', secretId]);
  assert.deepEqual(restored, { ok: true });

  const liveAfterRestore = await runJson(['list']);
  assert.equal(liveAfterRestore.length, 1);
  assert.equal(liveAfterRestore[0].id, secretId);
  assert.equal(liveAfterRestore[0].deleted, false);

  const recoveredSecret = await runJson(['get', secretId]);
  assert.deepEqual(recoveredSecret, {
    secret: secretValue,
    type: 'note',
  });
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
