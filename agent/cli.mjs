#!/usr/bin/env node
// probierz — command-line view of the cross-platform test toolkit.
//
// Reads the same lib the MCP server uses (one source of truth). The first four
// commands are read-only; `run` and `analyze` are the execution surface that
// actually spawns a suite and inspects what it produced.
//   probierz list                 — the four test surfaces + how to run each
//   probierz specs [surface]      — e2e/spec files discovered on disk
//   probierz describe <spec>      — static outline (describe/it titles) of a spec
//   probierz cmd <target>         — the exact shell command for a target (prints only)
//   probierz run <target> [opts]  — EXECUTE a target, capture result, auto-analyze
//   probierz analyze <report> [dir] — parse a report + inventory media
//
// run options: --record  --frames N  --timeout MS  --no-analyze  KEY=VALUE...
// analyze options: [artifactsDir] --frames N --tool playwright|wdio
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";
import { runSurface, targetList } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";

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
      "  probierz cmd <target>         exact command to run a target (prints only)",
      "  probierz run <target> [opts]  execute a target, capture result, auto-analyze",
      "  probierz analyze <report> [dir]  parse a report + inventory media",
      "",
      "run opts: --record  --frames N  --timeout MS  --no-analyze  KEY=VALUE...",
      "surfaces: web | electron | mobile | desktop-native",
      "targets:  web | electron | mobile:ios | mobile:android | desktop:mac | desktop:win",
    ].join("\n") + "\n",
  );
}

// Split run args into flags, an env map (KEY=VALUE), and leftovers.
function parseRunArgs(rest) {
  const opts = { env: {}, record: false, analyze: true, frames: Number("0"), timeoutMs: Number("0") };
  for (let i = Number("0"); i < rest.length; i += Number("1")) {
    const a = rest[i];
    if (a === "--record") opts.record = true;
    else if (a === "--no-analyze") opts.analyze = false;
    else if (a === "--frames") { i += Number("1"); opts.frames = Number(rest[i]); }
    else if (a === "--timeout") { i += Number("1"); opts.timeoutMs = Number(rest[i]); }
    else if (a.includes("=")) {
      const eq = a.indexOf("=");
      opts.env[a.slice(Number("0"), eq)] = a.slice(eq + Number("1"));
    }
  }
  return opts;
}

async function main() {
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
  if (cmd === "run") {
    const target = rest[Number("0")];
    if (!target) throw new Error(`run needs a target (one of ${targetList().join(", ")})`);
    const opts = parseRunArgs(rest.slice(Number("1")));
    const result = await runSurface(target, { env: opts.env, record: opts.record, timeoutMs: opts.timeoutMs });
    // Auto-analyze when a report landed, unless suppressed.
    let analysis = null;
    if (opts.analyze) {
      try {
        analysis = analyzeRun({ reportPath: result.reportPath, artifactsDir: result.artifactsDir, tool: result.tool, frames: opts.frames });
      } catch (e) {
        analysis = { error: e instanceof Error ? e.message : String(e) };
      }
    }
    out({ ...result, analysis });
    return;
  }
  if (cmd === "analyze") {
    const reportPath = rest[Number("0")];
    if (!reportPath) throw new Error("analyze needs a report path");
    const opts = parseRunArgs(rest.slice(Number("1")));
    const artifactsDir = rest[Number("1")] && !rest[Number("1")].startsWith("--") ? rest[Number("1")] : undefined;
    const toolIdx = rest.indexOf("--tool");
    const tool = toolIdx >= Number("0") ? rest[toolIdx + Number("1")] : undefined;
    out(analyzeRun({ reportPath, artifactsDir, tool, frames: opts.frames }));
    return;
  }
  usage();
  throw new Error(`unknown command: ${cmd}`);
}

main().catch((err) => {
  process.stderr.write(`probierz: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exitCode = Number("1");
});
