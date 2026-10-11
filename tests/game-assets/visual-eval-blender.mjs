#!/usr/bin/env node
// Real game-asset-creator visual evaluation through the Probierz binary: the
// GAC Blender adapter is run by node against the real game_asset_creator
// pipeline/blender.js and a real Blender MCP session. A readable model must
// render and be scored; an unreadable model must be refused with Blender's
// own cause at apphook.gac.blender, never with a node parse error of the
// adapter script itself.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
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
const gacRoot = resolve(required('GAC_ROOT'));
const config = resolve(required('GAC_TEST_CONFIG'));
const models = resolve(required('GAC_TEST_MODELS'));
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/gac-visual-eval');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const report = { revision, binary, binarySha256: digest(binary), gacRoot, config, models,
  commands: [], passed: false };
function command(program, args, env = process.env) {
  const result = spawnSync(program, args, { cwd: root, env, encoding: 'utf8' });
  report.commands.push({ program, args, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return result;
}
function visualEval(modelDir, out) {
  return command(binary, ['apphook', 'game-asset-creator.visual-eval',
    '--models', modelDir, '--out', out, '--config', config], { ...process.env, GAC_ROOT: gacRoot });
}
// The adapter's own source must never be what node refuses.
function assertAdapterParsed(result) {
  const diagnostic = `${result.stdout}\n${result.stderr}`;
  assert.ok(!diagnostic.includes('SyntaxError'), 'node must parse the GAC Blender adapter');
  assert.ok(!diagnostic.includes('pub(crate)'), 'the adapter must carry no Rust visibility tokens');
  return diagnostic;
}
try {
  const version = command(binary, ['--version']);
  assert.ok(version.status !== null && !version.status, version.stderr);
  const source = command('git', ['rev-parse', 'HEAD']);
  assert.equal(source.stdout.trim(), revision);
  const clean = command('git', ['status', '--porcelain', '--untracked-files=no']);
  assert.equal(clean.stdout.trim(), '', 'qualification requires committed source');
  assert.ok(existsSync(join(gacRoot, 'pipeline/blender.js')), 'GAC_ROOT must be a game_asset_creator checkout');
  const glbs = readdirSync(models).filter(name => name.endsWith('.glb'));
  const [readableModel] = glbs;
  assert.ok(readableModel, `${models} must hold at least one GLB`);

  const readableOut = join(evidence, 'readable');
  const readable = visualEval(models, readableOut);
  assertAdapterParsed(readable);
  const answer = JSON.parse(readable.stdout);
  const reportPath = join(readableOut, 'eval-report.json');
  assert.ok(existsSync(reportPath), 'the evaluation must write its report');
  report.evaluation = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.equal(answer.total, glbs.length, 'every GLB must be rendered and scored');
  const renders = readdirSync(readableOut).filter(name => name.endsWith('.png'));
  assert.ok(renders.length >= glbs.length, 'Blender must have written the renders');

  const damagedModels = join(evidence, 'damaged-models');
  mkdirSync(damagedModels);
  cpSync(join(models, readableModel), join(damagedModels, readableModel));
  writeFileSync(join(damagedModels, 'damaged.glb'), 'this is not a glTF binary');
  const damaged = visualEval(damagedModels, join(evidence, 'damaged'));
  assert.ok(damaged.status !== null && damaged.status !== version.status, 'an unreadable model must be refused');
  const diagnostic = assertAdapterParsed(damaged);
  assert.ok(diagnostic.includes('apphook.gac.blender:'), 'the refusal must name the Blender stage');
  assert.ok(diagnostic.includes('damaged.glb'), 'the refusal must carry Blender\'s cause for the unreadable model');
  report.passed = true;
} catch (error) {
  report.failure = error.stack ?? String(error);
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, '\t'));
  console.error(`evidence: ${evidence}`);
}
