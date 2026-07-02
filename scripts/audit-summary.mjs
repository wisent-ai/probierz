#!/usr/bin/env node
import { readFileSync } from 'node:fs';

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function latestByName(reports) {
  const byName = new Map();
  for (const report of reports) {
    for (const result of report.results || []) {
      byName.set(result.name, result);
    }
  }
  return [...byName.values()];
}

const [webPath, ...iosPaths] = process.argv.slice(2);
if (!webPath || iosPaths.length === 0) {
  console.error('Usage: audit-summary.mjs <web-report.json> <ios-report.json>...');
  process.exit(2);
}

const web = readJson(webPath);
const ios = latestByName(iosPaths.map(readJson));
const results = [...(web.results || []), ...ios];
const products = [...new Set(results.map((r) => r.product))].sort();
const statusCounts = results.reduce((acc, result) => {
  const status = String(result.status || 'UNKNOWN');
  const bucket = status.split('_')[0];
  acc[bucket] = (acc[bucket] || 0) + 1;
  return acc;
}, {});

console.log(JSON.stringify({
  productFamilies: products.length,
  targets: results.length,
  pass: statusCounts.PASS || 0,
  fail: statusCounts.FAIL || 0,
  skipped: statusCounts.SKIPPED || 0,
  products,
}, null, 2));
