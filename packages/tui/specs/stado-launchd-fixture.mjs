// launchd helpers for the `stado service …` journeys.
//
// These commands ask launchd itself which process is under a unit — the pid comes
// from `launchctl print <domain>/<label>` and nothing else — so a journey that
// wants to assert what the product reports about a live unit has to give it a
// live unit. Each job is created under a label namespaced to this fixture, its
// plist lives in the fixture HOME, and every helper is paired with a boot-out so
// no job outlives the journey.
//
// The program a unit runs must be a real executable image, not a script: the
// product reads `ps -o comm=`, and a script's process reports its interpreter.
// So the fixture compiles a six-line idle program instead of copying a system
// binary, which macOS refuses to execute once its signature no longer matches.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmod, copyFile, writeFile } from 'node:fs/promises';
import { userInfo } from 'node:os';
import { join } from 'node:path';

const IDLE_SOURCE = '#include <unistd.h>\nint main(void){for(;;){pause();}return 0;}\n';

/** Compile the idle program at `target`. Fails loudly when no compiler exists. */
export async function compileIdleProgram(dir, target) {
  const source = join(dir, 'idle.c');
  await writeFile(source, IDLE_SOURCE);
  const compiled = spawnSync('/usr/bin/cc', ['-O0', '-o', target, source], { encoding: 'utf8' });
  assert.equal(
    compiled.status,
    0,
    `cannot compile the fixture service program (needs the macOS command line tools): ${compiled.stderr}`,
  );
  await chmod(target, 0o755);
  return target;
}

/** A second, distinct executable beside the first — the artefact a relink points at. */
export async function copyProgram(from, to) {
  await copyFile(from, to);
  await chmod(to, 0o755);
  return to;
}

/** A launchd agent plist naming exactly `programArgs`. */
export async function writeAgentPlist(path, label, programArgs) {
  const args = programArgs
    .map((argument) => `        <string>${argument}</string>`)
    .join('\n');
  await writeFile(
    path,
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${label}</string>
    <key>ProgramArguments</key>
    <array>
${args}
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
`,
    { mode: 0o644 },
  );
  return path;
}

const domain = () => `gui/${userInfo().uid}`;

/** Load one agent and wait for launchd to report a pid for it. */
export function bootstrapAgent(plistPath, label, { timeoutMs = 15_000 } = {}) {
  const loaded = spawnSync('/bin/launchctl', ['bootstrap', domain(), plistPath], { encoding: 'utf8' });
  assert.equal(loaded.status, 0, `launchd refused the fixture job: ${loaded.stderr || loaded.stdout}`);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const pid = launchdPid(label);
    if (pid) return pid;
    spawnSync('/bin/sleep', ['0.3']);
  }
  throw new Error(`launchd started no process for ${label}`);
}

/** The pid launchd reports under `label`, or null. */
export function launchdPid(label) {
  const printed = spawnSync('/bin/launchctl', ['print', `${domain()}/${label}`], { encoding: 'utf8' });
  if (printed.status !== 0) return null;
  const line = String(printed.stdout || '')
    .split('\n')
    .map((row) => row.trim())
    .find((row) => row.startsWith('pid = '));
  const pid = line ? Number(line.slice('pid = '.length).trim()) : 0;
  return pid > 0 ? pid : null;
}

/** Remove the fixture job. Safe to call when it was never loaded. */
export function bootoutAgent(label) {
  spawnSync('/bin/launchctl', ['bootout', `${domain()}/${label}`], { encoding: 'utf8' });
}

/**
 * Start a process no unit owns.
 *
 * Orphaned deliberately: the product asks launchd for ownership and walks the
 * parent chain, and a child of this journey's own shell would inherit ownership
 * from whatever launchd job started the harness.
 */
export function spawnOrphan(command) {
  const spawned = spawnSync('/bin/sh', ['-c', `nohup ${command} >/dev/null 2>&1 & echo $!`], {
    encoding: 'utf8',
  });
  const pid = Number(String(spawned.stdout || '').trim());
  assert.ok(pid > 0, `could not start the unowned fixture process: ${spawned.stderr}`);
  return pid;
}

/** Is this pid still there? */
export function alive(pid) {
  return spawnSync('/bin/ps', ['-p', String(pid), '-o', 'pid='], { encoding: 'utf8' }).status === 0;
}

/** Stop a pid this journey started. */
export function stop(pid) {
  if (pid) spawnSync('/bin/kill', [String(pid)]);
}
