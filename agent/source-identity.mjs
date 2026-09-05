import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readlinkSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

function gitPaths(root) {
  const listed = spawnSync(
    "git",
    ["-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
  );
  if (listed.status !== Number("0")) {
    const detail = listed.error?.message || listed.stderr?.trim() || `git exited ${listed.status}`;
    throw new Error(`source inventory failed for ${root}: ${detail}`);
  }
  return listed.stdout.split("\0").filter(Boolean);
}

export function sourcePathAllowed(relative, { excludeRuntimeSecrets = false } = {}) {
  if (!relative || path.isAbsolute(relative)) return false;
  const parts = relative.split("/");
  if (parts.includes("..") || parts.some((part) => part === "node_modules" || part === "test-results")) {
    return false;
  }
  if (!excludeRuntimeSecrets) return true;
  if (parts.some((part) => part.startsWith(".env"))) return false;
  return !/^probierz-.*\.json$/.test(path.basename(relative));
}

export function repositorySourceFiles(root, {
  excludeRuntimeSecrets = false,
  includePackageLock = false,
} = {}) {
  const files = new Set(gitPaths(root).filter((relative) => {
    if (!sourcePathAllowed(relative, { excludeRuntimeSecrets })) return false;
    try {
      const metadata = lstatSync(path.join(root, relative));
      return metadata.isFile() || metadata.isSymbolicLink();
    } catch {
      return false;
    }
  }));
  if (includePackageLock && existsSync(path.join(root, "package-lock.json"))) files.add("package-lock.json");
  return [...files].sort();
}

function hashSourceFiles(root, files) {
  const hash = createHash("sha256");
  for (const relative of files) {
    const file = path.join(root, relative);
    const metadata = lstatSync(file);
    const kind = metadata.isSymbolicLink() ? "symlink" : "file";
    const payload = kind === "symlink" ? Buffer.from(readlinkSync(file)) : readFileSync(file);
    const header = JSON.stringify({ path: relative, kind, mode: metadata.mode & 0o777, bytes: payload.length });
    hash.update(`${Buffer.byteLength(header)}:`);
    hash.update(header);
    hash.update(payload);
  }
  return hash.digest("hex");
}

export function repositoryIdentity(root, name, index = null, {
  excludeRuntimeSecrets = false,
  includePackageLock = false,
} = {}) {
  const head = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
  const diff = spawnSync("git", ["-C", root, "diff", "--quiet", "HEAD", "--"]);
  const others = spawnSync(
    "git",
    ["-C", root, "ls-files", "--others", "--exclude-standard", "-z"],
    { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
  );
  const files = repositorySourceFiles(root, { excludeRuntimeSecrets, includePackageLock });
  const worktreeSha256 = hashSourceFiles(root, files);
  const identity = {
    ...(index === null ? {} : { index }),
    name,
    gitSha: head.status === Number("0") ? head.stdout.trim() : null,
    dirty: diff.status !== Number("0") || Boolean(others.stdout?.length),
    worktreeSha256,
  };
  const exactSource = { ...(index === null ? {} : { index }), name, worktreeSha256 };
  return {
    ...identity,
    sha256: createHash("sha256").update(JSON.stringify(exactSource)).digest("hex"),
  };
}
