import assert from "node:assert/strict";
import {
  launchCuaProcess,
  quitApp,
  selectSidebarRow,
  waitForText,
} from "../driver.mjs";

const app = launchCuaProcess({
  executable:
    process.env.CUA_APP_EXECUTABLE
    || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/tama-desktop/.build/Tama.app/Contents/MacOS/Tama",
  env: { TAMA_TEST_IDENTITY: "1" },
});

try {
  waitForText(
    app.pid,
    app.windowId,
    'AXStaticText = "Snapshot validation"',
    30_000,
  );

  selectSidebarRow(app.pid, app.windowId, 3);

  const validationTree = waitForText(
    app.pid,
    app.windowId,
    'AXStaticText = "Structurally valid"',
    30_000,
  );

  assert.match(
    validationTree,
    /AXStaticText = "Snapshot validation"/,
    "the Snapshot validation panel should be open",
  );
  assert.match(
    validationTree,
    /AXStaticText = "Status"/,
    "the snapshot structure status should render",
  );
  assert.match(
    validationTree,
    /AXStaticText = "Structurally valid"/,
    "the snapshot structure should render in a Valid state",
  );
  assert.match(
    validationTree,
    /Tama validates snapshot structure\./,
    "the rendered validation section should describe snapshot structure validation",
  );
  assert.doesNotMatch(
    validationTree,
    /AXStaticText = "Invalid"/,
    "the snapshot structure should not render an Invalid state",
  );
} finally {
  quitApp(app.pid);
}
