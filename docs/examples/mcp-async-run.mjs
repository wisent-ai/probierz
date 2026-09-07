#!/usr/bin/env node
// mcp-async-run.mjs — drive the in-process async run queue over the MCP
// stdio protocol: start a run, poll it to a settled state, list and read
// its artifacts, and show that cancel on a settled job is a no-op.
// Requires the demo app: sh docs/examples/register-demo-app.sh
// Run from the probierz checkout root: node docs/examples/mcp-async-run.mjs
// Protocol reference: docs/mcp.md
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const server = spawn("node", ["agent/mcp.mjs"], { stdio: ["pipe", "pipe", "inherit"] });
const lines = createInterface({ input: server.stdout });
const pending = new Map();
let id = 0;

lines.on("line", (line) => {
  const message = JSON.parse(line);
  pending.get(message.id)?.(message);
  pending.delete(message.id);
});

function rpc(method, params) {
  id += 1;
  server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve) => pending.set(id, resolve));
}

async function tool(name, args) {
  const response = await rpc("tools/call", { name, arguments: args });
  if (response.error) throw new Error(`${name}: ${response.error.message}`);
  return JSON.parse(response.result.content[0].text);
}

const init = await rpc("initialize", {
  protocolVersion: "2024-11-05",
  capabilities: {},
  clientInfo: { name: "docs-example", version: "0" },
});
console.log("server:", JSON.stringify(init.result.serverInfo));

const started = await tool("probierz_start_run", { target: "tui", appId: "docs-demo" });
console.log("started:", started.runId, started.status);

let status = started;
while (["queued", "running"].includes(status.status)) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  status = await tool("probierz_run_status", { runId: started.runId });
}
console.log("settled:", status.status, status.artifactsDir);

const artifacts = await tool("probierz_list_artifacts", { runId: started.runId });
console.log("artifacts:", artifacts.artifacts.map((entry) => entry.file).join(", "));

const report = await tool("probierz_get_artifact", { runId: started.runId, file: "report.json" });
const decoded = JSON.parse(Buffer.from(report.content, "base64").toString("utf8"));
console.log("report stamped with own runId:", decoded.probierz.runId === started.runId);

const cancel = await tool("probierz_cancel_run", { runId: started.runId });
console.log("cancel on settled job → cancelRequested:", cancel.cancelRequested);

server.stdin.end();
process.exit(status.status === "passed" ? 0 : 1);
