import { createHash, timingSafeEqual } from "node:crypto";

import type { AuthProvider } from "../provider";

/** One server-owned identity; credentials are challenged over Envoy's HTTPS. */
export function basicProvider(password: string): AuthProvider {
  if (password.length < 32) throw new Error("The login password must contain at least 32 characters");
  const digest = (value: string) => createHash("sha256").update(value).digest();
  const expected = digest(`admin:${password}`);
  return {
    async authenticate(headers) {
      const encoded = headers.get("authorization")?.match(/^Basic ([A-Za-z0-9+/]+=*)$/i)?.[1];
      if (!encoded) return { ok: false, reason: "signed_out" };
      const supplied = Buffer.from(encoded, "base64").toString("utf8");
      if (!timingSafeEqual(digest(supplied), expected)) return { ok: false, reason: "invalid" };
      return { ok: true, user: { id: "self-hosted-admin", email: "admin@localhost" } };
    },
  };
}
