import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * jeden CLI surface. Spawns the installed `jeden` binary (override with JEDEN_BIN).
 * The hermetic group never touches the network; the network group requires a
 * reachable brama control plane (BRAMA_URL env or ~/.jeden/.env) and skips
 * cleanly when its /health probe fails.
 */
const JEDEN = process.env.JEDEN_BIN || 'jeden';
const EXEC_TIMEOUT_MS = 60_000;
const TEST_TIMEOUT_MS = 90_000;

function tmpCwd(): string {
  return mkdtempSync(join(tmpdir(), 'jeden-probierz-'));
}

function runJeden(args: string[], options: { cwd?: string; input?: string } = {}): string {
  return execFileSync(JEDEN, args, {
    cwd: options.cwd ?? tmpCwd(),
    input: options.input,
    env: { ...process.env },
    encoding: 'utf8',
    timeout: EXEC_TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
  });
}

function bramaUrl(): string | undefined {
  if (process.env.BRAMA_URL) return process.env.BRAMA_URL;
  const envPath = join(homedir(), '.jeden', '.env');
  if (!existsSync(envPath)) return undefined;
  for (const line of readFileSync(envPath, 'utf8').split('\n')) {
    const match = line.match(/^BRAMA_URL=(.*)$/);
    if (match) return match[1].trim().replace(/^["']|["']$/g, '');
  }
  return undefined;
}

async function bramaReachable(url: string): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 5_000);
  try {
    const res = await fetch(`${url.replace(/\/+$/, '')}/health`, { signal: controller.signal });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

test.describe('jeden cli (hermetic)', () => {
  test.beforeEach(() => {
    test.setTimeout(TEST_TIMEOUT_MS);
  });

  test('--version prints a version string', () => {
    const out = runJeden(['--version']);
    expect(out.trim().length).toBeGreaterThan(0);
  });

  test('config prints tools.approvalMode', () => {
    const out = runJeden(['config', '--cwd', tmpCwd()]);
    expect(out).toContain('tools.approvalMode');
  });

  test('tools lists read_file and glob_paths', () => {
    const out = runJeden(['tools', '--cwd', tmpCwd()]);
    expect(out).toContain('read_file');
    expect(out).toContain('glob_paths');
  });

  test('completions bash prints a bash completion script', () => {
    const out = runJeden(['completions', 'bash']);
    expect(out.startsWith('# bash completion for jeden')).toBe(true);
    expect(/complete|compgen/.test(out)).toBe(true);
  });

  test('worktree list exits zero', () => {
    const out = runJeden(['worktree', 'list'], { cwd: tmpCwd() });
    expect(out.trim().length).toBeGreaterThan(0);
  });

  test('/settings shows settings group headers', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/settings\n' });
    expect(out).toContain('── tools (');
    expect(out).toContain('── commands (');
    expect(out).toContain('── startup (');
    expect(out).toContain('── secrets (');
  });

  test('/settings exposes scalar keys as INPUT prefill rows', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/settings\n' });
    expect(out).toContain('context.maxBytes: set value [INPUT]');
    expect(out).toContain('secrets.minLength: set value [INPUT]');
  });

  test('/settings text export groups rows into tab sections', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/settings\n' });
    expect(out).toContain('── context (');
    expect(out).toContain('── ui (');
  });

  test('stats --summary prints the one-line snapshot', () => {
    const out = runJeden(['stats', '--summary'], { cwd: tmpCwd() });
    expect(/\d+ events · \d+ tokens · cost \d+ · sessions \d+/.test(out)).toBe(true);
  });

  test('stats --json prints a parseable snapshot', () => {
    const out = runJeden(['stats', '--json'], { cwd: tmpCwd() });
    const stats = JSON.parse(out);
    expect(stats).toHaveProperty('version');
    expect(stats).toHaveProperty('usage.project');
    expect(stats).toHaveProperty('quota');
    expect(stats).toHaveProperty('sessions');
  });

  test('gallery renders fixtures for one theme', () => {
    const out = runJeden(['gallery', '--theme', 'nord'], { cwd: tmpCwd() });
    expect(out).toContain('── theme: nord ──');
    expect(out).toContain('Select model route');
    // The categories moved from a tab bar into the left pane of the picker,
    // so the fixture now shows the brands column and its divider.
    expect(out).toContain('● All');
    expect(out).toContain('│');
    expect(out).toContain('Confirm destructive action');
  });

  test('gallery --all sweeps every bundled theme', () => {
    const out = runJeden(['gallery', '--all'], { cwd: tmpCwd() });
    for (const theme of ['graphite-dark', 'paper-light', 'titanium', 'nord', 'color-blind']) {
      expect(out).toContain(`── theme: ${theme} ──`);
    }
  });

  test('gallery rejects an unknown theme', () => {
    expect(() => runJeden(['gallery', '--theme', 'bogus'], { cwd: tmpCwd() })).toThrow();
  });

  test('/collab start on an http relay prints QR codes for share URLs', async () => {
    // The relay stub must live in a worker thread: execFileSync blocks the
    // main event loop, so an in-process server would deadlock the exchange.
    const { Worker } = await import('node:worker_threads');
    const worker = new Worker(
      `const { parentPort } = require('node:worker_threads');
       const { createServer } = require('node:http');
       const server = createServer((req, res) => {
         res.setHeader('Content-Type', 'application/json');
         res.end(req.method === 'POST' ? '{"seq":1}' : '{"events":[],"cursor":0}');
       });
       server.listen(0, '127.0.0.1', () => parentPort.postMessage(server.address().port));`,
      { eval: true },
    );
    const port = await new Promise<number>((resolve) => worker.once('message', resolve));
    try {
      const out = runJeden([], {
        cwd: tmpCwd(),
        input: `/collab start http://127.0.0.1:${port}\n/collab stop\n`,
      });
      expect(out).toContain('View URL:');
      expect(out).toContain('Full write URL:');
      expect(out).toContain('█');
    } finally {
      await worker.terminate();
    }
  });

  test('/roles shows model role rows', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/roles\n' });
    expect(out).toContain('default model');
    expect(out).toContain('fast tier');
    expect(out).toContain('advisor');
  });
});

test.describe('jeden cli (network)', () => {
  test.beforeAll(async () => {
    const url = bramaUrl();
    if (!url || !(await bramaReachable(url))) {
      test.skip(true, 'brama unreachable: BRAMA_URL missing or /health probe failed');
    }
  });

  test.beforeEach(() => {
    test.setTimeout(TEST_TIMEOUT_MS);
  });

  test('run prints the model response', () => {
    const out = runJeden(['run', 'Respond exactly: OK', '--model', 'codex/gpt-5.6-sol', '--max-steps', '1'], {
      cwd: tmpCwd(),
    });
    expect(out).toContain('OK');
  });

  test('/model lists routes with availability badges', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/model\n' });
    expect(out).toContain('- any [AUTO]');
    expect(/\[AVAILABLE\]|\[ACTIVE\]/.test(out)).toBe(true);
    expect(out).toContain('Show all');
  });

  test('/model shows provider summary rows', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/model\n' });
    expect(/● .+ — \d+ models? · your subscription/.test(out)).toBe(true);
    expect(/○ catalog — \d+ models? · no credentials/.test(out)).toBe(true);
  });

  test('token redacts by default and reveals on demand', () => {
    const redacted = runJeden(['token'], { cwd: tmpCwd() });
    expect(redacted).toContain('…');
    expect(redacted).not.toContain(process.env.WISENT_APP_AGENT_AUTH_SECRET ?? '\u0000');
    const revealed = runJeden(['token', '--reveal'], { cwd: tmpCwd() }).trim();
    expect(revealed.length).toBeGreaterThan(16);
    expect(revealed).not.toContain('\n');
  });

  test('/token slash never reveals the secret', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/token\n' });
    expect(out).toContain('redacted');
    const secret = process.env.WISENT_APP_AGENT_AUTH_SECRET;
    if (secret) {
      expect(out).not.toContain(secret);
    }
  });

  test('/usage shows provider usage', () => {
    const out = runJeden([], { cwd: tmpCwd(), input: '/usage\n' });
    expect(out).toContain('Provider usage');
    expect(out.includes('[✔]') || out.includes('quota unavailable')).toBe(true);
  });

  test('doctor reports healthy', () => {
    const out = runJeden(['doctor', '--cwd', tmpCwd()]);
    expect(out).toContain('"healthy":true');
  });
});
