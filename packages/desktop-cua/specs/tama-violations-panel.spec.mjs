import assert from "node:assert/strict";
import {
  clickElement,
  elementIndexOf,
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
  waitForText(app.pid, app.windowId, 'AXStaticText = "Violations"', 30_000);
  selectSidebarRow(app.pid, app.windowId, 5);

  const scanTree = waitForText(
    app.pid,
    app.windowId,
    "AXButton (Scan)",
    30_000,
  );
  assert.match(scanTree, /AXStaticText = "No scan yet"/);

  clickElement(
    app.pid,
    app.windowId,
    elementIndexOf(scanTree, "AXButton (Scan)"),
  );

  const completedTree = waitForText(
    app.pid,
    app.windowId,
    'AXStaticText = "Total violations"',
    120_000,
  );
  assert.doesNotMatch(completedTree, /AXStaticText = "Scan failed"/);

  const countLabels = [
    "Files scanned",
    "Skipped files",
    "Scan errors",
    "Total violations",
  ];

  for (const [index, label] of countLabels.entries()) {
    const labelPosition = completedTree.indexOf(`AXStaticText = "${label}"`);
    assert.notEqual(labelPosition, -1, `${label} should be shown`);

    const nextLabel = countLabels[index + 1];
    const nextLabelPosition = nextLabel
      ? completedTree.indexOf(`AXStaticText = "${nextLabel}"`, labelPosition)
      : completedTree.length;
    assert.ok(
      nextLabelPosition > labelPosition,
      `${label} should precede the next summary count`,
    );

    const countRegion = completedTree.slice(labelPosition, nextLabelPosition);
    assert.match(
      countRegion,
      /AXStaticText = "[0-9][0-9,]*"/,
      `${label} should show a numeric count`,
    );
  }
} finally {
  quitApp(app.pid);
}
