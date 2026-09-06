import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { parse } from "yaml";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const APPS_ROOT = path.resolve(HERE, "..", "apps");
const SENSITIVE_KEY = /(auth|cookie|credential|email|key|otp|password|pii|secret|session|token)/i;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const PUBLICATION_ARTIFACT_KINDS = new Set(["screenshot", "recording", "trace"]);
const RECORDING_TARGETS = new Set(["web", "mobile:ios", "mobile:android", "desktop:mac", "desktop:cua", "desktop:win"]);

export function targetSupportsArtifactKind(target, kind) {
  if (!PUBLICATION_ARTIFACT_KINDS.has(kind)) return false;
  return kind !== "recording" || RECORDING_TARGETS.has(target);
}


function manifestPath(appId) {
  const clean = String(appId || "").trim();
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(clean)) throw new Error(`invalid app ID: ${appId}`);
  return path.join(APPS_ROOT, clean, "probierz.yaml");
}

function requireValue(condition, message) {
  if (!condition) throw new Error(`invalid app manifest: ${message}`);
}

export function validateManifest(document, file) {
  requireValue(document && typeof document === "object", `${file} is not an object`);
  requireValue(document.schemaVersion === 1, `${file} schemaVersion must be 1`);
  requireValue(typeof document.appId === "string" && document.appId.length > 0, `${file} appId is required`);
  requireValue(typeof document.owner === "string" && document.owner.length > 0, `${file} owner is required`);
  requireValue(Array.isArray(document.repositories) && document.repositories.length > 0, `${file} repositories are required`);
  requireValue(document.surfaces && typeof document.surfaces === "object", `${file} surfaces are required`);
  requireValue(document.journeys && typeof document.journeys === "object", `${file} journeys are required`);
  const firstUse = document.journeys["onboarding-first-use"];
  if (firstUse) {
    requireValue(/^[a-z0-9][a-z0-9._-]*$/i.test(document.productId || ""), `${file} productId is required and must be stable for onboarding-first-use`);
    for (const name of ["pullRequestDays", "nightlyDays", "adhocDays"]) {
      requireValue(
        Number.isFinite(Number(document.artifacts?.retain?.[name])) && Number(document.artifacts.retain[name]) > 0,
        `${file} artifacts.retain.${name} is required and must be positive for onboarding-first-use`,
      );
    }
    requireValue(
      Array.isArray(document.artifacts?.redact)
        && document.artifacts.redact.length > 0
        && document.artifacts.redact.every((name) => typeof name === "string" && name.length > 0),
      `${file} artifacts.redact must contain redaction keys for onboarding-first-use`,
    );
    for (const name of ["TOKEN", "SECRET", "PASSWORD", "KEY", "COOKIE", "AUTH"]) {
      requireValue(document.artifacts.redact.includes(name), `${file} artifacts.redact must include ${name} for onboarding-first-use`);
    }
  }


  for (const repository of document.repositories) {
    requireValue(typeof repository.root === "string" && path.isAbsolute(repository.root), `${file} repository root must be absolute`);
    requireValue(Array.isArray(repository.mappings), `${file} repository mappings are required`);
  }
  for (const [target, surface] of Object.entries(document.surfaces)) {
    requireValue(surface && typeof surface === "object", `${file} surface ${target} must be an object`);
    requireValue(typeof surface.spec === "string" && surface.spec.length > 0, `${file} surface ${target} spec is required`);
    requireValue(Array.isArray(surface.journeys) && surface.journeys.length > 0, `${file} surface ${target} journeys are required`);
    if (surface.testDirectory !== undefined) {
      requireValue(
        typeof surface.testDirectory === "string"
          && !path.isAbsolute(surface.testDirectory)
          && !surface.testDirectory.includes("\\")
          && surface.testDirectory.split("/").every((part) => part && part !== "." && part !== "..")
          && path.basename(surface.testDirectory) === "tests",
        `${file} surface ${target} testDirectory must be a relative product path ending in tests without parent traversal`,
      );
    }
    for (const journey of surface.journeys) {
      requireValue(Boolean(document.journeys[journey]), `${file} surface ${target} journey ${journey} is unknown`);
    }
    for (const [index, override] of (surface.journeyOverrides || []).entries()) {
      requireValue(override && typeof override === "object", `${file} surface ${target} journeyOverrides.${index} must be an object`);
      requireValue(override.when && typeof override.when === "object", `${file} surface ${target} journeyOverrides.${index}.when is required`);
      requireValue(Object.keys(override.when).length > 0, `${file} surface ${target} journeyOverrides.${index}.when must not be empty`);
      requireValue(Array.isArray(override.journeys) && override.journeys.length > 0, `${file} surface ${target} journeyOverrides.${index}.journeys are required`);
      for (const [key, value] of Object.entries(override.when)) {
        requireValue(!SENSITIVE_KEY.test(key), `${file} surface ${target} journey override ${key} must not be sensitive`);
        requireValue(["string", "number", "boolean"].includes(typeof value), `${file} surface ${target} journey override ${key} must be scalar`);
      }
      for (const journey of override.journeys) {
        requireValue(Boolean(document.journeys[journey]), `${file} surface ${target} journey override ${journey} is unknown`);
      }
    }
    for (const key of Object.keys(surface.conditions || {})) {
      requireValue(!SENSITIVE_KEY.test(key), `${file} secret condition ${key} must use secretRefs`);
    }
    for (const [targetName, sourceName] of Object.entries(surface.env || {})) {
      requireValue(typeof targetName === "string" && targetName.length > 0, `${file} surface ${target} env target is required`);
      requireValue(typeof sourceName === "string" && sourceName.length > 0, `${file} surface ${target} env source is required`);
    }
  }
  for (const [name, journey] of Object.entries(document.journeys)) {
    requireValue(journey && typeof journey === "object", `${file} journey ${name} must be an object`);
    requireValue(typeof journey.owner === "string" && journey.owner.length > 0, `${file} journey ${name} owner is required`);
    requireValue(Number(journey.timeoutMs) > 0, `${file} journey ${name} timeoutMs must be positive`);
    const hasJourneyIdentity = ["journeyId", "journeyVersion", "journeyVersionId", "firstSuccessFact"]
      .some((field) => journey[field] !== undefined);
    if (name === "onboarding-first-use" || hasJourneyIdentity) {
      requireValue(typeof journey.journeyId === "string" && journey.journeyId.length > 0, `${file} journey ${name} journeyId is required`);
      requireValue(typeof journey.journeyVersion === "string" && journey.journeyVersion.length > 0, `${file} journey ${name} journeyVersion is required`);
      requireValue(UUID.test(journey.journeyVersionId || ""), `${file} journey ${name} journeyVersionId must be a UUID`);
      requireValue(typeof journey.firstSuccessFact === "string" && journey.firstSuccessFact.length > 0, `${file} journey ${name} firstSuccessFact is required`);
    }
    if (name === "onboarding-first-use") {
      requireValue(journey.publication && typeof journey.publication === "object", `${file} journey ${name} publication is required`);
    }
    if (journey.publication) {
      const publication = journey.publication;
      requireValue(hasJourneyIdentity || name === "onboarding-first-use", `${file} journey ${name} publication requires immutable journey identity`);
      requireValue(typeof document.productId === "string" && document.productId.length > 0, `${file} journey ${name} publication requires productId`);
      requireValue(typeof publication.screenId === "string" && publication.screenId.length > 0, `${file} journey ${name} publication.screenId is required`);
      requireValue(Array.isArray(publication.artifactKinds) && publication.artifactKinds.length > 0, `${file} journey ${name} publication.artifactKinds are required`);
      requireValue(new Set(publication.artifactKinds).size === publication.artifactKinds.length, `${file} journey ${name} publication.artifactKinds must be unique`);
      for (const kind of publication.artifactKinds) {
        requireValue(PUBLICATION_ARTIFACT_KINDS.has(kind), `${file} journey ${name} publication artifact kind ${kind} is unsupported`);
      }
      requireValue(["E2", "E3"].includes(publication.minimumEvidence), `${file} journey ${name} publication.minimumEvidence must be E2 or E3`);
      requireValue(typeof publication.redactionRequired === "boolean", `${file} journey ${name} publication.redactionRequired must be boolean`);
    }
  }
  for (const [name, journey] of Object.entries(document.journeys)) {
    if (!journey.publication?.artifactKinds?.includes("recording")) continue;
    const targets = Object.entries(document.surfaces)
      .filter(([, surface]) => surface.journeys.includes(name))
      .map(([target]) => target);
    requireValue(targets.some((target) => targetSupportsArtifactKind(target, "recording")), `${file} journey ${name} claims recording but none of its drivers support recording`);
  }
  if (document.seo !== undefined) {
    requireValue(document.seo && typeof document.seo === "object" && !Array.isArray(document.seo), `${file} seo must be an object`);
    requireValue(typeof document.seo.policy === "string" && document.seo.policy.length > 0, `${file} seo.policy is required`);
    requireValue(typeof document.seo.brief === "string" && document.seo.brief.length > 0, `${file} seo.brief is required`);
    requireValue(document.seo.profiles && typeof document.seo.profiles === "object", `${file} seo.profiles are required`);
    for (const [profileName, profile] of Object.entries(document.seo.profiles)) {
      requireValue(["pull-request", "release", "nightly", "production"].includes(profileName), `${file} seo profile ${profileName} is unsupported`);
      requireValue(profile && typeof profile === "object", `${file} seo.profiles.${profileName} must be an object`);
      requireValue(typeof profile.requireSignature === "boolean", `${file} seo.profiles.${profileName}.requireSignature must be boolean`);
      requireValue(typeof profile.requireProductionEvidence === "boolean", `${file} seo.profiles.${profileName}.requireProductionEvidence must be boolean`);
    }
  }
  for (const [key, reference] of Object.entries(document.secretRefs || {})) {
    requireValue(typeof reference === "string" && reference.startsWith("vault://"), `${file} secretRefs.${key} must be a vault:// reference`);
  }
  for (const hookName of ["seed", "cleanup"]) {
    const hook = document.data?.[hookName];
    if (!hook) continue;
    requireValue(typeof hook.command === "string" && hook.command.length > 0, `${file} data.${hookName}.command is required`);
    requireValue(Array.isArray(hook.args) && hook.args.every((arg) => typeof arg === "string"), `${file} data.${hookName}.args must be strings`);
  }
  for (const [name, days] of Object.entries(document.artifacts?.retain || {})) {
    requireValue(Number.isFinite(Number(days)) && Number(days) > 0, `${file} artifacts.retain.${name} must be positive`);
  }
  for (const [profileName, profile] of Object.entries(document.matrix || {})) {
    requireValue(profile && typeof profile === "object", `${file} matrix.${profileName} must be an object`);
    requireValue(Array.isArray(profile.targets) && profile.targets.length > 0, `${file} matrix.${profileName}.targets are required`);
    for (const target of profile.targets) {
      requireValue(Boolean(document.surfaces[target]), `${file} matrix.${profileName} target ${target} has no surface`);
    }
    for (const [name, values] of Object.entries(profile.dimensions || {})) {
      requireValue(!SENSITIVE_KEY.test(name), `${file} matrix.${profileName} secret dimension ${name} must use secretRefs`);
      requireValue(Array.isArray(values) && values.length > 0, `${file} matrix.${profileName}.${name} needs values`);
      requireValue(values.every((value) => ["string", "number", "boolean"].includes(typeof value)), `${file} matrix.${profileName}.${name} values must be scalar`);
    }
    requireValue(["E2", "E3"].includes(profile.minimumCellEvidence || "E3"), `${file} matrix.${profileName}.minimumCellEvidence must be E2 or E3`);
    requireValue(["optional", "required"].includes(profile.artifactEncryption || "optional"), `${file} matrix.${profileName}.artifactEncryption must be optional or required`);
    requireValue(profile.removePlaintextAfterProtection === undefined || typeof profile.removePlaintextAfterProtection === "boolean", `${file} matrix.${profileName}.removePlaintextAfterProtection must be boolean`);
    requireValue(Number(profile.maxCells || 128) > 0, `${file} matrix.${profileName}.maxCells must be positive`);
    requireValue(Number(profile.maximumParallel || 4) > 0, `${file} matrix.${profileName}.maximumParallel must be positive`);
  }
  for (const [policyName, policy] of [["pullRequestPolicy", document.pullRequestPolicy], ["releasePolicy", document.releasePolicy]]) {
    if (!policy) continue;
    requireValue(["E2", "E3"].includes(policy.minimumEvidence || "E3"), `${file} ${policyName}.minimumEvidence must be E2 or E3`);
    requireValue(policy.requireProtectedArtifacts === undefined || typeof policy.requireProtectedArtifacts === "boolean", `${file} ${policyName}.requireProtectedArtifacts must be boolean`);
    requireValue(policy.requireSecretScan === undefined || typeof policy.requireSecretScan === "boolean", `${file} ${policyName}.requireSecretScan must be boolean`);
    for (const target of policy.requiredTargets || []) requireValue(Boolean(document.surfaces[target]), `${file} ${policyName} target ${target} has no surface`);
    for (const journey of policy.requiredJourneys || []) requireValue(Boolean(document.journeys[journey]), `${file} ${policyName} journey ${journey} is unknown`);
    if (policy.requiredMatrixProfile) requireValue(Boolean(document.matrix?.[policy.requiredMatrixProfile]), `${file} ${policyName} matrix ${policy.requiredMatrixProfile} is unknown`);
  }
  return document;
}

export function loadAppManifest(appId) {
  const file = manifestPath(appId);
  if (!existsSync(file)) throw new Error(`app manifest not found: ${file}`);
  const document = validateManifest(parse(readFileSync(file, "utf8")), file);
  if (document.appId !== appId) throw new Error(`app manifest ID mismatch: expected ${appId}, got ${document.appId}`);
  return { ...document, file };
}

function loadAppManifests() {
  if (!existsSync(APPS_ROOT)) return [];
  return readdirSync(APPS_ROOT, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(path.join(APPS_ROOT, entry.name, "probierz.yaml")))
    .map((entry) => loadAppManifest(entry.name));
}

export function listApps() {
  return loadAppManifests()
    .map((manifest) => ({
      appId: manifest.appId,
      owner: manifest.owner,
      file: manifest.file,
      targets: Object.keys(manifest.surfaces).sort(),
      journeys: Object.keys(manifest.journeys).sort(),
    }))
    .sort((left, right) => left.appId.localeCompare(right.appId));
}

function globRegex(pattern) {
  const escaped = String(pattern)
    .replace(/[.+^${}()|[\]\\]/g, "\\$&")
    .replace(/\*\*/g, "\u0000")
    .replace(/\*/g, "[^/]*")
    .replace(/\u0000/g, ".*");
  return new RegExp(`^${escaped}$`);
}

function repositoryRelative(repository, file) {
  const absolute = path.isAbsolute(file) ? path.normalize(file) : path.resolve(repository.root, file);
  const relative = path.relative(repository.root, absolute).split(path.sep).join("/");
  return relative === "" || (!relative.startsWith("../") && relative !== "..") ? relative : null;
}

export function affectedAppJourneys(files, manifests = loadAppManifests()) {
  const matches = [];
  for (const manifest of manifests) {
    const journeyTargets = new Map();
    for (const [target, surface] of Object.entries(manifest.surfaces)) {
      for (const journey of surface.journeys) {
        if (!journeyTargets.has(journey)) journeyTargets.set(journey, new Set());
        journeyTargets.get(journey).add(target);
      }
    }
    for (const repository of manifest.repositories) {
      for (const file of files) {
        const relative = repositoryRelative(repository, file);
        if (relative === null) continue;
        for (const mapping of repository.mappings) {
          const patterns = Array.isArray(mapping.paths) ? mapping.paths : [];
          if (!patterns.some((pattern) => globRegex(pattern).test(relative))) continue;
          const journeys = Array.isArray(mapping.journeys) ? mapping.journeys : [];
          matches.push({
            appId: manifest.appId,
            input: String(file),
            file: path.isAbsolute(file) ? path.normalize(file) : path.join(repository.root, file),
            repository: repository.root,
            journeys,
            targets: [...new Set(journeys.flatMap((journey) => [...(journeyTargets.get(journey) || [])]))].sort(),
          });
        }
      }
    }
  }
  return matches;
}

export function surfaceJourneys(surface, environment = {}) {
  const selected = String(environment.PROBIERZ_JOURNEY ?? "").trim();
  if (selected) {
    const declared = surface?.journeys?.includes(selected)
      || surface?.journeyOverrides?.some((override) => override.journeys.includes(selected));
    if (!declared) throw new Error(`PROBIERZ_JOURNEY ${selected} is not declared for this surface`);
    return [selected];
  }
  for (const override of surface?.journeyOverrides || []) {
    const matches = Object.entries(override.when || {}).every(
      ([name, value]) => String(environment[name] ?? "") === String(value),
    );
    if (matches) return override.journeys;
  }
  return surface?.journeys || [];
}

export function appSurface(appId, target) {
  const manifest = loadAppManifest(appId);
  const surface = manifest.surfaces[target];
  if (!surface) throw new Error(`app ${appId} has no ${target} surface`);
  return { manifest, surface };
}
