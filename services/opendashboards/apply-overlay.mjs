import { cpSync, readFileSync, writeFileSync } from "node:fs";

function replace(path, before, after) {
  const source = readFileSync(path, "utf8");
  if (source.split(before).length !== 2) throw new Error(`Upstream changed: ${path}`);
  writeFileSync(path, source.replace(before, after));
}

// Deployment-only additions. Auth, model orchestration and execution live in the fork.
cpSync("/overlay", ".", { recursive: true });
replace("config/index.ts", 'import { common }', 'import { operatorPrivateDestinations } from "./operator-network";\nimport { common }');
replace("config/index.ts", 'return { ...common, ...ENVIRONMENT_SETTINGS[name] };', `return {
    ...common, ...ENVIRONMENT_SETTINGS[name],
    ...(process.env.OPENDASHBOARDS_SELF_HOSTED === "true" && operatorPrivateDestinations !== null
      ? { network: { ...ENVIRONMENT_SETTINGS[name].network, privateDestinations: operatorPrivateDestinations } }
      : {}),
    ...(process.env.OPENDASHBOARDS_SELF_HOSTED === "true" ? { demoData: { neonProject: "", datasets: [] } } : {}),
  };`);

// Existing electricpeak credentials use a packed IV/tag and array AAD. Read them
// with the same key and owner binding; every new write uses the fork's format.
replace("lib/secrets/aes.ts", 'const ciphertext = Buffer.from(sealed.ciphertext, "base64");', `if (!sealed.iv) {
          const packed = Buffer.from(sealed.ciphertext, "base64");
          const legacy = createDecipheriv("aes-256-gcm", key, packed.subarray(0, 12));
          legacy.setAAD(Buffer.from(JSON.stringify([context.purpose, context.userId, context.connectionId])));
          legacy.setAuthTag(packed.subarray(12, 28));
          return Buffer.concat([legacy.update(packed.subarray(28)), legacy.final()]).toString("utf8");
        }
        const ciphertext = Buffer.from(sealed.ciphertext, "base64");`);

writeFileSync("app/(marketing)/page.tsx", 'import { redirect } from "next/navigation";\nexport default function Home() { redirect("/apps"); }\n');
