import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { isAbsolute, join } from 'node:path';

const repository = '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo-web';
const envFile = process.env.ECHO_ANALYTICS_TEST_ENV?.trim();
assert.ok(envFile, 'ECHO_ANALYTICS_TEST_ENV is required; see the echo-web manifest prerequisites');
assert.ok(isAbsolute(envFile), 'ECHO_ANALYTICS_TEST_ENV must be an absolute path');
assert.ok(statSync(envFile).isFile(), 'ECHO_ANALYTICS_TEST_ENV must identify a readable file');

function parseEnvironment(text) {
  const environment = {};
  for (const sourceLine of text.split(/\r?\n/)) {
    const line = sourceLine.trim();
    if (!line || line.startsWith('#')) continue;
    const separator = line.indexOf('=');
    if (separator < 1) continue;
    const name = line.slice(0, separator).trim().replace(/^export\s+/, '');
    let value = line.slice(separator + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    environment[name] = value;
  }
  return environment;
}

async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  assert.ok(address && typeof address === 'object');
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  return address.port;
}

async function waitUntilReady(url, process, logs) {
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    assert.equal(process.exitCode, null, `Echo exited before readiness:\n${logs.join('')}`);
    try {
      const response = await fetch(`${url}/api/health`);
      if (response.status === 200) return;
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  assert.fail(`Echo did not become ready:\n${logs.join('')}`);
}

const configured = parseEnvironment(readFileSync(envFile, 'utf8'));
for (const name of ['NEXT_PUBLIC_SUPABASE_URL', 'SUPABASE_SERVICE_ROLE_KEY']) {
  assert.ok(configured[name], `${name} is required in ECHO_ANALYTICS_TEST_ENV`);
}
configured.NEXT_PUBLIC_SITE_URL ||= 'https://echo.wisent.com';

const port = await availablePort();
const baseUrl = `http://127.0.0.1:${port}`;
const logs = [];
const app = spawn('npm', ['run', 'dev', '--', '--hostname', '127.0.0.1', '--port', String(port)], {
  cwd: repository,
  env: { ...process.env, ...configured },
  stdio: ['ignore', 'pipe', 'pipe'],
  detached: true,
});
app.stdout.on('data', chunk => logs.push(chunk.toString()));
app.stderr.on('data', chunk => logs.push(chunk.toString()));

let result;
try {
  await waitUntilReady(baseUrl, app, logs);
  result = spawnSync('npm', ['run', 'test:analytics'], {
    cwd: repository,
    env: { ...process.env, ...configured, ECHO_ANALYTICS_TEST_BASE_URL: baseUrl },
    encoding: 'utf8',
    timeout: 120000,
  });
} finally {
  const exited = new Promise(resolve => app.once('exit', resolve));
  if (app.pid) {
    try {
      process.kill(-app.pid, 'SIGTERM');
    } catch (error) {
      if (error?.code !== 'ESRCH') throw error;
    }
  }
  await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 5000))]);
}

assert.ifError(result.error);
process.stdout.write(result.stdout);
process.stderr.write(result.stderr);
assert.equal(result.status, 0, [result.stdout, result.stderr, logs.join('')].filter(Boolean).join('\n') || `analytics tests exited ${result.status}`);

const evidencePath = join(process.env.PROBIERZ_ARTIFACTS || 'test-results', 'echo-web-analytics-collectors.json');
await mkdir(join(evidencePath, '..'), { recursive: true });
await writeFile(evidencePath, `${JSON.stringify({
  command: 'npm run test:analytics',
  repository,
  assertions: ['web persisted once', 'mobile persisted once', 'invalid clients refused'],
}, null, 2)}\n`, { mode: 0o600 });
