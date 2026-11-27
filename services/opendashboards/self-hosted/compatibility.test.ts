import { createCipheriv, randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import { createAesSecretBox } from "../lib/secrets/aes";
import type { SealedSecret } from "../lib/secrets/types";

const key = "a".repeat(64);
const context = { purpose: "postgres-password", userId: "self-hosted-admin", connectionId: "existing-db" };

// Match the pre-upgrade deployment's exact persisted wire format.
function legacyCredential(): SealedSecret {
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", Buffer.from(key, "hex"), iv);
  cipher.setAAD(Buffer.from(JSON.stringify([context.purpose, context.userId, context.connectionId])));
  const ciphertext = Buffer.concat([cipher.update("existing-password", "utf8"), cipher.final()]);
  return JSON.parse(JSON.stringify({ v: 1, provider: "aes-gcm",
    ciphertext: Buffer.concat([iv, cipher.getAuthTag(), ciphertext]).toString("base64") }));
}

describe("electricpeak credential upgrade", () => {
  it("reads existing credentials with the original key and identity, and writes the native format", async () => {
    const box = createAesSecretBox(key);
    expect(await box.open(legacyCredential(), context)).toBe("existing-password");
    const sealed = await box.seal("new-password", context);
    expect(sealed).toHaveProperty("iv");
    expect(await box.open(sealed, context)).toBe("new-password");
  });

  it("preserves authenticated owner, connection, purpose and key boundaries for legacy records", async () => {
    const box = createAesSecretBox(key);
    for (const field of ["userId", "connectionId", "purpose"]) {
      await expect(box.open(legacyCredential(), { ...context, [field]: "different" })).rejects.toThrow();
    }
    await expect(createAesSecretBox("b".repeat(64)).open(legacyCredential(), context)).rejects.toThrow();
    const sealed = legacyCredential();
    if (sealed.provider !== "aes-gcm") throw new Error("Unexpected provider");
    await expect(box.open({ ...sealed, ciphertext: "malformed" }, context)).rejects.toThrow();
  });
});
