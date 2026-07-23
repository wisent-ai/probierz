// Autonomous app-manifest authoring: probe the real app, summarize the
// repository layout, and have a headless model draft the journey contract
// (journeys, mappings, surfaces, pull-request policy). The draft must pass
// the same manifest validation every hand-written manifest passes; failed
// rounds feed the validator's errors back into the next brief. With
// --specs the pipeline continues into author-spec for every declared
// journey, so declaration and coverage are both automatic.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { APPS_ROOT, loadAppManifest } from "./apps.mjs";
import { authorSpec, draftWithModel, probeNative, probeWeb } from "./author-spec.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const MAX_ROUNDS = Number("3");
const TREE_DIRS = Number("40");

function repoTree(repoRoot) {
  const lines = [];
  let first = [];
  try { first = readdirSync(repoRoot, { withFileTypes: true }); } catch { return "(unreadable)"; }
  for (const entry of first.slice(0, TREE_DIRS)) {
    if (!entry.isDirectory()) {
      lines.push(entry.name);
      continue;
    }
    if (["node_modules", ".git", ".build", "dist", "build"].includes(entry.name)) continue;
    let children = [];
    try { children = readdirSync(path.join(repoRoot, entry.name), { withFileTypes: true }); } catch { children = []; }
    lines.push(`${entry.name}/`);
    for (const child of children.slice(0, Number("12"))) {
      lines.push(`  ${entry.name}/${child.name}${child.isDirectory() ? "/" : ""}`);
    }
  }
  return lines.join("\n");
}

function buildManifestBrief({ appId, owner, desc, probe, trees, repositories, target, round, previousDraft, errors }) {
  const lines = [
    `Draft a probierz app manifest (YAML) for the app "${appId}".`,
    `What the app does: ${desc}`,
    `Owner string to use: ${owner}`,
    "",
    "Probe of the real app's entry screen:",
    probe,
    "",
    "Repository layout (only use mapping paths that exist below):",
    ...repositories.map((repo) => `${repo}:\n${trees[repo]}`),
    "",
    "Output contract (the file is validated strictly):",
    "schemaVersion: 1",
    `appId: ${appId}`,
    `owner: ${owner}`,
    "repositories: one entry per repository root above (absolute path, mappings with path globs -> journeys)",
    `surfaces: include the "${target}" surface with a spec filename and the journeys it covers`,
    "journeys: 3-8 critical user journeys, kebab-case names, each with owner, timeoutMs, and a one-line",
    "  'description' of the user goal (used later to author the spec)",
    "pullRequestPolicy: minimumEvidence E2",
    "",
    "Rules:",
    "- Journeys are critical user paths visible in the probe or implied by the app description, not features.",
    "- Mapping paths must be real paths from the repository layout above (globs ending in /** allowed).",
    "- Write ONLY the manifest YAML to the file path given at the end.",
  ];
  if (previousDraft && errors.length) {
    lines.push("", `Round ${round}: the previous draft FAILED validation. Fix it.`, "--- DRAFT ---", previousDraft, "--- VALIDATION ERRORS ---", ...errors);
  } else {
    lines.push("", `Round ${round} of ${MAX_ROUNDS}.`);
  }
  return lines.join("\n");
}

export async function authorManifest({ appId, desc, owner = null, repositories, target, baseUrl = null, appPath = null, model = "codex", dryRun = false, withSpecs = false }) {
  if (!appId || !desc) throw new Error("authorManifest needs appId and desc");
  if (!Array.isArray(repositories) || !repositories.length) throw new Error("authorManifest needs at least one repository");
  if (target !== "web" && target !== "electron" && !appPath) throw new Error(`${target} needs --app-path`);
  if ((target === "web" || target === "electron") && !baseUrl) throw new Error(`${target} needs --base-url`);
  const resolvedOwner = owner || `${appId} maintainers`;
  const probe = target === "web" || target === "electron" ? await probeWeb(baseUrl) : await probeNative(target, appPath);
  const trees = Object.fromEntries(repositories.map((repo) => [repo, repoTree(repo)]));
  const stagedDir = path.join(ROOT, "test-results", ".author-manifest");
  mkdirSync(stagedDir, { recursive: true });
  const stagedPath = path.join(stagedDir, `${appId}.probierz.yaml`);
  let previousDraft = null;
  let errors = [];
  let lastError = null;
  for (let round = 1; round <= MAX_ROUNDS; round += 1) {
    const brief = `${buildManifestBrief({ appId, owner: resolvedOwner, desc, probe, trees, repositories, target, round, previousDraft, errors })}\n\nWrite the manifest to exactly this file: ${stagedPath}`;
    if (dryRun) return { ok: true, dryRun: true, brief, stagedPath };
    const drafted = draftWithModel(model, brief, stagedDir);
    if (!existsSync(stagedPath)) {
      return { ok: false, reason: "model did not write the manifest", agentExit: drafted.status };
    }
    previousDraft = readFileSync(stagedPath, "utf8");
    const manifestDir = path.join(APPS_ROOT, appId);
    const manifestFile = path.join(manifestDir, "probierz.yaml");
    const backupFile = path.join(stagedDir, `${appId}.probierz.yaml.bak`);
    mkdirSync(manifestDir, { recursive: true });
    const hadExisting = existsSync(manifestFile);
    if (hadExisting) writeFileSync(backupFile, readFileSync(manifestFile));
    writeFileSync(manifestFile, previousDraft);
    try {
      const written = loadAppManifest(appId);
      const result = { ok: true, appId, manifest: written.file, journeys: Object.keys(written.journeys).sort(), rounds: round, specs: [] };
      if (withSpecs) {
        for (const journey of result.journeys) {
          const goal = written.journeys[journey]?.description || journey;
          const authored = await authorSpec({
            appId, journey, target, desc: goal, baseUrl, appPath, model,
          });
          result.specs.push({ journey, ok: authored.ok === true, spec: authored.spec || null, reason: authored.reason || null });
        }
      }
      return result;
    } catch (error) {
      if (hadExisting) writeFileSync(manifestFile, readFileSync(backupFile));
      lastError = error instanceof Error ? error.message : String(error);
      errors = [lastError];
    }
  }
  return { ok: false, reason: `manifest did not validate in ${MAX_ROUNDS} rounds: ${lastError}` };
}
