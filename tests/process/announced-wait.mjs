#!/usr/bin/env node
// A child process Probierz waits for is announced on stderr at both ends.
//
// `probierz hosts` asks Stado's registry for its run hosts by running
// `stado registry pull`. The built Probierz runs it twice, from this
// checkout, under an evidence directory in the crate's ignored target tree:
//
// - with the real Stado on PATH: stderr carries `czekam: stado registry pull;
//   …; id: <id>` before the wait and `koniec czekania: …` or `blad czekania:
//   …` with the same id after it, and the command still answers its hosts;
// - with a PATH that holds no `stado`: the wait still begins and ends with
//   `blad czekania` naming the launch failure, and `hosts` answers the local
//   selectors with the unread registry said on stderr.
//
// Required: PROBIERZ (the built probierz), PROBIERZ_TEST_SOURCE_REVISION
// (the commit it was built from). Stado is this account's own.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
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
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/announced-wait');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const record = { revision, binary, binarySha256: digest(binary), commands: [], passed: false };
function run(program, args, env = process.env) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8', env });
  record.commands.push({ program, args, path: env.PATH, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return result;
}
const WHAT = 'stado registry pull';
// The begin line and the end line of one wait on `stado registry pull`,
// matched by the id the begin line gives it.
function announced(stderr) {
  const lines = stderr.split('\n');
  const begun = lines.find(line => line.startsWith(`czekam: ${WHAT};`));
  assert.ok(begun, `no wait on ${WHAT} was announced before it began:\n${stderr}`);
  const id = begun.split('; ').find(field => field.startsWith('id: '));
  assert.ok(id, `the announced wait names no id: ${begun}`);
  const ended = lines.find(line => (line.startsWith(`koniec czekania: ${WHAT};`)
    || line.startsWith(`blad czekania: ${WHAT};`)) && line.split('; ').includes(id));
  assert.ok(ended, `the wait ${id} on ${WHAT} never said how it ended:\n${stderr}`);
  return { begun, ended };
}
try {
  const version = run(binary, ['--version']);
  assert.ok(version.status !== null && !version.status, version.stderr);
  const source = run('git', ['rev-parse', 'HEAD']);
  assert.equal(source.stdout.trim(), revision, 'the checkout is not at the revision under test');

  const real = run(binary, ['hosts']);
  assert.equal(real.status, version.status, real.stderr);
  record.withStado = announced(real.stderr);

  const bare = { ...process.env, PATH: evidence };
  const missing = run(binary, ['hosts'], bare);
  assert.equal(missing.status, version.status, missing.stderr);
  const { ended } = announced(missing.stderr);
  assert.ok(ended.startsWith('blad czekania:'), `a launch failure ended as a completed wait: ${ended}`);
  record.withoutStado = ended;
  record.passed = true;
} catch (error) {
  record.error = error.stack || String(error);
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), `${JSON.stringify(record, null, '  ')}\n`);
  console.log(`${record.passed ? 'passed' : 'failed'}: ${join(evidence, 'report.json')}`);
}
