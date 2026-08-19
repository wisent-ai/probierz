// Journey: release-quarantine — `stado release quarantine list|clear`.
//
// The agent quarantines a digest that failed to become ready and then never
// retries it, which is correct. This is the way back, and the whole value of the
// command is in what it refuses and what it leaves behind: no clear without a
// digest, no clear without a reason, the previous state copied beside itself
// before anything is written, one appended audit line carrying the reason and
// the entry it destroyed — and nothing started, stopped or restarted.
import assert from 'node:assert/strict';
import { readFile, readdir, stat } from 'node:fs/promises';
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

const slug = 'stado-release-quarantine';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const DESIRED = '0.2.27';
const DESIRED_DIGEST = 'a'.repeat(64);
const QUARANTINE_REASON = 'candidate did not become ready within 90s: pid 46748 is gone';
const installRoot = join(fixture.servicesRoot, FIXTURE_PRODUCT);
const statePath = join(fixture.stateDir, `${FIXTURE_PRODUCT}.json`);
const auditPath = join(fixture.stateDir, `${FIXTURE_PRODUCT}.quarantine-audit.jsonl`);
const quarantinedAt = new Date(Date.now() - 3_600_000).toISOString();

try {
  await fixture.registry(
    fixtureRegistry({
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
  await fixture.publishCapacity({
    disk_pressure_unresolved: false,
    disk_cleanup_policy_known: true,
    queue_paused: false,
    pinned_only: false,
  });

  // A host that has never reconciled this product is a different answer from a
  // host with nothing quarantined, and the command says which one it is.
  const unreconciled = await fixture.invokeJson([
    'release', 'quarantine', 'list', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--json',
  ]);
  assert.equal(unreconciled.status, 0, `listing an unreconciled host failed: ${unreconciled.output}`);
  assert.deepEqual(unreconciled.json.entries, []);
  const unreconciledText = await fixture.invoke([
    'release', 'quarantine', 'list', FIXTURE_PRODUCT, '--target', FIXTURE_HOST,
  ]);
  assert.ok(
    unreconciledText.output.includes(statePath),
    'an absent state file must be reported by the path the agent would have written',
  );

  // The state the incident leaves behind: observed one version back, the desired
  // digest quarantined with the agent's own reason.
  const state = {
    ...settledState({ version: '0.2.26', digest: 'd'.repeat(64), releaseDir: join(installRoot, '0.2.26') }),
    phase: 'quarantined',
    detail: QUARANTINE_REASON,
    quarantined: { [DESIRED_DIGEST]: { reason: QUARANTINE_REASON, quarantined_at: quarantinedAt } },
  };
  await fixture.writeReleaseState(state);
  const stateBefore = await readFile(statePath, 'utf8');

  const listed = await fixture.invokeJson([
    'release', 'quarantine', 'list', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--json',
  ]);
  assert.equal(listed.status, 0);
  assert.equal(listed.json.product, FIXTURE_PRODUCT);
  assert.equal(listed.json.target, FIXTURE_HOST);
  assert.equal(listed.json.entries.length, 1);
  assert.equal(listed.json.entries[0].digest, DESIRED_DIGEST);
  assert.equal(listed.json.entries[0].reason, QUARANTINE_REASON);
  assert.equal(
    listed.json.entries[0].is_desired_digest,
    true,
    'the entry blocking the current rollout must be called out',
  );

  // Every refusal, and after each one the state file is byte-identical: a
  // refused clear that had already rewritten the rollout state would be worse
  // than no command at all.
  const refusals = [
    {
      name: 'no reason',
      args: ['release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--digest', DESIRED_DIGEST],
      expect: /--reason <REASON>/,
    },
    {
      name: 'no digest',
      args: ['release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--reason', 'because'],
      expect: /--digest <DIGEST>/,
    },
    {
      name: 'no target',
      args: ['release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--digest', DESIRED_DIGEST, '--reason', 'because'],
      expect: /--target <TARGET>/,
    },
    {
      name: 'blank reason',
      args: [
        'release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST,
        '--digest', DESIRED_DIGEST, '--reason', '   ',
      ],
      expect: /--reason must say why this digest is being retried/,
    },
    {
      name: 'a digest nobody quarantined',
      args: [
        'release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST,
        '--digest', 'f'.repeat(64), '--reason', 'aiming at a digest nobody quarantined',
      ],
      expect: /f{16}/,
    },
  ];
  const refusalStatuses = {};
  for (const refusal of refusals) {
    const attempt = await fixture.invoke(refusal.args);
    assert.notEqual(attempt.status, 0, `clear with ${refusal.name} must be refused`);
    assert.match(attempt.output, refusal.expect, `the "${refusal.name}" refusal does not say why`);
    assert.equal(
      await readFile(statePath, 'utf8'),
      stateBefore,
      `a clear refused for ${refusal.name} still rewrote the rollout state`,
    );
    refusalStatuses[refusal.name] = attempt.status;
  }
  assert.deepEqual(
    (await readdir(fixture.stateDir)).sort(),
    [`${FIXTURE_PRODUCT}.json`],
    'a refused clear left a backup or an audit file behind',
  );

  // The clear itself.
  const reason = 'stderr named a missing config key; fixed and republished in 0.2.28';
  const cleared = await fixture.invokeJson([
    'release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST,
    '--digest', DESIRED_DIGEST, '--reason', reason, '--json',
  ]);
  assert.equal(cleared.status, 0, `clearing failed: ${cleared.output}`);
  assert.equal(cleared.json.digest, DESIRED_DIGEST);
  assert.equal(cleared.json.cleared, true);
  assert.equal(cleared.json.reason, reason);
  assert.ok(cleared.json.audited_at, 'the clear reports no audit instant');
  assert.ok(cleared.json.state_backup, 'the clear reports no state backup');

  // The entry is gone and NOTHING else about the rollout changed: the phase, the
  // generation and the process the host is running are the agent's business.
  const after = JSON.parse(await readFile(statePath, 'utf8'));
  assert.deepEqual(after.quarantined, {});
  assert.equal(after.phase, state.phase);
  assert.equal(after.rollout_generation, state.rollout_generation);
  assert.deepEqual(after.active, state.active);

  // The previous bytes are recoverable without this tool.
  assert.equal(
    await readFile(cleared.json.state_backup, 'utf8'),
    stateBefore,
    'the backup is not the state that was replaced',
  );

  // One appended audit line, carrying the reason AND the entry the clear
  // destroyed: an audit trail that deletes its own evidence is decoration.
  const auditLines = (await readFile(auditPath, 'utf8')).trim().split('\n');
  assert.equal(auditLines.length, 1);
  const audit = JSON.parse(auditLines[0]);
  assert.equal(audit.host, FIXTURE_HOST);
  assert.equal(audit.product, FIXTURE_PRODUCT);
  assert.equal(audit.digest, DESIRED_DIGEST);
  assert.equal(audit.reason, reason);
  assert.equal(audit.quarantine_reason, QUARANTINE_REASON);
  assert.equal(Date.parse(audit.quarantined_at), Date.parse(quarantinedAt));
  assert.equal(audit.state_backup, cleared.json.state_backup);
  assert.ok(audit.actor, 'the audit line names no actor');
  assert.ok(audit.audited_at, 'the audit line carries no instant');
  assert.equal((await stat(auditPath)).mode & 0o777, 0o600, 'the audit trail is not owner-only');

  // The same clear a second time is refused: the entry is gone, and a command
  // that "succeeded" here would invent a retry nobody can account for.
  const again = await fixture.invoke([
    'release', 'quarantine', 'clear', FIXTURE_PRODUCT, '--target', FIXTURE_HOST,
    '--digest', DESIRED_DIGEST, '--reason', 'already cleared; this pass must refuse',
  ]);
  assert.notEqual(again.status, 0, 'clearing a digest that is no longer quarantined must be refused');

  // And the rollout is no longer blocked on that digest — the agent will roll it
  // out on its next tick, which is the only thing that starts anything.
  const doctor = await fixture.invokeJson([
    'release', 'doctor', FIXTURE_PRODUCT, '--target', FIXTURE_HOST, '--json',
  ]);
  assert.ok(
    !doctor.json.blockers.includes('desired_digest_quarantined'),
    'the desired digest is still reported as quarantined after a clear',
  );
  assert.deepEqual(doctor.json.quarantined, [], 'the quarantine map is not empty after a clear');

  await recordTrace({
    slug,
    journey: 'release-quarantine',
    source,
    observations: {
      refusalStatuses,
      listedEntry: listed.json.entries[0],
      clear: {
        cleared: cleared.json.cleared,
        stateBackup: cleared.json.state_backup,
        auditPath,
        auditActor: audit.actor,
      },
      doctorBlockersAfterClear: doctor.json.blockers,
    },
    contracts: [
      'clear is refused without --digest, without --target, without --reason and with a blank reason',
      'a refused clear leaves the rollout state byte-identical and writes no file',
      'clear removes exactly the named entry and changes no other rollout field',
      'the previous state is copied to a timestamped backup before the rewrite',
      'one owner-only audit line records actor, reason and the quarantine entry it destroyed',
      'clearing a digest that is not quarantined is refused',
      'after a clear the desired digest is no longer a doctor blocker',
    ],
  });
} finally {
  await fixture.close();
}
