#!/usr/bin/env node
// probierz — command-line view of the cross-platform test toolkit surface.
//
// Reads the same lib the MCP server uses (one source of truth) and offers
// read-only views. It never runs a suite; `cmd` only prints the command to run.
//   probierz list                 — the four test surfaces + how to run each
//   probierz specs [surface]      — e2e/spec files discovered on disk
//   probierz describe <spec>      — static outline (describe/it titles) of a spec
//   probierz cmd <target>         — the exact shell command for a target
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";

function out(value) {
  process.stdout.write(JSON.stringify(value, null, Number("2")) + "\n");
}

function usage() {
  process.stderr.write(
    [
      "usage:",
      "  probierz list                 every test surface + run script",
      "  probierz specs [surface]      spec files on disk (optional surface filter)",
      "  probierz describe <spec>      static outline of a spec file",
      "  probierz cmd <target>         exact command to run a target",
      "",
      "surfaces: web | electron | mobile | desktop-native",
      "targets:  web | electron | mobile:ios | mobile:android | desktop:mac | desktop:win",
    ].join("\n") + "\n",
  );
}

function main() {
  const argv = process.argv.slice(Number("2"));
  const cmd = argv[Number("0")];
  const rest = argv.slice(Number("1"));

  if (!cmd || cmd === "help" || cmd === "-h" || cmd === "--help") {
    usage();
    return;
  }
  if (cmd === "list") {
    out(SURFACES);
    return;
  }
  if (cmd === "specs") {
    out(listSpecs(rest[Number("0")]));
    return;
  }
  if (cmd === "describe") {
    const spec = rest[Number("0")];
    if (!spec) throw new Error("describe needs a spec path");
    out(describeSpec(spec));
    return;
  }
  if (cmd === "cmd") {
    const target = rest[Number("0")];
    if (!target) throw new Error("cmd needs a target");
    out(runCommand(target));
    return;
  }
  usage();
  throw new Error(`unknown command: ${cmd}`);
}

try {
  main();
} catch (err) {
  process.stderr.write(`probierz: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exitCode = Number("1");
}
