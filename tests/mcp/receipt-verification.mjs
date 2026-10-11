#!/usr/bin/env node
// Verify a genuine receipt, refuse its modified copy, then reuse the same server.
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
const receipt = resolve(required('PROBIERZ_TEST_SIGNED_RECEIPT'));
const publicKey = resolve(required('PROBIERZ_TEST_RECEIPT_PUBLIC_KEY'));
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/mcp-receipt');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const harness = join(evidence, 'harness');
mkdirSync(join(harness, 'apps'), { recursive: true });
const report = { revision, binary, binarySha256: digest(binary),
  testSha256: digest(fileURLToPath(import.meta.url)), receipt, receiptSha256: digest(receipt),
  publicKey, publicKeySha256: digest(publicKey), commands: [], passed: false };
function run(program, args, input) {
  const result = spawnSync(program, args, { cwd: root, input, encoding: 'utf8' });
  report.commands.push({ program, args, input, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  assert.ok(result.status !== null && !result.status, result.stderr);
  return result.stdout;
}
function request(method, params) { return { jsonrpc: '2.0', id: randomUUID(), method, params }; }
function verify(file) {
  return request('tools/call', { name: 'probierz_verify_receipt', arguments: { file, publicKey } });
}
try {
  const releaseSource = readFileSync(join(release, 'SOURCE_REVISION'), 'utf8').trim();
  const releaseDigests = readFileSync(join(release, 'DIGESTS'), 'utf8');
  report.releaseEvidence = { directory: release, sourceRevision: releaseSource, digests: releaseDigests };
  assert.equal(releaseSource, revision);
  assert.deepEqual(releaseDigests.trim().split('\n').map(line =>
    line.match(/^\\?(?<sha256>[a-fA-F\d]+)[ \t]+/)?.groups.sha256), [report.binarySha256]);
  assert.equal(run('git', ['rev-parse', 'HEAD']).trim(), revision);
  assert.equal(run('git', ['status', '--porcelain', '--untracked-files=no']).trim(), '');
  const altered = JSON.parse(readFileSync(receipt, 'utf8'));
  assert.ok(altered.signing, 'the input must be a real product-issued signed receipt');
  altered.release = `${altered.release}:${randomUUID()}`;
  const corrupted = join(evidence, 'modified-receipt.json');
  writeFileSync(corrupted, JSON.stringify(altered));
  const original = verify(receipt);
  const rejected = verify(corrupted);
  const ping = request('ping');
  const recovered = verify(receipt);
  const requests = [original, rejected, ping, recovered];
  const output = run(binary, ['--harness', harness, 'mcp'],
    requests.map(value => JSON.stringify(value)).join('\n') + '\n');
  const responses = output.trim().split('\n').map(line => JSON.parse(line));
  const answer = request => {
    const matches = responses.filter(value => value.id === request.id);
    assert.deepEqual(matches.map(value => value.id), [request.id]);
    return matches.find(value => value.id === request.id);
  };
  const verification = request => {
    const response = answer(request);
    assert.equal(Object.hasOwn(response, 'error'), false);
    const content = response.result.content.filter(item => item.type === 'text');
    const values = content.map(item => JSON.parse(item.text));
    const result = values.find(value => value.valid === true);
    assert.ok(result, 'the genuine receipt must verify through the real MCP tool');
    assert.equal(result.signatureValid, true);
    assert.equal(result.trusted, true);
    assert.equal(result.payloadSha256Matches, true);
    return result;
  };
  const before = verification(original);
  const refusal = answer(rejected);
  assert.ok(refusal.error);
  assert.equal(Object.hasOwn(refusal, 'result'), false);
  for (const cause of [corrupted, 'signatureValid=false', 'trusted=true', 'payloadSha256Matches=false']) {
    assert.ok(refusal.error.message.includes(cause), refusal.error.message);
  }
  assert.deepEqual(answer(ping).result, {});
  assert.deepEqual(verification(recovered), before, 'the same server must still verify the unchanged receipt');
  assert.equal(digest(receipt), report.receiptSha256);
  assert.equal(digest(publicKey), report.publicKeySha256);
  report.passed = true;
} catch (error) {
  report.error = error.stack;
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, '\t'));
  console.error(`Retained signed-receipt evidence: ${evidence}`);
}
