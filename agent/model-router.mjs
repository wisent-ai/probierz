import { createHash, createHmac } from "node:crypto";

// Authenticated, provider-neutral model authoring through the Stado router.
// Product code receives only the router-scoped bearer; provider credentials
// remain inside the router runtime.

const HTTP_OK = Number("200");
const HTTP_REDIRECT = Number("300");
const MODEL_BUDGET_MS = Number("3600000");
const MAX_OUTPUT_TOKENS = Number("12000");
const MAX_TOOL_NAME_CHARS = Number("64");
const ROUTER_SELECTOR = "any";
const TOOL_NAME = /^[a-z][a-z\d_]*$/;

function requiredEnvironment(name) {
  const value = String(process.env[name] || "").trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function isLoopback(hostname) {
  const host = hostname.toLowerCase();
  return host === "localhost" || host.endsWith(".localhost") || host === "::1" || host === "[::1]" || host.startsWith("127.");
}

export function stadoModelRouterUrl(value = process.env.STADO_MODEL_ROUTER_URL) {
  const configured = String(value || "").trim();
  if (!configured) throw new Error("STADO_MODEL_ROUTER_URL is required");
  let parsed;
  try {
    parsed = new URL(configured);
  } catch {
    throw new Error("STADO_MODEL_ROUTER_URL must be a valid URL");
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new Error("STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment");
  }
  if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && isLoopback(parsed.hostname))) {
    throw new Error("STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP");
  }
  return parsed.href.replace(/\/+$/, "");
}

function routerToken() {
  const token = requiredEnvironment("STADO_MODEL_ROUTER_TOKEN");
  if (/\s/.test(token)) throw new Error("STADO_MODEL_ROUTER_TOKEN must not contain whitespace");
  return token;
}

function artifactFromResponse(payload, toolName) {
  const message = payload?.choices?.at(Number("0"))?.message;
  const calls = Array.isArray(message?.tool_calls)
    ? message.tool_calls.filter((call) => call?.type === "function" && call?.function?.name === toolName)
    : [];
  if (calls.length !== Number("1")) {
    throw new Error(`Stado model router response must contain exactly one ${toolName} tool call`);
  }
  let args;
  try {
    args = JSON.parse(calls.at(Number("0")).function.arguments);
  } catch {
    throw new Error(`Stado model router returned invalid ${toolName} arguments`);
  }
  if (!args || typeof args !== "object" || typeof args.content !== "string" || !args.content.trim()) {
    throw new Error(`Stado model router returned an empty ${toolName} artifact`);
  }
  return {
    content: args.content,
    routerModel: typeof payload.model === "string" ? payload.model : null,
    usage: payload.usage && typeof payload.usage === "object" ? payload.usage : null,
  };
}

export async function draftStructuredArtifact({ brief, toolName, description }) {
  if (typeof brief !== "string" || !brief.trim()) throw new Error("model-router brief is required");
  if (typeof toolName !== "string" || toolName.length > MAX_TOOL_NAME_CHARS || !TOOL_NAME.test(toolName)) {
    throw new Error("model-router tool name is invalid");
  }
  if (typeof description !== "string" || !description.trim()) throw new Error("model-router artifact description is required");

  const endpoint = `${stadoModelRouterUrl()}/v1/chat/completions`;
  const agentId = requiredEnvironment("PROBIERZ_MODEL_AGENT_ID");
  const agentSecret = requiredEnvironment("PROBIERZ_MODEL_AGENT_SECRET");
  const body = JSON.stringify({
    model: ROUTER_SELECTOR,
    max_tokens: MAX_OUTPUT_TOKENS,
    temperature: Number("0.1"),
    messages: [
      {
        role: "system",
        content: `You are a Probierz authoring worker. Produce the requested artifact, then call ${toolName} exactly once with the complete file contents. Do not modify files or return prose.`,
      },
      { role: "user", content: brief },
    ],
    tools: [
      {
        type: "function",
        function: {
          name: toolName,
          description,
          parameters: {
            type: "object",
            properties: {
              content: { type: "string", description: "Complete artifact contents, without Markdown fences." },
            },
            required: ["content"],
            additionalProperties: false,
          },
        },
      },
    ],
  });
  const timestamp = String(Math.floor(Date.now() / Number("1000")));
  const digest = createHash("sha256").update(body).digest("hex");
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${routerToken()}`,
      "Content-Type": "application/json",
      "x-agent-id": agentId,
      "x-agent-timestamp": timestamp,
      "x-agent-signature": createHmac("sha256", agentSecret).update(`${agentId}:${timestamp}:${digest}`).digest("hex"),
    },
    body,
    signal: AbortSignal.timeout(MODEL_BUDGET_MS),
  });
  const raw = await response.text();
  let payload;
  try {
    payload = JSON.parse(raw);
  } catch {
    throw new Error(`Stado model router returned non-JSON (${response.status})`);
  }
  if (response.status < HTTP_OK || response.status >= HTTP_REDIRECT) {
    const detail = String(payload?.error?.message || "request failed").slice(Number("0"), Number("500"));
    throw new Error(`Stado model router request failed (${response.status}): ${detail}`);
  }
  return artifactFromResponse(payload, toolName);
}
