import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const LOCK_ROOT = path.join(ROOT, "test-results", ".locks");
const MISSING_OWNER_GRACE_MS = 30 * 1000;

function lockName(resource) {
  const label = resource.replace(/[^a-zA-Z0-9._-]+/g, "-").slice(0, 80);
  const hash = createHash("sha256").update(resource).digest("hex").slice(0, 12);
  return `${label}-${hash}`;
}

function ownerFile(directory) {
  return path.join(directory, "owner.json");
}

function readOwner(directory) {
  try { return JSON.parse(readFileSync(ownerFile(directory), "utf8")); }
  catch { return null; }
}

function processAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function stale(directory, owner) {
  if (owner) return !processAlive(Number(owner.pid));
  try { return Date.now() - statSync(directory).mtimeMs >= MISSING_OWNER_GRACE_MS; }
  catch { return true; }
}

function reclaim(directory) {
  const tombstone = `${directory}.stale-${process.pid}-${Date.now()}`;
  try {
    renameSync(directory, tombstone);
    rmSync(tombstone, { recursive: true, force: true });
    return true;
  } catch {
    return false;
  }
}

function acquireOne(resource, runId) {
  mkdirSync(LOCK_ROOT, { recursive: true });
  const directory = path.join(LOCK_ROOT, lockName(resource));
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      mkdirSync(directory);
      const owner = { schemaVersion: 1, resource, runId, pid: process.pid, acquiredAt: new Date().toISOString() };
      try {
        writeFileSync(ownerFile(directory), `${JSON.stringify(owner, null, 2)}\n`, { mode: 0o600, flag: "wx" });
      } catch (error) {
        rmSync(directory, { recursive: true, force: true });
        throw error;
      }
      return { resource, directory, owner };
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      const owner = readOwner(directory);
      if (attempt === 0 && stale(directory, owner) && reclaim(directory)) continue;
      const detail = owner ? `run ${owner.runId} (pid ${owner.pid})` : "an unknown owner";
      const conflict = new Error(`resource locked: ${resource} by ${detail}`);
      conflict.code = "PROBIERZ_RESOURCE_LOCKED";
      conflict.resource = resource;
      conflict.owner = owner;
      throw conflict;
    }
  }
  throw new Error(`could not acquire resource: ${resource}`);
}

function releaseOne(lock, runId) {
  if (!existsSync(lock.directory)) return;
  const owner = readOwner(lock.directory);
  if (owner?.runId !== runId || owner?.pid !== process.pid) return;
  rmSync(lock.directory, { recursive: true, force: true });
}

export function resourcesFor(target, env = {}) {
  const resources = [];
  if (target === "mobile:ios" || target === "mobile:ios:byk-auth") {
    resources.push(`device:ios:${env.IOS_DEVICE || "default"}:${env.IOS_VERSION || "default"}`, "port:4723");
  } else if (target === "mobile:android") {
    resources.push(`device:android:${env.ANDROID_DEVICE || "default"}:${env.ANDROID_VERSION || "default"}`, "port:4723");
  } else if (target === "desktop:mac") {
    resources.push(`device:mac:${env.MAC_BUNDLE_ID || env.MAC_APP_PATH || "host"}`, "port:4723");
  } else if (target === "desktop:win") {
    resources.push(`device:win:${env.WIN_APP || "host"}`, "port:4723");
  }
  return [...new Set(resources)].sort();
}

function wait(delayMs, signal) {
  if (signal?.aborted) return Promise.reject(Object.assign(new Error("resource wait cancelled"), { code: "PROBIERZ_CANCELLED" }));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(done, delayMs);
    function done() {
      signal?.removeEventListener("abort", aborted);
      resolve();
    }
    function aborted() {
      clearTimeout(timer);
      signal?.removeEventListener("abort", aborted);
      reject(Object.assign(new Error("resource wait cancelled"), { code: "PROBIERZ_CANCELLED" }));
    }
    signal?.addEventListener("abort", aborted, { once: true });
  });
}

export async function acquireResourcesWait(resources, runId, {
  timeoutMs = 0,
  pollMs = 250,
  signal,
} = {}) {
  const deadline = Date.now() + Math.max(0, Number(timeoutMs) || 0);
  while (true) {
    if (signal?.aborted) {
      throw Object.assign(new Error("resource wait cancelled"), { code: "PROBIERZ_CANCELLED" });
    }
    try {
      return acquireResources(resources, runId);
    } catch (error) {
      if (error?.code !== "PROBIERZ_RESOURCE_LOCKED" || Date.now() >= deadline) throw error;
      await wait(Math.min(Math.max(10, Number(pollMs) || 250), Math.max(1, deadline - Date.now())), signal);
    }
  }
}

export function acquireResources(resources, runId) {
  const acquired = [];
  try {
    for (const resource of [...new Set(resources)].sort()) acquired.push(acquireOne(resource, runId));
  } catch (error) {
    for (const lock of acquired.reverse()) releaseOne(lock, runId);
    throw error;
  }
  let released = false;
  return {
    resources: acquired.map((lock) => lock.resource),
    release() {
      if (released) return;
      released = true;
      for (const lock of acquired.reverse()) releaseOne(lock, runId);
    },
  };
}
