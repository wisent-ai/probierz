import assert from 'node:assert/strict';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const binary = process.env.TUI_CMD;
assert.ok(binary, 'TUI_CMD must identify the staged Stado binary');
const source = process.env.PROBIERZ_APP_SOURCE;
const crate = source ? resolve(source, 'stado-rs') : resolve(dirname(binary), '../..');
const productSpec = resolve(crate, 'tests/host_dynamic_capacity/probierz.spec.mjs');
await import(pathToFileURL(productSpec).href);
