#!/usr/bin/env node
// Analyze an actual recorded product run, then a damaged copy of its recording.
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
const release = resolve(required('PROBIERZ_TEST_RELEASE_EVIDENCE'));
const reportPath = resolve(required('PROBIERZ_TEST_RECORDED_REPORT'));
const frames = required('PROBIERZ_TEST_FRAMES');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const evidenceRoot = join(root, 'probierz-rs/target/real-tests/media');
mkdirSync(evidenceRoot, { recursive: true });
const evidence = mkdtempSync(join(evidenceRoot, 'run-'));
const record = { revision, binary, binarySha256: digest(binary), reportPath,
  inputReportSha256: digest(reportPath), commands: [], passed: false };
function run(program, args) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8' });
  record.commands.push({ program, args, status: result.status, signal: result.signal,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return result;
}
try {
  assert.equal(readFileSync(join(release, 'SOURCE_REVISION'), 'utf8').trim(), revision);
  const releaseDigests = readFileSync(join(release, 'DIGESTS'), 'utf8');
  assert.deepEqual(releaseDigests.trim().split('\n').map(line =>
    line.match(/^\\?(?<sha256>[a-fA-F\d]+)[ \t]+/)?.groups.sha256), [record.binarySha256]);
  record.releaseEvidence = { directory: release, digests: releaseDigests };
  const version = run(binary, ['--version']);
  assert.ok(version.status !== null && !version.status, version.stderr);
  const source = run('git', ['rev-parse', 'HEAD']);
  assert.equal(source.status, version.status);
  assert.equal(source.stdout.trim(), revision);
  const clean = run('git', ['status', '--porcelain', '--untracked-files=no']);
  assert.equal(clean.status, version.status);
  assert.equal(clean.stdout.trim(), '', 'candidate source must be committed');
  const successful = run(binary, ['analyze', reportPath, join(evidence, 'valid'), '--frames', frames]);
  assert.equal(successful.status, version.status, successful.stderr);
  const analyzed = JSON.parse(successful.stdout);
  const video = analyzed.media.find(item => item.kind === 'video' && item.recording);
  assert.ok(video, 'the supplied real run must contain a decodable video recording');
  assert.ok(video.frames?.some(file => typeof file === 'string'), 'requested extraction must produce frame files');
  record.recording = { file: video.file, sha256: digest(video.file) };
  const noDestination = run(binary, ['analyze', reportPath, '--frames', frames]);
  assert.ok(noDestination.status !== null && noDestination.status !== version.status,
    'requested extraction must refuse an absent output destination');
  assert.ok(`${noDestination.stdout}\n${noDestination.stderr}`.includes('artifacts directory'));
  for (const frame of video.frames) {
    const inspected = run('ffprobe', ['-v', 'error', '-select_streams', 'v:0',
      '-show_entries', 'stream=width,height', '-of', 'json', frame]);
    assert.equal(inspected.status, version.status, inspected.stderr);
    const stream = JSON.parse(inspected.stdout).streams.find(item => item.width && item.height);
    assert.ok(stream, `the extracted frame must be decodable: ${frame}`);
    assert.equal(stream.width, video.recording.width);
    assert.equal(stream.height, video.recording.height);
  }
  const damaged = join(evidence, 'damaged-recording.mp4');
  writeFileSync(damaged, '');
  const copy = JSON.parse(readFileSync(reportPath, 'utf8'));
  for (const test of copy.tests) {
    if (test.video === video.file) test.video = damaged;
    if (Object.hasOwn(test, 'media')) {
      assert.ok(Array.isArray(test.media), 'the actual report media field must be an array');
      for (const item of test.media) {
        if (item.file === video.file) item.file = damaged;
      }
    }
  }
  const damagedReport = join(evidence, 'damaged-report.json');
  writeFileSync(damagedReport, JSON.stringify(copy));
  const optional = run(binary, ['analyze', damagedReport]);
  assert.equal(optional.status, version.status, optional.stderr);
  const failedMedia = JSON.parse(optional.stdout).media.find(item => item.file === damaged);
  assert.ok(failedMedia?.recordingError?.includes('ffprobe'));
  assert.ok(failedMedia.recordingError.includes(damaged));
  assert.equal(Object.hasOwn(failedMedia, 'recording'), false, 'failed metadata must not look valid');
  const refused = run(binary, ['analyze', damagedReport, join(evidence, 'damaged'), '--frames', frames]);
  assert.ok(refused.status !== null && refused.status !== version.status,
    'explicit extraction must fail rather than publish an empty successful montage');
  const diagnostic = `${refused.stdout}\n${refused.stderr}`;
  assert.ok(diagnostic.includes(damaged));
  assert.ok(diagnostic.includes('ffprobe'));
  assert.equal(digest(reportPath), record.inputReportSha256, 'the original run report stays unchanged');
  assert.equal(digest(video.file), record.recording.sha256, 'the original recording stays unchanged');
  record.passed = true;
} catch (error) {
  record.failure = error.stack ?? String(error);
  throw error;
} finally {
  writeFileSync(join(evidence, 'report.json'), JSON.stringify(record, null, '\t'));
  console.error(`evidence: ${evidence}`);
}
