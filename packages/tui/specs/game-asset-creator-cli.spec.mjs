// Probierz TUI journey for game_asset_creator: the pipeline CLI must
// work end-to-end against vault-backed config without a real vault,
// and the GLB quality gate must give the right verdicts from the CLI.
import assert from 'node:assert/strict';
import { spawnTui } from '../pty.mjs';
import { buildFixtures } from '../../../apps/game-asset-creator/fixtures.mjs';

const GAC_ROOT = process.env.GAC_ROOT ?? '/Users/lukaszbartoszcze/work/game_asset_creator';
const fixtures = await buildFixtures();

const shellReady = '__GAC_TUI_READY__';
const app = spawnTui(
  '/bin/sh',
  ['-c', `stty -echo; printf '${shellReady}\\n'; exec /bin/sh`],
  {
    env: {
      PATH: `${fixtures.dir}:${process.env.PATH}`,
      SKARBIEC_BIN: fixtures.fakeSkarbiec,
    },
    cwd: GAC_ROOT,
  },
);

let commandNumber = 0;
const run = async (command, { timeoutMs = 30_000, expectExit } = {}) => {
  commandNumber += 1;
  const marker = `__GAC_TUI_CMD_${commandNumber}_DONE__`;
  const logStart = app.fullLog().length;
  app.send(`${command}; printf "\\n${marker} exit=$?\\n"`);
  app.key('enter');
  await app.waitFor(marker, { timeoutMs, useFullLog: true });
  const output = app.fullLog().slice(logStart);
  const exitMatch = new RegExp(`${marker.replaceAll('$', '\\$')} exit=(\\d+)`).exec(output);
  const exitCode = exitMatch ? Number(exitMatch[1]) : null;
  if (expectExit !== undefined) {
    assert.equal(exitCode, expectExit, `expected exit ${expectExit} from: ${command}\n${output}`);
  }
  return output.slice(0, output.indexOf(marker));
};

const extractJson = (output) => {
  const start = output.indexOf('{');
  assert.notEqual(start, -1, `expected JSON in output:\n${output}`);
  // brace-match from the first '{'
  let depth = 0;
  for (let i = start; i < output.length; i += 1) {
    if (output[i] === '{') depth += 1;
    if (output[i] === '}') depth -= 1;
    if (depth === 0) return JSON.parse(output.slice(start, i + 1));
  }
  assert.fail(`unbalanced JSON in output:\n${output}`);
};

try {
  await app.waitFor(shellReady, { useFullLog: true });

  // ---- journey: cli — config resolves through the (fake) vault ----
  const checkOut = await run(`node pipeline/cli.js check-config --config ${fixtures.config}`, {
    expectExit: 0,
  });
  const checked = extractJson(checkOut);
  assert.equal(checked.credentials, '<resolved: ok>');
  assert.equal(checked.browser.headless, true);

  // ---- journey: verify — structural gate verdicts from the CLI ----
  const validOut = await run(`node pipeline/cli.js verify ${fixtures.valid} --config ${fixtures.config}`, {
    expectExit: 0,
  });
  const validReport = extractJson(validOut);
  assert.equal(validReport.ok, true);
  assert.equal(validReport.stats.triangles, 6000);

  const overOut = await run(`node pipeline/cli.js verify ${fixtures.overBudget} --config ${fixtures.config}`, {
    expectExit: 1,
  });
  const overReport = extractJson(overOut);
  assert.equal(overReport.ok, false);
  assert.ok(overReport.errors.some((e) => e.includes('triangle budget')));

  const corruptOut = await run(`node pipeline/cli.js verify ${fixtures.corrupt} --config ${fixtures.config}`, {
    expectExit: 1,
  });
  assert.match(corruptOut, /not a GLB|bad magic|too small/i);

  // ---- cli — the package's own MCP surface lists all six tools ----
  const mcpOut = await run(
    `printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\\n{"jsonrpc":"2.0","id":2,"method":"tools/list"}\\n' | node pipeline/mcp.js`,
  );
  const toolsLine = mcpOut
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('{') && line.includes('"tools"'))
    .pop();
  const toolsResult = JSON.parse(toolsLine);
  const toolNames = toolsResult.result.tools.map((t) => t.name).sort();
  assert.deepEqual(toolNames, [
    'gac_blender_health',
    'gac_check_config',
    'gac_create_asset',
    'gac_sculpt',
    'gac_verify_asset',
    'gac_weles_tools',
  ]);

  // ---- cli — blender-health reports cleanly when Blender is absent ----
  const healthOut = await run('node pipeline/cli.js blender-health', { expectExit: 1 });
  assert.match(healthOut, /failed to start|healthy"?:\s*false|setup/i);

  console.log('game-asset-creator cli+verify journeys: OK');
} finally {
  app.close?.();
}
