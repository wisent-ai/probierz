// Journey: host-reclaim — `stado host reclaim HOST [--dry-run|--apply --reason]`.
//
// The write half of the disk incident. Everything worth asserting here is a
// refusal or a guard, because the command deletes files: `--apply` without a
// reason is refused before the host is touched, a preview deletes nothing, and
// the three declared stages never take the newest delivered tree, what `current`
// resolves to, anything younger than the age gate, or a path a live process holds.
//
// The journey runs the preview only, against a fixture HOME this run created. It
// never applies, and it never enumerates the operator's own `~/.stado`.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { chmod, mkdir, utimes, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  FIXTURE_HOST,
  FIXTURE_PRODUCT,
  fixtureRegistry,
  openFleetFixture,
  recordTrace,
  sourceIdentity,
} from './stado-fleet-fixture.mjs';

const slug = 'stado-host-reclaim';
const source = sourceIdentity();
const fixture = await openFleetFixture(slug);
const STALE_SECONDS = 3 * 86_400;
const staleTime = new Date(Date.now() - STALE_SECONDS * 1000);

/** A delivered version tree with one file in it, aged if asked. */
async function deliveredTree(product, version, { stale = false } = {}) {
  const tree = join(fixture.servicesRoot, product, version);
  await mkdir(join(tree, 'bin'), { recursive: true });
  const file = join(tree, 'bin', 'start');
  await writeFile(file, `#!/bin/sh\n# ${product} ${version}\nwhile :; do /bin/sleep 5; done\n`);
  await chmod(file, 0o755);
  if (stale) {
    for (const path of [file, join(tree, 'bin'), tree]) await utimes(path, staleTime, staleTime);
  }
  return { tree, file };
}

let heldProcessPid = null;
try {
  await fixture.registry(fixtureRegistry());

  // The janitor stage is the host's OWN cleanup pass, invoked through the stado
  // binary the host has. The fixture gives it one, so all three declared stages
  // run rather than one being reported unavailable.
  const hostBin = join(fixture.home, '.stado', 'bin');
  await mkdir(hostBin, { recursive: true });
  const installed = spawnSync('/bin/cp', [
    process.env.TUI_CMD
      || '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute/stado-rs/target/release/stado',
    join(hostBin, 'stado'),
  ]);
  assert.equal(installed.status, 0, 'could not place a stado binary in the fixture home');

  // The release build scratch tree a from-scratch build leaves behind.
  const scratch = join(fixture.home, '.stado', 'build-work', 'stado');
  await mkdir(scratch, { recursive: true });
  const scratchFile = join(scratch, 'vendor.tar');
  await writeFile(scratchFile, 'x'.repeat(4096));
  for (const path of [scratchFile, scratch]) await utimes(path, staleTime, staleTime);

  // Four delivered trees: one stale and reclaimable, one stale but held by a live
  // process, the newest one, and what `current` resolves to.
  const reclaimable = await deliveredTree(FIXTURE_PRODUCT, '0.2.20', { stale: true });
  const held = await deliveredTree(FIXTURE_PRODUCT, '0.2.21', { stale: true });
  const linked = await deliveredTree(FIXTURE_PRODUCT, '0.2.24', { stale: true });
  const newest = await deliveredTree(FIXTURE_PRODUCT, '0.2.26');
  const current = join(fixture.servicesRoot, FIXTURE_PRODUCT, 'current');
  spawnSync('/bin/ln', ['-s', linked.tree, current]);
  assert.ok(existsSync(current), 'the fixture has no current link');

  // A live process executing out of the held tree, orphaned to launchd so its
  // parent chain cannot make it look owned by this journey's own shell.
  const spawned = spawnSync('/bin/sh', [
    '-c',
    `nohup /bin/sh ${JSON.stringify(held.file)} >/dev/null 2>&1 & echo $!`,
  ], { encoding: 'utf8' });
  heldProcessPid = Number(String(spawned.stdout || '').trim());
  assert.ok(heldProcessPid > 0, 'could not start the process that holds a delivered tree');

  // 1. `--apply` without a reason is refused, before anything is measured: the
  //    audit record on the host is the only account of why the space went.
  const refused = await fixture.invoke(['host', 'reclaim', FIXTURE_HOST, '--apply']);
  assert.notEqual(refused.status, 0, '--apply without --reason must be refused');
  assert.match(refused.output, /--apply removes files and needs --reason/);
  assert.match(refused.output, /appended to the host's own audit log/);
  assert.ok(!/STAGE/.test(refused.output), 'a refused apply must not have measured the stages');

  // 2. The preview. Every declared stage is reported and nothing is deleted.
  const preview = await fixture.invokeJson(['host', 'reclaim', FIXTURE_HOST, '--json']);
  assert.equal(preview.status, 0, `the preview failed: ${preview.output}`);
  assert.equal(preview.json.host, FIXTURE_HOST);
  assert.equal(preview.json.mode, 'dry_run');
  assert.deepEqual(
    preview.json.stages.map((stage) => stage.stage),
    ['registry_cleanup', 'build_scratch', 'delivered_trees'],
    'the three declared stages are not all reported',
  );
  assert.ok(typeof preview.json.free_gb_before === 'number');
  assert.ok(typeof preview.json.free_gb_after === 'number');

  const rendered = await fixture.invoke(['host', 'reclaim', FIXTURE_HOST]);
  assert.equal(rendered.status, 0, `the human preview failed: ${rendered.output}`);
  // Said BEFORE the table: an operator reading a list of paths has to know which
  // of the two things they are looking at.
  const banner = rendered.output.indexOf('DRY RUN');
  assert.ok(banner !== -1, 'the preview does not say it is a preview');
  assert.ok(banner < rendered.output.indexOf('STAGE'), 'the preview banner comes after the table');
  assert.match(rendered.output, /nothing on .* is deleted/);
  assert.match(rendered.output, /--apply --reason/);
  assert.ok(!/APPLIED/.test(rendered.output), 'a preview must not report an apply');

  // 3. The guards, read off the paths the preview named.
  assert.ok(rendered.output.includes(scratch), 'the stale build scratch tree was not named');
  assert.ok(rendered.output.includes(reclaimable.tree), 'the stale delivered tree was not named');
  assert.ok(!rendered.output.includes(newest.tree), 'the newest delivered tree must never be named');
  assert.ok(!rendered.output.includes(linked.tree), 'the tree `current` resolves to must never be named');
  assert.ok(!rendered.output.includes(held.tree), 'a tree a live process holds must never be named');

  // 4. And nothing moved: every path the preview walked still exists, including
  //    the one it named as reclaimable.
  for (const path of [scratch, scratchFile, reclaimable.tree, reclaimable.file, held.tree, linked.tree, newest.tree, current]) {
    assert.ok(existsSync(path), `the preview deleted ${path}`);
  }
  const alive = spawnSync('/bin/ps', ['-p', String(heldProcessPid), '-o', 'pid='], { encoding: 'utf8' });
  assert.equal(alive.status, 0, 'the preview killed the process holding a delivered tree');

  await recordTrace({
    slug,
    journey: 'host-reclaim',
    source,
    observations: {
      applyWithoutReason: { exit: refused.status },
      preview: {
        mode: preview.json.mode,
        stages: preview.json.stages,
        namedBuildScratch: scratch,
        namedDeliveredTree: reclaimable.tree,
      },
      guards: {
        newestTreeKept: newest.tree,
        currentTargetKept: linked.tree,
        heldTreeKept: held.tree,
        heldByPidStillRunning: heldProcessPid,
      },
    },
    contracts: [
      '--apply without --reason is refused before any stage is measured',
      'the preview is the default and reports all three declared stages',
      'the preview says it is a preview before it prints what it would remove',
      'a stale build scratch tree and a stale delivered tree are named',
      'the newest delivered tree, the current link target and a tree a live process holds are never named',
      'the preview deletes nothing and signals nothing',
    ],
  });
} finally {
  if (heldProcessPid) spawnSync('/bin/kill', [String(heldProcessPid)]);
  await fixture.close();
}
