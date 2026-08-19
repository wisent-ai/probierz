// Journey: manage-service-routes.
//
// A workload asks the capability broker for a resource -- never for a vault item
// -- so the capability routes table is the only place the two vocabularies meet.
// This journey drives `skarbiec routes list|add|verify` through a real PTY against
// an isolated vault and an isolated routes table, and asserts what an operator can
// observe: a reasonless add is refused without touching the table, an add with a
// reason keeps the table it replaced beside the new one, a second add leaves the
// first route alone, `list` says whether the vault actually holds each item and
// each field, and `verify` refuses a table that cannot deliver and names both the
// resource and the problem.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnTui } from '../pty.mjs';

const binary =
  process.env.TUI_CMD ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec';

const manifestText = await readFile(
  new URL('../../../apps/skarbiec/probierz.yaml', import.meta.url),
  'utf8',
);
const sourceRoot = manifestText.match(/^  - root: (.+)$/m)?.[1]?.trim();
assert.ok(sourceRoot, 'skarbiec manifest must provide the source repository root');

const runCommand = (executable, arguments_) =>
  new Promise((resolve, reject) => {
    const child = spawn(executable, arguments_, { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
    child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code, signal) => { resolve({ code, signal, stdout, stderr }); });
  });

const revision = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'rev-parse', 'HEAD']);
assert.equal(revision.code, 0, `cannot resolve skarbiec source revision: ${revision.stderr}`);
const sourceRevision = revision.stdout.trim();
assert.match(sourceRevision, /^[0-9a-f]{40}$/, 'skarbiec source revision is not a full Git SHA');
const status = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'status', '--porcelain']);
assert.equal(status.code, 0, `cannot inspect skarbiec source state: ${status.stderr}`);
const sourceDirty = status.stdout !== '';

// Fixtures live under a cache root this run creates and removes, never in the
// shared temporary directory and never beside a real vault or a real routes
// table: every command below writes, and a live broker reads the real one.
const fixtureRoot = join(homedir(), 'Library', 'Caches', 'probierz-journeys');
await mkdir(fixtureRoot, { recursive: true });
const tempDir = await mkdtemp(join(fixtureRoot, 'skb-routes-'));
const vaultFile = join(tempDir, 'routes.vault.json');
const routesTable = join(tempDir, 'capability-routes.json');
const besideJournal = join(tempDir, 'capability-routes.audit.jsonl');
const routesAuditFile = join(tempDir, 'routes.audit.jsonl');
// The vault fixture is built with its own audit journal, because the journal is
// shared state a fixture write can poison: `skarbiec set` discards its audit
// result (`Vault::set_item_with_writer` calls `append_sync(..).ok()`), so a
// failed append reports `ok` and can leave a stamped `<journal>.append.lock` that
// nobody removes for the 30s abandonment window -- observed twice while authoring
// this journey, and a `routes add` sharing that journal then failed on the lock it
// did not leak. Keeping the journal this journey asserts on separate isolates it
// from that defect instead of asserting the defect is correct.
const setupAuditFile = join(tempDir, 'fixture-setup.audit.jsonl');

const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const shellReady = '__SKARBIEC_ROUTES_READY__';

const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      GNUPGHOME: tempDir,
      SKARBIEC_VAULT_FILE: vaultFile,
      SKARBIEC_AUDIT_FILE: routesAuditFile,
      SKARBIEC_CAPABILITY_ROUTES_FILE: routesTable,
    },
  },
);

const executed = [];
let commandNumber = 0;
const run = async (args, { env = {}, timeoutMs = 60_000, label } = {}) => {
  commandNumber += 1;
  const marker = `__SKARBIEC_ROUTES_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [
    ...Object.entries(env).map(([name, value]) => `${name}=${shellQuote(value)}`),
    ...[binary, ...args].map(shellQuote),
  ].join(' ');

  app.send(`${command}; command_status=$?; printf '\\n${marker}:%s\\n' "$command_status"`);
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const commandLog = app.fullLog().slice(logStart);
  const statusMatch = commandLog.match(new RegExp(`${marker}:(\\d+)`));
  assert.ok(statusMatch, `missing exit status for skarbiec ${args.join(' ')}`);
  const result = {
    status: Number(statusMatch[1]),
    output: commandLog.slice(0, commandLog.indexOf(marker)),
  };
  executed.push({ command: label || `skarbiec ${args.join(' ')}`, exitStatus: result.status });
  return result;
};

// The first complete JSON value in the output. `routes verify` prints its report
// on stdout and then fails, so parsing to the end of the stream is not enough.
const jsonFrom = (output, what) => {
  const start = output.search(/[[{]/);
  assert.notEqual(start, -1, `${what} emitted no JSON`);
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let index = start; index < output.length; index += 1) {
    const character = output[index];
    if (inString) {
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === '"') inString = false;
      continue;
    }
    if (character === '"') inString = true;
    else if (character === '{' || character === '[') depth += 1;
    else if (character === '}' || character === ']') {
      depth -= 1;
      if (depth === 0) return JSON.parse(output.slice(start, index + 1));
    }
  }
  throw new assert.AssertionError({ message: `${what} emitted truncated JSON` });
};

const okJson = async (args, options = {}) => {
  const result = await run(args, options);
  assert.equal(
    result.status,
    0,
    `skarbiec ${args.join(' ')} exited ${result.status}:\n${result.output.slice(-2000)}`,
  );
  return jsonFrom(result.output, `skarbiec ${args.join(' ')}`);
};

const emailResource = 'origin:https://dash.cloudflare.com/email';
const passwordResource = 'origin:https://dash.cloudflare.com/password';
const missingItemResource = 'provider:probierz-absent-item';
const missingFieldResource = 'provider:probierz-absent-field';
const loginItem = 'platform-admin-cloudflare';
const secretValue = 'routes-journey-secret-4b71e0';
const firstReason = 'weles was refused at the cloudflare dashboard login: no route for this origin';
const secondReason = 'same login form, password field';

const backupsBeside = async () =>
  (await readdir(tempDir))
    .filter((name) => name.startsWith('capability-routes.json.before-'))
    .sort();
const lines = async (path) => {
  const text = await readFile(path, 'utf8');
  return text.split('\n').filter((line) => line !== '').map((line) => JSON.parse(line));
};

const evidence = {};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  // The table this journey asserts on is the one the command resolves.
  const help = await okJson(['routes', 'help']);
  assert.equal(help.table, routesTable);
  assert.deepEqual(help.commands, [
    'routes list [<consumer>]',
    'routes add --resource <resource> --item <item> --field <field> --reason <text>',
    'routes verify [<consumer>]',
  ]);

  const initialized = await okJson(['init', 'routes-journey-owner'], {
    env: { SKARBIEC_AUDIT_FILE: setupAuditFile },
    timeoutMs: 180_000,
  });
  assert.equal(initialized.ok, true);
  assert.equal(initialized.vault, vaultFile);

  const stored = await okJson(
    ['set', loginItem, '--type', 'login', 'username=ops@cloudflare.invalid', `password=${secretValue}`],
    { env: { SKARBIEC_AUDIT_FILE: setupAuditFile }, label: `skarbiec set ${loginItem} (redacted)` },
  );
  assert.deepEqual(stored, { id: loginItem, kind: 'login', ok: true });

  // Everything from here on is the journey's own surface, and none of it may
  // carry the credential the fixture item holds.
  const routesPhaseStart = app.fullLog().length;

  // An absent table is not an empty table: it is refused by name, because every
  // resource the broker resolves would map to nothing.
  const listWithoutTable = await run(['routes', 'list']);
  assert.notEqual(listWithoutTable.status, 0, 'routes list accepted an absent table');
  assert.ok(
    listWithoutTable.output.includes(`no capability routes table at ${routesTable}`),
    `routes list did not name the absent table:\n${listWithoutTable.output.slice(-2000)}`,
  );
  assert.deepEqual(await backupsBeside(), []);

  const firstAdd = await okJson([
    'routes', 'add',
    '--resource', emailResource,
    '--item', loginItem,
    '--field', 'username',
    '--reason', firstReason,
  ]);
  assert.deepEqual(firstAdd, {
    added: true,
    resource: emailResource,
    item: loginItem,
    field: 'username',
    backup: null,
  });
  const tableAfterFirstAdd = await readFile(routesTable);
  assert.deepEqual(JSON.parse(tableAfterFirstAdd.toString('utf8')), {
    [emailResource]: { item: loginItem, field: 'username' },
  });
  const besideAfterFirstAdd = await lines(besideJournal);
  assert.equal(besideAfterFirstAdd.length, 1);
  assert.equal(besideAfterFirstAdd[0].reason, firstReason);
  assert.equal(besideAfterFirstAdd[0].resource, emailResource);
  const chainedAfterFirstAdd = await lines(routesAuditFile);
  assert.equal(chainedAfterFirstAdd.length, 1);
  assert.equal(chainedAfterFirstAdd[0].op, 'capability-route-added');
  assert.equal(chainedAfterFirstAdd[0].extra.reason, firstReason);

  // A reasonless add is refused before anything is read or written: this table
  // decides which credential a login form receives.
  const reasonless = await run([
    'routes', 'add',
    '--resource', passwordResource,
    '--item', loginItem,
    '--field', 'password',
  ]);
  assert.notEqual(reasonless.status, 0, 'routes add without --reason was accepted');
  assert.ok(
    reasonless.output.includes('routes add requires an exact --reason'),
    `routes add without --reason did not report the missing reason:\n${reasonless.output.slice(-2000)}`,
  );
  assert.deepEqual(
    await readFile(routesTable),
    tableAfterFirstAdd,
    'a refused routes add still rewrote the table',
  );
  assert.deepEqual(await lines(besideJournal), besideAfterFirstAdd);
  assert.deepEqual(await lines(routesAuditFile), chainedAfterFirstAdd);
  assert.deepEqual(await backupsBeside(), [], 'a refused routes add still snapshotted the table');

  const secondAdd = await okJson([
    'routes', 'add',
    '--resource', passwordResource,
    '--item', loginItem,
    '--field', 'password',
    '--reason', secondReason,
  ]);
  assert.equal(secondAdd.added, true);
  assert.equal(secondAdd.resource, passwordResource);
  assert.equal(secondAdd.item, loginItem);
  assert.equal(secondAdd.field, 'password');
  assert.equal(typeof secondAdd.backup, 'string', 'routes add did not report a retained backup');
  assert.ok(secondAdd.backup.startsWith(`${routesTable}.before-`));
  assert.deepEqual(
    await readFile(secondAdd.backup),
    tableAfterFirstAdd,
    'the retained backup does not hold the table as it stood before the add',
  );
  // Idempotent in the sense that matters: the route already there is untouched.
  const tableAfterSecondAdd = await readFile(routesTable);
  assert.deepEqual(JSON.parse(tableAfterSecondAdd.toString('utf8')), {
    [emailResource]: { item: loginItem, field: 'username' },
    [passwordResource]: { item: loginItem, field: 'password' },
  });

  const repeatedAdd = await okJson([
    'routes', 'add',
    '--resource', passwordResource,
    '--item', loginItem,
    '--field', 'password',
    '--reason', 'provisioning sequence ran again',
  ]);
  assert.deepEqual(repeatedAdd, {
    added: false,
    resource: passwordResource,
    item: loginItem,
    field: 'password',
    backup: null,
  });
  assert.deepEqual(
    await readFile(routesTable),
    tableAfterSecondAdd,
    'a repeated routes add rewrote the table',
  );
  assert.equal((await lines(besideJournal)).length, 2, 'a repeated routes add recorded a mutation');
  assert.deepEqual(await backupsBeside(), [`${secondAdd.backup.split('/').at(-1)}`]);

  // Each route with its item, its field, and whether the vault holds both.
  const soundList = await okJson(['routes', 'list']);
  assert.equal(soundList.consumer, null);
  assert.deepEqual(soundList.routes, [
    {
      resource: emailResource,
      item: loginItem,
      field: 'username',
      item_present: true,
      field_present: true,
    },
    {
      resource: passwordResource,
      item: loginItem,
      field: 'password',
      item_present: true,
      field_present: true,
    },
  ]);

  // The optional argument narrows what is printed and authorises nothing.
  const narrowed = await okJson(['routes', 'list', 'dash.cloudflare.com/password']);
  assert.equal(narrowed.consumer, 'dash.cloudflare.com/password');
  assert.equal(narrowed.routes.length, 1);
  assert.equal(narrowed.routes[0].resource, passwordResource);

  const soundVerify = await okJson(['routes', 'verify']);
  assert.deepEqual(soundVerify, { checked: 2, broken: [] });
  evidence.soundVerify = soundVerify;

  // Two routes an operator could add today and only discover at a login: one
  // names an item the vault does not hold, one a field its item does not carry.
  const brokenItemAdd = await okJson([
    'routes', 'add',
    '--resource', missingItemResource,
    '--item', 'absent-login-item',
    '--field', 'username',
    '--reason', 'journey: route naming an item this vault does not hold',
  ]);
  assert.equal(brokenItemAdd.added, true);
  const brokenFieldAdd = await okJson([
    'routes', 'add',
    '--resource', missingFieldResource,
    '--item', loginItem,
    '--field', 'totp_secret',
    '--reason', 'journey: route naming a field this item does not carry',
  ]);
  assert.equal(brokenFieldAdd.added, true);

  const brokenList = await okJson(['routes', 'list']);
  const row = (resource) => {
    const found = brokenList.routes.find((entry) => entry.resource === resource);
    assert.ok(found, `routes list omitted ${resource}`);
    return found;
  };
  assert.equal(brokenList.routes.length, 4);
  assert.deepEqual(row(missingItemResource), {
    resource: missingItemResource,
    item: 'absent-login-item',
    field: 'username',
    item_present: false,
    field_present: false,
  });
  assert.deepEqual(row(missingFieldResource), {
    resource: missingFieldResource,
    item: loginItem,
    field: 'totp_secret',
    item_present: true,
    field_present: false,
  });
  assert.equal(row(emailResource).field_present, true);
  assert.equal(row(passwordResource).field_present, true);

  // The report reaches stdout even though the command fails: a console renders
  // the rows, a provisioning sequence reads the status.
  const brokenVerify = await run(['routes', 'verify']);
  assert.notEqual(brokenVerify.status, 0, 'routes verify passed a table that cannot deliver');
  const brokenReport = jsonFrom(brokenVerify.output, 'skarbiec routes verify');
  assert.equal(brokenReport.checked, 4);
  assert.deepEqual(
    [...brokenReport.broken].sort((left, right) => left.resource.localeCompare(right.resource)),
    [
      { resource: missingFieldResource, problem: `vault item ${loginItem} has no totp_secret field` },
      { resource: missingItemResource, problem: 'no vault item absent-login-item' },
    ],
  );
  assert.ok(
    brokenVerify.output.includes('2 of 4 capability routes do not resolve'),
    `routes verify did not summarise the broken routes:\n${brokenVerify.output.slice(-2000)}`,
  );
  evidence.brokenVerify = brokenReport;

  // Nothing on this surface hands out the credential the routes point at.
  const routesPhaseLog = app.fullLog().slice(routesPhaseStart);
  assert.ok(
    !routesPhaseLog.includes(secretValue),
    'a capability routes command emitted the secret its route points at',
  );
  for (const path of [routesTable, besideJournal, routesAuditFile]) {
    const contents = await readFile(path, 'utf8');
    assert.ok(!contents.includes(secretValue), `${path} carries secret material`);
  }

  const artifacts = process.env.PROBIERZ_ARTIFACTS;
  const mediaManifest = process.env.PROBIERZ_MEDIA_MANIFEST;
  assert.ok(artifacts, 'PROBIERZ_ARTIFACTS is required');
  assert.ok(mediaManifest, 'PROBIERZ_MEDIA_MANIFEST is required');
  const tracePath = join(artifacts, 'skarbiec-manage-service-routes.trace.json');
  await mkdir(dirname(tracePath), { recursive: true });
  await writeFile(tracePath, `${JSON.stringify({
    schemaVersion: 1,
    kind: 'probierz-skarbiec-manage-service-routes-trace',
    journey: 'manage-service-routes',
    runId: process.env.PROBIERZ_RUN_ID || null,
    status: 'completed',
    observation: {
      sourceRoot,
      sourceRevision,
      sourceDirty,
      binary,
      routesTable,
      commands: executed,
      soundVerify: evidence.soundVerify,
      brokenVerify: evidence.brokenVerify,
      retainedBackup: secondAdd.backup,
    },
    contracts: [
      'routes add without --reason exits non-zero and leaves the table, both journals, and the backup series untouched',
      'routes add with --reason reports the added resource, item, and field and retains the previous table as the backup path it names',
      'a second routes add leaves the existing route untouched, and repeating one reports added=false with no backup and no mutation',
      'routes list reports every route with its item, its field, and whether the vault holds that item and that field',
      'routes verify exits zero with no broken entries on a sound table',
      'routes verify exits non-zero on a broken table and names each broken resource and its problem on stdout',
      'no capability routes command emits the credential its routes point at',
    ],
    redaction: {
      status: 'verified_redacted',
      credentialsIncluded: false,
      privateRecordsIncluded: false,
    },
    publicationRequirements: {
      artifactKind: 'trace',
      minimumEvidence: 'E2',
      redactionStatus: 'verified_redacted',
    },
  }, null, 2)}\n`, { mode: 0o600 });
  await mkdir(dirname(mediaManifest), { recursive: true });
  await writeFile(mediaManifest, `${JSON.stringify([{
    file: tracePath,
    kind: 'trace',
    contentType: 'application/json',
  }], null, 2)}\n`, { mode: 0o600 });
} finally {
  await app.close();
  await rm(tempDir, { recursive: true, force: true });
}
