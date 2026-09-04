// Journey: host-gates — `stado host gates HOST`.
//
// A host whose queue agent has stopped claiming work, on purpose, for a reason it
// republishes every tick, used to be indistinguishable from a healthy host with
// an empty queue. The contract this journey defends: the blockers are the agent's
// OWN words, reported verbatim; a host that is not claiming exits non-zero so a
// script cannot mistake it for a healthy one; and "the agent said this an hour
// ago" and "nobody ever said anything" are different findings.
import assert from 'node:assert/strict';
import { rm } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  fixtureRegistry,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
  thisHostname,
} from './stado-fleet-fixture.mjs';

const slug = 'stado-host-gates';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const diskPolicy = {
  mode: 'enforce',
  low_free_gb: 10,
  target_free_gb: 20,
  check_interval_seconds: 300,
  max_bytes_per_pass: 21474836480,
  max_items_per_pass: 50,
  max_scan_items: 10000,
  cleaners: {},
};
const claiming = {
  disk_pressure_unresolved: false,
  disk_cleanup_policy_known: true,
  queue_paused: false,
  pinned_only: false,
};
const capacityPath = join(fixture.store, 'capacity', `local-${thisHostname()}.json`);
const gates = (extra = []) =>
  fixture.invokeJson(['host', 'gates', FIXTURE_HOST, '--json', ...extra]);

try {
  await fixture.registry(fixtureRegistry({ target: { disk_cleanup: diskPolicy } }));

  // 1. A host that is claiming: no blockers, and a zero exit so
  //    `stado host reclaim … && stado host gates …` is a usable sentence.
  await fixture.publishCapacity(claiming, { availableCpuCores: 2 });
  const healthy = await gates();
  assert.equal(healthy.status, 0, `a claiming host must exit zero: ${healthy.output}`);
  assert.equal(healthy.json.host, FIXTURE_HOST);
  assert.equal(healthy.json.claiming, true);
  assert.deepEqual(healthy.json.blockers, []);
  assert.equal(healthy.json.disk.low_watermark_gb, 10, 'the declared watermark is not reported');
  assert.equal(healthy.json.disk.target_free_gb, 20);
  assert.equal(healthy.json.disk.policy_mode, 'enforce');
  assert.ok(typeof healthy.json.disk.free_gb === 'number', 'the host did not report its free space');
  assert.equal(healthy.json.capacity.accepting_jobs, true);
  assert.equal(healthy.json.capacity.available_cpu_cores, 2);
  assert.ok(healthy.json.capacity.published_at, 'a live publication has no timestamp');
  assert.ok(healthy.json.capacity.age_seconds !== null, 'a live publication has no age');

  // 2. The incident: the agent publishes `disk_pressure_unresolved` and fails
  //    admission closed. The word comes back exactly as the agent published it.
  await fixture.publishCapacity(
    { ...claiming, disk_pressure_unresolved: true },
    { availableCpuCores: 0, acceptingJobs: false },
  );
  const blocked = await gates();
  assert.notEqual(blocked.status, 0, 'a host that is claiming nothing must not exit zero');
  assert.equal(blocked.json.claiming, false);
  assert.deepEqual(blocked.json.blockers, ['disk_pressure_unresolved']);
  assert.equal(blocked.json.capacity.available_cpu_cores, 0);
  const blockedText = await fixture.invoke(['host', 'gates', FIXTURE_HOST]);
  assert.notEqual(blockedText.status, 0);
  assert.match(blockedText.output, /claiming: no/);
  assert.match(blockedText.output, /blockers: disk_pressure_unresolved/);
  assert.match(
    blockedText.output,
    new RegExp(`${FIXTURE_HOST} is claiming nothing: disk_pressure_unresolved`),
    'the failure message must name the host and its blockers',
  );

  // 3. A host that cannot read its own disk policy is a different problem from a
  //    full disk, so it is named separately — and it never appears alone,
  //    because an unknown threshold also makes the pressure verdict true.
  await fixture.publishCapacity({ ...claiming, disk_cleanup_policy_known: false });
  const policyUnknown = await gates();
  assert.equal(policyUnknown.json.claiming, false);
  assert.ok(policyUnknown.json.blockers.includes('disk_cleanup_policy_unknown'), JSON.stringify(policyUnknown.json.blockers));

  // 4. `stado queue pause` is in effect: the agent publishes it per tick.
  await fixture.publishCapacity({ ...claiming, queue_paused: true });
  const paused = await gates();
  assert.deepEqual(paused.json.blockers, ['queue_paused']);

  // 5. A publication older than the fleet's staleness horizon is still reported,
  //    with its age, rather than deleted: a quiet agent is the finding.
  await fixture.publishCapacity(claiming, {
    publishedAt: new Date(Date.now() - 7_200_000).toISOString(),
  });
  const stale = await gates();
  assert.equal(stale.json.claiming, false);
  assert.ok(stale.json.blockers.includes('capacity_publication_stale'), JSON.stringify(stale.json.blockers));
  assert.ok(stale.json.capacity.age_seconds > 3600, 'the age of a stale publication is not reported');

  // 6. No publication at all: the scheduler cannot see this host, so it cannot be
  //    given anything. Not the agent's word — a silent agent has no words.
  await rm(capacityPath, { force: true });
  const silent = await gates();
  assert.equal(silent.json.claiming, false);
  assert.ok(silent.json.blockers.includes('no_capacity_publication'), JSON.stringify(silent.json.blockers));
  assert.equal(silent.json.capacity.published_at, null);
  const silentText = await fixture.invoke(['host', 'gates', FIXTURE_HOST]);
  assert.match(silentText.output, /nothing published for this host/);

  // 7. A pinned host claims only work addressed to it, and says so.
  await fixture.registry(fixtureRegistry({ target: { disk_cleanup: diskPolicy, pinned_only: true } }));
  await fixture.publishCapacity(claiming);
  const pinned = await gates();
  assert.ok(pinned.json.blockers.includes('pinned_only'), JSON.stringify(pinned.json.blockers));

  await recordTrace({
    slug,
    journey: 'host-gates',
    source,
    observations: {
      claiming: { exit: healthy.status, blockers: healthy.json.blockers, disk: healthy.json.disk },
      diskPressure: { exit: blocked.status, blockers: blocked.json.blockers },
      policyUnknown: policyUnknown.json.blockers,
      queuePaused: paused.json.blockers,
      stalePublication: { blockers: stale.json.blockers, ageSeconds: stale.json.capacity.age_seconds },
      noPublication: silent.json.blockers,
      pinnedOnly: pinned.json.blockers,
    },
    contracts: [
      'a claiming host reports no blockers and exits zero',
      "a host that is claiming nothing exits non-zero and names the agent's own blocker words",
      'an unreadable disk policy is named separately from disk pressure',
      'a paused queue is reported as the agent published it',
      'a stale publication is reported with its age rather than dropped',
      'no publication at all is reported as no publication, not as an empty one',
      'a pinned host is reported as pinned',
      'the read is read-only: one df, one state read and one object read',
    ],
  });
} finally {
  await fixture.close();
}
