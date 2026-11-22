import { randomBytes } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const state = process.env.OPENDASHBOARDS_STATE_DIR || "/data";
const names = ["database-password", "admin-password", "encryption-key"];
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
const apiKey = readFileSync(process.env.THESYS_API_KEY_FILE || "/run/secrets/thesys-api-key", "utf8").trim();
if (!apiKey) throw new Error("Thesys API key is empty");
const origin = new URL(process.env.APP_ORIGIN);
if (origin.protocol !== "https:" || origin.origin !== process.env.APP_ORIGIN) {
  throw new Error("APP_ORIGIN must be an HTTPS origin without a path");
}
Object.assign(process.env, {
  THESYS_API_KEY: apiKey,
  OPENDASHBOARDS_AUTH_PASSWORD: read("admin-password"),
  OPENDASHBOARDS_ENCRYPTION_KEY: read("encryption-key"),
  DATABASE_URL: `postgres://opendashboards:${read("database-password")}@${process.env.DATABASE_HOST || "opendashboards-db"}:5432/opendashboards`,
  OPENDASHBOARDS_SELF_HOSTED: "true",
  NEXT_TELEMETRY_DISABLED: "1",
});

// A healthy Postgres dependency starts first. Apply versioned, additive migrations
// on every start; never run the destructive development db:reset command.
const migration = spawnSync(process.execPath, ["scripts/db-migrate.mjs"], { stdio: "inherit" });
if (migration.status !== 0) process.exit(migration.status || 1);
process.execve(process.execPath, [process.execPath, "node_modules/next/dist/bin/next", "start", "--hostname", "0.0.0.0"], process.env);
