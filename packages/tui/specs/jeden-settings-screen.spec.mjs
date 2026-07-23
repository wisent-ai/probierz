import assert from 'node:assert/strict';
import { spawnTui } from '../pty.mjs';

const app = spawnTui(process.env.TUI_CMD || 'jeden');

try {
  const initialScreen = await app.waitFor('Welcome back!');
  assert.match(initialScreen, /Wisent Agent/);

  app.send('/settings');
  app.key('enter');

  const settingsScreen = await app.waitFor('── secrets (');
  for (const group of ['tools', 'commands', 'startup', 'secrets']) {
    assert.ok(
      settingsScreen.includes(`── ${group} (`),
      `expected the settings screen to show the ${group} group header`,
    );
  }
} finally {
  await app.close();
}
