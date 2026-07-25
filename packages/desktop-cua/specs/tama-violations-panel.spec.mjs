import assert from "node:assert/strict";
import {
  clickElement,
  elementIndexOf,
  launchCuaApp,
  launchCuaProcess,
  quitApp,
  selectSidebarRow,
  snapshotTree,
  waitForText,
} from "../driver.mjs";

const app = process.env.CUA_APP_EXECUTABLE
  ? launchCuaProcess({})
  : launchCuaApp({
      bundleId: process.env.CUA_BUNDLE_ID || "ai.wisent.tama.desktop",
    });

function assertDisplayedCounts(tree, labels) {
  const lines = String(tree).split("\n");
  const labelIndexes = labels.map((label) =>
    lines.findIndex((line) => line.includes(`"${label}"`)),
  );

  for (let index = 0; index < labels.length; index += 1) {
    const label = labels[index];
    const labelIndex = labelIndexes[index];
    assert.notEqual(labelIndex, -1, `${label} should be visible`);
    if (index > 0) {
      assert.ok(
        labelIndex > labelIndexes[index - 1],
        `${label} should follow ${labels[index - 1]} in the report`,
      );
    }

    const nextLabelIndex = labelIndexes[index + 1] ?? labelIndex + 8;
    const countRegion = lines
      .slice(labelIndex, nextLabelIndex)
      .join("\n");
    assert.match(
      countRegion,
      /(?:AXStaticText[^"\n]*=\s*"|,\s*)\d[\d,]*"/,
      `${label} should display its numeric count`,
    );
  }
}

try {
  waitForText(
    app.pid,
    app.windowId,
    'AXStaticText = "Violations"',
    30_000,
  );
  selectSidebarRow(app.pid, app.windowId, 5);

  waitForText(app.pid, app.windowId, "AXTextField", 30_000);
  const controlsTree = waitForText(
    app.pid,
    app.windowId,
    "AXButton (Scan",
    30_000,
  );
  const scanButtonLine = controlsTree
    .split("\n")
    .find((line) => line.includes("AXButton (Scan"));
  assert.ok(scanButtonLine, "the Violations panel should show its Scan button");
  assert.doesNotMatch(
    scanButtonLine,
    /\bDISABLED\b/,
    "Scan should be enabled for the configured repository",
  );

  clickElement(
    app.pid,
    app.windowId,
    elementIndexOf(controlsTree, "AXButton (Scan"),
  );

  snapshotTree(app.pid, app.windowId);
  const reportTree = waitForText(
    app.pid,
    app.windowId,
    "Total violations",
    180_000,
  );

  assertDisplayedCounts(reportTree, [
    "Files scanned",
    "Skipped files",
    "Scan errors",
    "Total violations",
  ]);
} finally {
  quitApp(app.pid);
}
