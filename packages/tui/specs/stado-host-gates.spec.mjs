import assert from 'node:assert/strict';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const binary = process.env.TUI_CMD;
assert.ok(binary, 'TUI_CMD must identify the staged Stado binary');
const productSpec = resolve(
  dirname(binary),
  '../..',
  'tests/host_dynamic_capacity/probierz.spec.mjs',
);
await import(pathToFileURL(productSpec).href);
