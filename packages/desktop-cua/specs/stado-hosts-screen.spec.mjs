import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { quitApp } from '../driver.mjs';
import * as consoleHarness from './stado-console.mjs';

const source = process.env.PROBIERZ_APP_SOURCE;
assert.ok(source, 'PROBIERZ_APP_SOURCE must identify the staged Stado source');
const productJourneys = new Map([
  ['host-dynamic-capacity', {
    path: 'desktop/StadoDesktop/tests/fleet/HostsDynamicCapacity.probierz.mjs',
    run: 'runHostsDynamicCapacityJourney',
  }],
  ['apple-challenge-desktop', {
    path: 'desktop/StadoDesktop/tests/apple_challenge/AppleChallengePreparation.probierz.mjs',
    run: 'runAppleChallengePreparationJourney',
  }],
]);
const journeys = (process.env.PROBIERZ_JOURNEYS || '').split(',').filter(Boolean);
assert.ok(journeys.length > 0, 'PROBIERZ_JOURNEYS must name the journeys selected by Probierz');
for (const journey of journeys) {
  const entry = productJourneys.get(journey);
  assert.ok(entry, `Unmapped Stado desktop journey selected by Probierz: ${journey}`);
  const product = await import(pathToFileURL(resolve(source, entry.path)).href);
  await product[entry.run]({ ...consoleHarness, quitApp });
}
