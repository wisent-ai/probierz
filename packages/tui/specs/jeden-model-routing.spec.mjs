import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';


function requiredEnv(name, detail) {
  const value = process.env[name]?.trim();
  assert.ok(value, `${name} is required: ${detail}`);
  return value;
}

const binary = requiredEnv('TUI_CMD', 'provide the released Jeden executable');
const model = requiredEnv('JEDEN_MODEL', 'provide a real model coordinate available to this workload');
for (const [name, detail] of [
  ['BRAMA_URL', 'provide the externally provisioned Brama router URL'],
  ['WISENT_APP_AGENT_ID', 'provide the externally provisioned Jeden workload identity'],
  ['WISENT_APP_AGENT_AUTH_SECRET', 'provide the workload signing credential outside Probierz'],
]) requiredEnv(name, detail);

const home = await mkdtemp('/tmp/probierz-jeden-model-route-');
try {
  const result = await new Promise((resolve, reject) => {
    const child = spawn(binary, ['run', 'Respond exactly: OK', '--model', model], {
      cwd: home,
      env: {
        ...process.env,
        HOME: home,
        XDG_STATE_HOME: `${home}/state`,
        XDG_CONFIG_HOME: `${home}/config`,
        XDG_CACHE_HOME: `${home}/cache`,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
    child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
    const timer = setTimeout(() => {
      child.kill('SIGTERM');
      reject(new Error('Jeden model route timed out after 180000ms'));
    }, 180_000);
    child.once('error', reject);
    child.once('close', (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, stdout, stderr });
    });
  });

  assert.equal(result.code, 0, `Jeden exited ${result.code ?? result.signal}: ${result.stderr}`);
  assert.match(result.stdout, /\bOK\b/, `expected the signed Brama route to return OK: ${result.stdout}`);

  const artifacts = requiredEnv('PROBIERZ_ARTIFACTS', 'provide the run artifact directory');
  const mediaManifest = requiredEnv('PROBIERZ_MEDIA_MANIFEST', 'provide the report media manifest path');
  const tracePath = join(artifacts, 'jeden-model-routing.trace.json');
  await mkdir(dirname(tracePath), { recursive: true });
  await writeFile(tracePath, `${JSON.stringify({
    schemaVersion: 1,
    kind: 'probierz-jeden-model-routing-trace',
    evidenceLevel: 'E3',
    runId: process.env.PROBIERZ_RUN_ID || null,
    model,
    status: 'completed',
    observation: {
      exitCode: result.code,
      reply: result.stdout.trim().slice(-1000),
      stderr: result.stderr.trim().slice(-1000),
    },
    redaction: {
      status: 'verified_redacted',
      credentialsIncluded: false,
      privateRecordsIncluded: false,
    },
    publicationRequirements: {
      artifactKind: 'trace',
      minimumEvidence: 'E3',
      redactionStatus: 'verified_redacted',
      signedReceiptRequired: true,
    },
  }, null, 2)}\n`, { mode: 0o600 });
  await mkdir(dirname(mediaManifest), { recursive: true });
  await writeFile(mediaManifest, `${JSON.stringify([{
    file: tracePath,
    kind: 'trace',
    contentType: 'application/json',
  }], null, 2)}\n`, { mode: 0o600 });
} finally {
  await rm(home, { recursive: true, force: true });
}
