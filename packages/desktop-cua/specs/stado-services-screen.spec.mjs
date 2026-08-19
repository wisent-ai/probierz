// services-screen — the Services screen (ServicesView.swift) asks every
// declared host what it runs (`service converge` in report mode) and which
// product processes no unit owns (`service list --unowned`), then renders, per
// unit, the version the host declares against the version it reports and
// whether the live process is executing the artefact its unit declares.
//
// The screen offers no mutating control at all: ServiceTruthStore has no write,
// and the unowned process it finds is reported rather than ended, in the
// screen's own words. That absence is the contract this journey asserts, so a
// Reclaim/Clear/Apply/Restart/Stop/End/Kill control appearing here is a failure.
import assert from "node:assert/strict";
import { quitApp } from "../driver.mjs";
import {
  assertAbsent,
  assertField,
  attempt,
  button,
  buttons,
  createEvidence,
  dumpWindows,
  findButton,
  launchConsole,
  openScreen,
  readWindow,
  selectRow,
} from "./stado-console.mjs";

const CONVERGE_MS = Number("180000");
const SELECT_MS = Number("60000");

// A control that would write to a host names its verb first: "Reclaim disk…",
// "Clear digest", "Restart unit", "End process". Prose does not — and prose is
// in scope here, because an alert panel is one combined accessibility element
// whose label carries whole sentences ("A restart is what makes the two
// agree"), so a substring search would find a write in an explanation.
const WRITING_TITLE = /^(Reclaim|Clear|Apply|Restart|Stop|End|Kill|Terminate|Ensure|Install|Converge|Delete|Remove)\b/i;
const READ_ONLY_BUTTONS = new Set([
  "Refresh",
  "Retry",
  "Show them",
  "Clear filters",
  "All units",
  "Serving replaced code",
  "Unowned processes",
  "Posture",
  "Queue",
  "Hosts",
  "Services",
  "Disk",
  "Registry",
  "Releases",
  "Deployments",
]);

function writingControls(view) {
  return buttons(view)
    .filter((item) => {
      const label = item.label.replace(/,\s*[\d,]+$/, "");
      if (READ_ONLY_BUTTONS.has(label)) return false;
      return WRITING_TITLE.test(label);
    })
    .map((item) => item.label);
}

function assertReadOnly(view, where) {
  const writes = writingControls(view);
  assert.deepEqual(
    writes,
    [],
    `the Services screen offers a mutating control ${where}: ${writes.join(" | ")}`,
  );
}

const evidence = createEvidence("stado-services-screen");
const app = launchConsole();

try {
  openScreen(app.pid, app.windowId, "Services", {
    loaded: /AX\w*Button \(All units, [1-9]/,
    failures: [
      "No registry hosts to ask",
      "No host reported its units",
      "No declared units",
    ],
    refresh: "Refresh",
    timeoutMs: CONVERGE_MS,
  });

  // One unit answers for itself in the inspector, so select a row and require
  // the fields this screen contracts to show for it.
  const { row } = selectRow(app.pid, app.windowId, {
    needle: /PROCESS MATCHES PROGRAM ON DISK/,
    timeoutMs: SELECT_MS,
  });

  const loaded = evidence.capture(app.pid, app.windowId, "loaded");

  assertAbsent(
    loaded,
    [
      "No registry hosts to ask",
      "No host reported its units",
      "No declared units",
      "Reading declared units on",
      "No row selected",
    ],
    "the Services screen did not load declared units from the hosts",
  );

  // Declared against reported: the version the registry declares for this unit
  // and the version the host says is installed.
  const declaredVersion = assertField(loaded, "Declared version");
  const installedVersion = assertField(loaded, "Installed version");
  assert.ok(
    declaredVersion !== "" && installedVersion !== "",
    `${row.label} renders no declared/installed version pair`,
  );

  // Whether the live process is executing the artefact the unit declares, in
  // the host's own answer — and never as a blank that reads as "fine".
  const declaredProgram = assertField(loaded, "Declared program");
  const runningBinary = assertField(loaded, "Running binary");
  const match = assertField(loaded, "Process matches program on disk", {
    pattern: /^(Yes|No|Not reported by this host)/,
  });
  assert.ok(
    declaredProgram.length > 0,
    "the unit renders no declared program",
  );
  if (runningBinary === "Not reported") {
    assert.equal(
      match,
      "Not reported by this host",
      "a host that named no running binary must not be rendered as a match",
    );
  }
  if (match.startsWith("No")) {
    assert.ok(
      loaded.tree.includes("The process is not executing the program on disk"),
      `a replaced binary must be called that; tree: ${loaded.tree.slice(-2000)}`,
    );
  }
  assertField(loaded, "Unit state");
  assertField(loaded, "Verdict");

  // Nothing on this screen writes to a host.
  assertReadOnly(loaded, "beside a declared unit");

  // The processes no unit owns: reported, with what is known about each, and
  // left to whoever knows what it is doing.
  const railFacet = button(readWindow(app.pid, app.windowId), "Unowned processes");
  const unowned = Number((railFacet.label.match(/,\s*(\d+)$/) || [])[1] ?? "0");
  attempt(app.pid, app.windowId, railFacet.label);

  if (unowned > 0) {
    const { row: process } = selectRow(app.pid, app.windowId, {
      needle: /Nothing supervises this process/,
      timeoutMs: SELECT_MS,
    });
    const reported = evidence.capture(app.pid, app.windowId, "unowned-process");

    assert.ok(
      reported.tree.includes(
        "No declared unit owns it, so no release updates it, nothing restarts it if it dies, and nothing stops it. Two processes in this state ran for four days before anybody looked. Ending it is a decision for whoever knows what it is doing, and this console does not make it.",
      ),
      `the unowned process ${process.label} is not reported in the screen's own words; tree: ${
        reported.tree.slice(-2500)
      }`,
    );
    assertField(reported, "PID", { pattern: /^[\d,]+$/ });
    assertField(reported, "Command");
    assertField(reported, "Product guess");
    assertReadOnly(reported, "beside an unowned process");
    assert.ok(
      !findButton(reported, "End process") && !findButton(reported, "Stop"),
      "the screen offers to end a process it says it does not end",
    );
  } else {
    const empty = evidence.capture(app.pid, app.windowId, "unowned-process");
    assert.ok(
      empty.tree.includes("Every product process belongs to a unit")
        || empty.tree.includes("Unowned processes are unknown"),
      `the unowned facet reports neither processes nor their absence; tree: ${
        empty.tree.slice(-2000)
      }`,
    );
    assertReadOnly(empty, "on the unowned facet");
  }
} finally {
  try {
    dumpWindows(app.pid, "stado-services-screen");
    evidence.write();
  } finally {
    quitApp(app.pid);
  }
}
