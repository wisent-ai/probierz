import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const repository = '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-backend';
const script = join(repository, 'tests/chat/production_latency.py');
const artifacts = process.env.PROBIERZ_ARTIFACTS || 'test-results';

const credential = spawnSync('skarbiec', ['get', 'wisent-backend-supabase'], {
  encoding: 'utf8',
  timeout: 30000,
});
assert.ifError(credential.error);
assert.equal(credential.status, 0, credential.stderr || 'Skarbiec could not read wisent-backend-supabase');
const fields = JSON.parse(credential.stdout).fields;
assert.ok(fields.url, 'wisent-backend-supabase.url is required');
assert.ok(fields.anon_key, 'wisent-backend-supabase.anon_key is required');

const result = spawnSync('python3', [script], {
  cwd: repository,
  env: {
    ...process.env,
    SUPABASE_URL: fields.url,
    SUPABASE_ANON_KEY: fields.anon_key,
    TEST_EMAIL: process.env.TEST_EMAIL || 'wisent+backend_hardcoded@wisent.ai',
    TEST_PASSWORD: process.env.TEST_PASSWORD || '123456',
  },
  encoding: 'utf8',
  timeout: 1200000,
});

await mkdir(artifacts, { recursive: true });
await writeFile(
  join(artifacts, 'wisent-backend-production-latency.json'),
  result.stdout || result.stderr,
  { mode: 0o600 },
);
assert.ifError(result.error);
process.stdout.write(result.stdout);
process.stderr.write(result.stderr);
assert.equal(
  result.status,
  0,
  [result.stdout, result.stderr].filter(Boolean).join('\n') || `Production latency measurement exited ${result.status}`,
);
