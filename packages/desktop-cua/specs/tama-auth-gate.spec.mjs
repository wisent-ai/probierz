import assert from "node:assert/strict";
import {
  elementIndexOf,
  launchCuaApp,
  quitApp,
  snapshotTree,
  typeText,
  waitForText,
} from "../driver.mjs";

const app = launchCuaApp({
  bundleId: process.env.CUA_BUNDLE_ID || "ai.wisent.tama.desktop",
});

try {
  const gateTree = waitForText(
    app.pid,
    app.windowId,
    "AXButton (Continue with GitHub)",
  );

  assert.match(gateTree, /AXStaticText = "Tama" id=wisent\.auth\.screen/);
  assert.match(
    gateTree,
    /AXStaticText = "Sign in with your Wisent account" id=wisent\.auth\.screen/,
  );
  assert.match(gateTree, /AXTextField id=wisent\.auth\.screen/);
  assert.match(
    gateTree,
    /AXButton \(Send one-time code\) id=wisent\.auth\.screen/,
  );
  assert.match(
    gateTree,
    /AXButton \(Continue with Google\) id=wisent\.auth\.screen/,
  );
  assert.match(
    gateTree,
    /AXButton \(Continue with GitHub\) id=wisent\.auth\.screen/,
  );

  const inputTree = waitForText(
    app.pid,
    app.windowId,
    "AXTextField id=wisent.auth.screen",
  );
  const email = "auth-gate@example.com";
  typeText(app.pid, email, {
    windowId: app.windowId,
    elementIndex: elementIndexOf(inputTree, "AXTextField id=wisent.auth.screen"),
  });

  const typedTree = snapshotTree(app.pid, app.windowId);
  assert.match(
    typedTree,
    /AXTextField(?: = "auth-gate@example\.com")? id=wisent\.auth\.screen/,
  );
  assert.match(
    typedTree,
    /auth-gate@example\.com/,
    "the email field should expose the typed email in the accessibility tree",
  );
} finally {
  quitApp(app.pid);
}
