import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { loadAppManifest } from "./apps.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const MODES = new Set(["pull-request", "release", "nightly", "production"]);
const DIMENSION_SOURCES = new Set(["model", "deterministic", "hybrid"]);

function requireValue(condition, message) {
  if (!condition) throw new Error(`invalid SEO contract: ${message}`);
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`cannot read ${label} ${file}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

function resolveFile(value, fallback, label) {
  const selected = String(value || fallback || "").trim();
  requireValue(selected, `${label} path is required`);
  return path.resolve(ROOT, selected);
}

function secureUrl(value, label) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(`invalid SEO contract: ${label} must be an absolute URL`);
  }
  const loopback = parsed.hostname === "localhost" || parsed.hostname === "::1" || parsed.hostname.startsWith("127.");
  requireValue(parsed.protocol === "https:" || (parsed.protocol === "http:" && loopback), `${label} must use HTTPS or loopback HTTP`);
  requireValue(!parsed.username && !parsed.password, `${label} must not contain credentials`);
  parsed.hash = "";
  return parsed;
}

function validatePolicy(raw, file) {
  requireValue(raw && typeof raw === "object" && !Array.isArray(raw), `${file} must be an object`);
  requireValue(raw.schemaVersion === 1, `${file} schemaVersion must be 1`);
  requireValue(typeof raw.name === "string" && raw.name.trim(), `${file} name is required`);
  requireValue(Number.isFinite(raw.qualityMinimum) && raw.qualityMinimum >= 0 && raw.qualityMinimum <= 1, `${file} qualityMinimum must be between 0 and 1`);
  requireValue(raw.dimensions && typeof raw.dimensions === "object" && !Array.isArray(raw.dimensions), `${file} dimensions are required`);
  let weight = 0;
  for (const [name, rule] of Object.entries(raw.dimensions)) {
    requireValue(/^[a-z][a-z0-9_]*$/.test(name), `${file} dimension ${name} has an invalid name`);
    requireValue(rule && typeof rule === "object", `${file} dimension ${name} must be an object`);
    requireValue(Number.isFinite(rule.weight) && rule.weight > 0, `${file} dimension ${name}.weight must be positive`);
    requireValue(Number.isFinite(rule.minimum) && rule.minimum >= 0 && rule.minimum <= 1, `${file} dimension ${name}.minimum must be between 0 and 1`);
    requireValue(DIMENSION_SOURCES.has(rule.source), `${file} dimension ${name}.source must be model, deterministic, or hybrid`);
    requireValue(typeof rule.criterion === "string" && rule.criterion.trim(), `${file} dimension ${name}.criterion is required`);
    weight += rule.weight;
  }
  requireValue(Math.abs(weight - 1) <= 0.000001, `${file} dimension weights must total 1, got ${weight}`);
  requireValue(Array.isArray(raw.routes) && raw.routes.length > 0, `${file} routes are required`);
  const paths = new Set();
  for (const [index, route] of raw.routes.entries()) {
    requireValue(route && typeof route === "object", `${file} routes.${index} must be an object`);
    requireValue(typeof route.path === "string" && route.path.startsWith("/"), `${file} routes.${index}.path must begin with /`);
    requireValue(!paths.has(route.path), `${file} route ${route.path} is duplicated`);
    paths.add(route.path);
    requireValue(typeof route.indexable === "boolean", `${file} route ${route.path}.indexable must be boolean`);
    requireValue(typeof route.locale === "string" && route.locale.trim(), `${file} route ${route.path}.locale is required`);
    requireValue(Array.isArray(route.intents) && route.intents.length > 0 && route.intents.every((item) => typeof item === "string" && item.trim()), `${file} route ${route.path}.intents are required`);
    requireValue(Array.isArray(route.requiredStructuredData), `${file} route ${route.path}.requiredStructuredData must be an array`);
    requireValue(route.maxClickDepth === undefined || (Number.isInteger(route.maxClickDepth) && route.maxClickDepth >= 0), `${file} route ${route.path}.maxClickDepth must be a non-negative integer`);
  }
  requireValue(raw.crawl && typeof raw.crawl === "object", `${file} crawl policy is required`);
  for (const key of ["maxPages", "maxDepth", "maxRedirects", "maxSitemaps"]) {
    requireValue(Number.isInteger(raw.crawl[key]) && raw.crawl[key] > 0, `${file} crawl.${key} must be a positive integer`);
  }
  requireValue(Array.isArray(raw.modelInstructions) && raw.modelInstructions.length > 0 && raw.modelInstructions.every((line) => typeof line === "string" && line.trim()), `${file} modelInstructions are required`);
  requireValue(raw.model && typeof raw.model === "object", `${file} model policy is required`);
  requireValue(Number.isFinite(raw.model.adjudicationDelta) && raw.model.adjudicationDelta > 0 && raw.model.adjudicationDelta <= 1, `${file} model.adjudicationDelta must be between 0 and 1`);
  requireValue(Number.isInteger(raw.model.maxOutputTokens) && raw.model.maxOutputTokens >= 800 && raw.model.maxOutputTokens <= 8000, `${file} model.maxOutputTokens must be an integer between 800 and 8000`);
  requireValue(Number.isInteger(raw.model.maxEvidenceCharacters) && raw.model.maxEvidenceCharacters >= 5000 && raw.model.maxEvidenceCharacters <= 500000, `${file} model.maxEvidenceCharacters must be an integer between 5000 and 500000`);
  requireValue(Number.isInteger(raw.model.maxScreenshots) && raw.model.maxScreenshots >= 0 && raw.model.maxScreenshots <= 20, `${file} model.maxScreenshots must be an integer between 0 and 20`);
  requireValue(raw.performance && typeof raw.performance === "object", `${file} performance policy is required`);
  for (const key of ["lcpMs", "inpMs", "cls", "tbtMs"]) requireValue(Number.isFinite(raw.performance[key]) && raw.performance[key] >= 0, `${file} performance.${key} must be non-negative`);
  requireValue(raw.production && typeof raw.production === "object", `${file} production policy is required`);
  requireValue(Number.isFinite(raw.production.maxAgeHours) && raw.production.maxAgeHours > 0, `${file} production.maxAgeHours must be positive`);
  requireValue(raw.hardGates && typeof raw.hardGates === "object" && Object.keys(raw.hardGates).length > 0, `${file} hardGates are required`);
  return raw;
}

function validateBrief(raw, file) {
  requireValue(raw && typeof raw === "object" && !Array.isArray(raw), `${file} must be an object`);
  requireValue(raw.schemaVersion === 1, `${file} schemaVersion must be 1`);
  for (const field of ["product", "audience", "problem", "promise"]) {
    requireValue(typeof raw[field] === "string" && raw[field].trim(), `${file} ${field} is required`);
  }
  requireValue(Array.isArray(raw.approvedClaims) && raw.approvedClaims.length > 0, `${file} approvedClaims are required`);
  requireValue(raw.seo && typeof raw.seo === "object", `${file} seo contract is required`);
  requireValue(Array.isArray(raw.seo.queryIntents) && raw.seo.queryIntents.length > 0, `${file} seo.queryIntents are required`);
  for (const [index, intent] of raw.seo.queryIntents.entries()) {
    requireValue(intent && typeof intent === "object", `${file} seo.queryIntents.${index} must be an object`);
    requireValue(typeof intent.queryFamily === "string" && intent.queryFamily.trim(), `${file} seo.queryIntents.${index}.queryFamily is required`);
    requireValue(typeof intent.userNeed === "string" && intent.userNeed.trim(), `${file} seo.queryIntents.${index}.userNeed is required`);
  }
  requireValue(Array.isArray(raw.seo.entities) && raw.seo.entities.length > 0, `${file} seo.entities are required`);
  for (const [index, entity] of raw.seo.entities.entries()) {
    requireValue(entity && typeof entity === "object", `${file} seo.entities.${index} must be an object`);
    requireValue(typeof entity.name === "string" && entity.name.trim(), `${file} seo.entities.${index}.name is required`);
    requireValue(typeof entity.type === "string" && entity.type.trim(), `${file} seo.entities.${index}.type is required`);
  }
  return raw;
}

export function resolveSeoContract({ appId = "landing-page", baseUrl, policyPath, briefPath, mode = "release" } = {}) {
  requireValue(MODES.has(mode), `mode must be one of ${[...MODES].join(", ")}`);
  const manifest = loadAppManifest(appId);
  requireValue(manifest.seo && typeof manifest.seo === "object", `${manifest.file} seo section is required`);
  const policyFile = resolveFile(policyPath, manifest.seo.policy, "SEO policy");
  const briefFile = resolveFile(briefPath || process.env.PROBIERZ_LANDING_BRIEF, manifest.seo.brief, "landing brief");
  const policy = validatePolicy(readJson(policyFile, "SEO policy"), policyFile);
  const brief = validateBrief(readJson(briefFile, "landing brief"), briefFile);
  const canonical = secureUrl(baseUrl || process.env.BASE_URL, "base URL");
  const allowedOrigins = new Set([canonical.origin]);
  for (const value of policy.crawl.allowedOrigins || []) allowedOrigins.add(secureUrl(value, "allowed crawl origin").origin);
  const routes = policy.routes.map((route) => ({ ...route, url: new URL(route.path, canonical).href }));
  return {
    appId,
    mode,
    manifest,
    policy: { ...policy, file: policyFile },
    brief: { ...brief, file: briefFile },
    baseUrl: canonical.href,
    canonicalOrigin: canonical.origin,
    allowedOrigins: [...allowedOrigins].sort(),
    routes,
  };
}
