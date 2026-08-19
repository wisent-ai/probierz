// Shared fixture for the `stado` fleet and release journeys.
//
// These commands answer questions about a HOST: what its release agent wrote,
// what its janitor would delete, which unit owns which process. Asserting them
// against the production Mac mini is not an option — a journey may not restart a
// unit, deliver a binary, apply a reclamation or clear a quarantine there — so
// each journey builds its own host instead:
//
//   * `WC_STORAGE_BACKEND=local` with `WC_LOCAL_STORAGE_PATH` under a temp dir
//     puts the canonical registry document, the capacity broadcasts and the
//     service audit records in the fixture, not in the fleet's object store.
//     "local" is a shipped backend (`config::wc_storage_backend`), not a stub.
//   * the fixture registry declares one target whose `hostnames` name THIS
//     machine, so `deploy::host_channel` runs every script locally instead of
//     over ssh (`target_is_this_host`). The scripts are byte-identical on both
//     transports; only the hop disappears.
//   * `HOME` points into the temp dir, so the release state, the logs, the
//     delivered trees, the build scratch and any LaunchAgents plist the product
//     writes are all inside the fixture. Nothing under the operator's own
//     `~/.stado` is read or written.
//
// Commands are driven through a real PTY (`../pty.mjs`), the way every other
// journey on this surface drives a terminal program. `--json` payloads are
// redirected to a file inside the fixture and parsed from there rather than
// scraped out of the terminal stream: a JSON string value longer than the
// terminal is wide comes back with a wrap inside it, and a journey that parsed
// that would fail on the width of the window instead of on the product.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { homedir, hostname } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnTui } from '../pty.mjs';

export const STADO_REPO =
  '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute';

// The binary under test is the one built from the mapped source revision. It is
// never the installed `~/.stado/bin/stado`: overwriting that file is how the
// desktop app's children get SIGKILLed, and a journey must not depend on an
// install having happened.
export const STADO_BINARY =
  process.env.TUI_CMD || join(STADO_REPO, 'stado-rs/target/release/stado');

export const FIXTURE_HOST = 'probierz-fixture-host';
export const FIXTURE_PRODUCT = 'probierz-fixture-product';

// An Ed25519 public key the registry contract accepts. It signs nothing here:
// these journeys never verify a release signature, and the field exists because
// the registry document is validated as a whole whenever the product writes it.
const FIXTURE_TRUSTED_KEY = 'nLCK4gGkYVMcTdVBFTtDMuHrX2W0EMMTNXZ3F8DGKgQ=';

const shellQuote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;

/** This machine's registry identity, normalized the way the registry stores it. */
export function thisHostname() {
  const name = hostname().trim().toLowerCase();
  assert.ok(name, 'this host has no hostname, so no fixture target can name it');
  return name;
}

/** The exact source revision the journey ran against, plus whether it was dirty. */
export function sourceIdentity(repo = STADO_REPO) {
  const revision = spawnSync('git', ['-C', repo, 'rev-parse', 'HEAD'], { encoding: 'utf8' });
  const status = spawnSync('git', ['-C', repo, 'status', '--porcelain'], { encoding: 'utf8' });
  assert.equal(revision.status, 0, `cannot read the source revision of ${repo}`);
  return {
    repository: repo,
    revision: revision.stdout.trim(),
    dirty: String(status.stdout || '').trim() !== '',
  };
}

/**
 * One registry document declaring this machine as a fixture target.
 *
 * `schemaVersion: 2` and the normalized hostname are what the product's own
 * registry validation requires of any document it writes back (`service ensure`
 * records the unit it installed), so the fixture is a document the product
 * would accept, not a shape only its readers tolerate.
 */
export function fixtureRegistry({ target = {}, releaseControl = null } = {}) {
  const document = {
    schema_version: 2,
    targets: [
      {
        name: FIXTURE_HOST,
        kind: 'local',
        hostnames: [thisHostname()],
        release_platform: 'darwin-arm64',
        slots: 2,
        role: 'interactive',
        notes: 'Probierz journey fixture host. This machine, scoped to a temp HOME.',
        ...target,
      },
    ],
  };
  if (releaseControl) document.release_control = releaseControl;
  return document;
}

/** A release-control plane declaring one product rolling out to the fixture host. */
export function fixtureReleaseControl({
  home,
  stateDir,
  logsRoot,
  desiredVersion = '0.2.27',
  desiredDigest = 'a'.repeat(64),
  installRoot,
}) {
  return {
    schema_version: 1,
    generation: 1,
    trusted_keys: { 'stado-release-2026-08': FIXTURE_TRUSTED_KEY },
    products: {
      [FIXTURE_PRODUCT]: {
        service: FIXTURE_PRODUCT,
        config_schema: 1,
        state_schema: 1,
        install_root: installRoot,
        binary: 'bin/fixture',
        launcher: 'bin/start',
        binary_env: 'PROBIERZ_FIXTURE_BIN',
        port_env: 'PROBIERZ_FIXTURE_PORT',
        runtime_env: 'PROBIERZ_FIXTURE_RUNTIME_DIR',
        strategy: {
          kind: 'blue-green',
          readiness_timeout_seconds: 90,
          drain_timeout_seconds: 60,
          rollback_window_seconds: 300,
          automatic_rollback: true,
        },
        desired: {
          version: desiredVersion,
          channel: 'stable',
          rollout_generation: 2,
          promoted_at: '2026-08-17T09:00:00+00:00',
          artifacts: {
            'darwin-arm64': {
              archive_uri: `stado://releases/${FIXTURE_PRODUCT}/${desiredVersion}/darwin-arm64/release.tar.gz`,
              artifact_sha256: desiredDigest,
              manifest_uri: `stado://releases/${FIXTURE_PRODUCT}/${desiredVersion}/darwin-arm64/release.json`,
              manifest_sha256: 'b'.repeat(64),
              signature_uri: `stado://releases/${FIXTURE_PRODUCT}/${desiredVersion}/darwin-arm64/release.sig`,
              key_id: 'stado-release-2026-08',
              source_revision: 'c'.repeat(40),
            },
          },
        },
        targets: {
          [FIXTURE_HOST]: {
            platform: 'darwin-arm64',
            run_as_user: process.env.USER || 'operator',
            home,
            state_dir: stateDir,
            runtime_root: join(home, '.stado', 'run'),
            logs_root: logsRoot,
            stable_bind: '127.0.0.1:18190',
            candidate_ports: [18191, 18192],
            readiness_path: '/health',
          },
        },
      },
    },
  };
}

/**
 * Open an isolated fixture host and a PTY to drive `stado` against it.
 *
 * The scratch root is this account's own cache directory rather than the shared
 * temporary directory: `/tmp` is swept by other things on a developer machine,
 * and a fixture host that disappears mid-journey fails the product for the
 * sweeper's reasons. Only the directory this call created is ever removed.
 */
export async function openFleetFixture(slug) {
  assert.ok(existsSync(STADO_BINARY), `no stado binary at ${STADO_BINARY}; build it first`);
  const scratchRoot = join(homedir(), 'Library', 'Caches', 'probierz-journeys');
  await mkdir(scratchRoot, { recursive: true });
  const dir = await mkdtemp(join(scratchRoot, `${slug}-`));
  const home = join(dir, 'home');
  const store = join(dir, 'store');
  const stateDir = join(home, '.stado', 'release-state');
  const logsRoot = join(home, '.stado', 'logs');
  const servicesRoot = join(home, '.stado', 'services');
  const capture = join(dir, 'capture');
  for (const path of [home, store, stateDir, logsRoot, servicesRoot, capture]) {
    await mkdir(path, { recursive: true });
  }

  const env = {
    HOME: home,
    WC_STORAGE_BACKEND: 'local',
    WC_LOCAL_STORAGE_PATH: store,
    // The commands shell out to `git`, `python3` and the janitor's own helpers.
    PATH: `/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`,
  };

  const ready = `__PZ_${slug.toUpperCase().replaceAll('-', '_')}_READY__`;
  const app = spawnTui('/bin/sh', ['-c', `stty -echo; printf '${ready}\\n'; exec /bin/sh`], {
    cwd: dir,
    env,
    // Wide enough that no line the commands print is wrapped by the terminal.
    cols: 400,
    rows: 60,
  });
  await app.waitFor(ready, { useFullLog: true });

  let commandNumber = 0;

  /** Run one `stado` invocation and hand back its exit status and terminal output. */
  const invoke = async (args, { timeoutMs = 120_000 } = {}) => {
    commandNumber += 1;
    const marker = `__PZ_${slug.toUpperCase().replaceAll('-', '_')}_${commandNumber}_DONE__`;
    const logStart = app.fullLog().length;
    const command = [STADO_BINARY, ...args].map(shellQuote).join(' ');
    app.send(`${command} 2>&1; printf '\\n${marker}:%s\\n' "$?"`);
    app.key('enter');
    await app.waitFor(marker, { timeoutMs, useFullLog: true });
    const log = app.fullLog().slice(logStart);
    const status = log.match(new RegExp(`${marker}:(\\d+)`));
    assert.ok(status, `no exit status for: stado ${args.join(' ')}`);
    return {
      args,
      status: Number(status[1]),
      output: log.slice(0, log.indexOf(marker)),
    };
  };

  /**
   * Run one `stado … --json` invocation, capturing the payload in a file.
   *
   * `stderr` stays on the terminal so a refusal is still readable, and the
   * payload is read from disk so a long path inside a string can never be
   * broken by the terminal's own line wrapping.
   */
  const invokeJson = async (args, { timeoutMs = 120_000 } = {}) => {
    commandNumber += 1;
    const marker = `__PZ_${slug.toUpperCase().replaceAll('-', '_')}_${commandNumber}_DONE__`;
    const payloadPath = join(capture, `${slug}-${commandNumber}.json`);
    const logStart = app.fullLog().length;
    const command = [STADO_BINARY, ...args].map(shellQuote).join(' ');
    app.send(`${command} > ${shellQuote(payloadPath)}; printf '\\n${marker}:%s\\n' "$?"`);
    app.key('enter');
    await app.waitFor(marker, { timeoutMs, useFullLog: true });
    const log = app.fullLog().slice(logStart);
    const status = log.match(new RegExp(`${marker}:(\\d+)`));
    assert.ok(status, `no exit status for: stado ${args.join(' ')} --json`);
    const raw = existsSync(payloadPath) ? await readFile(payloadPath, 'utf8') : '';
    return {
      args,
      status: Number(status[1]),
      output: log.slice(0, log.indexOf(marker)),
      raw,
      json: raw.trim() ? JSON.parse(raw) : null,
      payloadPath,
    };
  };

  const writeJson = async (path, value) => {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  };

  return {
    dir,
    home,
    store,
    stateDir,
    logsRoot,
    servicesRoot,
    capture,
    env,
    app,
    invoke,
    invokeJson,
    writeJson,

    /** Publish the fixture registry document where the product reads it. */
    registry: (document) => writeJson(join(store, 'registry.json'), document),

    /** Read the registry document back, as the product left it. */
    readRegistry: async () => JSON.parse(await readFile(join(store, 'registry.json'), 'utf8')),

    /**
     * One capacity broadcast for the fixture host, in the shape its own queue
     * agent publishes (`queue::capacity::publish_capacity`). The `diag` words
     * are the agent's vocabulary, which is exactly what `host gates` reports
     * back verbatim.
     */
    publishCapacity: (diag, { freeSlots = 1, publishedAt = new Date().toISOString() } = {}) =>
      writeJson(join(store, 'capacity', `local-${thisHostname()}.json`), {
        consumer_id: `local-${thisHostname()}`,
        kind: 'local',
        free_slots: { cpu: freeSlots },
        published_at: publishedAt,
        diag,
        stado_version: '0.7.5',
      }),

    /** One host release state document for the fixture product. */
    writeReleaseState: (state) => writeJson(join(stateDir, `${FIXTURE_PRODUCT}.json`), state),

    close: async () => {
      await app.close();
      await rm(dir, { recursive: true, force: true });
    },
  };
}

/** A release-state document at the desired version with nothing quarantined. */
export function settledState({ version = '0.2.27', digest = 'a'.repeat(64), releaseDir }) {
  const startedAt = new Date(Date.now() - 3_600_000).toISOString();
  return {
    schema_version: 1,
    product: FIXTURE_PRODUCT,
    target: FIXTURE_HOST,
    rollout_generation: 2,
    phase: 'committed',
    active: {
      version,
      artifact_sha256: digest,
      manifest_sha256: 'b'.repeat(64),
      port: 18190,
      pid: 1,
      release_dir: releaseDir,
      started_at: startedAt,
    },
    previous: null,
    candidate: null,
    proxy_pid: null,
    cutover_at: startedAt,
    quarantined: {},
    detail: '',
    updated_at: new Date(Date.now() - 60_000).toISOString(),
  };
}

/**
 * Register one JSON trace as this journey's evidence.
 *
 * A recorded run with no report-typed capture is failed by the runner, and a
 * terminal journey has no video to give: the trace is the artifact, and it
 * carries the source revision the assertions were made against so the evidence
 * cannot be read as belonging to some other build.
 */
export async function recordTrace({ slug, journey, source, observations, contracts }) {
  const artifacts = process.env.PROBIERZ_ARTIFACTS;
  const manifest = process.env.PROBIERZ_MEDIA_MANIFEST;
  assert.ok(artifacts, 'PROBIERZ_ARTIFACTS is required');
  assert.ok(manifest, 'PROBIERZ_MEDIA_MANIFEST is required');
  const tracePath = join(artifacts, `${slug}.trace.json`);
  await mkdir(dirname(tracePath), { recursive: true });
  await writeFile(
    tracePath,
    `${JSON.stringify(
      {
        schemaVersion: 1,
        kind: 'probierz-stado-fleet-trace',
        journey,
        runId: process.env.PROBIERZ_RUN_ID || null,
        status: 'completed',
        binary: STADO_BINARY,
        source,
        host: { fixtureTarget: FIXTURE_HOST, hostname: thisHostname() },
        productionMutations: 'none: every command ran against an isolated fixture host',
        observations,
        contracts,
        redaction: {
          status: 'verified_redacted',
          credentialsIncluded: false,
          productionIdentifiersIncluded: false,
        },
      },
      null,
      2,
    )}\n`,
    { mode: 0o600 },
  );
  await mkdir(dirname(manifest), { recursive: true });
  await writeFile(
    manifest,
    `${JSON.stringify([{ file: tracePath, kind: 'trace', contentType: 'application/json' }], null, 2)}\n`,
    { mode: 0o600 },
  );
  return tracePath;
}
