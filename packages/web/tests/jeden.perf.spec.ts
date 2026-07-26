import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * jeden CLI performance. Measures wall time of key commands (cold + warm run),
 * writes a JSON report per project into the artifacts dir, and asserts
 * generous ceilings so gross regressions trip without flaking on shared
 * hardware. The network group needs a reachable brama control plane.
 */
const JEDEN = process.env.JEDEN_BIN || 'jeden';
const EXEC_TIMEOUT_MS = 90_000;
const TEST_TIMEOUT_MS = 240_000;
const ARTIFACTS = process.env.PROBIERZ_ARTIFACTS || 'test-results';

interface Sample {
  command: string;
  coldMs: number;
  warmMs: number;
}

function tmpCwd(): string {
  return mkdtempSync(join(tmpdir(), 'jeden-perf-'));
}

function runOnce(args: string[], input?: string): number {
  const started = performance.now();
  execFileSync(JEDEN, args, {
    cwd: tmpCwd(),
    input,
    env: { ...process.env },
    encoding: 'utf8',
    timeout: EXEC_TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
  });
  return performance.now() - started;
}

function measure(samples: Sample[], command: string, args: string[], input?: string): Sample {
  const sample: Sample = { command, coldMs: runOnce(args, input), warmMs: runOnce(args, input) };
  samples.push(sample);
  return sample;
}

function report(samples: Sample[], project: string, group: string): void {
  mkdirSync(ARTIFACTS, { recursive: true });
  const file = join(ARTIFACTS, `jeden-perf-${group}-${project}.json`);
  writeFileSync(
    file,
    JSON.stringify(
      {
        binary: JEDEN,
        project,
        at: new Date().toISOString(),
        samples: samples.map((s) => ({
          ...s,
          coldMs: Math.round(s.coldMs),
          warmMs: Math.round(s.warmMs),
        })),
      },
      null,
      2,
    ),
  );
  for (const s of samples) {
    console.log(
      `[perf:${project}] ${s.command.padEnd(28)} cold ${String(Math.round(s.coldMs)).padStart(6)} ms · warm ${String(Math.round(s.warmMs)).padStart(6)} ms`,
    );
  }
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

const LOCAL_CEILING_MS = 15_000;
const NETWORK_CEILING_MS = 45_000;

test.describe('jeden perf (local)', () => {
  const samples: Sample[] = [];

  test('measures local command latency', () => {
    test.setTimeout(TEST_TIMEOUT_MS);
    measure(samples, '--version', ['--version']);
    measure(samples, 'config', ['config']);
    measure(samples, 'tools', ['tools']);
    measure(samples, 'completions bash', ['completions', 'bash']);
    measure(samples, 'gallery --theme nord', ['gallery', '--theme', 'nord']);
    measure(samples, '/settings (picker export)', [], '/settings\n');
    for (const sample of samples) {
      expect(
        sample.coldMs,
        `${sample.command} cold run above ${LOCAL_CEILING_MS} ms`,
      ).toBeLessThan(LOCAL_CEILING_MS);
      expect(
        sample.warmMs,
        `${sample.command} warm run above ${LOCAL_CEILING_MS} ms`,
      ).toBeLessThan(LOCAL_CEILING_MS);
    }
    report(samples, test.info().project.name, 'local');
  });
});

test.describe('jeden perf (network)', () => {
  const samples: Sample[] = [];

  test.beforeAll(async () => {
    const url = bramaUrl();
    if (!url || !(await bramaReachable(url))) {
      test.skip(true, 'brama unreachable: BRAMA_URL missing or /health probe failed');
    }
  });

  test('measures network command latency', () => {
    test.setTimeout(TEST_TIMEOUT_MS);
    measure(samples, '/model (picker export)', [], '/model\n');
    measure(samples, '/usage (quota view)', [], '/usage\n');
    measure(samples, 'stats --summary', ['stats', '--summary']);
    measure(samples, 'doctor', ['doctor']);
    for (const sample of samples) {
      expect(
        sample.coldMs,
        `${sample.command} cold run above ${NETWORK_CEILING_MS} ms`,
      ).toBeLessThan(NETWORK_CEILING_MS);
      expect(
        sample.warmMs,
        `${sample.command} warm run above ${NETWORK_CEILING_MS} ms`,
      ).toBeLessThan(NETWORK_CEILING_MS);
    }
    report(samples, test.info().project.name, 'network');
  });
});
