import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { loadAppManifest } from "./apps.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const SKIP_DIRECTORIES = new Set([".build", ".git", ".swiftpm", "dist", "node_modules", "test-results"]);

function filesBelow(root, extension) {
  const files = [];
  const pending = [root];
  while (pending.length) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!SKIP_DIRECTORIES.has(entry.name)) pending.push(path.join(directory, entry.name));
      } else if (entry.isFile() && entry.name.endsWith(extension)) {
        files.push(path.join(directory, entry.name));
      }
    }
  }
  return files.sort();
}

function lineNumber(content, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (content.charCodeAt(index) === 10) line += 1;
  }
  return line;
}

function matches(content, expression, file) {
  return [...content.matchAll(expression)].map((match) => ({
    value: match[1],
    file,
    line: lineNumber(content, match.index || 0),
  }));
}

function sourceInventory(manifest) {
  const modifiers = [];
  const dynamicPrefixes = new Set();
  let scannedFiles = 0;
  for (const repository of manifest.repositories) {
    for (const file of filesBelow(repository.root, ".swift")) {
      scannedFiles += 1;
      const content = readFileSync(file, "utf8");
      const explicit = matches(content, /\.accessibilityIdentifier\s*\(\s*["']([^"']+)["']\s*\)/g, file)
        .map((item) => ({ ...item, repository: repository.root }));
      modifiers.push(...explicit);
      if (content.includes(".accessibilityIdentifier")) {
        for (const match of content.matchAll(/["']([a-z][a-z0-9-]*(?:\.[a-zA-Z0-9_-]+)*\.)\\\(/g)) {
          dynamicPrefixes.add(match[1]);
        }
      }
    }
  }
  return { modifiers, dynamicPrefixes, scannedFiles };
}

function specFiles(manifest) {
  const basenames = new Set(Object.values(manifest.surfaces).map((surface) => path.basename(surface.spec)));
  return filesBelow(path.join(ROOT, "packages"), ".ts").filter((file) => basenames.has(path.basename(file)));
}

function specInventory(manifest) {
  const references = [];
  const forbidden = [];
  const prefix = `${manifest.appId}.`;
  const files = specFiles(manifest);
  for (const file of files) {
    const content = readFileSync(file, "utf8");
    for (const match of matches(content, /["']([a-z][a-z0-9-]*(?:\.[a-zA-Z0-9_-]+)+)["']/g, file)) {
      if (match.value.startsWith(prefix)) references.push(match);
    }
    for (const match of content.matchAll(/\$\(\s*(["'])([^"']+)\1\s*\)/g)) {
      const selector = match[2];
      const stable = selector.startsWith("~")
        || (/identifier/i.test(selector) && !/(?:label|name|text)\s*(?:=|contains)/i.test(selector))
        || selector.startsWith("[data-testid=");
      if (!stable) {
        forbidden.push({
          kind: "text-selector",
          selector,
          file,
          line: lineNumber(content, match.index || 0),
        });
      }
    }
  }
  return { references, forbidden, files };
}

function duplicateIdentifiers(modifiers) {
  const locations = new Map();
  for (const item of modifiers) {
    const key = `${item.repository}\0${item.value}`;
    const entry = locations.get(key) || { identifier: item.value, locations: [] };
    entry.locations.push({ file: item.file, line: item.line });
    locations.set(key, entry);
  }
  return [...locations.values()]
    .filter((entry) => new Set(entry.locations.map((location) => location.file)).size > 1)
    .map((entry) => ({ kind: "duplicate-identifier", ...entry }));
}

export function validateAccessibility(appId) {
  const manifest = loadAppManifest(appId);
  const source = sourceInventory(manifest);
  const specs = specInventory(manifest);
  const defined = new Set(source.modifiers.map((item) => item.value).filter((value) => !value.includes("\\(")));
  const missing = specs.references
    .filter((item, index, all) => (
      !defined.has(item.value)
      && ![...source.dynamicPrefixes].some((prefix) => item.value.startsWith(prefix))
      && all.findIndex((other) => other.value === item.value) === index
    ))
    .map((item) => ({
      kind: "missing-identifier",
      identifier: item.value,
      referencedAt: { file: item.file, line: item.line },
    }));
  const errors = [
    ...duplicateIdentifiers(source.modifiers),
    ...missing,
    ...specs.forbidden,
  ];
  const referenced = new Set(specs.references.map((item) => item.value));
  const unused = [...new Set(source.modifiers.map((item) => item.value))]
    .filter((identifier) => identifier.startsWith(`${appId}.`) && !referenced.has(identifier))
    .sort();
  return {
    schemaVersion: 1,
    appId,
    ok: errors.length === 0,
    sourceFiles: source.scannedFiles,
    specFiles: specs.files,
    identifiers: {
      defined: defined.size,
      explicit: source.modifiers.length,
      referenced: referenced.size,
      unused,
    },
    errors,
  };
}
