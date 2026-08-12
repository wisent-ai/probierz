import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-manage-secrets-');
const vaultFile = join(tempDir, 'manage-secrets.vault.json');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

const shellReady = '__SKARBIEC_MANAGE_SECRETS_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: vaultFile,
      SKARBIEC_AUDIT_FILE: join(tempDir, 'manage-secrets.audit.jsonl'),
    },
  },
);

let commandNumber = 0;
const runJson = async (args, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_MANAGE_SECRETS_COMMAND_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [binary, ...args].map(shellQuote).join(' ');

  app.send(
    `${command}; skarbiec_command_status=$?; printf '\\n${marker}:%s\\n' "$skarbiec_command_status"`,
  );
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const output = app.fullLog().slice(logStart);
  const appOutput = output.slice(0, output.indexOf(marker));
  const statusMatch = output.match(new RegExp(`${marker}:(\\d+)`));
  assert.ok(statusMatch, `expected completion status from: ${args.join(' ') || 'command menu'}`);
  assert.equal(
    Number(statusMatch[1]),
    0,
    `skarbiec command failed: ${args.join(' ') || 'command menu'}`,
  );
  const objectStart = appOutput.indexOf('{');
  const arrayStart = appOutput.indexOf('[');
  const jsonStart =
    objectStart === -1
      ? arrayStart
      : arrayStart === -1
        ? objectStart
        : Math.min(objectStart, arrayStart);

  assert.notEqual(jsonStart, -1, `expected JSON output from: ${args.join(' ') || 'command menu'}`);
  return JSON.parse(appOutput.slice(jsonStart).trim());
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  const commandMenu = await runJson([]);
  assert.ok(commandMenu.commands.includes('init'));
  assert.ok(commandMenu.commands.includes('set'));
  assert.ok(commandMenu.commands.includes('get'));
  assert.ok(commandMenu.commands.includes('list'));

  const initialized = await runJson(['init', 'manage-secrets-e2e-owner'], 120_000);
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);

  const secrets = [
    {
      id: 'database-password',
      value: 'db-secret-4f96d2',
    },
    {
      id: 'service-api-token',
      value: 'api-token-8c13ab',
    },
  ];

  for (const secret of secrets) {
    const stored = await runJson([
      'set',
      secret.id,
      '--type',
      'note',
      `value=${secret.value}`,
    ]);
    assert.deepEqual(stored, { id: secret.id, kind: 'note', ok: true });
  }

  for (const secret of secrets) {
    const retrieved = await runJson(['get', secret.id]);
    assert.deepEqual(retrieved, {
      schema: 'skarbiec.item.v2',
      kind: 'note',
      fields: { value: secret.value },
      context: {},
    });
  }

  const listed = await runJson(['list']);
  assert.equal(listed.length, secrets.length);
  assert.deepEqual(
    listed.map(({ id }) => id).sort(),
    secrets.map(({ id }) => id).sort(),
  );
  for (const item of listed) {
    assert.equal(item.deleted, false);
  }
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
