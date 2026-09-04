import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { quitApp } from '../driver.mjs';
import * as consoleHarness from './stado-console.mjs';

const source = process.env.PROBIERZ_APP_SOURCE;
assert.ok(source, 'PROBIERZ_APP_SOURCE must identify the staged Stado source');
const productSpec = resolve(
  source,
  'desktop/StadoDesktop/tests/fleet/HostsDynamicCapacity.probierz.mjs',
);
const { runHostsDynamicCapacityJourney } = await import(pathToFileURL(productSpec).href);
await runHostsDynamicCapacityJourney({ ...consoleHarness, quitApp });
