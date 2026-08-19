// releases-screen — the Releases screen (ReleasesView.swift) diagnoses every
// declared rollout against the real fleet, renders desired against observed
// with the rollout's verdict and its blockers, and refuses the one write it
// offers — `release quarantine clear` — until a reason has been typed.
//
// The journey stops at that refusal. Clearing a digest rewrites rollout state
// on a production host, so the only thing asserted about the write is that it
// did not happen: the dialog stays open, the command still carries the reason
// placeholder, no clearance is recorded, and every digest is still quarantined.
import assert from "node:assert/strict";
import { quitApp } from "../driver.mjs";
import {
  activate,
  assertAbsent,
  assertField,
  assertRefusedControl,
  attempt,
  buttons,
  createEvidence,
  dumpWindows,
  launchConsole,
  openScreen,
  poll,
  readWindow,
  rowButtons,
  selectRow,
  waitForScreen,
} from "./stado-console.mjs";

const DIAGNOSIS_MS = Number("240000");
const EVIDENCE_MS = Number("90000");
const DIALOG_MS = Number("30000");

function clearableDigests(view) {
  return buttons(view).filter((item) => item.label === "Clear…").length;
}

const evidence = createEvidence("stado-releases-screen");
const app = launchConsole();

try {
  // One `release status` and then one `release doctor` per product target, each
  // reading the host itself, so the screen answers in minutes rather than ms.
  openScreen(app.pid, app.windowId, "Releases", {
    loaded: "DESIRED VERSION",
    failures: [
      "No rollout could be listed",
      "Nothing was read",
      "No rollout is declared",
    ],
    refresh: "Re-diagnose",
    timeoutMs: DIAGNOSIS_MS,
  });

  const loaded = evidence.capture(app.pid, app.windowId, "loaded");

  // Loaded means diagnosed: no error banner, no empty state, and no spinner
  // still naming what it is reading.
  assertAbsent(
    loaded,
    [
      "No rollout could be listed",
      "Re-diagnosis failed",
      "Nothing was read",
      "No rollout is declared",
      "Diagnosing every declared rollout",
    ],
    "the Releases screen did not load real rollout state",
  );

  // Desired against observed, the rollout's own verdict, and its blockers.
  const desired = assertField(loaded, "Desired version");
  const observed = assertField(loaded, "Observed version");
  const phase = assertField(loaded, "Phase");
  assert.ok(
    phase !== "—",
    `the rollout renders no phase: ${JSON.stringify(phase)}`,
  );

  const verdict = loaded.tree.match(/\b(settled|rolling|blocked|unreported)\b/);
  assert.ok(
    verdict,
    `the rollout carries no verdict word from the CLI; tree: ${loaded.tree.slice(-2000)}`,
  );

  const blockers = assertField(loaded, "Blockers");
  if (verdict[1] === "blocked") {
    assert.notEqual(
      blockers,
      "None. Nothing is holding this rollout.",
      "a blocked rollout must name what is holding it",
    );
  }
  if (verdict[1] === "settled") {
    assert.equal(
      observed,
      desired,
      "a settled rollout renders the observed version as the desired one",
    );
  }

  // The gates the verdict was reached against, in the same pane.
  assertField(loaded, "Disk pressure", { pattern: /^(Resolved|Unresolved)/ });
  assertField(loaded, "Free space", { pattern: /GB|—/ });

  // The write lives against a quarantined digest, so find a rollout whose host
  // holds one. The screen selects the first row for the operator itself.
  let quarantined = poll(app.pid, app.windowId, "Clear…", EVIDENCE_MS);
  if (!quarantined) {
    const rows = rowButtons(readWindow(app.pid, app.windowId)).length;
    for (let skip = 1; skip < rows && !quarantined; skip += 1) {
      selectRow(app.pid, app.windowId, {
        needle: /Clear…|Nothing is quarantined for|Clearing is unavailable/,
        timeoutMs: EVIDENCE_MS,
        skip,
      });
      quarantined = poll(app.pid, app.windowId, "Clear…", EVIDENCE_MS);
    }
  }
  assert.ok(
    quarantined,
    `no rollout on this fleet holds a quarantined digest, so the screen's only write could not be reached; tree: ${
      readWindow(app.pid, app.windowId).tree.slice(-2500)
    }`,
  );

  const held = clearableDigests(quarantined);
  assert.ok(held > 0, "the quarantine pane offers no digest to clear");

  // Open the clearance dialog and leave its reason field untouched.
  const sheet = activate(app.pid, app.windowId, "Clear…", {
    needle: "Clear digest",
    timeoutMs: DIALOG_MS,
  });
  const dialog = sheet.view;

  assert.ok(
    dialog.tree.includes("REQUIRED, RECORDED IN THE AUDIT TRAIL"),
    `the clearance dialog does not state that a reason is required; tree: ${dialog.tree.slice(-2000)}`,
  );
  assert.ok(
    dialog.tree.includes(
      "Without a reason this command does not run. It is what an audit reads months from now, when nobody remembers why the digest was given another chance.",
    ),
    `the dialog does not refuse an empty reason in its own words; tree: ${dialog.tree.slice(-2000)}`,
  );
  // The command it would run, still carrying the placeholder rather than a
  // reason: nothing in this dialog has been filled in.
  assert.match(
    dialog.tree,
    /stado release quarantine clear .*--reason "<reason>" --json/,
    "the dialog does not show the exact command with an unfilled reason",
  );

  // The destructive action, with the reason field left as the dialog opened it:
  // the red button is drawn and taken out of reach, so no press — by an
  // operator or by this journey — can be delivered to it.
  const control = assertRefusedControl(dialog, "Clear digest");

  const refused = evidence.capture(app.pid, sheet.windowId, "refused-without-reason");

  // Nothing proceeded: the dialog is still asking for a reason, the clearance is
  // still out of reach, and the command it would run still carries the
  // placeholder rather than a reason.
  assert.ok(
    refused.tree.includes("REQUIRED, RECORDED IN THE AUDIT TRAIL"),
    `the dialog stopped asking for a reason; tree: ${refused.tree.slice(-2000)}`,
  );
  assert.equal(
    assertRefusedControl(refused, "Clear digest"),
    control,
    "the clearance became reachable without a reason being typed",
  );
  assert.match(
    refused.tree,
    /--reason "<reason>" --json/,
    "the refused dialog no longer shows the unfilled command",
  );

  // And the screen behind it recorded no clearance.
  const behind = readWindow(app.pid, app.windowId);
  assertAbsent(
    behind,
    [
      "LAST CLEARANCE",
      /Clearing sha256:|Clearing [0-9a-f]{12}/,
      "Previous state backed up on the host at",
    ],
    "the screen cleared a digest without a typed reason",
  );

  // Leave the host exactly as it was found.
  attempt(app.pid, sheet.windowId, "Leave it quarantined");
  const after = waitForScreen(app.pid, app.windowId, "Clear…", DIALOG_MS);
  assert.equal(
    clearableDigests(after),
    held,
    `the host's quarantine map changed: ${held} clearable digests before, ${
      clearableDigests(after)
    } after`,
  );
} finally {
  try {
    dumpWindows(app.pid, "stado-releases-screen");
    evidence.write();
  } finally {
    quitApp(app.pid);
  }
}
