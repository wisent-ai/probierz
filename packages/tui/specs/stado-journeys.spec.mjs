import assert from 'node:assert/strict';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const binary = process.env.TUI_CMD;
assert.ok(binary, 'TUI_CMD must identify the staged Stado binary');
const source = process.env.PROBIERZ_APP_SOURCE;
const crate = source ? resolve(source, 'stado-rs') : resolve(dirname(binary), '../..');

const productSpecs = new Map([
  ['host-dynamic-capacity', 'tests/host_dynamic_capacity/probierz.spec.mjs'],
  ['run-retention', 'tests/run_history/probierz.spec.mjs'],
  ['disk-cleanup', 'tests/cleanup/probierz.spec.mjs'],
  ['release-pipeline', 'tests/ci-cd/probierz.spec.mjs'],
  ['native-build', 'tests/builds/probierz.spec.mjs'],
  ['platform-matrix', 'tests/platform-matrix/probierz.spec.mjs'],
]);
const defaultJourneys = [
  'host-dynamic-capacity',
  'run-retention',
  'disk-cleanup',
  'release-pipeline',
];
const selectedJourney = process.env.PROBIERZ_JOURNEY;
const journeys = selectedJourney ? [selectedJourney] : defaultJourneys;

for (const journey of journeys) {
  const relativeSpec = productSpecs.get(journey);
  assert.ok(
    relativeSpec,
    `PROBIERZ_JOURNEY must be one of: ${[...productSpecs.keys()].join(', ')}`,
  );
  await import(pathToFileURL(resolve(crate, relativeSpec)).href);
}
