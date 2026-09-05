import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { quitApp } from '../driver.mjs';
import * as consoleHarness from './stado-console.mjs';

const source = process.env.PROBIERZ_APP_SOURCE;
assert.ok(source, 'PROBIERZ_APP_SOURCE must identify the staged Stado source');
const productSpecs = new Map([
  ['host-dynamic-capacity', {
    file: 'desktop/StadoDesktop/tests/fleet/HostsDynamicCapacity.probierz.mjs',
    run: 'runHostsDynamicCapacityJourney',
  }],
  ['service-convergence', {
    file: 'desktop/StadoDesktop/tests/service/ServicesConvergence.probierz.mjs',
    run: 'runServicesConvergenceJourney',
  }],
]);
const journeys = (process.env.PROBIERZ_JOURNEYS || '').split(',').filter(Boolean);
assert.ok(journeys.length > 0, 'PROBIERZ_JOURNEYS must name the journeys selected by Probierz');

for (const journey of journeys) {
  const product = productSpecs.get(journey);
  assert.ok(product, `Unmapped Stado desktop journey selected by Probierz: ${journey}`);
  const module = await import(pathToFileURL(resolve(source, product.file)).href);
  assert.equal(typeof module[product.run], 'function', `${product.file} must export ${product.run}`);
  await module[product.run]({ ...consoleHarness, quitApp });
}
