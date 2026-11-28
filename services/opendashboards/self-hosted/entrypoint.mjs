import { randomBytes } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { waitForDatabase, waitForModels } from "./readiness.mjs";

const state = process.env.OPENDASHBOARDS_STATE_DIR || "/data";
const names = ["database-password", "admin-password", "encryption-key", "session-secret"];
if (process.argv[2] === "init") {
  mkdirSync(state, { recursive: true, mode: 0o700 });
  for (const name of names) {
    try {
      writeFileSync(`${state}/${name}`, randomBytes(32).toString("hex") + "\n", { flag: "wx", mode: 0o600 });
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
    }
    if (!/^[a-f0-9]{64}$/.test(readFileSync(`${state}/${name}`, "utf8").trim())) {
      throw new Error(`Invalid state file: ${name}`);
    }
  }
  process.exit(0);
}

const read = (name) => readFileSync(`${state}/${name}`, "utf8").trim();
if (!process.env.LLM_BASE_URL?.trim() || !process.env.LLM_MODEL?.trim()) {
  throw new Error("LLM_BASE_URL and LLM_MODEL must identify your inference server and model");
}
if (process.env.LLM_API_KEY_FILE) {
  process.env.LLM_API_KEY = readFileSync(process.env.LLM_API_KEY_FILE, "utf8").trim();
  if (!process.env.LLM_API_KEY) throw new Error("Inference API key file is empty");
}
const origin = new URL(process.env.APP_ORIGIN);
if (origin.protocol !== "https:" || origin.origin !== process.env.APP_ORIGIN) {
  throw new Error("APP_ORIGIN must be an HTTPS origin without a path");
}
Object.assign(process.env, {
  AUTH_PASSWORD: read("admin-password"),
  AUTH_SECRET: read("session-secret"),
  AUTH_USER_ID: "self-hosted-admin",
  AUTH_USER_EMAIL: "admin@localhost",
  ENCRYPTION_KEY: read("encryption-key"),
  DATABASE_URL: `postgres://opendashboards:${read("database-password")}@${process.env.DATABASE_HOST || "opendashboards-db"}:5432/opendashboards`,
  OPENDASHBOARDS_SELF_HOSTED: "true",
  NEXT_TELEMETRY_DISABLED: "1",
});

// Compose describes ordering; readiness belongs to the container so it also
// works with orchestrators that omit Compose's service_healthy condition.
await waitForDatabase(process.env.DATABASE_URL);
if (process.env.LLM_WAIT_FOR_MODELS === "true") {
  await waitForModels({
    baseUrl: process.env.LLM_BASE_URL,
    models: [...new Set([process.env.LLM_MODEL, process.env.LLM_DASHBOARD_MODEL].filter(Boolean))],
    apiKey: process.env.LLM_API_KEY,
  });
}

// Apply versioned, additive migrations; never run development db:reset.
const migration = spawnSync(process.execPath, ["scripts/db-migrate.mjs"], { stdio: "inherit" });
if (migration.status !== 0) process.exit(migration.status || 1);
process.execve(process.execPath, [process.execPath, "node_modules/next/dist/bin/next", "start", "--hostname", "0.0.0.0"], process.env);
