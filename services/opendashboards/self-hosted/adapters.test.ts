import { describe, expect, it } from "vitest";
import { basicProvider } from "../lib/auth/providers/basic";
import { createAesSecretBox } from "../lib/secrets/aes-gcm";

const password = "a".repeat(64);
describe("single-user login", () => {
  it("rejects missing, bearer, malformed and incorrect credentials", async () => {
    const provider = basicProvider(password);
    for (const value of ["", "Bearer forged", "Basic !", `Basic ${Buffer.from("admin:wrong").toString("base64")}`]) {
      expect((await provider.authenticate(new Headers({ authorization: value }))).ok).toBe(false);
    }
  });
  it("uses a stable server-owned identity and rejects weak configuration", async () => {
    const authorization = `Basic ${Buffer.from(`admin:${password}`).toString("base64")}`;
    expect(await basicProvider(password).authenticate(new Headers({ authorization }))).toEqual({
      ok: true, user: { id: "self-hosted-admin", email: "admin@localhost" },
    });
    expect(() => basicProvider("short")).toThrow();
  });
});

describe("encrypted connection credentials", () => {
  const box = createAesSecretBox("a".repeat(64));
  const context = { purpose: "postgres-password", userId: "user", connectionId: "db" };
  it("round trips without storing plaintext and uses fresh nonces", async () => {
    const sealed = await box.seal("credential", context);
    expect(JSON.stringify(sealed)).not.toContain("credential");
    expect(await box.open(sealed, context)).toBe("credential");
    expect(await box.seal("credential", context)).not.toEqual(sealed);
  });
  it("rejects row swaps, wrong keys, tampering, plaintext and oversized secrets", async () => {
    const sealed = await box.seal("credential", context);
    for (const field of ["purpose", "userId", "connectionId"]) {
      await expect(box.open(sealed, { ...context, [field]: "different" })).rejects.toThrow();
    }
    await expect(createAesSecretBox("b".repeat(64)).open(sealed, context)).rejects.toThrow();
    if (sealed.provider !== "aes-gcm") throw new Error("wrong provider");
    const bytes = Buffer.from(sealed.ciphertext, "base64");
    bytes[bytes.length - 1] ^= 1;
    await expect(box.open({ ...sealed, ciphertext: bytes.toString("base64") }, context)).rejects.toThrow();
    await expect(box.open({ v: 1, provider: "plaintext", value: "plain" }, context)).rejects.toThrow();
    await expect(box.seal("a".repeat(4097), context)).rejects.toThrow();
    expect(() => createAesSecretBox("short")).toThrow();
  });
});
