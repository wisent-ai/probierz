import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-json-output-');
const vaultFile = join(tempDir, 'json-output.vault.json');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const shellReady = '__SKARBIEC_JSON_OUTPUT_READY__';

const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: vaultFile,
      SKARBIEC_AUDIT_FILE: join(tempDir, 'json-output.audit.jsonl'),
    },
  },
);

let commandNumber = 0;
const runJson = async (args, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_JSON_OUTPUT_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [binary, ...args].map(shellQuote).join(' ');

  app.send(
    `${command}; command_status=$?; printf '\\n${marker}:%s\\n' "$command_status"`,
  );
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const commandLog = app.fullLog().slice(logStart);
  const markerIndex = commandLog.indexOf(marker);
  const statusMatch = commandLog.match(new RegExp(`${marker}:(\\d+)`));
  assert.ok(statusMatch, `missing exit status for skarbiec ${args.join(' ')}`);
  assert.equal(
    Number(statusMatch[1]),
    0,
    `skarbiec ${args.join(' ')} exited unsuccessfully`,
  );

  const output = commandLog.slice(0, markerIndex);
  const objectStart = output.indexOf('{');
  const arrayStart = output.indexOf('[');
  const jsonStart =
    objectStart === -1
      ? arrayStart
      : arrayStart === -1
        ? objectStart
        : Math.min(objectStart, arrayStart);
  assert.notEqual(jsonStart, -1, `skarbiec ${args.join(' ')} did not emit JSON`);

  const jsonText = output.slice(jsonStart).trim();
  const parsed = JSON.parse(jsonText);
  assert.ok(
    parsed !== null && typeof parsed === 'object',
    `skarbiec ${args.join(' ')} emitted a non-structured JSON value`,
  );
  return parsed;
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  const menu = await runJson([]);
  const requiredCommands = [
    'init',
    'set',
    'get',
    'list',
    'delete',
    'restore',
    'purge',
    'generate',
  ];
  assert.ok(Array.isArray(menu.commands));
  for (const command of requiredCommands) {
    assert.ok(menu.commands.includes(command), `command menu is missing ${command}`);
  }

  const initialized = await runJson(['init', 'json-output-e2e-owner'], 120_000);
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);
  assert.match(initialized.owner_fpr, /^[0-9A-F]+$/);
  assert.match(initialized.recovery_fpr, /^[0-9A-F]+$/);

  const secretId = 'machine-readable-note';
  const secretValue = 'json-secret-73c5d9';
  const stored = await runJson([
    'set',
    secretId,
    '--type',
    'note',
    `secret=${secretValue}`,
  ]);
  assert.deepEqual(stored, { id: secretId, ok: true });

  const retrieved = await runJson(['get', secretId]);
  assert.deepEqual(retrieved, { secret: secretValue, type: 'note' });

  const listed = await runJson(['list']);
  assert.equal(listed.length, 1);
  assert.equal(listed[0].id, secretId);
  assert.equal(listed[0].type, 'note');
  assert.equal(listed[0].deleted, false);

  const deleted = await runJson(['delete', secretId]);
  assert.deepEqual(deleted, { ok: true });
  assert.deepEqual(await runJson(['list']), []);

  const trashed = await runJson(['list', '--all']);
  assert.equal(trashed.length, 1);
  assert.equal(trashed[0].id, secretId);
  assert.equal(trashed[0].deleted, true);

  const restored = await runJson(['restore', secretId]);
  assert.deepEqual(restored, { ok: true });
  assert.deepEqual(await runJson(['get', secretId]), {
    secret: secretValue,
    type: 'note',
  });

  assert.deepEqual(await runJson(['delete', secretId]), { ok: true });
  assert.deepEqual(await runJson(['purge', secretId]), { ok: true });
  assert.deepEqual(await runJson(['list', '--all']), []);

  const generated = await runJson([
    'generate',
    '--length',
    '24',
    '--lower',
    '--upper',
    '--digits',
  ]);
  assert.equal(typeof generated.password, 'string');
  assert.equal(generated.password.length, 24);
  assert.match(generated.password, /^[A-Za-z0-9]+$/);
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
