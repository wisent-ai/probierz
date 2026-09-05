// Shared driving surface for the Stado operator console screens.
//
// Every console screen is the same shell (ConsoleView.swift): a sidebar of
// destination buttons, a facet rail, a dense table whose rows are plain
// SwiftUI buttons, and an inspector of WisentField label-over-value pairs.
// Three properties of that shell decide how a journey reads it:
//
//   * ConsoleTableHead is accessibilityHidden(true), so the column heads are
//     not in the accessibility tree at all. A screen's contracted fields are
//     therefore read from the inspector, which is also where an operator reads
//     them in full rather than middle-truncated.
//   * WisentField upper-cases its label and combines label and value into one
//     accessibility element, so a field reads as `DESIRED VERSION 0.9.14`.
//   * Sidebar destinations and facets are buttons whose accessibility label
//     carries the attention count, e.g. `Hosts, 2` or `All units, 7`.
//
// An element index belongs to the snapshot it was resolved against, so every
// action here takes that snapshot and addresses the element by the strongest
// handle the snapshot offers.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { cuaCall, launchCuaProcess, snapshotState } from "../driver.mjs";

const POLL_MS = Number("500");

// Buttons the console shows around its data: window chrome, the sidebar
// destinations, and the toolbar. Never a table row, so a row search may not
// pick one of them by accident.
export const CONSOLE_CHROME = [
  "close",
  "minimize",
  "minimise",
  "zoom",
  "fullscreen",
  "Posture",
  "Queue",
  "Hosts",
  "Services",
  "Disk",
  "Registry",
  "Releases",
  "Deployments",
  "Refresh",
  "Re-diagnose",
  "Retry",
  "Dismiss",
  "Show them",
  "Read again",
  "Clear filters",
  "All hosts",
  "Not claiming",
  "Unavailable",
  "Stale",
  "Live",
  "Declared",
  "Undeclared",
  "Pinned only",
  "All units",
  "Serving replaced code",
  "Unowned processes",
];

export function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function readPromptFreeCuaReadiness() {
  return cuaCall("check_permissions", { prompt: false });
}

function matches(tree, needle) {
  if (typeof needle === "function") return Boolean(needle(tree));
  return needle instanceof RegExp ? needle.test(tree) : tree.includes(needle);
}

function describe(needle) {
  if (typeof needle === "function") return needle.name || "a state this journey reads";
  return needle instanceof RegExp ? String(needle) : JSON.stringify(needle);
}

// One reading of the window: the tree an assertion reads, plus whatever
// addressing handles this snapshot minted for an action.
export function readWindow(pid, windowId, { screenshotOutFile } = {}) {
  const state = snapshotState(pid, windowId, { screenshotOutFile }) || {};
  const content = state.structuredContent || state;
  return {
    tree: String(state.tree_markdown || content.tree_markdown || ""),
    snapshotId: content.snapshot_id ?? state.snapshot_id ?? null,
    elements: content.elements || state.elements || [],
  };
}

// The accessibility tree of every window this app has, and the element table
// behind it, written beside the run's other evidence. A screen journey that
// could not drive a control has to be able to say what the tree actually
// offered.
export function dumpWindows(pid, slug) {
  const artifacts = process.env.PROBIERZ_ARTIFACTS;
  if (!artifacts) return null;
  const file = path.join(path.resolve(artifacts), `${slug}-ax-tree.txt`);
  const sections = [];
  for (const win of windowsOf(pid)) {
    let body;
    try {
      const view = readWindow(pid, win.window_id);
      body = `${view.tree}\n\n## elements\n${JSON.stringify(view.elements, null, 1)}`;
    } catch (error) {
      body = `unreadable: ${String(error.message || error)}`;
    }
    sections.push(
      `# window ${win.window_id} ${JSON.stringify(win.title || "")} ${JSON.stringify(win.bounds || {})}\n${body}`,
    );
  }
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, `${sections.join("\n\n")}\n`, { mode: 0o600 });
  return file;
}

export function elementOf(view, elementIndex) {
  return (view.elements || []).find(
    (item) => Number(item.element_index) === Number(elementIndex),
  ) || null;
}

function refusal(result) {
  if (result && typeof result === "object" && String(result.status || "") === "refused") {
    return result.refusal?.code || "refused";
  }
  return null;
}

// Click a control, resolving it against a snapshot taken in the same breath.
// The app re-renders while it reads hosts, and a rebuilt tree makes every token
// from the previous snapshot stale, so a stale refusal is re-resolved rather
// than reported as a screen that cannot be driven.
export function click(pid, windowId, label, { retries = Number("5") } = {}) {
  let last = null;
  for (let attempt = 0; attempt < retries; attempt += 1) {
    const view = readWindow(pid, windowId);
    const target = button(view, label);
    const element = elementOf(view, target.index);
    const result = cuaCall(
      "click",
      element?.element_token
        ? { pid, element_token: element.element_token }
        : {
          pid,
          window_id: windowId,
          element_index: target.index,
          ...(view.snapshotId ? { snapshot_id: view.snapshotId } : {}),
        },
    );
    last = { view, element, result, code: refusal(result) };
    if (!last.code || last.code !== "stale_element_token") return last;
  }
  return last;
}

// Open something and prove it opened.
export function activate(pid, windowId, label, { needle, timeoutMs }) {
  const pressed = click(pid, windowId, label);
  const opened = waitForAnyWindow(pid, needle, timeoutMs);
  if (opened) return { ...opened, pressed };
  throw new Error(
    `pressing ${JSON.stringify(label)} showed nothing carrying ${describe(needle)} within ${
      timeoutMs
    } ms; the driver answered ${JSON.stringify(pressed?.result).slice(0, Number("300"))}`,
  );
}

// Every button in the snapshot, with the accessibility label the app gave it.
export function buttons(view) {
  const found = [];
  for (const line of view.tree.split("\n")) {
    const label = line.match(/AX\w*Button \(([^)]*)\)/);
    const index = line.match(/\[(\d+)\]/);
    if (!label || !index) continue;
    found.push({ index: Number(index[1]), label: label[1], line: line.trim() });
  }
  return found;
}

function labelled(view, label) {
  return buttons(view).filter(
    (item) => item.label === label || item.label.startsWith(`${label},`),
  );
}

export function findButton(view, label) {
  return labelled(view, label)[0] || null;
}

export function button(view, label) {
  const found = findButton(view, label);
  assert.ok(
    found,
    `no button labelled ${JSON.stringify(label)}; buttons on screen: ${
      buttons(view).map((item) => item.label).join(" | ") || "none"
    }`,
  );
  return found;
}

// A control the app has disabled. The accessibility tree still carries it — an
// operator sees the red button — but cua-driver gives it neither an element
// index nor an action, because the application exposes nothing to deliver to
// it. That is the refusal, in the only place a driver can read it.
export function assertRefusedControl(view, label) {
  const pattern = new RegExp(
    `^\\s*-\\s+AX\\w*Button \\(${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\)\\s*$`,
  );
  const line = view.tree.split("\n").find((item) => pattern.test(item));
  assert.ok(
    line,
    `the screen renders no ${JSON.stringify(label)} control at all; buttons on screen: ${
      buttons(view).map((item) => item.label).join(" | ") || "none"
    }`,
  );
  const actionable = findButton(view, label);
  assert.equal(
    actionable,
    null,
    `${JSON.stringify(label)} is offered as an actionable control: ${
      JSON.stringify(actionable).slice(0, Number("300"))
    }`,
  );
  return line.trim();
}

// A table row: a button that lives inside the table's own scrolling content and
// whose label is the row's cells joined together, so it carries several
// separators.
//
// The container test is what keeps a row search honest. An alert panel is also
// one combined button with a comma-heavy label, and pressing it fires the
// panel's action — "Show them" switches the facet — which reads exactly like a
// row selection that worked. Rows are nested under the table's provider group;
// panels, facets, destinations and toolbar items are not.
const ROW_CONTAINER = /ProviderGroup|ScrollArea|Table|Outline|List|Grid/i;

export function rowButtons(view, { minimumFields = Number("3") } = {}) {
  const byIndex = new Map(
    (view.elements || []).map((item) => [Number(item.element_index), item]),
  );
  return buttons(view).filter((item) => {
    if (CONSOLE_CHROME.some((known) => item.label === known || item.label.startsWith(`${known},`))) {
      return false;
    }
    const parent = byIndex.get(Number(byIndex.get(item.index)?.parent_index));
    if (!ROW_CONTAINER.test(String(parent?.role || ""))) return false;
    return (item.label.match(/,/g) || []).length >= minimumFields - 1;
  });
}

export function poll(pid, windowId, needle, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let view = readWindow(pid, windowId);
  while (!matches(view.tree, needle)) {
    if (Date.now() >= deadline) return null;
    sleep(POLL_MS);
    view = readWindow(pid, windowId);
  }
  return view;
}

export function waitForScreen(pid, windowId, needle, timeoutMs) {
  const view = poll(pid, windowId, needle, timeoutMs);
  if (view) return view;
  const last = readWindow(pid, windowId);
  throw new Error(
    `timed out after ${timeoutMs} ms waiting for ${describe(needle)}; last tree: ${last.tree.slice(-2500)}`,
  );
}

export function windowsOf(pid) {
  const listed = cuaCall("list_windows") || {};
  return (listed.windows || []).filter((win) => win.pid === pid);
}

// A macOS sheet may be a window of its own, so a dialog opened from a screen is
// not necessarily in the screen's tree. Search every window of the app.
export function waitForAnyWindow(pid, needle, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    for (const win of windowsOf(pid)) {
      let view = null;
      try {
        view = readWindow(pid, win.window_id);
      } catch {
        continue;
      }
      if (matches(view.tree, needle)) return { windowId: win.window_id, view };
    }
    if (Date.now() >= deadline) return null;
    sleep(POLL_MS);
  }
}

// Every value rendered under a WisentField label, in tree order.
//
// The component upper-cases the label and combines label and value into one
// accessibility element, so the snapshot's element table carries them as
// `FREE SPACE, 24.8 GB free · claims stop below 20 GB`. A bare eyebrow followed
// by its own text — the shape a list of blockers takes — is not an element of
// its own, so that case is read off the tree instead.
export function fieldValues(view, label) {
  const upper = label.toUpperCase();
  const combined = [];
  for (const element of view.elements || []) {
    const text = String(element.label ?? "");
    if (text !== upper && !text.startsWith(`${upper},`)) continue;
    combined.push(clean(text.slice(upper.length)));
  }
  if (combined.some((value) => value.length > 0)) return combined;

  const lines = view.tree.split("\n");
  const values = [];
  for (const [position, line] of lines.entries()) {
    if (!line.includes(`"${upper}"`) && !line.includes(`(${upper})`)) continue;
    const following = lines[position + 1]?.match(/= "([^"]*)"|\(([^)]*)\)/);
    const value = clean(following?.[1] ?? following?.[2] ?? "");
    if (value) values.push(value);
  }
  return values.length ? values : combined;
}

function clean(text) {
  return String(text)
    .replace(/^[\s",:;=]+/, "")
    .replace(/[\s",]+$/, "")
    .trim();
}

export function fieldValue(view, label, { pattern } = {}) {
  const values = fieldValues(view, label);
  if (!pattern) return values[0] ?? null;
  return values.find((value) => pattern.test(value)) ?? null;
}

export function assertField(view, label, { pattern } = {}) {
  const values = fieldValues(view, label);
  assert.ok(
    values.length > 0,
    `the inspector renders no ${label} field; tree: ${view.tree.slice(-2000)}`,
  );
  const value = pattern ? values.find((item) => pattern.test(item)) : values[0];
  assert.ok(
    value !== undefined && value.length > 0,
    `${label} reads ${JSON.stringify(values)}, which does not answer ${
      pattern ? String(pattern) : "the field"
    }`,
  );
  return value;
}

export function assertAbsent(view, needles, why) {
  for (const needle of needles) {
    assert.ok(
      !matches(view.tree, needle),
      `${why}: the screen shows ${describe(needle)}`,
    );
  }
}

// Launch the console the manifest points at and wait for its shell. A window
// that is still asking for a backend has no fleet state to assert against, so
// that state fails here rather than as a missing field later.
export function launchConsole() {
  const app = launchCuaProcess({ executable: process.env.CUA_APP_EXECUTABLE });
  const view = waitForScreen(app.pid, app.windowId, /AX\w*Button \(Posture/, Number("60000"));
  assertAbsent(
    view,
    ["Connect to Stado", "This source cannot be read"],
    "the console has no configured Stado source, so no screen can load fleet state",
  );
  return { ...app, view };
}

// Reach a screen the way an operator does — the sidebar destination button —
// and let it finish reading the fleet.
//
// Every one of these screens reads by running `stado` against the fleet's own
// control plane, and a read that loses a race with that control plane comes
// back `infra_down` rather than with rollout state. The screen's own answer to
// that is its refresh action, so a journey that has to assert loaded state runs
// the read again — a bounded number of times, quoting every refusal it saw if
// the screen never loads.
export function openScreen(pid, windowId, title, {
  loaded,
  failures = [],
  refresh,
  timeoutMs,
  attempts = Number("3"),
}) {
  click(pid, windowId, title);
  const answered = (tree) => matches(tree, loaded) || failures.some((text) => tree.includes(text));
  let view = waitForScreen(pid, windowId, answered, timeoutMs);
  const refusals = [];
  for (let attempt = 0; attempt < attempts && !matches(view.tree, loaded); attempt += 1) {
    const state = failures.find((text) => view.tree.includes(text));
    refusals.push(findButton(view, state)?.label || state || "an unreadable state");
    click(pid, windowId, refresh);
    view = poll(pid, windowId, loaded, timeoutMs) || readWindow(pid, windowId);
  }
  assert.ok(
    matches(view.tree, loaded),
    `${title} never reached ${describe(loaded)} in ${attempts + 1} reads; the screen answered: ${
      refusals.join(" || ").slice(0, Number("1500"))
    }`,
  );
  return view;
}

// Select a table row and prove the inspector answered for it. Only a row
// produces the field the caller names, so a selection that does not produce it
// was the wrong element and the next candidate is tried.
export function selectRow(pid, windowId, { needle, timeoutMs, minimumFields, skip = 0 }) {
  const first = readWindow(pid, windowId);
  const rows = rowButtons(first, { minimumFields });
  assert.ok(
    rows.length > skip,
    `the table shows no selectable row past ${skip}; buttons on screen: ${
      buttons(first).map((item) => item.label).join(" | ") || "none"
    }`,
  );
  const tried = [];
  for (const row of rows.slice(skip)) {
    // A row carries live figures — an age ticks over between two reads — so a
    // row that has been relabelled since the list was taken is skipped rather
    // than treated as a screen that cannot be driven.
    let pressed = null;
    try {
      pressed = click(pid, windowId, row.label);
    } catch (error) {
      tried.push(`${row.label} (gone: ${String(error.message || error).slice(0, Number("120"))})`);
      continue;
    }
    tried.push(`${row.label}${pressed?.code ? ` (${pressed.code})` : ""}`);
    const answered = poll(pid, windowId, needle, timeoutMs);
    if (answered) return { view: answered, row };
  }
  throw new Error(
    `no row selection produced ${describe(needle)}; rows tried: ${tried.join(" | ") || "none"}`,
  );
}

// Screenshots are the recorded evidence for a screen journey: the runner fails
// a recorded row that registered no report-typed capture, and it only accepts
// screenshots that live inside the run's artifact root.
export function createEvidence(slug) {
  const artifacts = process.env.PROBIERZ_ARTIFACTS;
  const manifestPath = process.env.PROBIERZ_SPEC_MEDIA_PATH;
  assert.ok(artifacts, "PROBIERZ_ARTIFACTS is required to record screen evidence");
  assert.ok(manifestPath, "PROBIERZ_SPEC_MEDIA_PATH is required to record screen evidence");
  const root = path.resolve(artifacts);
  const entries = [];
  return {
    entries,
    capture(pid, windowId, name) {
      const file = path.join(root, `${slug}-${name}.png`);
      mkdirSync(path.dirname(file), { recursive: true });
      const view = readWindow(pid, windowId, { screenshotOutFile: file });
      assert.ok(
        existsSync(file) && statSync(file).isFile(),
        `cua-driver wrote no screenshot at ${file}`,
      );
      entries.push({ kind: "screenshot", file, contentType: "image/png" });
      return view;
    },
    write() {
      mkdirSync(path.dirname(manifestPath), { recursive: true });
      writeFileSync(manifestPath, `${JSON.stringify(entries, null, 2)}\n`, { mode: 0o600 });
      return entries.map((entry) => entry.file);
    },
  };
}

// Pressing a control the screen has refused is an attempt, not an expectation:
// the driver declines to deliver to a control the app disabled, and that
// decline is itself evidence. The postcondition is asserted by the caller
// either way, because a delivered press must also have changed nothing.
export function attempt(pid, windowId, label) {
  try {
    const pressed = click(pid, windowId, label);
    return {
      enabled: pressed?.element?.enabled !== false,
      code: pressed?.code || null,
      result: JSON.stringify(pressed?.result).slice(0, Number("400")),
    };
  } catch (error) {
    return {
      enabled: null,
      code: "error",
      result: String(error.message || error).slice(0, Number("400")),
    };
  }
}
