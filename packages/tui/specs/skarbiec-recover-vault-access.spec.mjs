import assert from 'node:assert/strict';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-recover-vault-access-');
const ownerKeyring = join(tempDir, 'owner-keyring');
const recoveredKeyring = join(tempDir, 'recovered-keyring');
const recoveryBackup = join(tempDir, 'recovery-private-key.asc');
const vaultFile = join(tempDir, 'recovery-journey.vault.json');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

await mkdir(ownerKeyring, { mode: 0o700 });
await mkdir(recoveredKeyring, { mode: 0o700 });

const shellReady = '__SKARBIEC_RECOVER_VAULT_ACCESS_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: ownerKeyring,
      SKARBIEC_VAULT_FILE: vaultFile,
      SKARBIEC_AUDIT_FILE: join(tempDir, 'recovery-journey.audit.jsonl'),
    },
  },
);

let commandNumber = 0;
const runCommand = async (command, description, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_RECOVERY_COMMAND_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;

  app.send(
    `${command}; skarbiec_command_status=$?; printf '\\n${marker}:%s\\n' "$skarbiec_command_status"`,
  );
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const output = app.fullLog().slice(logStart);
  const markerIndex = output.indexOf(marker);
  const statusMatch = output.match(new RegExp(`${marker}:(\\d+)`));
  assert.ok(statusMatch, `expected completion status from ${description}`);
  assert.equal(Number(statusMatch[1]), 0, `${description} failed`);
  return output.slice(0, markerIndex).trim();
};

const runJson = async (args, options = {}) => {
  const { keyring = ownerKeyring, timeoutMs = 30_000 } = options;
  const command = [
    'env',
    `GNUPGHOME=${keyring}`,
    binary,
    ...args,
  ]
    .map(shellQuote)
    .join(' ');
  const description = args.join(' ') || 'command menu';
  const output = await runCommand(command, description, timeoutMs);
  const objectStart = output.indexOf('{');
  const arrayStart = output.indexOf('[');
  const jsonStart =
    objectStart === -1
      ? arrayStart
      : arrayStart === -1
        ? objectStart
        : Math.min(objectStart, arrayStart);

  assert.notEqual(jsonStart, -1, `expected JSON output from ${description}`);
  return JSON.parse(output.slice(jsonStart));
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  const commandMenu = await runJson([]);
  for (const command of ['init', 'set', 'get', 'recovery-status']) {
    assert.ok(commandMenu.commands.includes(command), `expected command menu to include ${command}`);
  }

  const ownerUid = 'recover-vault-access-e2e-owner';
  const secretId = 'recovery-proof-note';
  const secretValue = 'vault-access-restored-73d9c1';

  const initialized = await runJson(['init', ownerUid], { timeoutMs: 120_000 });
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);
  assert.match(initialized.owner_fpr, /^[0-9A-F]{40}$/);
  assert.match(initialized.recovery_fpr, /^[0-9A-F]{40}$/);
  assert.notEqual(initialized.owner_fpr, initialized.recovery_fpr);

  const stored = await runJson([
    'set',
    secretId,
    '--type',
    'note',
    `value=${secretValue}`,
  ]);
  assert.deepEqual(stored, { id: secretId, kind: 'note', ok: true });

  const recoveryStatus = await runJson(['recovery-status']);
  assert.equal(recoveryStatus.recovery_fpr, initialized.recovery_fpr);
  assert.equal(recoveryStatus.item_count, 1);
  assert.match(recoveryStatus.note, /shares one failure domain with the owner key/i);

  const beforeRecovery = await runJson(['get', secretId]);
  assert.deepEqual(beforeRecovery, {
    schema: 'skarbiec.item.v2',
    kind: 'note',
    fields: { value: secretValue },
    context: {},
  });

  const exportRecoveryKey = [
    'env',
    `GNUPGHOME=${ownerKeyring}`,
    'gpg',
    '--batch',
    '--yes',
    '--armor',
    '--output',
    recoveryBackup,
    '--export-secret-keys',
    initialized.recovery_fpr,
  ]
    .map(shellQuote)
    .join(' ');
  await runCommand(exportRecoveryKey, 'recovery-key backup');

  const importRecoveryKey = [
    'env',
    `GNUPGHOME=${recoveredKeyring}`,
    'gpg',
    '--batch',
    '--yes',
    '--import',
    recoveryBackup,
  ]
    .map(shellQuote)
    .join(' ');
  await runCommand(importRecoveryKey, 'recovery-key import');

  const listRecoveredKeys = [
    'env',
    `GNUPGHOME=${recoveredKeyring}`,
    'gpg',
    '--batch',
    '--with-colons',
    '--list-secret-keys',
  ]
    .map(shellQuote)
    .join(' ');
  const recoveredKeys = await runCommand(listRecoveredKeys, 'recovered keyring inspection');
  assert.match(recoveredKeys, new RegExp(initialized.recovery_fpr));
  assert.doesNotMatch(recoveredKeys, new RegExp(initialized.owner_fpr));

  const statusFromRecoveredKeyring = await runJson(['recovery-status'], {
    keyring: recoveredKeyring,
  });
  assert.equal(statusFromRecoveredKeyring.recovery_fpr, initialized.recovery_fpr);
  assert.equal(statusFromRecoveredKeyring.item_count, 1);

  const recoveredSecret = await runJson(['get', secretId], {
    keyring: recoveredKeyring,
  });
  assert.deepEqual(recoveredSecret, {
    schema: 'skarbiec.item.v2',
    kind: 'note',
    fields: { value: secretValue },
    context: {},
  });
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
