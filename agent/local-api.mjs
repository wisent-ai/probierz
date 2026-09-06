import { createServer } from "node:http";
import { adoptProject, listProjectAdoptions } from "./project-adoption.mjs";

const BODY_LIMIT = 1024 * 1024;

function json(response, statusCode, body) {
  const bytes = Buffer.from(`${JSON.stringify(body)}\n`);
  response.writeHead(statusCode, {
    "content-type": "application/json; charset=utf-8",
    "content-length": bytes.length,
    "cache-control": "no-store",
  });
  response.end(bytes);
}

async function requestBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > BODY_LIMIT) throw new Error("request body exceeds 1 MiB");
    chunks.push(chunk);
  }
  if (!chunks.length) return {};
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

export function startLocalAPI({ projectRoot, port = 0 }) {
  return new Promise((resolve, reject) => {
    const server = createServer(async (request, response) => {
      try {
        const url = new URL(request.url || "/", "http://127.0.0.1");
        if (request.method === "GET" && url.pathname === "/v1/health") {
          json(response, 200, { ok: true, product: "probierz" });
          return;
        }
        if (request.method === "GET" && url.pathname === "/v1/project-adoptions") {
          json(response, 200, listProjectAdoptions({ projectRoot }));
          return;
        }
        if (request.method === "POST" && url.pathname === "/v1/project-adoptions") {
          const body = await requestBody(request);
          if (typeof body.sourceRoot !== "string" || !body.sourceRoot.trim()) {
            json(response, 400, { error: "sourceRoot is required" });
            return;
          }
          json(response, 200, adoptProject({
            projectRoot,
            sourceRoot: body.sourceRoot,
            replace: body.replace === true,
          }));
          return;
        }
        json(response, 404, { error: "not found" });
      } catch (error) {
        json(response, 400, { error: error instanceof Error ? error.message : String(error) });
      }
    });
    server.on("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      const actualPort = typeof address === "object" && address ? address.port : port;
      process.stdout.write(`${JSON.stringify({ ready: true, host: "127.0.0.1", port: actualPort })}\n`);
      resolve(server);
    });
  });
}
