import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { adoptProject } from "./project-adoption.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEFINITION = path.join(HERE, "onboarding_first_use.json");

function stateFile() {
  const root = process.env.XDG_STATE_HOME?.trim()
    ? process.env.XDG_STATE_HOME.trim()
    : path.join(os.homedir(), ".local", "state");
  return path.join(root, "probierz", "onboarding.json");
}

function readDefinition() {
  return JSON.parse(readFileSync(DEFINITION, "utf8"));
}

function readProgress() {
  try {
    return JSON.parse(readFileSync(stateFile(), "utf8"));
  } catch {
    return null;
  }
}

function writeProgress(progress) {
  const file = stateFile();
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(progress, null, 2)}\n`, { mode: 0o600 });
}

function screensInOrder(definition) {
  const byId = new Map(definition.screens.map((screen) => [screen.screen_id, screen]));
  const ordered = [];
  let current = byId.get(definition.entry_screen_id);
  while (current && !ordered.includes(current)) {
    ordered.push(current);
    const next = [...current.transitions].sort((left, right) => left.priority - right.priority)[0];
    current = next ? byId.get(next.next_screen_id) : undefined;
  }
  return ordered;
}

function flagsFor(argv) {
  const flags = { reset: false, json: false, replace: false, sourceRoot: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--reset") flags.reset = true;
    else if (arg === "--json") flags.json = true;
    else if (arg === "--replace") flags.replace = true;
    else if (arg === "--source") {
      flags.sourceRoot = argv[index + 1];
      if (!flags.sourceRoot || flags.sourceRoot.startsWith("--")) {
        const error = new Error("--source needs a repository path");
        error.exitCode = 2;
        throw error;
      }
      index += 1;
    } else {
      const error = new Error(`unknown onboarding option: ${arg}`);
      error.exitCode = 2;
      throw error;
    }
  }
  if (flags.replace && !flags.sourceRoot) {
    const error = new Error("--replace requires --source <repository>");
    error.exitCode = 2;
    throw error;
  }
  return flags;
}

function emitLine(value) {
  process.stdout.write(`${value}\n`);
}

/**
 * Record the journey's first success at the point a passing run's evidence
 * block has been written to its manifest. Progress must never break evidence
 * persistence, which remains the product operation.
 */
export function recordPassingQualityEvidenceWritten() {
  try {
    const progress = readProgress() ?? {};
    if (progress.status === "completed") return;
    const definition = readDefinition();
    writeProgress({
      ...progress,
      product_id: definition.product_id,
      journey_id: definition.journey_id,
      journey_version: definition.journey_version,
      status: "completed",
      evidence: {
        ...(progress.evidence ?? {}),
        [definition.first_success_fact]: true,
      },
      completed_at: new Date().toISOString(),
    });
  } catch {
    // Walkthrough progress is a convenience; writing run evidence is the product.
  }
}

export function runOnboarding(argv, projectRoot) {
  const flags = flagsFor(argv);
  const definition = readDefinition();
  const screens = screensInOrder(definition);
  const adoption = flags.sourceRoot
    ? adoptProject({ projectRoot, sourceRoot: flags.sourceRoot, replace: flags.replace })
    : null;

  let progress = readProgress();
  let reset = false;
  if (flags.reset && progress) {
    progress = null;
    reset = true;
    writeProgress({
      product_id: definition.product_id,
      journey_id: definition.journey_id,
      journey_version: definition.journey_version,
      status: "in_progress",
      evidence: {},
      started_at: new Date().toISOString(),
    });
  } else if (!progress) {
    writeProgress({
      product_id: definition.product_id,
      journey_id: definition.journey_id,
      journey_version: definition.journey_version,
      status: "in_progress",
      evidence: {},
      started_at: new Date().toISOString(),
    });
  }

  if (adoption && adoption.status !== "conflict") {
    const adoptedProgress = readProgress() ?? {};
    writeProgress({
      ...adoptedProgress,
      product_id: definition.product_id,
      journey_id: definition.journey_id,
      journey_version: definition.journey_version,
      status: adoptedProgress.status ?? "in_progress",
      evidence: {
        ...(adoptedProgress.evidence ?? {}),
        project_definitions_adopted: true,
      },
      adoption: {
        source_root: adoption.sourceRoot,
        source_digest: adoption.sourceDigest,
        accepted_at: new Date().toISOString(),
      },
    });
    progress = readProgress();
  }

  const done = progress?.status === "completed";

  if (flags.json) {
    process.stdout.write(`${JSON.stringify({
      product_id: definition.product_id,
      journey_id: definition.journey_id,
      journey_version: definition.journey_version,
      source_revision: definition.source_revision,
      first_success_fact: definition.first_success_fact,
      status: done ? "completed" : "in_progress",
      reset,
      adoption,
      screens: screens.map((screen) => ({
        screen_id: screen.screen_id,
        title: screen.presentation?.title ?? screen.title_key,
        body: screen.presentation?.body ?? screen.body_key,
        command: screen.presentation?.command ?? null,
      })),
    }, null, 2)}\n`);
    return;
  }

  if (reset) {
    emitLine("First-run walkthrough reset: walkthrough progress and its first-success evidence discarded, showing it again now.");
    emitLine("");
  }
  if (adoption) {
    if (adoption.status === "conflict") {
      emitLine(`Existing project not adopted: ${adoption.conflicting} conflicting definition(s). No files changed.`);
      for (const item of adoption.conflicts) emitLine(`       ${item.path}: ${item.reason}`);
      emitLine("       Resolve the files or repeat with --replace after reviewing the conflicts.");
      process.exitCode = 1;
    } else {
      emitLine(`Existing project ${adoption.status}: ${adoption.imported} imported, ${adoption.unchanged} unchanged, ${adoption.removed} removed.`);
      emitLine("       Journey definitions were persisted but not run.");
    }
    emitLine("");
  }
  for (const [index, screen] of screens.entries()) {
    emitLine(`${index + 1}/${screens.length}  ${screen.presentation?.title ?? screen.title_key}`);
    emitLine(`       ${screen.presentation?.body ?? screen.body_key}`);
    if (screen.presentation?.command) emitLine(`       $ ${screen.presentation.command}`);
    emitLine("");
  }
  emitLine(
    done
      ? `First-run journey already complete: ${definition.first_success_fact} was observed on an earlier run.`
      : `No passing quality evidence written from this shell yet, so ${definition.first_success_fact} is still open; the next passing completed run closes it.`,
  );
}
