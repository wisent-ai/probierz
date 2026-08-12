import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-manage-users-sharing-');
const vaultFile = join(tempDir, 'manage-users-and-sharing.vault.json');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

const shellReady = '__SKARBIEC_MANAGE_USERS_SHARING_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: vaultFile,
      SKARBIEC_AUDIT_FILE: join(tempDir, 'manage-users-and-sharing.audit.jsonl'),
    },
  },
);

let commandNumber = 0;
const runJson = async (args, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_MANAGE_USERS_SHARING_COMMAND_${commandNumber}_DONE__`;
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
  for (const command of ['init', 'set', 'add-user', 'share', 'users', 'revoke']) {
    assert.ok(commandMenu.commands.includes(command), `expected command menu to include ${command}`);
  }

  const ownerUid = 'sharing-journey-owner';
  const memberUid = 'sharing-journey-member';
  const secretId = 'shared-deployment-note';
  const secretValue = 'deployment-secret-51c9e7';

  const initialized = await runJson(['init', ownerUid], 120_000);
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);

  const stored = await runJson([
    'set',
    secretId,
    '--type',
    'note',
    `value=${secretValue}`,
  ]);
  assert.deepEqual(stored, { id: secretId, kind: 'note', ok: true });

  const added = await runJson(['add-user', memberUid, '--role', 'member'], 120_000);
  assert.equal(added.ok, true);
  assert.equal(added.uid, memberUid);
  assert.equal(added.role, 'member');
  assert.match(added.fingerprint, /^[0-9A-F]{40}$/);

  const shared = await runJson(['share', secretId, memberUid]);
  assert.equal(shared.ok, true);
  assert.equal(shared.item, secretId);
  assert.deepEqual(shared.recipients, [memberUid]);

  const users = await runJson(['users']);
  assert.deepEqual(Object.keys(users).sort(), [memberUid, ownerUid].sort());
  assert.equal(users[ownerUid].role, 'owner');
  assert.match(users[ownerUid].fingerprint, /^[0-9A-F]{40}$/);
  assert.equal(users[memberUid].role, 'member');
  assert.equal(users[memberUid].fingerprint, added.fingerprint);
  assert.match(users[memberUid].added_at, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);

  const revoked = await runJson(['revoke', secretId, memberUid]);
  assert.equal(revoked.ok, true);
  assert.equal(revoked.item, secretId);
  assert.deepEqual(revoked.recipients, []);
  assert.ok(!revoked.recipients.includes(memberUid));

  const ownerCanStillRead = await runJson(['get', secretId]);
  assert.deepEqual(ownerCanStillRead, {
    schema: 'skarbiec.item.v2',
    kind: 'note',
    fields: { value: secretValue },
    context: {},
  });
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
