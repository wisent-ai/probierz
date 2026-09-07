import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { loadAppManifest } from '../../../agent/apps.mjs';

const source = process.env.PROBIERZ_APP_SOURCE || loadAppManifest('stado-docs').repositories[0].root;
const productSpecs = new Map([
  ['cli-scoped-generation', 'tests/docs/cli-scoped-generation.probierz.spec.mjs'],
  ['cli-command-pages', 'tests/docs/cli-commands.probierz.spec.mjs'],
]);
const journeys = (process.env.PROBIERZ_JOURNEYS || '').split(',').filter(Boolean);
assert.ok(journeys.length > 0, 'PROBIERZ_JOURNEYS must name the journeys selected by Probierz');

for (const journey of journeys) {
  const relativeSpec = productSpecs.get(journey);
  assert.ok(relativeSpec, `Unmapped Stado documentation journey selected by Probierz: ${journey}`);
  await import(pathToFileURL(resolve(source, relativeSpec)).href);
}
