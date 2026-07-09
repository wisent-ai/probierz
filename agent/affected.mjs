// Change -> affected-target selection for the probierz test toolkit. Given the
// files a change touched, it returns which run targets that change could affect,
// so an orchestrator / CI / human re-runs only what is relevant instead of the
// whole matrix. Purely deterministic and structural: it maps a file to a target
// by filesystem containment against that target's package directory (from the
// TARGETS single source of truth), never by matching words or extensions. No
// LLM, no reasoning -- interpreting a result is brama's job, not this module's.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { TARGETS } from "./runner.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");

// Package directory (repo-relative, posix) -> the targets that live in it.
function targetsByPackage() {
  const map = new Map();
  for (const [name, t] of Object.entries(TARGETS)) {
    if (!map.has(t.pkg)) map.set(t.pkg, []);
    map.get(t.pkg).push(name);
  }
  return map;
}

// Is `child` inside `parent` (or equal)? Structural containment via
// path.relative -- handles ./, .., and normalization -- not string prefixing.
function isInside(parent, child) {
  const rel = path.relative(parent, child);
  return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
}

// Repo-relative posix form of a changed-file path.
function normalize(file) {
  return path.normalize(String(file)).split(path.sep).join("/").replace(/^\.\//, "");
}

// Map a set of changed files to affected run targets.
//   - a file inside packages/<x> affects that package's target(s)
//   - a file under agent/ or a repo-root file changes HOW any suite runs, so it
//     is cross-cutting and conservatively affects every target
//   - anything else (docs, skills, unrelated trees) affects nothing
// Returns { targets (sorted), crossCutting, files: [{ file, affects }] }.
export function affectedTargets(files) {
  const byPkg = targetsByPackage();
  const allTargets = Object.keys(TARGETS);
  const hit = new Set();
  let crossCutting = false;
  const classified = [];

  for (const raw of files) {
    const file = normalize(raw);
    let pkgMatch = null;
    for (const [pkg, names] of byPkg) {
      if (isInside(pkg, file)) { pkgMatch = { pkg, names }; break; }
    }
    if (pkgMatch) {
      pkgMatch.names.forEach((n) => hit.add(n));
      classified.push({ file, affects: [...pkgMatch.names] });
      continue;
    }
    const underAgent = isInside("agent", file);
    const atRepoRoot = path.dirname(file) === ".";
    if (underAgent || atRepoRoot) {
      crossCutting = true;
      classified.push({ file, affects: "all" });
      continue;
    }
    classified.push({ file, affects: [] });
  }

  const targets = crossCutting ? [...allTargets] : [...hit];
  return { targets: targets.sort(), crossCutting, files: classified };
}

// The files a change touched, from git. `ref` (default HEAD) is what the working
// tree is compared against: HEAD = uncommitted edits; origin/main = everything
// that diverged from the base. Throws if git is unavailable or the ref is bad.
export function changedFiles(ref) {
  const against = ref || "HEAD";
  const r = spawnSync("git", ["-C", ROOT, "diff", "--name-only", against], { encoding: "utf8" });
  if (r.status !== 0) {
    throw new Error(`git diff --name-only ${against} failed: ${String(r.stderr || "").trim()}`);
  }
  return r.stdout.split("\n").map((s) => s.trim()).filter(Boolean);
}

// Convenience: affected targets for the current git diff against `ref`.
export function affectedFromGit(ref) {
  return { ref: ref || "HEAD", ...affectedTargets(changedFiles(ref)) };
}
