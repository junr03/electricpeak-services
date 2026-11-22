import { cpSync, readFileSync, writeFileSync } from "node:fs";

function replace(path, before, after) {
  const source = readFileSync(path, "utf8");
  if (source.split(before).length !== 2) throw new Error(`Upstream changed: ${path}`);
  writeFileSync(path, source.replace(before, after));
}

cpSync("/overlay", ".", { recursive: true });
replace("config/prod.ts", 'publicHost: "opendashboards.com"', 'publicHost: new URL(process.env.APP_ORIGIN || "https://example.invalid").host');
replace("config/prod.ts", 'appOrigin: "https://opendashboards.com"', 'appOrigin: process.env.APP_ORIGIN || "https://example.invalid"');
replace("config/index.ts", 'return { ...common, ...ENVIRONMENT_SETTINGS[name] };', `return {
    ...common, ...ENVIRONMENT_SETTINGS[name],
    ...(process.env.OPENDASHBOARDS_SELF_HOSTED === "true" ? { demoData: { neonProject: "", datasets: [] } } : {}),
  };`);
replace("config/index.ts", '!env.SUPABASE_URL)', '!env.SUPABASE_URL && !env.OPENDASHBOARDS_AUTH_PASSWORD)');
replace("config/index.ts", '!env.KMS_KEY_ID)', '!env.KMS_KEY_ID && !env.OPENDASHBOARDS_ENCRYPTION_KEY)');
replace("config/index.ts", '!env.KMS_ROLE_ARN)', '!env.KMS_ROLE_ARN && !env.OPENDASHBOARDS_ENCRYPTION_KEY)');
replace("config/index.ts", 'if (!env.THESYS_API_KEY) problems.push("THESYS_API_KEY is not set");', `if (!env.THESYS_API_KEY) problems.push("THESYS_API_KEY is not set");
  if (env.OPENDASHBOARDS_AUTH_PASSWORD && env.OPENDASHBOARDS_AUTH_PASSWORD.length < 32) problems.push("Login password is too short");
  if (env.OPENDASHBOARDS_ENCRYPTION_KEY && !/^[a-f0-9]{64}$/.test(env.OPENDASHBOARDS_ENCRYPTION_KEY)) problems.push("Invalid encryption key");`);
replace("lib/auth/index.ts", 'import { devProvider }', 'import { basicProvider } from "./providers/basic";\nimport { devProvider }');
replace("lib/auth/index.ts", 'if (settings.supabaseUrl) return', 'if (process.env.OPENDASHBOARDS_AUTH_PASSWORD) return basicProvider(process.env.OPENDASHBOARDS_AUTH_PASSWORD);\n  if (settings.supabaseUrl) return');
replace("lib/secrets/types.ts", '| { v: 1; provider: "none" };', '| { v: 1; provider: "aes-gcm"; ciphertext: string }\n  | { v: 1; provider: "none" };');
replace("lib/secrets/index.ts", 'import { createKmsSecretBox }', 'import { createAesSecretBox } from "./aes-gcm";\nimport { createKmsSecretBox }');
replace("lib/secrets/index.ts", 'export function getSecretBox(settings: SecretSettings = current()): SecretBox {', 'export function getSecretBox(settings: SecretSettings = current()): SecretBox {\n  if (process.env.OPENDASHBOARDS_ENCRYPTION_KEY) return createAesSecretBox(process.env.OPENDASHBOARDS_ENCRYPTION_KEY);');
replace("lib/secrets/index.ts", 'export function getSecretMode(settings: SecretSettings = current()): SecretProvider | "unconfigured" {', 'export function getSecretMode(settings: SecretSettings = current()): SecretProvider | "unconfigured" {\n  if (process.env.OPENDASHBOARDS_ENCRYPTION_KEY) return "aes-gcm";');
replace("proxy.ts", 'new NextResponse("Unauthorized", { status: 401 })', 'new NextResponse("Unauthorized", { status: 401, headers: { "WWW-Authenticate": \'Basic realm="OpenDashboards", charset="UTF-8"\', "Cache-Control": "no-store" } })');
replace("app/(app)/connections/page.tsx", 'secretMode === "kms"\n      ?', 'secretMode === "aes-gcm"\n      ? "Keys and passwords are encrypted on this server."\n      : secretMode === "kms"\n      ?');
replace("scripts/db-migrate.mjs", 'import { config } from "dotenv";\n', '');
replace("scripts/db-migrate.mjs", 'config({ path: ".env.local", quiet: true });', '');
replace("lib/site.ts", '"https://opendashboards.com";', 'process.env.APP_ORIGIN || "https://example.invalid";');

// The installation opens the app directly instead of advertising the hosted site.
writeFileSync("app/(marketing)/page.tsx", 'import { redirect } from "next/navigation";\nexport default function Home() { redirect("/apps"); }\n');
