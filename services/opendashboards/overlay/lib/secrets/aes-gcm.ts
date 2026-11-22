import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

import {
  assertSecretSize,
  SecretProviderMismatchError,
  SecretUnreadableError,
  type SecretBox,
  type SecretContext,
} from "./types";

/** The persistent key is separate from Postgres; context binds each encrypted row. */
export function createAesSecretBox(encodedKey: string): SecretBox {
  if (!/^[a-f0-9]{64}$/.test(encodedKey)) throw new Error("Encryption key must be 32 hex-encoded bytes");
  const key = Buffer.from(encodedKey, "hex");
  const aad = (context: SecretContext) => Buffer.from(JSON.stringify([
    context.purpose, context.userId, context.connectionId,
  ]));
  return {
    provider: "aes-gcm",
    async seal(plaintext, context) {
      assertSecretSize(plaintext);
      const iv = randomBytes(12);
      const cipher = createCipheriv("aes-256-gcm", key, iv);
      cipher.setAAD(aad(context));
      const ciphertext = Buffer.concat([cipher.update(plaintext, "utf8"), cipher.final()]);
      return {
        v: 1, provider: "aes-gcm",
        ciphertext: Buffer.concat([iv, cipher.getAuthTag(), ciphertext]).toString("base64"),
      };
    },
    async open(sealed, context) {
      if (sealed.provider !== "aes-gcm") throw new SecretProviderMismatchError("aes-gcm", sealed.provider);
      try {
        const bytes = Buffer.from(sealed.ciphertext, "base64");
        const decipher = createDecipheriv("aes-256-gcm", key, bytes.subarray(0, 12));
        decipher.setAAD(aad(context));
        decipher.setAuthTag(bytes.subarray(12, 28));
        return Buffer.concat([decipher.update(bytes.subarray(28)), decipher.final()]).toString("utf8");
      } catch (cause) {
        throw new SecretUnreadableError({ cause });
      }
    },
  };
}
