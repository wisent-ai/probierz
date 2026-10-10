#!/usr/bin/env node
// Exercise the actual stdio server without allowing a malformed publication
// request to dispatch a write. No alternate executable or provider is used.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
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
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/mcp-admission');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const harness = join(evidence, 'harness');
mkdirSync(join(harness, 'apps'), { recursive: true });
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const report = { revision, binary, binarySha256: digest(binary),
  testSha256: digest(fileURLToPath(import.meta.url)), commands: [], passed: false };
function run(program, args, input) {
  const result = spawnSync(program, args, { cwd: root, input, encoding: 'utf8' });
  report.commands.push({ program, args, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  assert.ok(result.status !== null && !result.status, result.stderr);
  return result.stdout;
}
function request(method, params) { return { jsonrpc: '2.0', id: randomUUID(), method, params }; }
try {
  assert.equal(run('git', ['rev-parse', 'HEAD']).trim(), revision);
  assert.equal(run('git', ['status', '--porcelain', '--untracked-files=no']).trim(), '',
    'candidate source must be committed');
  const catalogue = request('tools/list');
  const malformed = request('tools/call', {
    name: 'probierz_create_publication_manifest', arguments: 'not an object',
  });
  const missing = request('tools/call', {
    name: 'probierz_create_publication_manifest',
    arguments: { attemptId: randomUUID(), journeyId: randomUUID() },
  });
  const wrongEnvironment = request('tools/call', {
    name: 'probierz_run', arguments: { target: 'tui', env: 'not an object' },
  });
  const wrongWatch = request('tools/call', {
    name: 'probierz_stado_run',
    arguments: { target: 'tui', appId: randomUUID(), host: randomUUID(), watch: 'false' },
  });
  const wrongArrayItem = request('tools/call', {
    name: 'probierz_affected', arguments: { files: [false] },
  });
  const ping = request('ping');
  const requests = [catalogue, malformed, missing, wrongEnvironment, wrongWatch, wrongArrayItem, ping];
  const output = run(binary, ['--harness', harness, 'mcp'],
    requests.map(value => JSON.stringify(value)).join('\n') + '\n');
  const responses = output.trim().split('\n').map(line => JSON.parse(line));
  const response = request => {
    const matches = responses.filter(value => value.id === request.id);
    assert.deepEqual(matches.map(value => value.id), [request.id], 'one terminal answer per request');
    return matches.find(value => value.id === request.id);
  };
  const declared = response(catalogue).result.tools.find(tool =>
    tool.name === 'probierz_create_publication_manifest');
  assert.ok(declared.inputSchema.required.includes('receipt'));
  assert.equal(response(malformed).error.message, 'arguments must be a JSON object');
  assert.equal(response(missing).error.message,
    'probierz_create_publication_manifest requires argument receipt');
  assert.equal(response(wrongEnvironment).error.message, 'probierz_run argument env must be object');
  assert.equal(response(wrongWatch).error.message, 'probierz_stado_run argument watch must be boolean');
  assert.ok(response(wrongArrayItem).error.message.startsWith('probierz_affected argument files['));
  assert.ok(response(wrongArrayItem).error.message.endsWith('must be string'));
  assert.deepEqual(response(ping).result, {}, 'a refusal must not end the server');
  assert.deepEqual(readdirSync(harness), ['apps'], 'refused publication must not create evidence or jobs');
  assert.deepEqual(readdirSync(join(harness, 'apps')), [], 'refused publication must not change applications');
  report.passed = true;
} catch (error) {
  report.error = error.stack;
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(report));
  console.log(`Retained MCP admission evidence: ${evidence}`);
}
