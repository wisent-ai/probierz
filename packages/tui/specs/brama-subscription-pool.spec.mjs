// Journey: subscription-pool.
//
// An operator whose `best`-aliased calls all answer 429 needs two things from
// Brama: a reading of the pool that is safe to run against a gateway serving
// traffic, and a repair that records why it was run. This spec exercises both
// halves against a fixture deployment: a fake entitlements router that serves
// one vault listing and refuses every other verb, and a usage ledger seeded so
// each of the four documented credential states is on the report.
//
// Nothing here contacts a provider. The refresh half stops at the refusals --
// the missing `--reason`, and the billable path's missing cost acknowledgement
// -- both of which the CLI answers before any credential is read, which the
// fixture router's own invocation log is asserted to prove.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnTui } from '../pty.mjs';

function requiredEnv(name, hint) {
  const value = process.env[name]?.trim();
  assert.ok(value, `${name} is required: ${hint}`);
  return value;
}

const binary = requiredEnv('TUI_CMD', 'provide the released Brama executable');
const artifacts = requiredEnv('PROBIERZ_ARTIFACTS', 'provide the run artifact directory');
const mediaManifest = requiredEnv('PROBIERZ_MEDIA_MANIFEST', 'provide the report media manifest path');

// The source revision this evidence describes comes from the app manifest's own
// repository root, so a trace can never claim a revision the run did not read.
const manifestText = await readFile(new URL('../../../apps/brama/probierz.yaml', import.meta.url), 'utf8');
const sourceRoot = manifestText.match(/^ {2}- root: (.+)$/m)?.[1]?.trim();
assert.ok(sourceRoot, 'the Brama manifest must provide the source repository root');

async function runCommand(executable, argumentList, { cwd } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, argumentList, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
    child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code, signal) => { resolve({ code, signal, stdout, stderr }); });
  });
}

const revision = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'rev-parse', 'HEAD']);
assert.equal(revision.code, 0, `cannot resolve the Brama source revision: ${revision.stderr}`);
const sourceRevision = revision.stdout.trim();
assert.match(sourceRevision, /^[0-9a-f]{40}$/, 'the Brama source revision is not a full Git SHA');
const worktree = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'status', '--porcelain']);
assert.equal(worktree.code, 0, `cannot inspect the Brama source state: ${worktree.stderr}`);
const sourceDirty = worktree.stdout.trim() !== '';

// Scratch state lives under this run's own cache directory, never /tmp, and
// only this directory is removed at the end.
const scratchRoot = join(homedir(), 'Library', 'Caches', 'probierz-journeys');
await mkdir(scratchRoot, { recursive: true });
const tempDir = await mkdtemp(join(scratchRoot, 'brama-subscription-pool-'));

const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const stateDir = join(tempDir, 'state');
const fixtureHome = join(tempDir, 'home');
const ledgerPath = join(stateDir, 'subscription-usage.json');
const journalPath = join(stateDir, 'journal.jsonl');
const routerBin = join(tempDir, 'bin', 'entitlements-router');
const routerLogPath = join(tempDir, 'entitlements-router.invocations');
const vaultListPath = join(tempDir, 'vault-list.json');
// Planted in the fixture vault listing's item payload. Brama must surface item
// ids, providers and states and never a value from the vault, so this string
// appearing in the report at all is a leak.
const vaultPayloadSentinel = 'probierz-fixture-vault-payload-must-never-be-printed';

const now = Date.now();
const at = (offsetMs) => now + offsetMs;
const burntExpiry = at(-1_800_000);
const expiredExpiry = at(-600_000);
const liveExpiry = at(86_400_000);
const shortLiveExpiry = at(3_600_000);
const activeBlockReason = '429 from provider: this account is over its plan';
const lapsedBlockReason = '429 from provider: a lapsed block that must not be reported';
const burntCause = 'invalid_grant: refresh token is no longer accepted';
const retiredCause = 'retired by an operator';

const vaultItem = (itemId, provider, subscriptionId, agent, deleted = false) => ({
  id: itemId,
  type: 'subscription',
  tags: [
    'brama:subscription',
    `brama:agent:${agent}`,
    `brama:provider:${provider}`,
    `brama:id:${subscriptionId}`,
  ],
  deleted,
  updated_at: '2026-08-01T00:00:00Z',
  versions: [{ version: 1, value: vaultPayloadSentinel }],
});

// The deployment listing the gateway routes over. `claude_code` is spelled with
// an underscore on purpose: one account must not be two providers.
const vaultListing = [
  vaultItem('provider:codex:primary', 'codex', 'brama-sub-fixture-codex-primary', 'wisent-app'),
  vaultItem('provider:codex:secondary', 'codex', 'brama-sub-fixture-codex-secondary', 'wisent-app'),
  vaultItem('provider:kimi:primary', 'kimi', 'brama-sub-fixture-kimi-primary', 'other-agent'),
  vaultItem('provider:kimi:secondary', 'kimi', 'brama-sub-fixture-kimi-secondary', 'wisent-app'),
  vaultItem('provider:claude_code:primary', 'claude_code', 'brama-sub-fixture-claude-code-primary', 'wisent-app'),
  vaultItem('provider:codex:removed', 'codex', 'brama-sub-fixture-codex-removed', 'wisent-app', true),
];

// The ledger the listing is joined to. Every documented state is represented,
// including the two rows that only one of the two sources knows about.
const ledger = {
  subscriptions: {
    'brama-sub-fixture-codex-primary': {
      provider: 'codex',
      credential: {
        state: 'needs_reauthorization',
        cause: burntCause,
        recorded_at_ms: at(-3_600_000),
        expires_at_ms: burntExpiry,
      },
    },
    'brama-sub-fixture-codex-secondary': {
      provider: 'codex',
      credential: { state: 'active', recorded_at_ms: at(-7_200_000), expires_at_ms: expiredExpiry },
      block: { blocked_until_ms: at(1_800_000), reason: activeBlockReason, recorded_at_ms: at(-900_000) },
    },
    'brama-sub-fixture-kimi-primary': {
      provider: 'kimi',
      credential: { state: 'active', recorded_at_ms: at(-300_000), expires_at_ms: liveExpiry },
    },
    'brama-sub-fixture-kimi-secondary': {
      provider: 'kimi',
      credential: { state: 'active', recorded_at_ms: at(-300_000), expires_at_ms: shortLiveExpiry },
      block: { blocked_until_ms: at(-60_000), reason: lapsedBlockReason, recorded_at_ms: at(-3_600_000) },
    },
    // Only the ledger knows this one: its vault item is gone, the burnt grant is not.
    'brama-sub-fixture-codex-retired': {
      provider: 'codex',
      credential: { state: 'disabled', cause: retiredCause, recorded_at_ms: at(-86_400_000) },
    },
  },
};

// Every subscription the report must carry, in the order the product emits, and
// what each row must say. `expires_at` is compared as an instant, because the
// contract is the provider's own instant rather than one spelling of it.
const expectedRows = [
  {
    subscription_id: 'brama-sub-fixture-claude-code-primary',
    provider: 'claude-code',
    state: 'unknown',
    expiresAtMs: null,
    last_redeem_error: null,
  },
  {
    subscription_id: 'brama-sub-fixture-codex-primary',
    provider: 'codex',
    state: 'burnt',
    expiresAtMs: burntExpiry,
    last_redeem_error: burntCause,
  },
  {
    subscription_id: 'brama-sub-fixture-codex-retired',
    provider: 'codex',
    state: 'burnt',
    expiresAtMs: null,
    last_redeem_error: retiredCause,
  },
  {
    subscription_id: 'brama-sub-fixture-codex-secondary',
    provider: 'codex',
    state: 'expired',
    expiresAtMs: expiredExpiry,
    last_redeem_error: activeBlockReason,
  },
  {
    subscription_id: 'brama-sub-fixture-kimi-primary',
    provider: 'kimi',
    state: 'live',
    expiresAtMs: liveExpiry,
    last_redeem_error: null,
  },
  {
    subscription_id: 'brama-sub-fixture-kimi-secondary',
    provider: 'kimi',
    state: 'live',
    expiresAtMs: shortLiveExpiry,
    last_redeem_error: null,
  },
];
const ROW_FIELDS = ['expires_at', 'last_redeem_error', 'provider', 'state', 'subscription_id'];
const DOCUMENTED_STATES = new Set(['live', 'expired', 'burnt', 'unknown']);

await mkdir(stateDir, { recursive: true });
await mkdir(fixtureHome, { recursive: true });
await mkdir(dirname(routerBin), { recursive: true });
await writeFile(vaultListPath, `${JSON.stringify(vaultListing, null, 2)}\n`, { mode: 0o600 });
await writeFile(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 });
// A journal the reader may consult and must not append to.
await writeFile(journalPath, '', { mode: 0o600 });
// The only channel between Brama and any credential is the entitlements
// router. This stand-in serves the deployment listing, logs every invocation so
// the spec can prove which verbs were used, and refuses every other verb, so a
// capability redemption inside this fixture cannot succeed quietly.
await writeFile(routerBin, [
  '#!/bin/sh',
  `printf '%s\\n' "$*" >> ${shellQuote(routerLogPath)}`,
  `if [ "$1" = list ]; then cat ${shellQuote(vaultListPath)}; exit 0; fi`,
  "printf 'fixture entitlements router refuses %s: a Probierz fixture redeems no capability\\n' \"$1\" >&2",
  'exit 3',
  '',
].join('\n'), { mode: 0o700 });
await chmod(routerBin, 0o700);

const env = {
  HOME: fixtureHome,
  XDG_STATE_HOME: join(tempDir, 'xdg-state'),
  BRAMA_STATE_DIR: stateDir,
  BRAMA_SUBSCRIPTION_USAGE_FILE: ledgerPath,
  BRAMA_MODEL_CATALOG_CACHE: join(tempDir, 'model-catalog.json'),
  BRAMA_PERF_PATH: join(tempDir, 'perf.json'),
  BRAMA_DONATED_SUBSCRIPTIONS_FILE: join(tempDir, 'donated-subscriptions.json'),
  BRAMA_SUBSCRIPTION_CATALOG: '{"items":[]}',
  SKARBIEC_CAPABILITY_ROUTES_FILE: join(tempDir, 'capability-routes.json'),
  ENTITLEMENTS_ROUTER_BIN: routerBin,
};

const fingerprint = async (root) => {
  const rows = [];
  const walk = async (directory) => {
    let entries;
    try {
      entries = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of [...entries].sort((left, right) => left.name.localeCompare(right.name))) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) {
        rows.push(`${full}/`);
        await walk(full);
        continue;
      }
      const bytes = await readFile(full);
      const info = await stat(full);
      rows.push(`${full}\t${bytes.length}\t${createHash('sha256').update(bytes).digest('hex')}\t${info.mtimeMs}`);
    }
  };
  await walk(root);
  return rows.join('\n');
};
const fixtureState = async () => `${await fingerprint(stateDir)}\n--\n${await fingerprint(fixtureHome)}`;
const routerInvocations = async () => {
  try {
    return (await readFile(routerLogPath, 'utf8')).split('\n').filter((line) => line.trim() !== '');
  } catch {
    return [];
  }
};

const shellReady = '__BRAMA_POOL_SHELL_READY__';
const app = spawnTui('/bin/sh', ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`], { env });

let commandNumber = 0;
const run = async (argumentList, timeoutMs = 30_000) => {
  commandNumber += 1;
  const marker = `__BRAMA_POOL_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  const command = [binary, ...argumentList].map(shellQuote).join(' ');
  app.send(`${command}; command_status=$?; printf '\\n${marker}:%s\\n' "$command_status"`);
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });

  const commandLog = app.fullLog().slice(logStart);
  const statusMatch = commandLog.match(new RegExp(`${marker}:(\\d+)`));
  assert.ok(statusMatch, `missing exit status for brama ${argumentList.join(' ')}`);
  return {
    status: Number(statusMatch[1]),
    output: commandLog.slice(0, commandLog.indexOf(marker)),
  };
};

const parseJson = (result, label) => {
  const start = result.output.indexOf('{');
  assert.notEqual(start, -1, `${label} emitted no JSON`);
  const parsed = JSON.parse(result.output.slice(start).trim());
  assert.ok(parsed !== null && typeof parsed === 'object', `${label} emitted a non-structured JSON value`);
  return parsed;
};

const observations = { refusals: [] };
try {
  await app.waitFor(shellReady, { timeoutMs: 30_000, useFullLog: true });
  const stateBeforeRead = await fixtureState();

  // ---- The read: `subscriptions list --json` ----------------------------
  const listJson = await run(['subscriptions', 'list', '--json']);
  assert.equal(listJson.status, 0, `subscriptions list --json exited ${listJson.status}`);
  const report = parseJson(listJson, 'subscriptions list --json');
  assert.deepEqual(Object.keys(report), ['providers'], 'the pool report carries exactly one key');
  assert.ok(Array.isArray(report.providers), '`providers` is an array');

  // The join, not a concatenation: one row per subscription, each source able
  // to widen the other, and a deleted vault item on neither.
  const ids = report.providers.map((row) => row.subscription_id);
  assert.deepEqual(ids, expectedRows.map((row) => row.subscription_id), 'the pool lists exactly the joined subscriptions, deterministically ordered');
  assert.equal(new Set(ids).size, ids.length, 'a subscription known to both the listing and the ledger is one row, not two');
  assert.ok(ids.includes('brama-sub-fixture-claude-code-primary'), 'a subscription the listing alone knows is on the report');
  assert.ok(ids.includes('brama-sub-fixture-codex-retired'), 'a subscription the ledger alone knows is on the report');
  assert.ok(!ids.includes('brama-sub-fixture-codex-removed'), 'a deleted vault item is not a subscription');

  for (const [index, expected] of expectedRows.entries()) {
    const row = report.providers[index];
    const where = `row ${index} (${expected.subscription_id})`;
    assert.deepEqual(Object.keys(row).sort(), ROW_FIELDS, `${where} carries exactly the documented fields`);
    assert.equal(row.subscription_id, expected.subscription_id, `${where} names its subscription`);
    assert.equal(row.provider, expected.provider, `${where} names its provider, normalized`);
    assert.ok(DOCUMENTED_STATES.has(row.state), `${where} state ${row.state} is one of the four documented words`);
    assert.equal(row.state, expected.state, `${where} state`);
    assert.equal(row.last_redeem_error, expected.last_redeem_error, `${where} last_redeem_error`);
    if (expected.expiresAtMs === null) {
      assert.equal(row.expires_at, null, `${where} states no expiry`);
    } else {
      assert.equal(typeof row.expires_at, 'string', `${where} states an expiry`);
      assert.match(row.expires_at, /^\d{4}-\d{2}-\d{2}T[\d:.]+\+00:00$/, `${where} expiry is an instant a human reads`);
      assert.equal(Date.parse(row.expires_at), expected.expiresAtMs, `${where} expiry is the provider's own instant`);
    }
  }

  // A block still in force is the refusal in the way; a lapsed one is not
  // reported beside a live grant, though the ledger still holds it.
  const lapsed = report.providers.find((row) => row.subscription_id === 'brama-sub-fixture-kimi-secondary');
  assert.equal(lapsed.state, 'live', 'a grant whose block has lapsed is live');
  assert.equal(lapsed.last_redeem_error, null, 'a lapsed block is not reported as a refusal');
  assert.ok((await readFile(ledgerPath, 'utf8')).includes(lapsedBlockReason), 'the fixture ledger does hold the lapsed block that was not reported');

  // ---- No credential material in the output ----------------------------
  const reportText = JSON.stringify(report);
  assert.ok(!reportText.includes(vaultPayloadSentinel), 'no vault item payload reaches the pool report');
  const credentialShapedKey = /token|secret|password|bearer|credential|authorization|api[-_]?key|grant_type|client_secret/i;
  const keys = new Set(report.providers.flatMap((row) => Object.keys(row)));
  for (const key of ['providers', ...keys]) {
    assert.ok(!credentialShapedKey.test(key), `the report emits no credential-shaped field: ${key}`);
  }
  // Values are exempted from the key test on purpose: `last_redeem_error` is
  // the provider's own refusal sentence and legitimately says "refresh token
  // is no longer accepted". Credential *material* is what must be absent.
  for (const value of report.providers.flatMap((row) => Object.values(row)).filter((value) => typeof value === 'string')) {
    assert.ok(!value.includes(vaultPayloadSentinel), 'no row value carries vault payload');
    assert.doesNotMatch(value, /\bsk-[A-Za-z0-9_-]{8,}/, 'no row value carries an API key');
    assert.doesNotMatch(value, /\beyJ[A-Za-z0-9_-]{8,}\./, 'no row value carries a JWT');
    assert.doesNotMatch(value, /\bBearer\s+\S+/i, 'no row value carries a bearer credential');
  }

  // ---- The read is read-only -------------------------------------------
  const readInvocations = await routerInvocations();
  assert.ok(readInvocations.length > 0, 'the report read the deployment listing');
  for (const invocation of readInvocations) {
    assert.equal(invocation, 'list', `the report used only the listing verb, not: ${invocation}`);
  }
  assert.equal(await fixtureState(), stateBeforeRead, 'subscriptions list wrote nothing');

  // The same pool read twice is the same answer: the reader neither mutates
  // the ledger it loads nor depends on having done so.
  const listAgain = await run(['subscriptions', 'list', '--json']);
  assert.equal(listAgain.status, 0, `the second subscriptions list --json exited ${listAgain.status}`);
  assert.deepEqual(parseJson(listAgain, 'the second subscriptions list --json'), report, 'two reads of an unchanged pool agree');
  assert.equal(await fixtureState(), stateBeforeRead, 'a second subscriptions list still wrote nothing');

  // ---- The same report as lines ---------------------------------------
  const listLines = await run(['subscriptions', 'list']);
  assert.equal(listLines.status, 0, `subscriptions list exited ${listLines.status}`);
  const liveCount = expectedRows.filter((row) => row.state === 'live').length;
  const headline = `${liveCount} of ${expectedRows.length} subscription credentials are live`;
  assert.ok(listLines.output.includes(headline), `the lines report leads with the live count: ${headline}`);
  assert.match(listLines.output, /burnt\s+codex\s+brama-sub-fixture-codex-primary/, 'a burnt grant is named with its provider and id');
  assert.ok(listLines.output.includes(`last_redeem_error: ${burntCause}`), "the burnt grant's refusal is printed in the provider's words");
  assert.ok(!listLines.output.includes(lapsedBlockReason), 'the lines report does not print a lapsed block either');
  assert.ok(!listLines.output.includes(vaultPayloadSentinel), 'the lines report carries no vault payload');
  assert.equal(await fixtureState(), stateBeforeRead, 'the lines report wrote nothing either');

  // ---- The repair, refused: no reason ---------------------------------
  const invocationsBeforeRefresh = await routerInvocations();
  const noReason = await run(['subscription', 'refresh', 'codex', '--json']);
  assert.notEqual(noReason.status, 0, 'a refresh without a reason is refused');
  assert.equal(noReason.status, 2, 'the refusal is a usage error');
  assert.match(noReason.output, /required arguments were not provided/, 'the refusal says an argument is missing');
  assert.match(noReason.output, /--reason <REASON>/, 'the refusal names the missing reason');
  assert.ok(!/"result"|"attempted"|"provider":/.test(noReason.output), 'a refused refresh reports no verdict');
  assert.deepEqual(await routerInvocations(), invocationsBeforeRefresh, 'the refused refresh read no credential: the router was never called');
  assert.equal(await fixtureState(), stateBeforeRead, 'the refused refresh changed no state and journaled nothing');
  observations.refusals.push({
    command: 'subscription refresh codex --json',
    exitStatus: noReason.status,
    refusal: 'error: the following required arguments were not provided: --reason <REASON>',
  });

  // ---- The repair carries no cost acknowledgement, because it is not the
  // billable path. The pool's billable path is `test`, and that one refuses
  // without the acknowledgement, before any provider is contacted.
  const costFlagOnRefresh = await run(['subscription', 'refresh', 'codex', '--reason', 'probierz fixture: never sent', '--allow-provider-cost']);
  assert.notEqual(costFlagOnRefresh.status, 0, 'refresh takes no cost-acknowledgement flag');
  assert.equal(costFlagOnRefresh.status, 2, 'an unknown flag is a usage error');
  assert.match(costFlagOnRefresh.output, /unexpected argument '--allow-provider-cost' found/, 'refresh has no billable-cost path to acknowledge');
  assert.deepEqual(await routerInvocations(), invocationsBeforeRefresh, 'the rejected invocation read no credential either');

  const billableWithoutAck = await run(['test', '--agent-id', 'wisent-app', '--model', 'openai/default']);
  assert.notEqual(billableWithoutAck.status, 0, 'the billable path is refused without a cost acknowledgement');
  assert.equal(billableWithoutAck.status, 1, 'the billable refusal exits 1');
  assert.ok(
    billableWithoutAck.output.includes('refusing billable inference without explicit --allow-provider-cost'),
    'the billable refusal names the missing cost acknowledgement',
  );
  assert.deepEqual(await routerInvocations(), invocationsBeforeRefresh, 'the refused billable request read no credential');
  assert.equal(await fixtureState(), stateBeforeRead, 'the refused billable request changed no state');
  observations.refusals.push({
    command: 'subscription refresh codex --reason <text> --allow-provider-cost',
    exitStatus: costFlagOnRefresh.status,
    refusal: "error: unexpected argument '--allow-provider-cost' found",
  });
  observations.refusals.push({
    command: 'test --agent-id wisent-app --model openai/default',
    exitStatus: billableWithoutAck.status,
    refusal: 'refusing billable inference without explicit --allow-provider-cost',
  });

  observations.list = {
    exitStatus: listJson.status,
    providerCount: report.providers.length,
    liveCount,
    headline,
    rows: report.providers,
  };
  observations.routerInvocations = await routerInvocations();
  observations.fixtureStateUnchanged = true;
  observations.vaultPayloadLeaked = false;

  // ---- Evidence -------------------------------------------------------
  const tracePath = join(artifacts, 'brama-subscription-pool.trace.json');
  await mkdir(dirname(tracePath), { recursive: true });
  await writeFile(tracePath, `${JSON.stringify({
    schemaVersion: 1,
    kind: 'probierz-brama-subscription-pool-trace',
    evidenceLevel: 'E2',
    runId: process.env.PROBIERZ_RUN_ID || null,
    specName: process.env.PROBIERZ_SPEC_NAME || 'brama-subscription-pool',
    status: 'completed',
    source: { root: sourceRoot, revision: sourceRevision, dirty: sourceDirty },
    fixture: {
      note: 'isolated fixture deployment; no production gateway, provider or vault was contacted',
      stateDir,
      home: fixtureHome,
      ledgerRows: Object.keys(ledger.subscriptions).length,
      vaultListingItems: vaultListing.length,
      entitlementsRouter: 'fixture stand-in: serves one listing, refuses every other verb',
    },
    observation: observations,
    contracts: [
      'subscriptions list --json exits 0 and joins the deployment listing to the usage ledger as one row per subscription',
      'each row carries exactly provider, subscription_id, state, expires_at and last_redeem_error',
      'state is one of live, expired, burnt, unknown; expires_at is the provider instant or null',
      'a block still in force is reported as the refusal in the way; a lapsed block is not',
      'the report prints no vault payload and no credential-shaped field',
      'reading the pool writes nothing, calls only the listing verb, and answers the same twice',
      'the lines report leads with how many credentials are live',
      'subscription refresh without --reason is refused, names the missing reason, and contacts nothing',
      'refresh has no cost-acknowledgement flag; the billable path refuses without --allow-provider-cost',
    ],
    redaction: {
      status: 'verified_redacted',
      credentialsIncluded: false,
      privateRecordsIncluded: false,
      note: 'every identifier and refusal sentence here is fixture-authored',
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
