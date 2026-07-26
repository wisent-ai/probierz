import { test, expect } from '@playwright/test';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
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

/**
 * Named replays of user-reported flows. Each scenario encodes an actual bug
 * report so the regression stays executable long after the fix: the original
 * `/model` hang that ended in ^C, and the agent not knowing it is jeden.
 */

const REPLAY_TIMEOUT_MS = TIMEOUTS.test;

function bramaConfigured(): boolean {
  if (process.env.BRAMA_URL) return true;
  const envPath = join(homedir(), '.jeden', '.env');
  return existsSync(envPath) && readFileSync(envPath, 'utf8').includes('BRAMA_URL=');
}

/** The secret `/token` must never print in full. Read from the same file the
 * app reads, so the assertion compares against the real value. */
function agentSecret(): string | undefined {
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return undefined;
  const [, value] = readFileSync(envPath, 'utf8').match(/^WISENT_APP_AGENT_AUTH_SECRET=(.*)$/m) ?? [];
  return value?.trim().replace(/^["']|["']$/g, '') || undefined;
}

test.describe('jeden replays of user-reported flows', () => {
  let session: TuiSession;

  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
  });

  test.beforeEach(async () => {
    test.setTimeout(REPLAY_TIMEOUT_MS);
    // The reported hang was a cold-catalog fetch; a warm cache would make
    // this replay pass for the wrong reason.
    session = TuiSession.jeden({ warmCache: false });
    const ready = await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready);
    expect(ready.found, 'jeden TUI did not reach the welcome screen').toBe(true);
  });

  test.afterEach(() => {
    session?.kill();
  });

  test('replay: /login then /model opens the picker instead of hanging (the ^C report)', async () => {
    if (!bramaConfigured()) test.skip(true, 'brama not configured');
    // Baseline first, in its own cold session: without it a red result cannot
    // distinguish "the catalog is slow" from "the second view waits for the
    // first" — and the original report was the second one.
    const alone = TuiSession.jeden({ warmCache: false });
    let standaloneMs = TIMEOUTS.ready;
    try {
      expect((await watchFor(() => alone.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await alone.command('/model', { escapeFirst: false });
      standaloneMs = (await watchFor(() => alone.capture(), /Select model route/, TIMEOUTS.ready)).ms;
    } finally {
      alone.kill();
    }

    await session.command('/login', { escapeFirst: false });
    expect((await watchFor(() => session.capture(), /authentication status/i, TIMEOUTS.ready)).found).toBe(true);
    await session.command('/model');
    // The original report: /model sat silent until the user hit ^C. With the
    // background-turn spinner and the disk cache the picker must appear well
    // inside the ready budget even on a cold catalog (isolated HOME).
    const opened = await watchFor(() => session.capture(), /Select model route/, TIMEOUTS.ready);
    const detail = `${opened.ms}ms after /login, ${standaloneMs}ms standalone`;
    console.log(`[replay.hang] ${detail}`);
    expect(
      checked('replay.hang', 'jeden', opened.found, detail),
      `model picker did not open within ${TIMEOUTS.ready}ms of submit (${detail}) — a network view queued behind the previous one`,
    ).toBe(true);
  });

  test('replay: the agent knows it is jeden (identity report)', async () => {
    await session.command('/prompt', { escapeFirst: false });
    const shown = await watchFor(() => session.capture(), /[Jj]eden/, TIMEOUTS.ready);
    expect(
      checked('replay.identity', 'jeden', shown.found),
      '/prompt did not surface the jeden identity',
    ).toBe(true);
  });
});

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

  test('/checkpoint creates a checkpoint that is still there afterwards', async () => {
    await session.command('/checkpoint', { escapeFirst: false });
    const created = await watchFor(() => session.capture(), /checkpoint .* created/i, TIMEOUTS.settle);
    expect(created.found, '/checkpoint did not report creating one').toBe(true);
    const [, id] = session.capture().match(/Checkpoint (\S+) created/i) ?? [];
    await session.command('/checkpoint');
    const listed = id
      ? await watchFor(() => session.capture(), new RegExp(id), TIMEOUTS.settle)
      : { found: false, ms: TIMEOUTS.settle };
    expect(
      checked('fn.checkpoint-roundtrip', 'jeden', listed.found, id ?? 'no id parsed'),
      `checkpoint ${id} was created but running /checkpoint again does not mention it`,
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

test.describe('discovery contracts — jeden', () => {
  test.beforeAll(() => {
    if (!HAS_TMUX) test.skip(true, 'tmux not installed');
  });

  test('/extensions lists an extension module planted in the workspace', async () => {
    test.setTimeout(TIMEOUTS.test);
    const cwd = mkdtempSync(join(tmpdir(), 'probierz-ext-'));
    mkdirSync(join(cwd, '.jeden', 'extensions'), { recursive: true });
    cpSync(join(FIXTURES, 'probe-ext.mjs'), join(cwd, '.jeden', 'extensions', 'probe-ext.mjs'));
    const session = TuiSession.jeden({ cwd });
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await session.command('/extensions', { escapeFirst: false });
      // The row carries the absolute path, which the frame truncates with an
      // ellipsis — matching the fixture's file name would test the pane
      // width, not discovery. The kind and root survive truncation.
      const listed = await watchFor(
        () => flattened(session.capture()),
        /Native extension.*\.jeden\/extensions/,
        TIMEOUTS.settle,
      );
      expect(
        checked('fn.extension-discovery', 'jeden', listed.found, cwd),
        'an extension module in .jeden/extensions is not discovered by /extensions',
      ).toBe(true);
    } finally {
      session.kill();
    }
  });

  test('/agents lists and shows a custom agent planted in the workspace', async () => {
    test.setTimeout(TIMEOUTS.test);
    const cwd = mkdtempSync(join(tmpdir(), 'probierz-agents-'));
    mkdirSync(join(cwd, '.jeden', 'agents'), { recursive: true });
    cpSync(join(FIXTURES, 'probe-agent.json'), join(cwd, '.jeden', 'agents', 'probe-agent.json'));
    const session = TuiSession.jeden({ cwd });
    try {
      expect((await watchFor(() => session.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await session.command('/agents', { escapeFirst: false });
      const listed = await watchFor(() => flattened(session.capture()), /probe-agent/, TIMEOUTS.settle);
      await session.command('/agents show probe-agent');
      const shown = await watchFor(() => flattened(session.capture()), /probe agent for tests/i, TIMEOUTS.settle);
      expect(
        checked('fn.agent-discovery', 'jeden', listed.found && shown.found, `${listed.found}/${shown.found}`),
        'a custom agent in .jeden/agents is not listed and shown by /agents',
      ).toBe(true);
    } finally {
      session.kill();
    }
  });

  test('/setup marks the router configured only when the credentials exist', async () => {
    test.setTimeout(TIMEOUTS.test);
    // The TUI opens the wizard, not the piped checklist: a bare home offers
    // "Set BRAMA_URL [INPUT]", a configured one reports it "[OK]".
    const bare = TuiSession.jeden({ credentials: false });
    let promptsForCredentials = false;
    try {
      expect((await watchFor(() => bare.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await bare.command('/setup', { escapeFirst: false });
      promptsForCredentials = (
        await watchFor(() => flattened(bare.capture()), /Set BRAMA_URL.*\[INPUT\]/i, TIMEOUTS.view)
      ).found;
    } finally {
      bare.kill();
    }
    const configured = TuiSession.jeden();
    let reportsConfigured = false;
    try {
      expect((await watchFor(() => configured.capture(), /Tips|Welcome back/, TIMEOUTS.ready)).found).toBe(true);
      await configured.command('/setup', { escapeFirst: false });
      reportsConfigured = (
        await watchFor(() => flattened(configured.capture()), /BRAMA_URL configured.*\[OK\]/i, TIMEOUTS.view)
      ).found;
    } finally {
      configured.kill();
    }
    expect(
      checked(
        'fn.setup-checklist',
        'jeden',
        promptsForCredentials && reportsConfigured,
        `bare-prompts ${promptsForCredentials}, configured-ok ${reportsConfigured}`,
      ),
      '/setup does not distinguish a credential-less home from a configured one',
    ).toBe(true);
  });
});

/**
 * Read-only views must contain their own subject matter. Weak individually,
 * strong together: paired with the command scan (renders, no error, fits,
 * navigable) it is the difference between "a box appeared" and "the box is
 * about the thing the command promises".
 */
const CONTENT_CONTRACTS: [command: string, expected: RegExp][] = [
  ['/help', /\/model/],
  ['/hotkeys', /Enter|Esc/],
  ['/context', /token/i],
  ['/status', /capabilit|health|allow|ask/i],
  ['/tools', /read|write|command|search/i],
  ['/prompt', /[Jj]eden/],
  ['/login', /account|Weles|authentication/i],
  ['/usage', /usage|quota|token|event/i],
  ['/roles', /default|fast|advisor|role/i],
  ['/agents', /agent/i],
  ['/jobs', /job/i],
  ['/session', /session/i],
  ['/memory', /memory|queue|index/i],
  ['/hooks', /hook/i],
  ['/extensions', /extension|plugin|discover/i],
  ['/plugins', /plugin/i],
  ['/marketplace', /marketplace|catalog|plugin/i],
  ['/mcp', /mcp|server/i],
  ['/ssh', /ssh|host/i],
  ['/browser', /browser|runtime|chrom/i],
  ['/changelog', /\d|release|change/i],
  ['/settings', /tools\.|ui\.|context\.|secrets\./],
  ['/model', /claude|codex|kimi|catalog|model/i],
  ['/approval', /approval|ask|allow|deny/i],
  ['/collab status', /collab|host|guest|relay/i],
  ['/fast status', /fast mode/i],
  ['/plan status', /plan/i],
  ['/stats', /stat|usage|dashboard|report/i],
];

test.describe('content contracts — jeden', () => {
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

  test('every read-only view contains its own subject matter', async () => {
    const missing: string[] = [];
    for (const [command, expected] of CONTENT_CONTRACTS) {
      await session.command(command);
      // Matched against de-wrapped text: content that hits the frame edge
      // continues on the next row, and a raw match would call it absent.
      const seen = await watchFor(() => flattened(session.capture()), expected, TIMEOUTS.settle);
      if (!seen.found) {
        missing.push(`${command} (no ${expected})`);
        console.log(`[content-contract] ${command} missed ${expected}:\n${paneTail(session.capture())}`);
      }
    }
    expect(
      checked('fn.view-content', 'jeden', !missing.length, missing.join('; ')),
      `these views rendered without their own subject matter: ${missing.join(', ')}`,
    ).toEqual(true);
  });
});
