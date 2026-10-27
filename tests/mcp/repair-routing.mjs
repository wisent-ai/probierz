#!/usr/bin/env node
// Real journey of the MCP repair route through the built binary's stdio
// server: a probierz_repair call that names a recorded run by runId must
// reach `probierz repair <app> --run <id>`, the option the CLI declares, and
// be answered by the repair command itself (here: refusing an application
// and run the harness does not hold, naming them), never refused at argument
// parsing over an undeclared `--run-id`. The server must still answer a ping
// afterwards. Every command and its answer go to the run's report.json.
//
// Usage: PROBIERZ=<built binary> PROBIERZ_TEST_REPAIR_ROUNDS=<rounds> node tests/mcp/repair-routing.mjs
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function required(name) {
  assert.ok(process.env[name]?.trim(), `${name} is required`);
  return process.env[name];
}
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const binary = resolve(required('PROBIERZ'));
// The route is under test, not the repair, so any stated round count serves;
// it is the runner's, not a number this journey chooses.
const rounds = Number(required('PROBIERZ_TEST_REPAIR_ROUNDS'));
assert.ok(Number.isSafeInteger(rounds), 'PROBIERZ_TEST_REPAIR_ROUNDS must be a whole number');
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/mcp-repair-routing');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const harness = join(evidence, 'harness');
mkdirSync(join(harness, 'apps'), { recursive: true });
const revision = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim();
const report = { revision, binary, commands: [], passed: false };

function request(method, params) { return { jsonrpc: '2.0', id: randomUUID(), method, params }; }

try {
  const appId = `routing-${randomUUID()}`;
  const runId = randomUUID();
  const repair = request('tools/call', {
    name: 'probierz_repair', arguments: { appId, runId, rounds, dryRun: true },
  });
  const ping = request('ping');
  const input = [repair, ping].map((value) => JSON.stringify(value)).join('\n') + '\n';
  const result = spawnSync(binary, ['--harness', harness, 'mcp'], { cwd: root, input, encoding: 'utf8' });
  report.commands.push({ program: binary, args: ['--harness', harness, 'mcp'], input,
    status: result.status, signal: result.signal, stdout: result.stdout, stderr: result.stderr,
    error: result.error?.message });
  assert.ifError(result.error);
  const responses = result.stdout.trim().split('\n').map((line) => JSON.parse(line));
  const answer = (sent) => {
    const matches = responses.filter((value) => value.id === sent.id);
    const [only, ...others] = matches;
    assert.ok(only, `no answer to ${sent.method}`);
    assert.deepEqual(others, [], `more than one answer to ${sent.method}`);
    return only;
  };

  const repaired = answer(repair);
  const message = repaired.error ? repaired.error.message : JSON.stringify(repaired.result);
  report.repairAnswer = message;
  assert.ok(!message.includes('--run-id'),
    `the runId must be routed to --run, not --run-id: ${message}`);
  assert.ok(message.includes(appId) || message.includes(runId),
    `the repair command itself must answer, naming the application or run it could not find: ${message}`);
  assert.deepEqual(answer(ping).result, {}, 'the server must still answer after the repair call');
  report.passed = true;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, '  '));
}
