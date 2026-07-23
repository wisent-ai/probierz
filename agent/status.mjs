// Coverage/thoroughness status for one app: which manifest journeys have
// evidence at all, how fresh it is against the current HEAD, how strong it is
// (evidence levels), which journeys the current base..HEAD diff affects, and
// whether the pull-request policy would let HEAD merge right now. Pure
// composition of the app manifest, run history, affected-mappings, and git.
import { spawnSync } from "node:child_process";
import path from "node:path";
import { affectedAppJourneys, loadAppManifest } from "./apps.mjs";
import { gateStatus } from "./gate.mjs";
import { runHistory } from "./history.mjs";

const LEVEL = { E0: 0, E1: 1, E2: 2, E3: 3 };

// Mirrors the evidence level computation in receipt.mjs.
function evidenceLevel(run) {
  if (run.status !== "passed") return "E0";
  if (run.conditions?.record && run.evidence?.report && run.evidence?.analysis && run.evidence?.capturePresent) return "E3";
  return "E2";
}

function git(root, args) {
  const out = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  if (out.status !== 0) return null;
  return out.stdout.trim() || null;
}

function gitLines(root, args) {
  const out = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  if (out.status !== 0) return [];
  return out.stdout.split("\n").map((line) => line.trim()).filter(Boolean);
}

export function appStatus({ appId, baseRef = "origin/main", historyLimit = 1000 } = {}) {
  const manifest = loadAppManifest(appId);
  const history = runHistory({ appId, limit: historyLimit });
  const gates = gateStatus(appId);

  const repositories = manifest.repositories.map((repository) => ({
    root: repository.root,
    headSha: git(repository.root, ["rev-parse", "HEAD"]),
    baseSha: git(repository.root, ["rev-parse", "--verify", baseRef]),
  }));

  const diffFiles = repositories.flatMap((repository) => (
    repository.baseSha && repository.headSha
      ? gitLines(repository.root, ["diff", "--name-only", `${repository.baseSha}..${repository.headSha}`])
        .map((file) => path.join(repository.root, file))
      : []
  ));
  const affected = affectedAppJourneys(diffFiles);
  const affectedJourneys = [...new Set(affected.flatMap((match) => match.journeys))].sort();

  const latestByJourney = new Map();
  for (const run of history.runs) {
    for (const journey of run.journeys) {
      if (!latestByJourney.has(journey)) latestByJourney.set(journey, run);
    }
  }

  const journeys = Object.keys(manifest.journeys).sort().map((journey) => {
    const run = latestByJourney.get(journey) || null;
    const recordedShas = new Map((run?.source?.repositories || []).map((repo) => [repo.name || repo.root, repo.gitSha]));
    const fresh = run !== null && repositories.every((repository) => {
      const names = [...recordedShas.keys()];
      const key = names.find((name) => repository.root.endsWith(name) || name === repository.root);
      return key && recordedShas.get(key) === repository.headSha;
    });
    return {
      journey,
      lastRun: run ? {
        runId: run.runId,
        target: run.target,
        status: run.status,
        startedAt: run.startedAt,
        evidenceLevel: evidenceLevel(run),
      } : null,
      fresh,
      affected: affectedJourneys.includes(journey),
    };
  });

  const minimumEvidence = manifest.pullRequestPolicy?.minimumEvidence || "E2";
  const evaluated = journeys.filter((journey) => journey.affected);
  const blockingReasons = evaluated.flatMap((journey) => {
    const reasons = [];
    if (!journey.lastRun) reasons.push(`${journey.journey}: no runs recorded`);
    else {
      if (!journey.lastRun.status || journey.lastRun.status !== "passed") reasons.push(`${journey.journey}: last run is ${journey.lastRun.status}`);
      if (!journey.fresh) reasons.push(`${journey.journey}: evidence is older than HEAD`);
      if (LEVEL[journey.lastRun.evidenceLevel] < LEVEL[minimumEvidence]) {
        reasons.push(`${journey.journey}: ${journey.lastRun.evidenceLevel} is below ${minimumEvidence}`);
      }
    }
    return reasons;
  });

  return {
    schemaVersion: 1,
    appId,
    generatedAt: new Date().toISOString(),
    baseRef,
    repositories,
    journeys,
    untested: journeys.filter((journey) => !journey.lastRun).map((journey) => journey.journey),
    affectedJourneys,
    mergeEligibility: {
      mode: "pull-request",
      minimumEvidence,
      evaluatedJourneys: evaluated.map((journey) => journey.journey),
      blockingReasons,
      eligible: blockingReasons.length === 0,
      gate: gates.modes?.["pull-request"] || null,
    },
  };
}

export function renderAppStatus(status) {
  const lines = [`app: ${status.appId}`];
  lines.push("  journeys:");
  for (const journey of status.journeys) {
    if (!journey.lastRun) {
      lines.push(`    ${journey.journey.padEnd(24)} — no runs —  E0  untested`);
      continue;
    }
    const run = journey.lastRun;
    const date = String(run.startedAt || "").slice(0, 10);
    lines.push(`    ${journey.journey.padEnd(24)} ${date}  ${run.runId}  ${run.evidenceLevel}  ${journey.fresh ? "fresh" : "stale"}`);
  }
  const eligibility = status.mergeEligibility;
  lines.push(`  affected (${status.baseRef}..HEAD): ${status.affectedJourneys.join(", ") || "(none)"}`);
  lines.push(`  merge-eligibility(${eligibility.mode}, min ${eligibility.minimumEvidence}): ${eligibility.eligible ? "ELIGIBLE" : "BLOCKED"}`);
  for (const reason of eligibility.blockingReasons) lines.push(`    - ${reason}`);
  return lines.join("\n");
}
