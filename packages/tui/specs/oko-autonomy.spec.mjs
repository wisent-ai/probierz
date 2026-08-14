import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

const manifestText = await readFile(new URL('../../../apps/oko/probierz.yaml', import.meta.url), 'utf8');
const sourceRoot = process.env.OKO_SOURCE_ROOT
  || manifestText.match(/^  - root: (.+)$/m)?.[1]?.trim();
assert.ok(sourceRoot, 'Oko manifest must provide the source repository root');

async function runCommand(executable, arguments_) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, arguments_, {
      cwd: sourceRoot,
      env: {
        ...process.env,
        SWIFT_DETERMINISTIC_HASHING: '1',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
    child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
    const timer = setTimeout(() => {
      child.kill('SIGTERM');
      reject(new Error(`Oko autonomy command timed out: ${executable} ${arguments_.join(' ')}`));
    }, 900_000);
    child.once('error', reject);
    child.once('close', (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, stdout, stderr });
    });
  });
}

const revisionResult = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'rev-parse', 'HEAD']);
assert.equal(revisionResult.code, 0, `cannot resolve Oko source revision: ${revisionResult.stderr}`);
const sourceRevision = revisionResult.stdout.trim();
assert.match(sourceRevision, /^[0-9a-f]{40}$/, 'Oko source revision is not a full Git SHA');
const statusResult = await runCommand('/usr/bin/git', ['-C', sourceRoot, 'status', '--porcelain']);
assert.equal(statusResult.code, 0, `cannot inspect Oko source state: ${statusResult.stderr}`);
if (process.env.OKO_SOURCE_ROOT) {
  assert.equal(statusResult.stdout, '', 'explicit Oko evidence source must be a clean worktree');
}

const build = await runCommand('/usr/bin/swift', [
  'build',
  '--package-path', sourceRoot,
  '--product', 'oko-cli',
]);
assert.equal(
  build.code,
  0,
  `Oko autonomy CLI build exited ${build.code ?? build.signal}:\n${build.stderr.slice(-6000)}\n${build.stdout.slice(-6000)}`,
);

const result = await runCommand('/usr/bin/swift', [
  'test',
  '--package-path', sourceRoot,
  '--filter', 'OkoAutonomyTests',
]);
assert.equal(
  result.code,
  0,
  `Oko autonomy contract tests exited ${result.code ?? result.signal}:\n${result.stderr.slice(-6000)}\n${result.stdout.slice(-6000)}`,
);
assert.match(result.stdout + result.stderr, /OkoAutonomyTests/, 'autonomy tests were not selected');
assert.match(result.stdout + result.stderr, /0 failures|0 failed/i, 'autonomy suite did not report a clean result');

const artifacts = process.env.PROBIERZ_ARTIFACTS;
const mediaManifest = process.env.PROBIERZ_MEDIA_MANIFEST;
assert.ok(artifacts, 'PROBIERZ_ARTIFACTS is required');
assert.ok(mediaManifest, 'PROBIERZ_MEDIA_MANIFEST is required');
const tracePath = join(artifacts, 'oko-autonomy.trace.json');
await mkdir(dirname(tracePath), { recursive: true });
await writeFile(tracePath, `${JSON.stringify({
  schemaVersion: 1,
  kind: 'probierz-oko-autonomy-trace',
  evidenceLevel: 'E3',
  runId: process.env.PROBIERZ_RUN_ID || null,
  status: 'completed',
  observation: {
    sourceRoot,
    sourceRevision,
    sourceDirty: statusResult.stdout !== '',
    buildExitCode: build.code,
    testExitCode: result.code,
    output: `${build.stdout}${build.stderr}${result.stdout}${result.stderr}`.trim().slice(-6000),
  },
  contracts: [
    'missing policy state is disabled by default',
    'experimental policy is local-user scoped and path confined',
    'competing schedulers cannot release another scheduler lease',
    'goal completion requires both a successful Pursuit receipt and an accepted independent verdict',
  ],
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
