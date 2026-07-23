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
