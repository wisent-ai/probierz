// Journey: crate-builds.
//
// The cheapest thing that can be true about a change to skrzynka, and until
// this manifest existed nothing checked it: the crate compiles, from the
// dependency versions it has committed, including its test targets.
//
// `--locked` is the point of the first half. A Cargo.lock that disagrees with
// Cargo.toml is the same defect that stopped every job in wisent-ai/weles
// (`npm ci` refusing `Missing: tap@18.8.0 from lock file`), and `cargo build`
// without `--locked` hides it by silently updating the lock. Here a drifted
// lock fails instead.
//
// `--all-targets` is the point of the second half: it compiles the `mailboxes`
// integration target and the unit tests inside the binary as well as the
// binary itself, so a test file that no longer builds is a failure here rather
// than a surprise inside the journeys that run them.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const repository =
  process.env.SKRZYNKA_REPO ||
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skrzynka';

assert.ok(
  existsSync(join(repository, 'Cargo.toml')),
  `no skrzynka checkout at ${repository}; set SKRZYNKA_REPO`,
);
assert.ok(
  existsSync(join(repository, 'Cargo.lock')),
  `${repository} has no Cargo.lock, so --locked would have nothing to hold the build to`,
);

const build = spawnSync('cargo', ['build', '--locked', '--all-targets'], {
  cwd: repository,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
});

assert.equal(
  build.status,
  0,
  `cargo build --locked --all-targets failed with ${build.status}${build.signal ? ` (${build.signal})` : ''}\n${build.stderr || build.stdout}`,
);

// The binary the product actually ships, not just "the compiler exited zero".
const binary = join(repository, 'target', 'debug', 'skrzynka');
assert.ok(existsSync(binary), `the build reported success but produced no ${binary}`);

const version = spawnSync(binary, ['--version'], { encoding: 'utf8' });
assert.equal(
  version.status,
  0,
  `${binary} could not report its version: ${version.stderr || version.stdout}`,
);
assert.match(
  version.stdout,
  /^skrzynka \d+\.\d+\.\d+/,
  `unexpected --version output: ${JSON.stringify(version.stdout)}`,
);
