// Journey: service-ensure — `stado service ensure NAME --host H --from P --reason R`.
//
// `deploy` refuses a unit that is already declared and bootstraps unconditionally
// otherwise, so there was no command an operator could run twice, or run from a
// script, to assert what a host must be running. This is that command, and what it
// is worth asserting is idempotency: the second pass reports `already_correct`
// with the SAME pid, having touched nothing — no rewrite of the unit file, no
// restart, no unload.
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdir, stat } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  fixtureRegistry,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';
import { bootoutAgent, compileIdleProgram, launchdPid } from './stado-launchd-fixture.mjs';

const slug = 'stado-service-ensure';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const NAME = 'probierz-fixture-ensure';
// The label the product derives for a service name; the plist lands in the
// fixture HOME's LaunchAgents, never the operator's.
const LABEL = `com.wisent.compute.service.${NAME}`;
const plistPath = join(fixture.home, 'Library', 'LaunchAgents', `${LABEL}.plist`);
const ensure = (extra = []) =>
  fixture.invokeJson([
    'service', 'ensure', NAME,
    '--host', FIXTURE_HOST,
    '--from', program,
    '--reason', ...extra,
  ]);

let program;
try {
  await fixture.registry(fixtureRegistry());
  const programDir = join(fixture.servicesRoot, NAME, 'bin');
  await mkdir(programDir, { recursive: true });
  await mkdir(join(fixture.home, 'Library', 'LaunchAgents'), { recursive: true });
  program = await compileIdleProgram(fixture.dir, join(programDir, NAME));

  // 1. No reason, no unit: the record this pass would leave is the only account
  //    of why a host started running something.
  const refused = await fixture.invoke([
    'service', 'ensure', NAME, '--host', FIXTURE_HOST, '--from', program,
  ]);
  assert.notEqual(refused.status, 0, 'ensure without --reason must be refused');
  assert.match(refused.output, /--reason <REASON>/);
  assert.ok(!existsSync(plistPath), 'a refused ensure installed a unit anyway');
  assert.equal(launchdPid(LABEL), null, 'a refused ensure started a job anyway');

  // 2. The creating pass. A unit that was installed and started has to be able to
  //    prove it: the exit status is part of the contract, because a script runs
  //    this command and reads it.
  const created = await ensure([
    'the fixture host must run this program; first pass installs the unit', '--json',
  ]);
  assert.equal(created.status, 0, `the creating pass reported failure: ${created.output}`);
  assert.equal(created.json.action, 'created');
  assert.equal(created.json.name, NAME);
  assert.equal(created.json.label, LABEL);
  assert.ok(existsSync(plistPath), 'the unit file was not installed in the fixture HOME');
  const livePid = launchdPid(LABEL);
  assert.ok(livePid, 'launchd is running no process under the label ensure created');
  assert.equal(Number(created.json.pid), livePid, 'the reported pid is not the pid launchd holds');
  const unitBefore = await stat(plistPath);

  // 3. The second pass: nothing to do, and nothing done.
  const again = await ensure(['second pass must change nothing', '--json']);
  assert.equal(again.status, 0, `the idempotent pass reported failure: ${again.output}`);
  assert.equal(again.json.action, 'already_correct', 'a host already running the program was not reported as correct');
  assert.equal(Number(again.json.pid), livePid, 'the idempotent pass restarted the unit');
  assert.equal(launchdPid(LABEL), livePid, 'the unit is running a different process after the second pass');
  const unitAfter = await stat(plistPath);
  assert.equal(unitAfter.mtimeMs, unitBefore.mtimeMs, 'the idempotent pass rewrote the unit file');

  // 4. A program that is not there is reported as that, and nothing is installed
  //    for it.
  const missing = await fixture.invoke([
    'service', 'ensure', 'probierz-fixture-absent',
    '--host', FIXTURE_HOST,
    '--from', join(programDir, 'no-such-program'),
    '--reason', 'a program the host does not have',
  ]);
  assert.notEqual(missing.status, 0, 'ensuring an absent program must not succeed');
  assert.equal(launchdPid('com.wisent.compute.service.probierz-fixture-absent'), null);

  // The unit ensure installed is recorded in the registry it was declared
  // against, so a later command can act on it.
  const registryAfter = await fixture.readRegistry();
  const declared = (registryAfter.targets[0].services || []).find((service) => service.label === LABEL);
  assert.ok(declared, 'the ensured unit was not recorded as a managed service');
  assert.equal(declared.path, plistPath);

  await recordTrace({
    slug,
    journey: 'service-ensure',
    source,
    observations: {
      refusedWithoutReason: { exit: refused.status, unitInstalled: existsSync(plistPath) },
      created: { action: created.json.action, pid: created.json.pid },
      secondPass: { action: again.json.action, pid: again.json.pid, unitRewritten: false },
      recordedService: declared,
    },
    contracts: [
      'ensure without --reason is refused and installs nothing',
      'the creating pass exits zero, reports created, and launchd holds the pid it reports',
      'the second pass reports already_correct with the same pid',
      'the second pass does not rewrite the unit file and does not restart the job',
      'an absent program is refused and no unit is installed for it',
      'the unit ensure installed is recorded as a managed service in the registry',
    ],
  });
} finally {
  bootoutAgent(LABEL);
  await fixture.close();
}
