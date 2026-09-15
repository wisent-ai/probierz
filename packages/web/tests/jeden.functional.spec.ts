import { test, expect } from '@playwright/test';
import { cpSync, existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import {
  FIXTURES,
  HAS_TMUX,
  TIMEOUTS,
  TuiSession,
  checked,
  flattened,
  paneTail,
  settledCapture,
  watchFor,
} from './helpers/tui';

/** The secret `/token` must never print in full. Read from the same file the
 * app reads, so the assertion compares against the real value. */
function agentSecret(): string | undefined {
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return undefined;
  const [, value] = readFileSync(envPath, 'utf8').match(/^WISENT_APP_AGENT_AUTH_SECRET=(.*)$/m) ?? [];
  return value?.trim().replace(/^["']|["']$/g, '') || undefined;
}

/**
 * Functional contracts: what a command DOES, not what it paints. The command
 * scan proves every command renders something; these prove the commands with
 * an observable effect actually work — mode toggles read back, config writes
 * reach disk, renames stick, checkpoints exist afterwards — and that every
 * read-only view contains its own subject matter instead of an empty box.
 * Every session runs in an isolated HOME and a scratch cwd, so "destructive"
 * commands are safe to exercise for real.
 */
test.describe('functional contracts — jeden', () => {
  let session: TuiSession;

  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
  });

  test.beforeEach(async () => {
    test.setTimeout(TIMEOUTS.test);
    session = TuiSession.jeden();
    expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('/todo add writes a todo that the reopened view still shows', async () => {
    const marker = `probierz-${Date.now().toString(36)}`;
    await session.command(`/todo add ${marker}`, { escapeFirst: false });
    await settledCapture(session);
    session.key('Escape');
    await settledCapture(session);
    await session.command('/todo');
    const shown = await watchFor(() => session.capture(), new RegExp(marker), TIMEOUTS.settle);
    expect(
      checked('fn.todo-roundtrip', 'jeden', shown.found, marker),
      `/todo add accepted "${marker}" but reopening /todo does not list it`,
    ).toBe(true);
  });

  test('/branch creates a lineage node that /tree lists', async () => {
    const marker = `probe${Date.now().toString(36)}`;
    await session.command(`/branch ${marker}`, { escapeFirst: false });
    await settledCapture(session);
    session.key('Escape');
    await settledCapture(session);
    await session.command('/tree');
    const shown = await watchFor(() => session.capture(), new RegExp(marker), TIMEOUTS.settle);
    expect(
      checked('fn.branch-roundtrip', 'jeden', shown.found, marker),
      `/branch ${marker} reported success but /tree does not show the branch`,
    ).toBe(true);
  });

  test('/token redacts the credential it prints', async () => {
    await session.command('/token', { escapeFirst: false });
    const screen = await settledCapture(session);
    const secret = agentSecret();
    test.skip(!secret, 'no agent secret configured to compare against');
    const leaked = Boolean(secret) && screen.includes(secret as string);
    expect(
      checked('fn.token-redacted', 'jeden', !leaked),
      '/token printed the raw agent secret into the transcript the model reads',
    ).toBe(true);
  });

  test('/plan on is still on when /plan status is asked afterwards', async () => {
    await session.command('/plan on', { escapeFirst: false });
    const enabled = await watchFor(() => session.capture(), /plan mode enabled/i, TIMEOUTS.settle);
    expect(enabled.found, '/plan on did not report the mode as enabled').toBe(true);
    await session.command('/plan status');
    const persisted = await watchFor(() => session.capture(), /enabled/i, TIMEOUTS.settle);
    expect(
      checked('fn.mode-roundtrip', 'jeden', persisted.found),
      '/plan on reported success but /plan status does not report the mode as enabled',
    ).toBe(true);
  });

  test('/settings set writes the value through to config.yml on disk', async () => {
    await session.command('/settings set tools.approvalMode always-ask', { escapeFirst: false });
    const acknowledged = await watchFor(() => flattened(session.capture()), /config\.ya?ml/i, TIMEOUTS.settle);
    expect(
      acknowledged.found,
      `/settings set did not name the file it wrote:\n${paneTail(session.capture())}`,
    ).toBe(true);
    const configPath = join(session.home, '.jeden', 'config.yml');
    const written = existsSync(configPath) && readFileSync(configPath, 'utf8').includes('always-ask');
    expect(
      checked('fn.settings-write-through', 'jeden', written, configPath),
      `/settings set reported success but ${configPath} does not carry the value`,
    ).toBe(true);
  });

  test('/rename sticks: the session view reports the new name', async () => {
    const marker = `probe-${Date.now().toString(36)}`;
    await session.command(`/rename ${marker}`, { escapeFirst: false });
    expect((await watchFor(() => session.capture(), /renamed/i, TIMEOUTS.settle)).found).toBe(true);
    await session.command('/session');
    const shown = await watchFor(() => session.capture(), new RegExp(marker), TIMEOUTS.settle);
    expect(
      checked('fn.rename-roundtrip', 'jeden', shown.found, marker),
      `/rename ${marker} reported success but /session does not show the name`,
    ).toBe(true);
  });

  test('/checkpoint mints a fresh durable checkpoint id every time', async () => {
    await session.command('/checkpoint', { escapeFirst: false });
    expect(
      (await watchFor(() => flattened(session.capture()), /checkpoint .* created/i, TIMEOUTS.settle)).found,
      '/checkpoint did not report creating one',
    ).toBe(true);
    const [, first] = flattened(session.capture()).match(/Checkpoint (event-\S+) created/i) ?? [];
    await session.command('/checkpoint');
    await watchFor(() => flattened(session.capture()), /checkpoint .* created/i, TIMEOUTS.settle);
    const [, second] = flattened(session.capture()).match(/Checkpoint (event-\S+) created/i) ?? [];
    // Views replace each other, so the first id is off-screen by design; what
    // must hold is that each call mints a distinct durable event id rather
    // than reporting success and reusing (or losing) the previous one.
    const minted = Boolean(first) && Boolean(second) && first !== second;
    expect(
      checked('fn.checkpoint-roundtrip', 'jeden', minted, `${first ?? 'none'} → ${second ?? 'none'}`),
      `/checkpoint did not mint distinct ids (${first} → ${second})`,
    ).toBe(true);
  });

  test('/omfg names a rules file and writing a rule creates it', async () => {
    const marker = `probe rule ${Date.now().toString(36)}`;
    await session.command(`/omfg ${marker}`, { escapeFirst: false });
    await settledCapture(session);
    const rulesPath = join(session.cwd, '.jeden', 'rules.jsonl');
    const stored = existsSync(rulesPath) && readFileSync(rulesPath, 'utf8').includes(marker);
    expect(
      checked('fn.omfg-persists', 'jeden', stored, rulesPath),
      `/omfg accepted the rule but ${rulesPath} does not contain it`,
    ).toBe(true);
  });

  test('/collab start opens a durable relay and /collab stop closes it', async () => {
    await session.command('/collab start', { escapeFirst: false });
    const started = await watchFor(() => flattened(session.capture()), /collab-relay\.jsonl/i, TIMEOUTS.settle);
    expect(started.found, '/collab start named no relay').toBe(true);
    const relay = join(session.cwd, '.jeden', 'collab-relay.jsonl');
    // The relay is a file, so the contract reads it: a host that "started"
    // without writing its own start event started nothing.
    const hostStart = existsSync(relay) && readFileSync(relay, 'utf8').includes('host-start');
    await session.command('/collab status');
    const hosting = await watchFor(() => flattened(session.capture()), /collab host:/i, TIMEOUTS.settle);
    await session.command('/collab stop');
    const stopped = await watchFor(() => flattened(session.capture()), /hosting stopped/i, TIMEOUTS.settle);
    await session.command('/collab status');
    const off = await watchFor(() => flattened(session.capture()), /collab off/i, TIMEOUTS.settle);
    const ok = hostStart && hosting.found && stopped.found && off.found;
    expect(
      checked(
        'fn.collab-relay',
        'jeden',
        ok,
        `host-start ${hostStart}, hosting ${hosting.found}, stopped ${stopped.found}, off ${off.found}`,
      ),
      `/collab start → status → stop → status did not complete against ${relay}`,
    ).toBe(true);
  });

  test('/marketplace add registers a local catalog and lists its plugins', async () => {
    const source = join(session.cwd, 'probe-market');
    cpSync(join(FIXTURES, 'probe-market'), source, { recursive: true });
    await session.command(`/marketplace add ${source}`, { escapeFirst: false });
    const added = await watchFor(() => flattened(session.capture()), /added marketplace source/i, TIMEOUTS.settle);
    expect(added.found, `/marketplace add did not accept ${source}`).toBe(true);
    await session.command('/marketplace');
    const listed = await watchFor(() => flattened(session.capture()), /probe-plugin/, TIMEOUTS.settle);
    expect(
      checked('fn.marketplace-source', 'jeden', listed.found, source),
      '/marketplace registered the source but its plugin is not offered in the view',
    ).toBe(true);
  });
});
