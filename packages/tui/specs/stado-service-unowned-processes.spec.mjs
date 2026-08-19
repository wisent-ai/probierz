// Journey: service-unowned-processes — `stado service list --unowned`.
//
// Two `stado agent` processes ran for four days on the always-on Mac with no
// launchd unit behind them, and every command in this group answered about
// declared units, so none of them said anything at all about those two. The
// contract: ownership is asked of launchd itself, a process a unit owns is not
// reported, a process merely mentioning a managed root on its command line is not
// reported, and the read signals nothing.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  FIXTURE_PRODUCT,
  fixtureRegistry,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';
import {
  alive,
  bootoutAgent,
  bootstrapAgent,
  compileIdleProgram,
  spawnOrphan,
  stop,
  writeAgentPlist,
} from './stado-launchd-fixture.mjs';

const slug = 'stado-service-unowned-processes';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const LABEL = 'ai.wisent.probierz.fixture.owned';
const plistPath = join(fixture.home, 'Library', 'LaunchAgents', `${LABEL}.plist`);
let orphanPid = null;
let taillPid = null;

try {
  await fixture.registry(
    fixtureRegistry({
      target: {
        services: [
          {
            name: 'probierz-fixture-owned',
            kind: 'launchd',
            label: LABEL,
            unit: '',
            path: plistPath,
            managed_since: '2026-08-18T00:00:00Z',
          },
        ],
      },
    }),
  );

  // A delivered tree under the managed services root, with an entry point an
  // interpreter can run — the shape a release tree actually runs under.
  const tree = join(fixture.servicesRoot, FIXTURE_PRODUCT, '0.2.26', 'bin');
  await mkdir(tree, { recursive: true });
  const entry = join(tree, 'start');
  await writeFile(entry, '#!/bin/sh\nwhile :; do /bin/sleep 5; done\n', { mode: 0o755 });
  const logFile = join(fixture.servicesRoot, FIXTURE_PRODUCT, '0.2.26', 'run.log');
  await writeFile(logFile, 'a log under the managed root\n');

  // 1. A product process no unit owns.
  orphanPid = spawnOrphan(`/bin/sh ${JSON.stringify(entry)}`);
  // 2. A process launchd DOES own, executing out of the same root.
  await mkdir(join(fixture.home, 'Library', 'LaunchAgents'), { recursive: true });
  const ownedProgram = await compileIdleProgram(fixture.dir, join(tree, 'owned-daemon'));
  await writeAgentPlist(plistPath, LABEL, [ownedProgram]);
  const ownedPid = bootstrapAgent(plistPath, LABEL);
  // 3. A reader that merely names a path under the root on its command line.
  taillPid = spawnOrphan(`/usr/bin/tail -f ${JSON.stringify(logFile)}`);

  const listed = await fixture.invokeJson(['service', 'list', '--unowned', '--json']);
  assert.equal(listed.status, 0, `listing unowned processes failed: ${listed.output}`);
  const pids = listed.json.unowned.map((entryRow) => Number(entryRow.pid));

  assert.ok(pids.includes(orphanPid), `the unowned product process ${orphanPid} was not reported`);
  assert.ok(!pids.includes(ownedPid), `a process launchd owns (${ownedPid}) must not be reported`);
  assert.ok(!pids.includes(taillPid), 'a reader that merely names the root must not be reported');

  const reported = listed.json.unowned.find((entryRow) => Number(entryRow.pid) === orphanPid);
  assert.equal(reported.host, FIXTURE_HOST);
  assert.ok(reported.command.includes(entry), 'the report does not name what the process is running');
  assert.ok(reported.started_at, 'the report does not say when the process started');
  assert.equal(
    reported.product_guess,
    FIXTURE_PRODUCT,
    'the report does not attribute the process to the product whose root it runs from',
  );

  const rendered = await fixture.invoke(['service', 'list', '--unowned']);
  assert.equal(rendered.status, 0);
  assert.ok(rendered.output.includes(String(orphanPid)), 'the human rendering omits the unowned pid');

  // The read starts nothing, stops nothing and signals nothing.
  assert.ok(alive(orphanPid), 'the read killed the unowned process');
  assert.ok(alive(ownedPid), 'the read killed the owned process');

  await recordTrace({
    slug,
    journey: 'service-unowned-processes',
    source,
    observations: {
      reported,
      ownedPidExcluded: ownedPid,
      readerPidExcluded: taillPid,
      unownedCount: listed.json.unowned.length,
    },
    contracts: [
      'a product process no launchd job owns is reported with pid, command, start time and product',
      'a process launchd owns is not reported, even under the same managed root',
      'a reader that merely names a path under the root is not reported',
      'the read signals nothing: every process it enumerated is still running afterwards',
    ],
  });
} finally {
  bootoutAgent(LABEL);
  stop(orphanPid);
  stop(taillPid);
  await fixture.close();
}
