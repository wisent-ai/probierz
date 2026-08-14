#!/usr/bin/env node
import { createHash } from "node:crypto";

const REQUIRED_ACCOUNT = [
  "OKO_E2E_EMAIL",
  "OKO_E2E_SUPABASE_URL",
  "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
const REQUIRED_BROKER = [
  "OKO_E2E_EMAIL",
  "OKO_E2E_OTP_BROKER_URL",
  "OKO_E2E_OTP_BROKER_TOKEN",
];

function requiredEnv(names, source = process.env) {
  const missing = names.filter((name) => !source[name]);
  if (missing.length) throw new Error(`missing E2E configuration: ${missing.join(", ")}`);
  return Object.fromEntries(names.map((name) => [name, source[name]]));
}

function assertTechnicalEmail(email) {
  const local = String(email).split("@")[0]?.toLowerCase() || "";
  if (!local.includes("e2e") && !local.includes("probierz")) {
    throw new Error("OKO_E2E_EMAIL must be a dedicated address containing 'e2e' or 'probierz'");
  }
}

function safeIdentity(email) {
  return createHash("sha256").update(String(email).toLowerCase()).digest("hex").slice(0, 12);
}

async function requestJson(url, options) {
  const response = await fetch(url, options);
  const text = await response.text();
  let body = null;
  try { body = text ? JSON.parse(text) : null; } catch { body = { message: text }; }
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${body?.message || body?.error || "request failed"}`);
  }
  return body;
}

function validatedOtp(value, sourceName) {
  const code = String(value || "").trim();
  if (!/^\d{6,8}$/.test(code)) {
    throw new Error(`${sourceName} returned an OTP outside the supported 6-8 digit range`);
  }
  return code;
}

async function generateAdminOtp(source) {
  const env = requiredEnv(REQUIRED_ACCOUNT, source);
  const email = env.OKO_E2E_EMAIL.toLowerCase();
  assertTechnicalEmail(email);
  const base = env.OKO_E2E_SUPABASE_URL.replace(/\/$/, "");
  const body = await requestJson(`${base}/auth/v1/admin/generate_link`, {
    method: "POST",
    headers: {
      apikey: env.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY,
      Authorization: `Bearer ${env.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ type: "magiclink", email }),
  });
  return validatedOtp(body?.properties?.email_otp ?? body?.email_otp, "Supabase Admin API");
}

export async function ensureTechnicalAccount(source = process.env) {
  const env = requiredEnv(REQUIRED_ACCOUNT, source);
  const email = env.OKO_E2E_EMAIL.toLowerCase();
  assertTechnicalEmail(email);
  const base = env.OKO_E2E_SUPABASE_URL.replace(/\/$/, "");
  const headers = {
    apikey: env.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY,
    Authorization: `Bearer ${env.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY}`,
    "Content-Type": "application/json",
  };

  const page = await requestJson(`${base}/auth/v1/admin/users?page=1&per_page=1000`, { headers });
  const users = Array.isArray(page?.users) ? page.users : [];
  const existing = users.find((user) => String(user.email || "").toLowerCase() === email);
  if (existing) {
    return { created: false, userId: existing.id, emailHash: safeIdentity(email) };
  }

  const created = await requestJson(`${base}/auth/v1/admin/users`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      email,
      email_confirm: true,
      user_metadata: { purpose: "probierz-e2e" },
      app_metadata: { provider: "email", providers: ["email"], purpose: "probierz-e2e" },
    }),
  });
  if (!created?.id) throw new Error("Supabase did not return a created user ID");
  return { created: true, userId: created.id, emailHash: safeIdentity(email) };
}

export async function waitForOtp({ after = new Date(Date.now() - 30_000), timeoutMs = 90_000 } = {}, source = process.env) {
  const hasBrokerURL = Boolean(source.OKO_E2E_OTP_BROKER_URL);
  const hasBrokerToken = Boolean(source.OKO_E2E_OTP_BROKER_TOKEN);
  if (!hasBrokerURL && !hasBrokerToken) return generateAdminOtp(source);
  const env = requiredEnv(REQUIRED_BROKER, source);
  const email = env.OKO_E2E_EMAIL.toLowerCase();
  assertTechnicalEmail(email);
  const endpoint = new URL("v1/otp", `${env.OKO_E2E_OTP_BROKER_URL.replace(/\/$/, "")}/`);
  endpoint.searchParams.set("email", email);
  endpoint.searchParams.set("after", new Date(after).toISOString());
  if (source.PROBIERZ_RUN_ID) endpoint.searchParams.set("runId", source.PROBIERZ_RUN_ID);

  const deadline = Date.now() + Number(timeoutMs);
  while (Date.now() < deadline) {
    const body = await requestJson(endpoint, {
      headers: { Authorization: `Bearer ${env.OKO_E2E_OTP_BROKER_TOKEN}` },
    });
    if (body?.code) {
      return validatedOtp(body.code, "OTP broker");
    }
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
  throw new Error(`OTP broker timed out after ${timeoutMs}ms`);
}

async function main() {
  const command = process.argv[2];
  if (command === "ensure-account") {
    const result = await ensureTechnicalAccount();
    process.stdout.write(`${JSON.stringify(result)}\n`);
    return;
  }
  throw new Error("usage: otp-broker.mjs ensure-account");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    process.stderr.write(`oko e2e broker: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
