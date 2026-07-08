#!/usr/bin/env node
// Read-only stdio JSON-RPC (Model Context Protocol) server for probierz.
// Mirrors the echo / weles / skarbiec MCP servers: newline-delimited JSON in on
// stdin, exactly one response line per request out on stdout, diagnostics on
// stderr. The tool allow-list is read-only: it discovers surfaces/specs and
// emits run commands as strings, but never executes a test (that needs
// Chromium / Appium / a simulator and stays out of this surface).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";

const PROTOCOL_VERSION = "2024-11-05";
const JSONRPC_VERSION = "2.0";
const CODE_PARSE_ERROR = "-32700";
const CODE_METHOD_NOT_FOUND = "-32601";
const CODE_INTERNAL_ERROR = "-32000";
const NOT_FOUND = "-1";

function code(raw) {
  return Number(raw);
}

function serverVersion() {
  try {
    const here = dirname(fileURLToPath(import.meta.url));
    const pkg = JSON.parse(readFileSync(join(here, "..", "package.json"), "utf8"));
    return pkg.version || "unknown";
  } catch {
    return "unknown";
  }
}

const objectSchema = (properties, required) => ({
  type: "object",
  properties: properties || {},
  required: required || [],
});

const TOOLS = [
  {
    name: "probierz_list_surfaces",
    description: "List the cross-platform test surfaces (web, electron, mobile, desktop-native): tool, npm script, targets, and relevant env vars.",
    inputSchema: objectSchema({}),
  },
  {
    name: "probierz_list_specs",
    description: "Discover e2e/spec files on disk; optional surface narrows to one (web|electron|mobile|desktop-native).",
    inputSchema: objectSchema({
      surface: { type: "string", description: "Optional surface filter." },
    }),
  },
  {
    name: "probierz_describe_spec",
    description: "Static outline of a spec (describe/it/test titles in file order) by its path under the probierz root. Does not execute anything.",
    inputSchema: objectSchema({
      spec: { type: "string", description: "Spec path, e.g. packages/mobile/test/specs/byk.e2e.ts" },
    }, ["spec"]),
  },
  {
    name: "probierz_run_command",
    description: "Return the exact shell command to run a target yourself (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win). Read-only: probierz never runs it.",
    inputSchema: objectSchema({
      target: { type: "string", description: "One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win." },
    }, ["target"]),
  },
];

function textResult(value) {
  return { content: [{ type: "text", text: JSON.stringify(value, null, code("2")) }] };
}

async function callTool(name, args) {
  if (name === "probierz_list_surfaces") return textResult(SURFACES);
  if (name === "probierz_list_specs") return textResult(listSpecs(args.surface));
  if (name === "probierz_describe_spec") return textResult(describeSpec(args.spec));
  if (name === "probierz_run_command") return textResult(runCommand(args.target));
  const err = new Error(`unknown tool: ${name}`);
  err.rpcCode = CODE_METHOD_NOT_FOUND;
  throw err;
}

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function asString(value, name) {
  if (typeof value !== "string" || value.length === Number("0")) {
    throw new Error(`${name} must be a non-empty string`);
  }
  return value;
}

async function handle(request) {
  if (!request || !request.method) return;
  const hasId = Object.prototype.hasOwnProperty.call(request, "id");
  if (!hasId) return;
  const id = request.id ?? null;

  try {
    if (request.method === "initialize") {
      send({
        jsonrpc: JSONRPC_VERSION,
        id,
        result: {
          protocolVersion: PROTOCOL_VERSION,
          capabilities: { tools: {} },
          serverInfo: { name: "probierz", version: serverVersion() },
        },
      });
      return;
    }
    if (request.method === "ping") {
      send({ jsonrpc: JSONRPC_VERSION, id, result: {} });
      return;
    }
    if (request.method === "tools/list") {
      send({ jsonrpc: JSONRPC_VERSION, id, result: { tools: TOOLS } });
      return;
    }
    if (request.method === "tools/call") {
      const params = request.params || {};
      const name = asString(params.name, "name");
      const args = params.arguments && typeof params.arguments === "object" ? params.arguments : {};
      const result = await callTool(name, args);
      send({ jsonrpc: JSONRPC_VERSION, id, result });
      return;
    }
    send({
      jsonrpc: JSONRPC_VERSION,
      id,
      error: { code: code(CODE_METHOD_NOT_FOUND), message: `method not found: ${request.method}` },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const rpcCode = error && error.rpcCode ? error.rpcCode : CODE_INTERNAL_ERROR;
    send({ jsonrpc: JSONRPC_VERSION, id, error: { code: code(rpcCode), message } });
  }
}

function serve() {
  let buffer = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffer += chunk;
    for (;;) {
      const newline = buffer.indexOf("\n");
      if (newline === code(NOT_FOUND)) break;
      const line = buffer.slice(Number("0"), newline).trim();
      buffer = buffer.slice(newline + Number("1"));
      if (!line) continue;
      let request;
      try {
        request = JSON.parse(line);
      } catch {
        send({
          jsonrpc: JSONRPC_VERSION,
          id: null,
          error: { code: code(CODE_PARSE_ERROR), message: "parse error" },
        });
        continue;
      }
      void handle(request);
    }
  });
}

serve();
