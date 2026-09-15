import { test, expect } from '@playwright/test';
import { cpSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  FIXTURES,
  HAS_TMUX,
  TIMEOUTS,
  TuiSession,
  checked,
  flattened,
  watchFor,
} from './helpers/tui';

/**
 * Discovery contracts: what jeden finds in a workspace it was pointed at.
 * Each story plants a real file — an extension module, a custom agent — in a
 * scratch workspace and asks the app to list it, so "discovered" means read
 * off the disk rather than compiled in. The setup checklist is the same idea
 * against a home that has credentials and one that does not.
 */

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
      // The row carries an absolute path that the frame truncates, so the
      // assertion is the kind of row plus the absence of the empty state —
      // matching the fixture's file name would test the pane width instead.
      const listed = await watchFor(
        () => flattened(session.capture()),
        /Native extension/,
        TIMEOUTS.settle,
      );
      const empty = /No extensions or plugins found/.test(session.capture());
      expect(
        checked('fn.extension-discovery', 'jeden', listed.found && !empty, cwd),
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
