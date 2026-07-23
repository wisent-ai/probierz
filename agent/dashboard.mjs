import { loadAppManifest } from "./apps.mjs";
import { runHistory } from "./history.mjs";

function versionKey(run) {
  return run.build?.sha256 || run.harness?.sha256 || "unknown";
}

function deviceKey(run) {
  const name = run.device?.name || "host";
  const runtime = run.device?.runtime || "default";
  return `${name}:${runtime}`;
}

function artifactProjection(run) {
  return run.artifacts.map((artifact) => ({ file: artifact.file, sha256: artifact.sha256 || null, bytes: Number(artifact.bytes || 0) }));
}

function resultProjection(run) {
  if (!run) return null;
  return {
    runId: run.runId,
    status: run.status,
    startedAt: run.startedAt,
    completedAt: run.completedAt,
    durationMs: run.durationMs,
    evidence: run.evidence,
    artifacts: artifactProjection(run),
  };
}

export function dashboardProjection({ appId, limit = 500 } = {}) {
  if (!appId) throw new Error("appId is required");
  const manifest = loadAppManifest(appId);
  const history = runHistory({ appId, limit });
  const versions = new Map();
  for (const run of history.runs) {
    const key = versionKey(run);
    if (!versions.has(key)) {
      versions.set(key, {
        version: key,
        harness: run.harness,
        source: run.source,
        build: run.build,
        latestAt: run.startedAt,
        runs: [],
      });
    }
    versions.get(key).runs.push(run);
  }
  const projectedVersions = [...versions.values()].map((version) => {
    const journeys = Object.entries(manifest.journeys).map(([journeyId, journey]) => {
      const surfaces = Object.entries(manifest.surfaces)
        .filter(([, surface]) => surface.journeys.includes(journeyId))
        .map(([target]) => {
          const relevant = version.runs.filter((run) => run.target === target && run.journeys.includes(journeyId));
          const devices = new Map();
          for (const run of relevant) {
            const key = deviceKey(run);
            if (!devices.has(key)) devices.set(key, []);
            devices.get(key).push(run);
          }
          return {
            target,
            status: relevant[0]?.status || "missing",
            latest: resultProjection(relevant[0]),
            devices: [...devices.entries()].map(([device, runs]) => ({
              device,
              status: runs[0].status,
              latest: resultProjection(runs[0]),
              runs: runs.map(resultProjection),
            })).sort((left, right) => left.device.localeCompare(right.device)),
          };
        })
        .sort((left, right) => left.target.localeCompare(right.target));
      const statuses = surfaces.map((surface) => surface.status);
      const status = statuses.length && statuses.every((value) => value === "passed")
        ? "passed"
        : (statuses.some((value) => value === "failed") ? "failed" : "incomplete");
      return {
        journey: journeyId,
        owner: journey.owner,
        status,
        surfaces,
      };
    }).sort((left, right) => left.journey.localeCompare(right.journey));
    return {
      version: version.version,
      harness: version.harness,
      source: version.source,
      build: version.build,
      latestAt: version.latestAt,
      status: journeys.length && journeys.every((journey) => journey.status === "passed")
        ? "passed"
        : (journeys.some((journey) => journey.status === "failed") ? "failed" : "incomplete"),
      journeys,
    };
  });
  return {
    schemaVersion: 2,
    generatedAt: new Date().toISOString(),
    product: { appId, owner: manifest.owner, manifest: manifest.file },
    requirements: Object.entries(manifest.journeys).map(([journey, detail]) => ({
      journey,
      owner: detail.owner,
      surfaces: Object.entries(manifest.surfaces)
        .filter(([, surface]) => surface.journeys.includes(journey))
        .map(([target]) => target)
        .sort(),
    })).sort((left, right) => left.journey.localeCompare(right.journey)),
    summary: {
      versions: projectedVersions.length,
      runs: history.summary.runs,
      latestRunId: history.summary.latestRunId,
      lastGreenRunId: history.summary.lastGreenRunId,
    },
    versions: projectedVersions,
  };
}
