import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
  const flags = { reset: false, json: false };
  for (const arg of argv) {
    if (arg === "--reset") flags.reset = true;
    else if (arg === "--json") flags.json = true;
    else {
      const error = new Error(`unknown onboarding option: ${arg}`);
      error.exitCode = 2;
      throw error;
    }
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
      evidence: { [definition.first_success_fact]: true },
      completed_at: new Date().toISOString(),
    });
  } catch {
    // Walkthrough progress is a convenience; writing run evidence is the product.
  }
}

export function runOnboarding(argv) {
  const flags = flagsFor(argv);
  const definition = readDefinition();
  const screens = screensInOrder(definition);

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
