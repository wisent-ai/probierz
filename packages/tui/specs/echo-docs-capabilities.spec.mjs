import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { join } from 'node:path';

const repository = '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo-landing';

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
    assert.equal(process.exitCode, null, `Echo docs exited before readiness:\n${logs.join('')}`);
    try {
      const response = await fetch(`${url}/docs`);
      if (response.status === 200) return;
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  assert.fail(`Echo docs did not become ready:\n${logs.join('')}`);
}

const port = await availablePort();
const baseUrl = `http://127.0.0.1:${port}`;
const logs = [];
const app = spawn('npm', ['run', 'dev', '--', '--hostname', '127.0.0.1', '--port', String(port)], {
  cwd: repository,
  env: process.env,
  stdio: ['ignore', 'pipe', 'pipe'],
  detached: true,
});
app.stdout.on('data', chunk => logs.push(chunk.toString()));
app.stderr.on('data', chunk => logs.push(chunk.toString()));

let result;
try {
  await waitUntilReady(baseUrl, app, logs);
  result = spawnSync('npm', ['run', 'test:docs'], {
    cwd: repository,
    env: { ...process.env, ECHO_DOCS_BASE_URL: baseUrl },
    encoding: 'utf8',
    timeout: 300000,
  });
} finally {
  const exited = new Promise(resolve => app.once('exit', resolve));
  if (app.pid) {
    try { process.kill(-app.pid, 'SIGTERM'); } catch (error) { if (error?.code !== 'ESRCH') throw error; }
  }
  await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 5000))]);
}

assert.ifError(result.error);
process.stdout.write(result.stdout);
process.stderr.write(result.stderr);
assert.equal(result.status, 0, [result.stdout, result.stderr, logs.join('')].filter(Boolean).join('\n') || `Echo docs tests exited ${result.status}`);

const evidencePath = join(process.env.PROBIERZ_ARTIFACTS || 'test-results', 'echo-docs-capabilities.json');
await mkdir(join(evidencePath, '..'), { recursive: true });
await writeFile(evidencePath, `${JSON.stringify({
  command: 'npm run test:docs',
  repository,
  assertions: ['capability catalogue', '17 canonical CLI command pages', 'CLI index links'],
}, null, 2)}\n`, { mode: 0o600 });
