// Journey: oauth-failure-diagnosis.
//
// skrzynka's loopback Gmail authorization had never once completed on this
// fleet. For the full ten-minute flow lifetime it reported only
// `GMAIL_OAUTH_FLOW_EXPIRED`, while the actual cause -- Google refusing the
// authorization with `redirect_uri_mismatch`, because the shared OAuth client
// has no loopback redirect URI registered -- was reachable only by decoding an
// `authError` payload out of a browser's landing URL by hand.
//
// The unit tests inside the binary are what hold that diagnosis in place. They
// are pinned to a REAL landing URL captured from Google, not a stubbed server,
// so they fail if the decoding, the error code, the operands, or the
// operator-facing sentence stops matching what Google actually sends.
//
// skrzynka has no `[lib]` target -- the tests live in the binary -- so this is
// `--bins`, not `--lib`. Asserting the four names individually rather than only
// the summary line is deliberate: `test result: ok. 0 passed` is also "ok", and
// a test silently renamed out of the suite would otherwise pass unnoticed.
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

const REQUIRED = [
  'gmail::tests::the_captured_google_error_decodes_to_its_code',
  'gmail::tests::a_landing_url_without_an_error_carries_no_code',
  'gmail::tests::the_operands_come_from_the_url_that_was_handed_out',
  'gmail::tests::the_refusal_names_the_client_the_uri_and_the_setting',
];

const run = spawnSync('cargo', ['test', '--locked', '--bins', '--', '--nocapture'], {
  cwd: repository,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
});
const output = `${run.stdout || ''}${run.stderr || ''}`;

assert.equal(
  run.status,
  0,
  `cargo test --bins failed with ${run.status}${run.signal ? ` (${run.signal})` : ''}\n${output}`,
);

for (const name of REQUIRED) {
  assert.match(
    output,
    new RegExp(`test ${name.replaceAll(':', '\\:')} \\.\\.\\. ok`),
    `${name} did not run and pass; a renamed or removed test is not a passing one\n${output}`,
  );
}

assert.match(
  output,
  new RegExp(`test result: ok\\. ${REQUIRED.length} passed; 0 failed;`),
  `expected exactly ${REQUIRED.length} passing unit tests and no failures\n${output}`,
);
