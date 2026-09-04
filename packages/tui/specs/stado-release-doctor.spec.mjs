// Journey: release-doctor — `stado release doctor PRODUCT --target TARGET`.
//
// One verdict over desired versus observed, the candidate, the host's
// quarantine map and its claiming gates. The two blocking conditions are the
// point: a desired artefact whose digest sits in the host's quarantine map is
// never retried by the agent, and an unresolved disk gate is the state in which
// the queue agent claims nothing at all. `settled` is reserved for the host that
// is actually running what the registry wants.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  FIXTURE_PRODUCT,
  fixtureRegistry,
  fixtureReleaseControl,
  openFleetFixture,
  recordTrace,
  settledState,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';

const slug = 'stado-release-doctor';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const DESIRED = '0.2.27';
const DESIRED_DIGEST = 'a'.repeat(64);
const installRoot = join(fixture.servicesRoot, FIXTURE_PRODUCT);
const statePath = join(fixture.stateDir, `${FIXTURE_PRODUCT}.json`);
const healthyGates = {
  disk_pressure_unresolved: false,
  disk_cleanup_policy_known: true,
  queue_paused: false,
  pinned_only: false,
};
const diagnose = () =>
  fixture.invokeJson([
    'release', 'doctor', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--json',
  ]);

try {
  await fixture.registry(
    fixtureRegistry({
      // A declared disk policy, so the host has a known low watermark and an
      // unresolved gate can only come from the agent's own published verdict.
      target: {
        disk_cleanup: {
          mode: 'enforce',
          low_free_gb: 10,
          target_free_gb: 20,
          check_interval_seconds: 300,
          max_bytes_per_pass: 21474836480,
          max_items_per_pass: 50,
          max_scan_items: 10000,
          cleaners: {},
        },
      },
      releaseControl: fixtureReleaseControl({
        home: fixture.home,
        stateDir: fixture.stateDir,
        logsRoot: fixture.logsRoot,
        desiredVersion: DESIRED,
        desiredDigest: DESIRED_DIGEST,
        installRoot,
      }),
    }),
  );
  await fixture.publishCapacity(healthyGates);

  // 1. The host runs exactly what the registry wants and nothing is in its way.
  await fixture.writeReleaseState(
    settledState({ version: DESIRED, digest: DESIRED_DIGEST, releaseDir: join(installRoot, DESIRED) }),
  );
  const settled = await diagnose();
  assert.equal(settled.status, 0, `doctor failed on a settled host: ${settled.output}`);
  assert.equal(settled.json.desired_version, DESIRED);
  assert.equal(settled.json.observed_version, DESIRED);
  assert.equal(settled.json.verdict, 'settled');
  assert.deepEqual(settled.json.blockers, []);
  assert.deepEqual(settled.json.quarantined, []);
  assert.equal(settled.json.gates.disk_pressure_unresolved, false);
  // The report carries every contracted key, so a console consuming it does not
  // have to guess which half of the answer it received.
  for (const key of [
    'product', 'target', 'desired_version', 'observed_version', 'phase', 'detail',
    'candidate', 'quarantined', 'gates', 'verdict', 'blockers',
  ]) {
    assert.ok(key in settled.json, `the report is missing ${key}`);
  }
  for (const key of ['port', 'health_status', 'pid_alive']) {
    assert.ok(key in settled.json.candidate, `the candidate section is missing ${key}`);
  }
  // Nothing was started to answer the question: with no candidate recorded
  // there is nothing to probe, and that is said rather than guessed.
  assert.equal(settled.json.candidate.health_status, 'no_candidate');

  // 2. The desired digest is quarantined: the agent will skip that exact release
  //    on every pass until it is cleared, so the rollout is blocked, not rolling.
  const quarantinedAt = new Date(Date.now() - 3_600_000).toISOString();
  const blockedState = {
    ...settledState({ version: '0.2.26', digest: 'd'.repeat(64), releaseDir: join(installRoot, '0.2.26') }),
    phase: 'quarantined',
    detail: 'candidate did not become ready within 90s: pid 46748 is gone',
    quarantined: {
      [DESIRED_DIGEST]: {
        reason: 'candidate did not become ready within 90s: pid 46748 is gone',
        quarantined_at: quarantinedAt,
      },
    },
  };
  await fixture.writeReleaseState(blockedState);
  const stateBefore = await readFile(statePath, 'utf8');
  const blocked = await diagnose();
  assert.equal(blocked.status, 0, 'a blocked verdict is a finding, not a failed command');
  assert.equal(blocked.json.observed_version, '0.2.26');
  assert.equal(blocked.json.verdict, 'blocked');
  assert.ok(
    blocked.json.blockers.includes('desired_digest_quarantined'),
    `the quarantined desired digest is not named: ${JSON.stringify(blocked.json.blockers)}`,
  );
  assert.equal(blocked.json.quarantined.length, 1);
  assert.equal(blocked.json.quarantined[0].digest, DESIRED_DIGEST);
  assert.equal(blocked.json.quarantined[0].is_desired_digest, true);
  // The instant the entry carried, not a spelling of it: the product prints
  // RFC3339 with an explicit offset and the fixture wrote a `Z`.
  assert.equal(
    Date.parse(blocked.json.quarantined[0].quarantined_at),
    Date.parse(quarantinedAt),
  );
  assert.equal(blocked.json.phase, 'quarantined');
  assert.match(blocked.json.detail, /pid 46748 is gone/);

  // Read-only: diagnosing wrote nothing back into the rollout state.
  assert.equal(await readFile(statePath, 'utf8'), stateBefore);

  // The human rendering hands the operator the next command instead of a verdict
  // they have to translate.
  const rendered = await fixture.invoke(['release', 'doctor', FIXTURE_PRODUCT, '--target', FIXTURE_HOST]);
  assert.equal(rendered.status, 0);
  assert.match(rendered.output, /verdict\s+blocked/);
  assert.match(rendered.output, /blockers\s+desired_digest_quarantined/);
  assert.match(rendered.output, new RegExp(`next: stado release logs ${FIXTURE_PRODUCT}`));

  // 3. A quarantined digest that is NOT the desired one does not block: the
  //    registry has moved on from it.
  await fixture.writeReleaseState({
    ...settledState({ version: DESIRED, digest: DESIRED_DIGEST, releaseDir: join(installRoot, DESIRED) }),
    quarantined: {
      ['e'.repeat(64)]: { reason: 'an older candidate never became ready', quarantined_at: quarantinedAt },
    },
  });
  const stale = await diagnose();
  assert.equal(stale.json.verdict, 'settled', 'a stale quarantine entry must not block the current release');
  assert.equal(stale.json.quarantined[0].is_desired_digest, false);
  assert.deepEqual(stale.json.blockers, []);

  // 4. An unresolved disk gate blocks even at the desired version: this is the
  //    state in which the host claims nothing at all.
  await fixture.publishCapacity(
    { ...healthyGates, disk_pressure_unresolved: true },
    { availableCpuCores: 0, acceptingJobs: false },
  );
  await fixture.writeReleaseState(
    settledState({ version: DESIRED, digest: DESIRED_DIGEST, releaseDir: join(installRoot, DESIRED) }),
  );
  const gated = await diagnose();
  assert.equal(gated.json.observed_version, DESIRED, 'the host is at the desired version');
  assert.equal(gated.json.verdict, 'blocked', 'an unresolved disk gate blocks a rollout at the desired version');
  assert.ok(gated.json.blockers.includes('disk_pressure_unresolved'), JSON.stringify(gated.json.blockers));
  assert.equal(gated.json.gates.disk_pressure_unresolved, true);
  assert.equal(gated.json.gates.low_watermark_gb, 10, 'the gate is reported against the declared watermark');

  await recordTrace({
    slug,
    journey: 'release-doctor',
    source,
    observations: {
      settled: { verdict: settled.json.verdict, observed: settled.json.observed_version },
      quarantinedDesiredDigest: { verdict: blocked.json.verdict, blockers: blocked.json.blockers },
      staleQuarantine: { verdict: stale.json.verdict, blockers: stale.json.blockers },
      unresolvedDiskGate: { verdict: gated.json.verdict, blockers: gated.json.blockers },
    },
    contracts: [
      'settled only when the version the host reports equals the desired one',
      'blocked when the desired artefact digest sits in the host quarantine map',
      'a quarantined digest that is not the desired one does not block',
      'blocked when the host disk gate is unresolved, even at the desired version',
      'the report carries the complete contracted key set including the candidate section',
      'diagnosing starts nothing and writes nothing back to the rollout state',
    ],
  });
} finally {
  await fixture.close();
}
