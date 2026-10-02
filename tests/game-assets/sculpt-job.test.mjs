import assert from 'node:assert/strict';
import { spawnSync, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, test } from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const glina = resolve(root, '../glina');
const runner = join(root, 'apps/game-asset-creator/remote/sculpt-job.sh');
const evidenceRoot = join(root, 'probierz-rs/target/sculpt-job-tests');
await mkdir(evidenceRoot, { recursive: true });
const evidence = await mkdtemp(join(evidenceRoot, 'run-'));
const runs = [];

async function exercise(name, configText) {
  const fixture = await mkdtemp(join(evidence, 'workspace-'));
  try {
    // Execute the actual Glina CLI without copying a checkout or exposing the
    // operator's configuration to a regression in the worker runner.
    const product = join(fixture, 'glina');
    await mkdir(product);
    await symlink(join(glina, 'pipeline'), join(product, 'pipeline'), 'dir');
    const existing = join(product, 'pipeline.config.json');
    const original = '{"worker":"preserve-existing-configuration"}\n';
    await writeFile(existing, original);
    const selected = join(fixture, 'selected.json');
    if (configText !== undefined) await writeFile(selected, configText);
    const results = join(fixture, 'results');
    const args = [runner];
    const run = spawnSync('bash', args, {
      cwd: root,
      encoding: 'utf8',
      env: {
        ...process.env,
        GAC_ROOT: product,
        RESOLVED_CONFIG: selected,
        RESULTS_DIR: results,
        SCULPT_OUT: join(results, 'models'),
      },
    });
    const record = { name, command: ['bash', ...args], status: run.status, signal: run.signal };
    runs.push(record);
    await writeFile(join(evidence, `${name}.stdout`), run.stdout ?? '');
    await writeFile(join(evidence, `${name}.stderr`), run.stderr ?? String(run.error ?? ''));
    assert.ifError(run.error);
    assert.equal(run.status, 1);
    assert.equal(await readFile(existing, 'utf8'), original);
    for (const artifact of ['setup-report.json', 'sculpt-result.json', 'verify-report.json']) {
      await assert.rejects(readFile(join(results, artifact)), { code: 'ENOENT' });
    }
  } finally {
    await rm(fixture, { recursive: true });
  }
}

test('missing selected configuration refuses before provisioning and preserves worker configuration', async () => {
  await exercise('missing-config');
});

test('Glina rejects malformed selected configuration before provisioning and preserves worker configuration', async () => {
  await exercise('malformed-config', '{');
});

after(async () => {
  const files = [runner, join(glina, 'pipeline/cli.js'), join(glina, 'pipeline/config.js')];
  const sources = [];
  for (const file of files) {
    sources.push({ path: file, sha256: createHash('sha256').update(await readFile(file)).digest('hex') });
  }
  const revision = (cwd) => execFileSync('git', ['rev-parse', 'HEAD'], { cwd, encoding: 'utf8' }).trim();
  await writeFile(join(evidence, 'report.json'), `${JSON.stringify({
    revisions: { probierz: revision(root), glina: revision(glina) },
    sources,
    runs,
    qualification: 'Configuration refusals only. A successful sculpt still requires a Stado-selected worker with live Blender and model dependencies.',
  }, null, 2)}\n`);
  console.log(`sculpt job evidence: ${evidence}`);
});
