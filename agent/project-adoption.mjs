import { createHash, randomUUID } from "node:crypto";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { parse as parseYaml } from "yaml";
import { validateManifest } from "./apps.mjs";

const INDEX_SCHEMA = "ai.wisent.probierz.project-adoptions.v1";
const INDEX_RELATIVE_PATH = "apps/.adoptions.json";
const SPEC_DIRECTORIES = ["test/specs", "tests", "specs"];
const TARGET_PACKAGES = Object.freeze({
  web: "packages/web",
  electron: "packages/electron",
  "mobile:ios": "packages/mobile",
  "mobile:ios:byk-auth": "packages/mobile",
  "mobile:android": "packages/mobile",
  "desktop:mac": "packages/desktop-native",
  "desktop:win": "packages/desktop-native",
  "desktop:cua": "packages/desktop-cua",
  tui: "packages/tui",
});

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function slash(value) {
  return value.split(path.sep).join("/");
}

function absolute(root, relative) {
  const resolvedRoot = path.resolve(root);
  const resolved = path.resolve(resolvedRoot, ...String(relative).split("/"));
  if (resolved !== resolvedRoot && !resolved.startsWith(`${resolvedRoot}${path.sep}`)) {
    throw new Error(`project definition path escapes its repository: ${relative}`);
  }
  return resolved;
}

function requireDirectory(value, label) {
  let canonical;
  try {
    canonical = realpathSync(path.resolve(String(value || "")));
  } catch {
    throw new Error(`${label} is not an existing directory: ${value || ""}`);
  }
  const metadata = lstatSync(canonical);
  if (!metadata.isDirectory()) throw new Error(`${label} is not a directory: ${canonical}`);
  return canonical;
}

function repositoryRoot(value, label) {
  const root = requireDirectory(value, label);
  const git = path.join(root, ".git");
  if (!existsSync(git)) throw new Error(`${label} is not a Git repository: ${root}`);
  const metadata = lstatSync(git);
  if (metadata.isSymbolicLink() || (!metadata.isDirectory() && !metadata.isFile())) {
    throw new Error(`${label} has an unsupported .git entry: ${git}`);
  }
  return root;
}

function filesBelow(root, relativeRoot) {
  const start = absolute(root, relativeRoot);
  if (!existsSync(start)) return [];
  const initial = lstatSync(start);
  if (initial.isSymbolicLink() || !initial.isDirectory()) {
    throw new Error(`unsupported project definition directory: ${start}`);
  }
  const files = [];
  const pending = [relativeRoot];
  while (pending.length) {
    const current = pending.pop();
    for (const entry of readdirSync(absolute(root, current), { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      const relative = slash(path.join(current, entry.name));
      const source = absolute(root, relative);
      const metadata = lstatSync(source);
      if (metadata.isSymbolicLink()) throw new Error(`project definitions must not contain symlinks: ${source}`);
      if (metadata.isDirectory()) pending.push(relative);
      else if (metadata.isFile()) files.push({ relative, source, mode: metadata.mode & 0o777 });
      else throw new Error(`project definitions contain an unsupported filesystem entry: ${source}`);
    }
  }
  return files.sort((left, right) => left.relative.localeCompare(right.relative));
}

function globExpression(pattern) {
  const escaped = pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^${escaped.replaceAll("**", "\u0000").replaceAll("*", ".*").replaceAll("\u0000", ".*")}$`);
}

function matchesDeclaredSpec(spec, relativeToPackage) {
  const normalized = slash(String(spec || "")).replace(/^\.\//, "");
  const candidate = slash(relativeToPackage);
  const compared = normalized.includes("/") ? candidate : path.posix.basename(candidate);
  return globExpression(normalized).test(compared);
}

function sourceDefinitions(sourceRoot) {
  const appsRoot = path.join(sourceRoot, "apps");
  if (!existsSync(appsRoot) || lstatSync(appsRoot).isSymbolicLink() || !lstatSync(appsRoot).isDirectory()) {
    throw new Error(`selected repository has no supported Probierz apps directory: ${appsRoot}`);
  }

  const appIDs = [];
  const manifests = [];
  for (const entry of readdirSync(appsRoot, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
    if (entry.name.startsWith(".")) continue;
    const appRoot = path.join(appsRoot, entry.name);
    const metadata = lstatSync(appRoot);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw new Error(`unsupported entry in Probierz apps directory: ${appRoot}`);
    }
    const manifestFile = path.join(appRoot, "probierz.yaml");
    if (!existsSync(manifestFile)) continue;
    if (lstatSync(manifestFile).isSymbolicLink() || !lstatSync(manifestFile).isFile()) {
      throw new Error(`Probierz app directory has a non-regular probierz.yaml: ${appRoot}`);
    }
    const document = validateManifest(parseYaml(readFileSync(manifestFile, "utf8")), manifestFile);
    if (document.appId !== entry.name) {
      throw new Error(`invalid app manifest: expected appId ${entry.name}, got ${document.appId}`);
    }
    appIDs.push(document.appId);
    manifests.push({ document, file: manifestFile });
  }
  if (!manifests.length) throw new Error(`selected repository contains no Probierz application manifests: ${appsRoot}`);

  const roots = new Set(["apps"]);
  for (const packageRoot of new Set(Object.values(TARGET_PACKAGES))) {
    for (const specDirectory of SPEC_DIRECTORIES) {
      const relative = `${packageRoot}/${specDirectory}`;
      if (existsSync(absolute(sourceRoot, relative))) roots.add(relative);
    }
  }

  const gathered = [...roots].flatMap((relative) => filesBelow(sourceRoot, relative));
  const skippedLocalState = gathered
    .filter((entry) => entry.relative === INDEX_RELATIVE_PATH)
    .map((entry) => entry.relative);
  const definitions = gathered.filter((entry) => entry.relative !== INDEX_RELATIVE_PATH);
  const relativeSet = new Set(definitions.map((entry) => entry.relative));

  for (const { document, file } of manifests) {
    for (const [target, surface] of Object.entries(document.surfaces)) {
      const packageRoot = TARGET_PACKAGES[target];
      if (!packageRoot) throw new Error(`invalid app manifest: ${file} surface ${target} has no supported Probierz package`);
      const candidates = [...relativeSet]
        .filter((relative) => relative.startsWith(`${packageRoot}/`))
        .map((relative) => relative.slice(packageRoot.length + 1));
      if (!candidates.some((candidate) => matchesDeclaredSpec(surface.spec, candidate))) {
        throw new Error(`invalid app manifest: ${file} surface ${target} spec ${surface.spec} was not found in ${packageRoot}`);
      }
    }
  }

  const files = definitions.map((entry) => {
    const bytes = readFileSync(entry.source);
    return { ...entry, bytes, sha256: sha256(bytes) };
  });
  const identity = createHash("sha256");
  for (const file of files) {
    identity.update(file.relative);
    identity.update("\0");
    identity.update(file.sha256);
    identity.update("\0");
    identity.update(String(file.mode));
    identity.update("\0");
  }
  return {
    appIDs,
    files,
    skippedLocalState,
    sourceDigest: identity.digest("hex"),
  };
}

function emptyIndex() {
  return { schema: INDEX_SCHEMA, sources: [] };
}

function readIndex(projectRoot) {
  const file = absolute(projectRoot, INDEX_RELATIVE_PATH);
  if (!existsSync(file)) return emptyIndex();
  const metadata = lstatSync(file);
  if (metadata.isSymbolicLink() || !metadata.isFile()) throw new Error(`Probierz adoption index is not a regular file: ${file}`);
  const document = JSON.parse(readFileSync(file, "utf8"));
  if (document?.schema !== INDEX_SCHEMA || !Array.isArray(document.sources)) {
    throw new Error(`unsupported Probierz adoption index: ${file}`);
  }
  for (const source of document.sources) {
    if (
      typeof source?.sourceKey !== "string"
      || !/^[0-9a-f]{64}$/.test(source.sourceKey)
      || typeof source.sourceRoot !== "string"
      || !Array.isArray(source.files)
    ) {
      throw new Error(`invalid Probierz adoption source record: ${file}`);
    }
    for (const retained of source.files) {
      if (
        typeof retained?.path !== "string"
        || typeof retained.sha256 !== "string"
        || !/^[0-9a-f]{64}$/.test(retained.sha256)
        || !Number.isInteger(retained.mode)
      ) {
        throw new Error(`invalid Probierz adoption file record: ${file}`);
      }
      absolute(projectRoot, retained.path);
    }
  }
  return document;
}

function fileOwners(index) {
  const owners = new Map();
  for (const source of index.sources) {
    for (const file of source.files) {
      if (typeof file?.path !== "string") continue;
      const existing = owners.get(file.path) || [];
      existing.push(source.sourceKey);
      owners.set(file.path, existing);
    }
  }
  return owners;
}

function currentFile(file) {
  if (!existsSync(file)) return null;
  const metadata = lstatSync(file);
  if (metadata.isSymbolicLink() || !metadata.isFile()) return "unsupported";
  return { sha256: sha256(readFileSync(file)), mode: metadata.mode & 0o777 };
}

function conflict(relative, reason, existingSha256, incomingSha256) {
  return { path: relative, reason, existingSha256, incomingSha256 };
}

function result(status, sourceRoot, sourceDigest, appIDs, counts, conflicts, skippedLocalState) {
  return {
    schema: "ai.wisent.probierz.project-adoption-result.v1",
    status,
    sourceRoot,
    sourceDigest,
    applications: appIDs,
    imported: counts.imported,
    unchanged: counts.unchanged,
    removed: counts.removed,
    conflicting: conflicts.length,
    rejected: counts.rejected,
    conflicts,
    skippedLocalState,
    executedJourneys: false,
  };
}

export function listProjectAdoptions({ projectRoot }) {
  const root = repositoryRoot(projectRoot, "Probierz project root");
  const index = readIndex(root);
  return {
    schema: INDEX_SCHEMA,
    file: absolute(root, INDEX_RELATIVE_PATH),
    sources: index.sources.map(({ files, ...source }) => ({ ...source, fileCount: files.length })),
  };
}

export function adoptProject({ projectRoot, sourceRoot, replace = false }) {
  const destination = repositoryRoot(projectRoot, "Probierz project root");
  const source = repositoryRoot(sourceRoot, "Adoption source");
  if (source === destination) throw new Error("adoption source is already this Probierz project");

  const definitions = sourceDefinitions(source);
  const index = readIndex(destination);
  const sourceKey = sha256(source);
  const existingSource = index.sources.find((entry) => entry.sourceKey === sourceKey);
  const previousByPath = new Map((existingSource?.files || []).map((file) => [file.path, file]));
  const ownership = fileOwners(index);
  const incoming = new Map(definitions.files.map((file) => [file.relative, file]));
  const conflicts = [];
  const planned = [];
  let unchanged = 0;

  for (const file of definitions.files) {
    const target = absolute(destination, file.relative);
    const current = currentFile(target);
    const digest = current === null || current === "unsupported" ? current : current.sha256;
    if (current !== null && current !== "unsupported" && current.sha256 === file.sha256 && current.mode === file.mode) {
      unchanged += 1;
      continue;
    }
    const otherOwners = (ownership.get(file.relative) || []).filter((owner) => owner !== sourceKey);
    const previous = previousByPath.get(file.relative);
    const locallyChanged = current !== null
      && current !== "unsupported"
      && previous
      && (current.sha256 !== previous.sha256 || current.mode !== previous.mode);
    if (otherOwners.length) {
      conflicts.push(conflict(file.relative, "destination is owned by another adopted source", digest, file.sha256));
    } else if (current === "unsupported") {
      conflicts.push(conflict(file.relative, "destination is not a regular file", digest, file.sha256));
    } else if (locallyChanged) {
      conflicts.push(conflict(file.relative, "previously adopted definition has local content or mode changes", digest, file.sha256));
    } else if (current !== null && !replace) {
      conflicts.push(conflict(file.relative, "destination content or mode differs; repeat with explicit replacement", digest, file.sha256));
    } else {
      planned.push(file);
    }
  }

  const removals = [];
  for (const previous of existingSource?.files || []) {
    if (incoming.has(previous.path)) continue;
    const target = absolute(destination, previous.path);
    const current = currentFile(target);
    const digest = current === null || current === "unsupported" ? current : current.sha256;
    if (current === null) continue;
    if (!replace) {
      conflicts.push(conflict(previous.path, "previously adopted definition is absent from the selected source", digest, null));
    } else if (
      current === "unsupported"
      || current.sha256 !== previous.sha256
      || current.mode !== previous.mode
    ) {
      conflicts.push(conflict(previous.path, "previously adopted definition has local content or mode changes", digest, null));
    } else {
      removals.push(previous.path);
    }
  }

  if (conflicts.length) {
    return result(
      "conflict",
      source,
      definitions.sourceDigest,
      definitions.appIDs,
      { imported: 0, unchanged, removed: 0, rejected: conflicts.length },
      conflicts,
      definitions.skippedLocalState,
    );
  }

  if (existingSource?.sourceDigest === definitions.sourceDigest && planned.length === 0 && removals.length === 0) {
    return result(
      "unchanged",
      source,
      definitions.sourceDigest,
      definitions.appIDs,
      { imported: 0, unchanged, removed: 0, rejected: 0 },
      [],
      definitions.skippedLocalState,
    );
  }

  const adoptedAt = new Date().toISOString();
  const record = {
    sourceKey,
    sourceRoot: source,
    sourceDigest: definitions.sourceDigest,
    adoptedAt,
    applications: definitions.appIDs,
    files: definitions.files.map((file) => ({ path: file.relative, sha256: file.sha256, mode: file.mode })),
  };
  const nextIndex = {
    schema: INDEX_SCHEMA,
    sources: [...index.sources.filter((entry) => entry.sourceKey !== sourceKey), record]
      .sort((left, right) => left.sourceRoot.localeCompare(right.sourceRoot)),
  };

  const transactionID = `${process.pid}-${randomUUID()}`;
  const stageRoot = path.join(destination, `.probierz-adoption-stage-${transactionID}`);
  const backupRoot = path.join(destination, `.probierz-adoption-backup-${transactionID}`);
  const indexBytes = Buffer.from(`${JSON.stringify(nextIndex, null, 2)}\n`);
  const staged = [...planned, {
    relative: INDEX_RELATIVE_PATH,
    bytes: indexBytes,
    mode: 0o600,
    sha256: sha256(indexBytes),
  }];
  const backedUp = [];
  const placed = [];

  try {
    for (const file of staged) {
      const stagedFile = absolute(stageRoot, file.relative);
      mkdirSync(path.dirname(stagedFile), { recursive: true });
      writeFileSync(stagedFile, file.bytes, { mode: file.mode, flag: "wx" });
      chmodSync(stagedFile, file.mode);
    }

    for (const relative of removals) {
      const target = absolute(destination, relative);
      const backup = absolute(backupRoot, relative);
      mkdirSync(path.dirname(backup), { recursive: true });
      renameSync(target, backup);
      backedUp.push({ target, backup });
    }
    for (const file of staged) {
      const target = absolute(destination, file.relative);
      const stagedFile = absolute(stageRoot, file.relative);
      mkdirSync(path.dirname(target), { recursive: true });
      if (existsSync(target)) {
        const backup = absolute(backupRoot, file.relative);
        mkdirSync(path.dirname(backup), { recursive: true });
        renameSync(target, backup);
        backedUp.push({ target, backup });
      }
      renameSync(stagedFile, target);
      placed.push(target);
    }
  } catch (error) {
    for (const target of placed.reverse()) rmSync(target, { force: true });
    for (const { target, backup } of backedUp.reverse()) {
      if (existsSync(backup)) {
        mkdirSync(path.dirname(target), { recursive: true });
        renameSync(backup, target);
      }
    }
    throw error;
  } finally {
    rmSync(stageRoot, { recursive: true, force: true });
    rmSync(backupRoot, { recursive: true, force: true });
  }

  return result(
    existingSource ? "replaced" : "imported",
    source,
    definitions.sourceDigest,
    definitions.appIDs,
    { imported: planned.length, unchanged, removed: removals.length, rejected: 0 },
    [],
    definitions.skippedLocalState,
  );
}
