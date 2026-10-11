#!/usr/bin/env node
// Real Oko seed refusal against an isolated, empty Auth project without the
// organizations relation. The failed organization cleanup must not prevent
// deletion of the technical account that this invocation actually creates.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function required(name) {
  assert.ok(process.env[name]?.trim(), `${name} is required`);
  return process.env[name];
}
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const binary = resolve(required('PROBIERZ'));
const revision = required('PROBIERZ_TEST_SOURCE_REVISION');
const release = resolve(required('PROBIERZ_TEST_RELEASE_EVIDENCE'));
const base = required('OKO_E2E_SUPABASE_URL').replace(/\/$/, '');
const key = required('OKO_E2E_SUPABASE_SERVICE_ROLE_KEY');
required('OKO_E2E_SUPABASE_ANON_KEY');
required('OKO_E2E_EMAIL');
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/oko-rollback');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const runId = randomUUID();
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const report = { revision, binary, binarySha256: digest(binary), runId,
  commands: [], observations: [], passed: false };
function command(program, args, env = process.env) {
  const result = spawnSync(program, args, { cwd: root, env, encoding: 'utf8' });
  report.commands.push({ program, args, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return result;
}
async function observe(path) {
  const response = await fetch(`${base}${path}`, { headers: {
    apikey: key, Authorization: `Bearer ${key}`,
  } });
  const text = await response.text();
  const body = JSON.parse(text);
  report.observations.push({ method: 'GET', path, status: response.status, body });
  return { ok: response.ok, body };
}
try {
  const releaseSource = readFileSync(join(release, 'SOURCE_REVISION'), 'utf8').trim();
  const releaseDigests = readFileSync(join(release, 'DIGESTS'), 'utf8');
  assert.equal(releaseSource, revision);
  assert.deepEqual(releaseDigests.trim().split('\n').map(line =>
    line.match(/^\\?(?<sha256>[a-fA-F\d]+)[ \t]+/)?.groups.sha256), [report.binarySha256]);
  report.releaseEvidence = { directory: release, sourceRevision: releaseSource, digests: releaseDigests };
  const version = command(binary, ['--version']);
  assert.ok(version.status !== null && !version.status, version.stderr);
  const source = command('git', ['rev-parse', 'HEAD']);
  assert.equal(source.status, version.status);
  assert.equal(source.stdout.trim(), revision);
  const clean = command('git', ['status', '--porcelain', '--untracked-files=no']);
  assert.equal(clean.status, version.status);
  assert.equal(clean.stdout.trim(), '', 'qualification requires committed source');

  const before = await observe('/auth/v1/admin/users');
  assert.ok(before.ok, 'the real Auth dependency must answer');
  assert.deepEqual(before.body.users, [], 'only an empty isolated Auth project may be used');
  const organization = await observe('/rest/v1/organizations');
  assert.equal(organization.ok, false, 'this refusal fixture must have no organizations relation');
  assert.ok(typeof organization.body.message === 'string' &&
    organization.body.message.includes('organizations'), 'refusal must identify the absent relation');

  const seeded = command(binary, ['apphook', 'oko.seed'], {
    ...process.env, PROBIERZ_RUN_ID: runId, PROBIERZ_ARTIFACTS: evidence,
    PROBIERZ_JOURNEYS: 'writer-update',
  });
  assert.ok(seeded.status !== null && seeded.status !== version.status, 'seed must be refused');
  const diagnostic = `${seeded.stdout}\n${seeded.stderr}`;
  assert.ok(diagnostic.includes(organization.body.message), 'retain the real dependency cause');
  assert.ok(diagnostic.includes('rollback could not confirm removal'), 'report unsuccessful rollback');
  assert.match(diagnostic, /organization [a-f\d-]+:/, 'name the isolated organization coordinate');
  const after = await observe('/auth/v1/admin/users');
  assert.ok(after.ok);
  assert.deepEqual(after.body.users, before.body.users,
    'account rollback must still run after the organization rollback fails');
  report.passed = true;
} catch (error) {
  report.failure = error.stack ?? String(error);
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, '\t'));
  console.error(`evidence: ${evidence}`);
}
