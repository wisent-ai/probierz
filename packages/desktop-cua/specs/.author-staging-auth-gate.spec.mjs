import assert from 'node:assert/strict';
import {
  launchCuaApp,
  snapshotTree,
  waitForText,
  elementIndexOf,
  clickElement,
  typeText,
  quitApp,
} from '../driver.mjs';

const app = launchCuaApp({
  bundleId: process.env.CUA_BUNDLE_ID || 'ai.wisent.tama.desktop',
});

try {
  const tree = waitForText(
    app.pid,
    app.windowId,
    'Sign in with your Wisent account',
  );

  assert.match(tree, /AXStaticText = "Tama"/);
  assert.match(tree, /AXStaticText = "Sign in with your Wisent account"/);
  assert.match(tree, /AXTextField id=wisent\.auth\.screen/);
  assert.match(tree, /AXButton \(Send one-time code\)/);
  assert.match(tree, /AXButton \(Continue with Google\)/);
  assert.match(tree, /AXButton \(Continue with GitHub\)/);

  const emailField = elementIndexOf(tree, 'AXTextField id=wisent.auth.screen');
  clickElement(app.pid, app.windowId, emailField);

  const focusedTree = snapshotTree(app.pid, app.windowId);
  assert.match(focusedTree, /AXTextField id=wisent\.auth\.screen/);

  const email = 'auth-gate@example.com';
  typeText(app.pid, email, { windowId: app.windowId });

  const typedTree = waitForText(app.pid, app.windowId, email);
  assert.ok(
    typedTree.includes(email),
    'the email field should expose the typed email in the accessibility tree',
  );
} finally {
  quitApp(app.pid);
}
