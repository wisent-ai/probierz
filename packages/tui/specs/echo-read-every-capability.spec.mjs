import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';

const repository = '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo';
const envFile = process.env.ECHO_TEST_ENV?.trim();
assert.ok(envFile, 'ECHO_TEST_ENV is required; see the echo manifest prerequisites');
assert.ok(isAbsolute(envFile), 'ECHO_TEST_ENV must be an absolute path');
assert.ok(statSync(envFile).isFile(), 'ECHO_TEST_ENV must identify a readable file');

function parseEnvironment(text) {
  const environment = {};
  for (const sourceLine of text.split(/\r?\n/)) {
    const line = sourceLine.trim();
    if (!line || line.startsWith('#')) continue;
    const separator = line.indexOf('=');
    if (separator < 1) continue;
    const name = line.slice(0, separator).trim().replace(/^export\s+/, '');
    let value = line.slice(separator + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) value = value.slice(1, -1);
    environment[name] = value;
  }
  return environment;
}

const configured = parseEnvironment(readFileSync(envFile, 'utf8'));
for (const name of ['NEXT_PUBLIC_SUPABASE_URL', 'SUPABASE_SERVICE_ROLE_KEY']) {
  assert.ok(configured[name], `${name} is required in ECHO_TEST_ENV`);
}

const result = spawnSync('npm', ['run', 'test:capabilities'], {
  cwd: repository,
  env: { ...process.env, ...configured },
  encoding: 'utf8',
  timeout: 180000,
});
const diagnosticDirectory = process.env.PROBIERZ_ARTIFACTS || 'test-results';
await mkdir(diagnosticDirectory, { recursive: true });
await writeFile(join(diagnosticDirectory, 'echo-read-every-capability.tap'), `${result.stdout}\n${result.stderr}`, { mode: 0o600 });
assert.ifError(result.error);
process.stdout.write(result.stdout);
process.stderr.write(result.stderr);
assert.equal(result.status, 0, [result.stdout, result.stderr].filter(Boolean).join('\n') || `Echo capability tests exited ${result.status}`);

const evidencePath = join(process.env.PROBIERZ_ARTIFACTS || 'test-results', 'echo-read-every-capability.json');
await mkdir(join(evidencePath, '..'), { recursive: true });
await writeFile(evidencePath, `${JSON.stringify({
  command: 'npm run test:capabilities',
  repository,
  assertions: [
    'analytics', 'experiments', 'onboarding', 'blogs', 'generation', 'assets',
    'products', 'personas', 'characters', 'social', 'outreach', 'paid ads',
    'market', 'reliability', 'operations',
  ],
}, null, 2)}\n`, { mode: 0o600 });
