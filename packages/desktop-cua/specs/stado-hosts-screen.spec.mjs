// hosts-screen — the Hosts screen (HostsView.swift) reconciles the published
// capacity snapshot with `stado host gates` read off each declared host, says
// whether a host is claiming work and, when it is not, names the blockers its
// own agent published beside the disk figures behind them. Its one write —
// `host reclaim --apply` — is refused until a reason has been typed.
//
// The journey stops at that refusal. Reclamation deletes on a production host,
// so the assertion about the write is that it did not happen: the sheet keeps
// asking, the command still carries the reason placeholder, no pass is
// reported, and the host's gate and disk figures are unchanged afterwards.
import assert from "node:assert/strict";
import { quitApp } from "../driver.mjs";
import {
  activate,
  assertAbsent,
  assertField,
  assertRefusedControl,
  attempt,
  createEvidence,
  dumpWindows,
  launchConsole,
  openScreen,
  selectRow,
  waitForScreen,
} from "./stado-console.mjs";

const GATES_MS = Number("180000");
const SHEET_MS = Number("120000");

const evidence = createEvidence("stado-hosts-screen");
const app = launchConsole();

try {
  // The published snapshot lists the hosts; one `host gates` per declared host
  // then says which of them is claiming work.
  openScreen(app.pid, app.windowId, "Hosts", {
    loaded: /AX\w*Button \(All hosts/,
    failures: ["No host inventory", "No registered hosts"],
    refresh: "Refresh",
    timeoutMs: GATES_MS,
  });

  // A host answers for itself in the inspector, so select a row and require the
  // gate fields this screen contracts to show for it.
  const { row } = selectRow(app.pid, app.windowId, {
    needle: /CLEANUP POLICY MODE/,
    timeoutMs: Number("60000"),
  });

  const loaded = evidence.capture(app.pid, app.windowId, "loaded");

  assertAbsent(
    loaded,
    [
      "No host inventory",
      "No registered hosts",
      "No hosts in this filter",
      "Reading host capacity reports",
      "No host selected",
    ],
    "the Hosts screen did not load real fleet state",
  );

  // Whether this host is claiming, and — when it is not — the blockers its own
  // agent published, in an alarm that says so rather than an empty cell.
  const claiming = assertField(loaded, "Claiming work", { pattern: /^(Yes|No)$/ });
  const blockers = assertField(loaded, "Blockers");
  assert.notEqual(
    blockers,
    "Reading…",
    `${row.label} renders a spinner where its blockers belong`,
  );
  if (claiming === "No") {
    assert.ok(
      loaded.tree.includes("This host is claiming no work"),
      `a host that claims nothing must say so; tree: ${loaded.tree.slice(-2000)}`,
    );
  }

  // The disk figures beside the blockers: free space against the watermark the
  // declared cleanup policy holds it to.
  const free = assertField(loaded, "Free space", {
    pattern: /^([\d.,]+ GB free|Not reported)/,
  });
  assertField(loaded, "Cleanup policy mode");
  assert.match(
    loaded.tree,
    /[\d.,]+ GB/,
    "the screen renders no disk figure for any host",
  );

  // The write. Opening the sheet runs the janitor's own planning phase, which
  // the product states writes nothing; the apply is what stays refused.
  const dialog = activate(app.pid, app.windowId, "Reclaim disk…", {
    needle: "Reclaim disk on ",
    timeoutMs: SHEET_MS,
  });
  const sheet = dialog.view;

  assert.ok(
    sheet.tree.includes("Why this host needs the space"),
    `the reclamation sheet does not ask why; tree: ${sheet.tree.slice(-2000)}`,
  );
  // Either refusal refuses the same apply: no dry run has answered yet, or one
  // has and no reason has been typed.
  const refusal = [
    "Type a reason to enable the apply.",
    "The apply stays unavailable until the dry run above has answered for",
  ].find((text) => sheet.tree.includes(text));
  assert.ok(
    refusal,
    `the sheet does not state why the apply is unavailable; tree: ${sheet.tree.slice(-2500)}`,
  );
  // The exact command, still carrying the placeholder rather than a reason.
  assert.match(
    sheet.tree,
    /stado host reclaim .*--apply --reason "why this host needs the space" --json/,
    "the sheet does not show the apply command with an unfilled reason",
  );

  // The apply, with the reason field left as the sheet opened it: the red button
  // is drawn and taken out of reach, so no press — by an operator or by this
  // journey — can be delivered to it.
  const control = assertRefusedControl(sheet, "Reclaim now");

  const refused = evidence.capture(app.pid, dialog.windowId, "refused-without-reason");

  // Nothing was applied: the sheet is still asking, the apply is still out of
  // reach, and no pass was reported.
  assert.ok(
    refused.tree.includes("Why this host needs the space"),
    `the sheet stopped asking why; tree: ${refused.tree.slice(-2000)}`,
  );
  assert.equal(
    assertRefusedControl(refused, "Reclaim now"),
    control,
    "the apply became reachable without a reason being typed",
  );
  assertAbsent(
    refused,
    [
      "What reclamation freed",
      "Reclaiming disk on ",
      "The pass ran and reported no stages",
      "A reason is required",
    ],
    "the screen applied a reclamation without a typed reason",
  );
  assert.match(
    refused.tree,
    /--apply --reason "why this host needs the space" --json/,
    "the refused sheet no longer shows the unfilled apply command",
  );

  // Leave the host as it was found: dismiss the sheet without applying.
  attempt(app.pid, dialog.windowId, "Cancel");
  const after = waitForScreen(app.pid, app.windowId, /CLEANUP POLICY MODE/, SHEET_MS);
  assertAbsent(
    after,
    ["What reclamation freed", "Reclaim disk on "],
    "the reclamation sheet outlived a cancel",
  );
  assert.equal(
    assertField(after, "Free space", { pattern: /^([\d.,]+ GB free|Not reported)/ }),
    free,
    "this host's free space changed while the journey refused to reclaim anything",
  );
  assert.equal(
    assertField(after, "Claiming work", { pattern: /^(Yes|No)$/ }),
    claiming,
    "this host's claiming gate changed while the journey refused to reclaim anything",
  );
} finally {
  try {
    dumpWindows(app.pid, "stado-hosts-screen");
    evidence.write();
  } finally {
    quitApp(app.pid);
  }
}
