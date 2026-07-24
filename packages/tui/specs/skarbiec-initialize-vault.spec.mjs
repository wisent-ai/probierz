import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';
const tempDir = await mkdtemp('/tmp/skarbiec-initialize-vault-');
const vaultFile = join(tempDir, 'fresh.vault.json');
const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

const shellReady = '__SKARBIEC_INITIALIZE_VAULT_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: vaultFile,
    },
  },
);

let commandNumber = 0;
const runJson = async (args, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_INITIALIZE_COMMAND_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [binary, ...args].map(shellQuote).join(' ');

  app.send(`${command} && printf '\\n${marker}\\n'`);
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const output = app.fullLog().slice(logStart);
  const appOutput = output.slice(0, output.indexOf(marker));
  const jsonStart = appOutput.indexOf('{');
  assert.notEqual(jsonStart, -1, `expected JSON output from: ${args.join(' ') || 'command menu'}`);
  return JSON.parse(appOutput.slice(jsonStart).trim());
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  const commandMenu = await runJson([]);
  assert.ok(commandMenu.commands.includes('init'));

  const initialized = await runJson(['init', 'initialize-vault-e2e-owner'], 120_000);
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);
  assert.match(initialized.owner_fpr, /^[0-9A-F]{40}$/);
  assert.match(initialized.recovery_fpr, /^[0-9A-F]{40}$/);
  assert.notEqual(initialized.owner_fpr, initialized.recovery_fpr);
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
