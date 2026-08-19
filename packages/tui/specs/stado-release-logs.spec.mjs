// Journey: release-logs — `stado release logs PRODUCT --target TARGET`.
//
// The command exists because a candidate died in under ninety seconds and the
// rollout state said only that it had. The contract worth defending is the
// three-way distinction it draws about each of the candidate's two logs:
// `missing` (the agent never opened that file), `empty` (it is there and the
// candidate wrote nothing) and `read` (here are its last lines) — plus the size
// beside a tail, so a tail is never mistaken for the whole file.
import assert from 'node:assert/strict';
import { readFile, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  FIXTURE_PRODUCT,
  fixtureRegistry,
  fixtureReleaseControl,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';

const slug = 'stado-release-logs';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const stream = (report, name) => {
  const found = report.streams.find((entry) => entry.stream === name);
  assert.ok(found, `the report carries no ${name} stream`);
  return found;
};

try {
  await fixture.registry(
    fixtureRegistry({
      releaseControl: fixtureReleaseControl({
        home: fixture.home,
        stateDir: fixture.stateDir,
        logsRoot: fixture.logsRoot,
        installRoot: join(fixture.servicesRoot, FIXTURE_PRODUCT),
      }),
    }),
  );

  // The candidate's own account of why it died: stderr has it, stdout is there
  // and empty. That asymmetry is the incident, and it is the reason the command
  // reads stderr first.
  const errPath = join(fixture.logsRoot, `${FIXTURE_PRODUCT}-0.2.27.err`);
  const outPath = join(fixture.logsRoot, `${FIXTURE_PRODUCT}-0.2.27.out`);
  const errLines = Array.from({ length: 10 }, (_, index) => `line ${index + 1}: starting listener`);
  errLines.push("panic: missing config key 'router.upstream'");
  await writeFile(errPath, `${errLines.join('\n')}\n`);
  await writeFile(outPath, '');
  const errBytes = (await stat(errPath)).size;

  const read = await fixture.invokeJson([
    'release', 'logs', FIXTURE_PRODUCT,
    '--target', FIXTURE_HOST,
    '--stream', 'both',
    '--lines', '3',
    '--json',
  ]);
  assert.equal(read.status, 0, `reading logs failed: ${read.output}`);
  assert.equal(read.json.product, FIXTURE_PRODUCT);
  assert.equal(read.json.target, FIXTURE_HOST);
  // `--version` was not passed, so the version read is the desired one: the
  // version any candidate on this host is running.
  assert.equal(read.json.version, '0.2.27');

  const errRead = stream(read.json, 'err');
  assert.equal(errRead.state, 'read');
  assert.equal(errRead.path, errPath);
  assert.deepEqual(errRead.lines, errLines.slice(-3), 'the tail is not the last lines of the file');
  assert.equal(errRead.bytes, errBytes, 'a tail must report the size of the WHOLE file beside it');
  assert.ok(errRead.lines.at(-1).includes('router.upstream'), 'the reason the candidate died is missing');

  const outRead = stream(read.json, 'out');
  assert.equal(outRead.state, 'empty', 'a present, empty log is not the same answer as a missing one');
  assert.equal(outRead.bytes, 0);
  assert.deepEqual(outRead.lines, []);

  // stderr first, in the human rendering too: the answer was in `.err` and
  // printing stdout first buries it.
  const rendered = await fixture.invoke([
    'release', 'logs', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--lines', '3',
  ]);
  assert.equal(rendered.status, 0, `the human rendering failed: ${rendered.output}`);
  assert.ok(
    rendered.output.indexOf(errPath) < rendered.output.indexOf(outPath),
    'stderr must be rendered before stdout',
  );

  // A version this host never ran: both files are absent, and absent is its own
  // state with no byte count at all.
  const missing = await fixture.invokeJson([
    'release', 'logs', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--version', '9.9.9', '--json',
  ]);
  assert.equal(missing.status, 0, `reading a missing version failed: ${missing.output}`);
  assert.equal(missing.json.version, '9.9.9');
  for (const name of ['err', 'out']) {
    const entry = stream(missing.json, name);
    assert.equal(entry.state, 'missing');
    assert.equal(entry.bytes, null, 'a missing log has no size, not a size of zero');
    assert.deepEqual(entry.lines, []);
  }

  // One stream when one is asked for.
  const only = await fixture.invokeJson([
    'release', 'logs', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--stream', 'err', '--json',
  ]);
  assert.equal(only.status, 0);
  assert.deepEqual(only.json.streams.map((entry) => entry.stream), ['err']);

  // A tail of nothing is refused rather than answered with an empty list that
  // would read as an empty log.
  const refused = await fixture.invoke([
    'release', 'logs', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--lines', '0',
  ]);
  assert.notEqual(refused.status, 0, '--lines 0 must be refused');
  assert.match(refused.output, /--lines must be at least 1/);

  // Read-only: the candidate's logs are exactly as they were.
  assert.equal((await readFile(errPath, 'utf8')), `${errLines.join('\n')}\n`);
  assert.equal((await stat(outPath)).size, 0);

  await recordTrace({
    slug,
    journey: 'release-logs',
    source,
    observations: {
      readState: { state: errRead.state, bytes: errRead.bytes, lines: errRead.lines.length },
      emptyState: { state: outRead.state, bytes: outRead.bytes },
      missingState: stream(missing.json, 'err').state,
      refusedExitStatus: refused.status,
    },
    contracts: [
      'a log that was read reports its last lines and the size of the whole file',
      'a present, empty log is reported as empty, not as missing',
      'a log the agent never wrote is reported as missing, with no size',
      'stderr is read and rendered before stdout',
      '--lines 0 is refused instead of answered with an empty tail',
      'reading a candidate log changes nothing on the host',
    ],
  });
} finally {
  await fixture.close();
}
