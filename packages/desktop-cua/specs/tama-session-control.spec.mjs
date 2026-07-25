import assert from "node:assert/strict";
import {
  launchCuaProcess,
  pressKey,
  quitApp,
  selectSidebarRow,
  snapshotTree,
  waitForText,
} from "../driver.mjs";

const executable =
  process.env.CUA_APP_EXECUTABLE
  || "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/tama-desktop/.build/Tama.app/Contents/MacOS/Tama";

function waitForTree(pid, windowId, description, predicate, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let tree = "";

  while (Date.now() < deadline) {
    tree = snapshotTree(pid, windowId);
    if (predicate(tree)) return tree;
    Atomics.wait(
      new Int32Array(new SharedArrayBuffer(4)),
      0,
      0,
      400,
    );
  }

  throw new Error(
    `Timed out waiting for ${description}; last tree (tail): ${tree.slice(-800)}`,
  );
}

function countOccurrences(text, needle) {
  return text.split(needle).length - 1;
}

function staticTextValues(tree) {
  return [...tree.matchAll(/AXStaticText = "([^"]*)"/g)].map(
    (match) => match[1],
  );
}

function panelIsRendered(tree, title) {
  return countOccurrences(tree, `AXStaticText = "${title}"`) >= 2;
}

const app = launchCuaProcess({
  executable,
  env: { TAMA_TEST_IDENTITY: "1" },
});

try {
  waitForText(app.pid, app.windowId, "AXOutline (Sidebar)", 30_000);

  selectSidebarRow(app.pid, app.windowId, 4);

  const repositoryTree = waitForTree(
    app.pid,
    app.windowId,
    "the Repository hooks panel and its installed hook rows",
    (tree) =>
      panelIsRendered(tree, "Repository hooks")
      && /AXStaticText = "repo-githooks\/[^"]+\/pre-[a-z-]+"/.test(tree),
  );

  const repositoryValues = staticTextValues(repositoryTree);
  const installedHookPaths = repositoryValues.filter((value) =>
    /^repo-githooks\/[^/]+\/pre-[a-z-]+$/.test(value)
  );
  const renderedEvents = repositoryValues.filter((value) =>
    /^pre-[a-z-]+$/.test(value)
  );

  assert.ok(
    installedHookPaths.length > 0,
    "Repository hooks should render at least one installed hook",
  );
  assert.equal(
    new Set(installedHookPaths).size,
    installedHookPaths.length,
    "Each installed repository hook should render once",
  );

  const expectedEventCounts = new Map();
  for (const path of installedHookPaths) {
    const event = path.split("/").at(-1);
    expectedEventCounts.set(
      event,
      (expectedEventCounts.get(event) ?? 0) + 1,
    );
  }

  const renderedEventCounts = new Map();
  for (const event of renderedEvents) {
    renderedEventCounts.set(
      event,
      (renderedEventCounts.get(event) ?? 0) + 1,
    );
  }

  assert.deepEqual(
    renderedEventCounts,
    expectedEventCounts,
    "Every installed repository hook should render its event status",
  );

  // Keep the outline's current keyboard focus. Re-focusing the sidebar by
  // coordinate can land on the already-selected Repository hooks row.
  snapshotTree(app.pid, app.windowId);
  pressKey(app.pid, "up", { windowId: app.windowId });

  waitForTree(
    app.pid,
    app.windowId,
    "the intermediate Snapshot validation panel",
    (tree) =>
      panelIsRendered(tree, "Snapshot validation")
      && !tree.includes('AXStaticText = "repo-githooks/'),
  );

  snapshotTree(app.pid, app.windowId);
  pressKey(app.pid, "up", { windowId: app.windowId });

  const justificationsTree = waitForTree(
    app.pid,
    app.windowId,
    "the Justifications panel content",
    (tree) =>
      panelIsRendered(tree, "Justifications")
      && !tree.includes('AXStaticText = "repo-githooks/')
      && (
        tree.includes("AXRadioButton")
        || tree.includes('AXStaticText = "Registry unavailable"')
        || tree.includes('AXStaticText = "No justification hooks"')
      ),
  );

  assert.match(
    justificationsTree,
    /AXRadioButton|AXStaticText = "Registry unavailable"|AXStaticText = "No justification hooks"/,
    "Justifications should render its registry controls or an explicit content state",
  );
} finally {
  quitApp(app.pid);
}
