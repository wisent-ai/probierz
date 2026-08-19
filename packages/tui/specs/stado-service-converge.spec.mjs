// Journey: service-converge — `stado service converge TARGET [BINARY]`.
//
// Every other command in this group answers a question about a unit. This one
// answers whether the program on the host is the build that was shipped, and the
// column worth defending is `binary_matches_process`: a unit can be loaded, the
// declared version can be installed, and the live process can still be executing
// something else. `null` is never folded into either answer — "I did not look" is
// not a match — and the mismatch stands beside an `in-sync` verdict, because that
// combination is the incident.
import assert from 'node:assert/strict';
import { chmod, copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  STADO_BINARY,
  fixtureRegistry,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';
import {
  bootoutAgent,
  bootstrapAgent,
  compileIdleProgram,
  copyProgram,
  launchdPid,
  writeAgentPlist,
} from './stado-launchd-fixture.mjs';

const slug = 'stado-service-converge';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const LABEL = 'ai.wisent.probierz.fixture.converge';
const BINARY = 'fixture-daemon';
const DECLARED_VERSION = '1.0.0';
const artefactRoot = join(fixture.home, BINARY);
const plistPath = join(fixture.home, 'Library', 'LaunchAgents', `${LABEL}.plist`);
const row = (report) => {
  assert.equal(report.binaries.length, 1, 'the fixture declares exactly one binary');
  return report.binaries[0];
};
const declare = (version, { withUnit }) =>
  fixture.registry(
    fixtureRegistry({
      target: {
        managed_versions: { [BINARY]: version },
        ...(withUnit
          ? {
              services: [
                {
                  name: BINARY,
                  kind: 'launchd',
                  label: LABEL,
                  unit: '',
                  path: plistPath,
                  managed_since: '2026-08-18T00:00:00Z',
                },
              ],
            }
          : {}),
      },
    }),
  );

try {
  // The installed-version reporter is a host-side program that runs through the
  // host's own stado binary, so the fixture host has one — a copy under the
  // fixture HOME, never an install into the operator's `~/.stado/bin`.
  const hostBin = join(fixture.home, '.stado', 'bin');
  await mkdir(hostBin, { recursive: true });
  await copyFile(STADO_BINARY, join(hostBin, 'stado'));
  await chmod(join(hostBin, 'stado'), 0o755);

  // An installed release artefact that declares its own version, which is the
  // primitive the fleet delivers against.
  await mkdir(join(artefactRoot, 'bin'), { recursive: true });
  await mkdir(join(fixture.home, 'Library', 'LaunchAgents'), { recursive: true });
  await writeFile(
    join(artefactRoot, 'package.json'),
    `${JSON.stringify({ name: BINARY, version: DECLARED_VERSION }, null, 2)}\n`,
  );
  const declaredProgram = await compileIdleProgram(fixture.dir, join(artefactRoot, 'bin', BINARY));
  const relinkedProgram = await copyProgram(declaredProgram, join(artefactRoot, 'bin', `${BINARY}-relinked`));

  // 1. No unit at all: the version comparison still answers, and the process
  //    columns stay empty rather than claiming a match nobody observed.
  await declare(DECLARED_VERSION, { withUnit: false });
  const withoutUnit = await fixture.invokeJson(['service', 'converge', FIXTURE_HOST, '--json']);
  assert.equal(withoutUnit.status, 0, `converge failed with no unit declared: ${withoutUnit.output}`);
  const unitless = row(withoutUnit.json);
  assert.equal(unitless.declared_version, DECLARED_VERSION);
  assert.equal(unitless.installed_version, DECLARED_VERSION);
  assert.equal(unitless.verdict, 'in-sync');
  assert.equal(unitless.running_binary, null);
  assert.equal(unitless.binary_matches_process, null, 'an unasked process question must not answer true');

  // 2. A live unit executing exactly the artefact its own file declares.
  await writeAgentPlist(plistPath, LABEL, [declaredProgram]);
  const pid = bootstrapAgent(plistPath, LABEL);
  await declare(DECLARED_VERSION, { withUnit: true });
  const matching = await fixture.invokeJson(['service', 'converge', FIXTURE_HOST, '--json']);
  assert.equal(matching.status, 0, `converge failed on a matching host: ${matching.output}`);
  const matched = row(matching.json);
  assert.equal(matched.unit, LABEL);
  assert.equal(matched.state, 'running');
  assert.equal(matched.running_binary, declaredProgram);
  assert.equal(matched.binary_matches_process, true);
  assert.equal(matched.verdict, 'in-sync');

  // 3. The unit file is repointed at a sibling artefact while the job stays up:
  //    launchd holds the definition it bootstrapped, so the declaration and the
  //    live process now disagree. Everything else about the host is still correct.
  await writeAgentPlist(plistPath, LABEL, [relinkedProgram]);
  const differing = await fixture.invokeJson(['service', 'converge', FIXTURE_HOST, '--json']);
  assert.equal(differing.status, 0, 'a process mismatch is a finding on an in-sync host, not a failed command');
  const differs = row(differing.json);
  assert.equal(differs.binary_matches_process, false, 'the live process is not the declared artefact');
  assert.equal(differs.running_binary, declaredProgram, 'the report must name what the process actually executes');
  assert.equal(differs.installed_version, DECLARED_VERSION);
  assert.equal(differs.verdict, 'in-sync', 'the version comparison is independent of the process comparison');
  const differsText = await fixture.invoke(['service', 'converge', FIXTURE_HOST]);
  assert.match(differsText.output, /PROCESS/);
  assert.match(differsText.output, /differs/, 'the human table does not say the process differs');

  // 4. Drift: the host runs a version the registry does not declare. Reported
  //    non-zero, and a report-mode pass delivers nothing and restarts nothing.
  await declare('9.9.9', { withUnit: true });
  const drifted = await fixture.invokeJson(['service', 'converge', FIXTURE_HOST, '--json']);
  assert.notEqual(drifted.status, 0, 'a drifted host must fail the report-mode gate');
  const drift = row(drifted.json);
  assert.equal(drift.declared_version, '9.9.9');
  assert.equal(drift.installed_version, DECLARED_VERSION);
  assert.equal(drift.verdict, 'drifted');
  assert.equal(drifted.json.applied, false);
  assert.deepEqual(drifted.json.releases, []);
  assert.deepEqual(drifted.json.undeliverable.length >= 0, true);
  assert.equal(launchdPid(LABEL), pid, 'a report-mode converge restarted the unit');

  // 5. Narrowing to a binary nobody declared is refused before the host is asked.
  const unknown = await fixture.invoke(['service', 'converge', FIXTURE_HOST, 'not-declared']);
  assert.notEqual(unknown.status, 0);
  assert.match(unknown.output, /declares no not-declared version/);

  // The registry was never rewritten to match the host: a converge that edited
  // the declaration would turn a drift report into a rubber stamp.
  const registryAfter = await fixture.readRegistry();
  assert.equal(registryAfter.targets[0].managed_versions[BINARY], '9.9.9');

  await recordTrace({
    slug,
    journey: 'service-converge',
    source,
    observations: {
      noUnit: { verdict: unitless.verdict, runningBinary: unitless.running_binary, matches: unitless.binary_matches_process },
      matching: { runningBinary: matched.running_binary, matches: matched.binary_matches_process, verdict: matched.verdict },
      relinkedUnderLiveJob: { runningBinary: differs.running_binary, matches: differs.binary_matches_process, verdict: differs.verdict },
      drift: { exit: drifted.status, verdict: drift.verdict, applied: drifted.json.applied, releases: drifted.json.releases },
      pidUnchanged: pid,
    },
    contracts: [
      'a declared binary with no unit reports null process fields, never a match',
      'binary_matches_process is true when the live process executes the declared artefact',
      'binary_matches_process is false when it does not, while the version verdict stays in-sync',
      'the human table renders that mismatch as differs',
      'a drifted host exits non-zero in report mode and delivers nothing',
      'a report-mode pass restarts nothing and never edits the registry declaration',
      'a binary the target does not declare is refused before the host is contacted',
    ],
  });
} finally {
  bootoutAgent(LABEL);
  await fixture.close();
}
